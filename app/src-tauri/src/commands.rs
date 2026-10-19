use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use futures::stream::{self, StreamExt};
use rusqlite::params;
use serde::Serialize;
use serde_json::json;

use tauri::Emitter;
use tauri_plugin_notification::NotificationExt;

use crate::audit::AuditIssue;
use crate::backtest;
use crate::config::{self, StrategyConfig};
use crate::features::{self, Bar};
use crate::history;
use crate::journal::{self, JournalEntry, JournalInput, JournalStats};
use crate::ledger::{self, Fill, FillInput, LedgerDay};
use crate::market::{self, DayBar, Quote};
use crate::research;
use crate::risk::{self, RiskState};
use crate::sample;
use crate::session;
use crate::state::{AppState, FeedStatus, ScanCacheEntry, ScanFail, SeriesCache};
use crate::statement::{self, ImportResult};
use crate::store;
use crate::strategy::{self, Signal};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub stack: String,
    pub mode: String,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        stack: "tauri2 + vue3 + ts".to_string(),
        mode: "skeleton".to_string(),
    }
}

#[tauri::command]
pub async fn fetch_quote(
    state: tauri::State<'_, AppState>,
    symbol: Option<String>,
) -> Result<Quote, String> {
    let sym = match symbol {
        Some(s) => s,
        None => state.symbol.lock().map_err(|e| e.to_string())?.clone(),
    };
    market::fetch_quote(&state.client, &sym).await
}

#[tauri::command]
pub fn feed_status(state: tauri::State<'_, AppState>) -> Result<FeedStatus, String> {
    Ok(state.feed.lock().map_err(|e| e.to_string())?.clone())
}

#[tauri::command]
pub fn submit_fill(state: tauri::State<'_, AppState>, input: FillInput) -> Result<Fill, String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    ledger::submit(&mut conn, &input)
}

#[tauri::command]
pub fn void_fill(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    ledger::void_fill(&mut conn, id)
}

#[tauri::command]
pub fn list_fills(
    state: tauri::State<'_, AppState>,
    day: String,
    symbol: String,
) -> Result<Vec<Fill>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    ledger::list_by_day(&conn, &day, &symbol)
}

#[tauri::command]
pub fn ledger_day(
    state: tauri::State<'_, AppState>,
    day: String,
    symbol: String,
    price: Option<f64>,
) -> Result<LedgerDay, String> {
    let cfg = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        config::load_for(&conn, &symbol)
    };
    let base_cost = {
        let stored = *state.base_cost.lock().map_err(|e| e.to_string())?;
        if cfg.base_cost > 0.0 {
            cfg.base_cost
        } else {
            stored
        }
    };
    let mut ledger_day = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        ledger::day(&conn, &day, &symbol, cfg.base_qty, cfg.base_cost)?
    };
    let mark_price = match price.filter(|p| *p > 0.0) {
        Some(p) => p,
        None => {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            store::load_minutes(&conn, &symbol, &day)?
                .last()
                .map(|b| b.price)
                .unwrap_or(0.0)
        }
    };
    if mark_price > 0.0 {
        ledger::mark(&mut ledger_day, cfg.base_qty, base_cost, mark_price);
    }
    Ok(ledger_day)
}

#[tauri::command]
pub fn import_statement(
    state: tauri::State<'_, AppState>,
    day: String,
    symbol: String,
    file_name: String,
    bytes: Vec<u8>,
) -> Result<ImportResult, String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    statement::import(&mut conn, &day, &symbol, &file_name, &bytes)
}

pub(crate) fn fmt_day(d: &str) -> String {
    if d.len() == 8 {
        format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..])
    } else {
        d.to_string()
    }
}

/// 接受 "2026-10-19" 或 "20261019"，统一成数据库里的 YYYYMMDD。
pub(crate) fn day_key(raw: &str) -> String {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 8 {
        digits
    } else {
        raw.to_string()
    }
}

fn moving_avg(daily: &[DayBar], i: usize, n: usize) -> Option<f64> {
    if i + 1 >= n {
        let sum: f64 = daily[i + 1 - n..=i].iter().map(|x| x.c).sum();
        Some(sum / n as f64)
    } else {
        None
    }
}

fn build_kline(daily: &[DayBar]) -> Vec<serde_json::Value> {
    let start = daily.len().saturating_sub(60);
    (start..daily.len())
        .map(|i| {
            json!({
                "d": daily[i].d,
                "o": daily[i].o,
                "c": daily[i].c,
                "h": daily[i].h,
                "l": daily[i].l,
                "v": daily[i].v,
                "ma5": moving_avg(daily, i, 5),
                "ma10": moving_avg(daily, i, 10),
                "ma20": moving_avg(daily, i, 20)
            })
        })
        .collect()
}

pub(crate) fn stats_for(signals: &[Signal]) -> serde_json::Value {
    let mut veto: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
    for s in signals {
        for v in &s.veto {
            let key = v.split('（').next().unwrap_or(v.as_str()).to_string();
            *veto.entry(key).or_insert(0) += 1;
        }
    }
    let accepted = signals.iter().filter(|s| s.accepted).count();
    json!({
        "candidates": signals.len(),
        "accepted": accepted,
        "filled": 0, "wins": 0, "losses": 0, "winRate": 0.0,
        "avgNetPct": 0.0, "avgWin": 0.0, "avgLoss": 0.0,
        "totalNet": 0.0, "totalCost": 0.0, "totalGross": 0.0,
        "costRatio": null, "maxDd": 0.0,
        "vetoReasons": veto
    })
}

/// 启动时拉一次全量：快照 + 分时 + 日K，组装成前端直接可渲染的 DeskPayload。
#[tauri::command]
pub async fn bootstrap(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    let symbol = cfg.symbol.clone();
    let quote = market::fetch_quote(&state.client, &symbol).await?;
    let (day, raw) = market::fetch_minute(&state.client, &symbol).await?;
    if raw.is_empty() {
        return Err("分时数据为空（可能非交易日）".to_string());
    }

    {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        let _ = store::save_minutes(&mut conn, &symbol, &day, &raw);
    }

    let bars = features::build(&raw, quote.prev_close);
    {
        let mut cache = state.series.lock().map_err(|e| e.to_string())?;
        *cache = Some(SeriesCache {
            day: day.clone(),
            prev_close: quote.prev_close,
            bars: bars.clone(),
        });
    }

    let daily = market::fetch_daily(&state.client, &symbol, 120).await?;
    let kline = build_kline(&daily);

    let atr_pct = if daily.len() > 2 && quote.prev_close > 0.0 {
        features::atr(&daily[..daily.len() - 1], 14) / quote.prev_close
    } else {
        0.0
    };

    let base_cost = if cfg.base_cost > 0.0 {
        cfg.base_cost
    } else if daily.len() >= 20 {
        daily[daily.len() - 20..].iter().map(|d| d.c).sum::<f64>() / 20.0
    } else {
        quote.prev_close
    };
    *state.base_cost.lock().map_err(|e| e.to_string())? = base_cost;

    let day_fmt = fmt_day(&day);
    let mut signals = strategy::scan(&bars, &day_fmt, &cfg);
    let day_amount = raw.last().map(|b| b.cum_amt).unwrap_or(0.0);
    let risk_state = risk_filter(
        &state,
        &cfg,
        &symbol,
        &day_fmt,
        quote.price,
        day_amount,
        base_cost,
        &mut signals,
    );
    if state.emergency.load(Ordering::SeqCst) {
        for signal in signals.iter_mut() {
            signal.accepted = false;
            signal.veto.push("全局急停：禁止开仓".to_string());
        }
    }
    let health = health_for(&state);
    {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        let _ = store::save_signals(&mut conn, &day_fmt, &signals);
    }

    let last = match bars.last() {
        Some(b) => json!({
            "t": b.t, "regime": b.regime, "z": b.z,
            "price": b.price, "vwap": b.vwap, "slope": b.slope
        }),
        None => json!({
            "t": "--:--", "regime": "震荡", "z": 0.0,
            "price": quote.price, "vwap": quote.price, "slope": 0.0
        }),
    };

    let accepted_n = signals.iter().filter(|s| s.accepted).count();
    let conclusion = if accepted_n > 0 {
        format!(
            "今日 {} 个候选，{} 个通过，按纪律执行。",
            signals.len(),
            accepted_n
        )
    } else {
        format!(
            "今日 {} 个候选全部否决：回归空间不足以覆盖 {:.2}% 往返成本。",
            signals.len(),
            strategy::COST_PCT * 100.0
        )
    };
    let stats = stats_for(&signals);

    let code = symbol.get(2..).unwrap_or("").to_string();
    let market_code = symbol.get(..2).unwrap_or("").to_uppercase();

    Ok(json!({
        "meta": {
            "symbol": quote.name,
            "symbolId": symbol,
            "code": code,
            "market": market_code,
            "date": day_fmt,
            "prevClose": quote.prev_close,
            "open": quote.open,
            "high": quote.high,
            "low": quote.low,
            "close": quote.price,
            "atrPct": atr_pct,
            "generatedAt": session::now_cn().format("%Y-%m-%d %H:%M:%S").to_string(),
            "mode": "live",
            "conclusion": conclusion
        },
        "account": {
            "baseQty": cfg.base_qty,
            "baseCost": base_cost,
            "available": cfg.base_qty,
            "cash": cfg.cash,
            "lot": 200,
            "maxTRatio": cfg.max_t_ratio
        },
        "minutes": bars,
        "kline": kline,
        "signals": signals,
        "trades": [],
        "equity": [],
        "stats": stats,
        "risk": risk_state,
        "health": health,
        "scenarios": [],
        "sensitivity": [],
        "last": last
    }))
}

/// 取某日分时（带内存缓存与 SQLite 落盘）。day 为空时取当日。
#[tauri::command]
pub async fn series(
    state: tauri::State<'_, AppState>,
    day: Option<String>,
) -> Result<Vec<Bar>, String> {
    {
        let cache = state.series.lock().map_err(|e| e.to_string())?;
        if let Some(c) = cache.as_ref() {
            let hit = day.as_deref().map_or(true, |d| {
                d == c.day.as_str() || d == fmt_day(&c.day).as_str()
            });
            if hit {
                return Ok(c.bars.clone());
            }
        }
    }

    let symbol = state
        .config
        .lock()
        .map_err(|e| e.to_string())?
        .symbol
        .clone();
    let quote = market::fetch_quote(&state.client, &symbol).await?;
    let (d, raw) = market::fetch_minute(&state.client, &symbol).await?;
    if raw.is_empty() {
        return Err("分时数据为空".to_string());
    }

    {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        let _ = store::save_minutes(&mut conn, &symbol, &d, &raw);
    }

    let bars = features::build(&raw, quote.prev_close);
    {
        let mut cache = state.series.lock().map_err(|e| e.to_string())?;
        *cache = Some(SeriesCache {
            day: d,
            prev_close: quote.prev_close,
            bars: bars.clone(),
        });
    }
    Ok(bars)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryImportResult {
    pub symbol: String,
    pub days: i64,
    pub rows: i64,
    pub skipped: i64,
}

#[tauri::command]
pub fn import_minutes_csv(
    state: tauri::State<'_, AppState>,
    symbol: String,
    bytes: Vec<u8>,
) -> Result<HistoryImportResult, String> {
    let symbol = symbol.trim().to_lowercase();
    if symbol.len() != 8 {
        return Err("标的代码格式应为 sh600519 / sz000858".to_string());
    }
    let text = history::decode(&bytes);
    let parsed = history::parse_csv(&text)?;
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut days = 0i64;
    for (day, raw) in &parsed.days {
        store::save_minutes(&mut conn, &symbol, day, raw)?;
        days += 1;
    }
    Ok(HistoryImportResult {
        symbol,
        days,
        rows: parsed.rows,
        skipped: parsed.skipped,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MinuteDay {
    pub day: String,
    pub source: String,
}

#[tauri::command]
pub fn list_minute_days(
    state: tauri::State<'_, AppState>,
    symbol: String,
) -> Result<Vec<MinuteDay>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT trade_day, MAX(source) FROM minute_raw
             WHERE symbol = ?1 GROUP BY trade_day ORDER BY trade_day",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![symbol], |row| {
            Ok(MinuteDay {
                day: row.get(0)?,
                source: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleResult {
    pub symbol: String,
    pub days: i64,
    pub rows: i64,
    pub seed: u64,
}

/// 生成确定性合成数据，只用于演示回测/研究流程。
#[tauri::command]
pub fn generate_sample_data(
    state: tauri::State<'_, AppState>,
    symbol: String,
    days: Option<u32>,
    seed: Option<u64>,
) -> Result<SampleResult, String> {
    let symbol = symbol.trim().to_lowercase();
    if symbol.len() != 8 {
        return Err("标的代码格式应为 sh600519 / sz000858".to_string());
    }
    let cfg = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        config::load_for(&conn, &symbol)
    };
    let start_price = if cfg.base_cost > 1.0 {
        cfg.base_cost
    } else {
        72.0
    };
    let count = days.unwrap_or(10).clamp(1, 60) as usize;
    let seed = seed.unwrap_or(20261019);

    // 合成数据绝不覆盖真实数据：从最早的真实交易日前一个工作日起往前生成。
    let real_min: Option<String> = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT MIN(trade_day) FROM minute_raw WHERE symbol = ?1 AND source = 'real'",
            params![&symbol],
            |row| row.get::<_, String>(0),
        )
        .ok()
    };
    let end_day = match real_min {
        Some(day) => {
            use chrono::{Datelike, Duration as ChronoDuration, NaiveDate, Weekday};
            let base = NaiveDate::parse_from_str(&day, "%Y%m%d")
                .unwrap_or_else(|_| session::now_cn().date_naive());
            let mut cursor = base - ChronoDuration::days(1);
            while matches!(cursor.weekday(), Weekday::Sat | Weekday::Sun) {
                cursor -= ChronoDuration::days(1);
            }
            cursor.format("%Y-%m-%d").to_string()
        }
        None => session::now_cn().format("%Y-%m-%d").to_string(),
    };
    let data = sample::generate(count, start_price, seed, &end_day)?;

    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut rows = 0i64;
    for (day, bars) in &data {
        store::save_demo_minutes(&mut conn, &symbol, day, bars)?;
        rows += bars.len() as i64;
    }
    Ok(SampleResult {
        symbol,
        days: data.len() as i64,
        rows,
        seed,
    })
}

#[tauri::command]
pub fn clear_sample_data(
    state: tauri::State<'_, AppState>,
    symbol: String,
) -> Result<usize, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    store::clear_demo_minutes(&conn, &symbol)
}

/// 多日回测记分卡：按日期顺序读取缓存分钟，未缓存的日期会被跳过。
#[tauri::command]
pub fn backtest_range(
    state: tauri::State<'_, AppState>,
    symbol: String,
    days: Vec<String>,
) -> Result<backtest::Scorecard, String> {
    let cfg = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        config::load_for(&conn, &symbol)
    };
    let mut days: Vec<String> = days.iter().map(|d| day_key(d)).collect();
    days.sort();
    days.dedup();

    let mut results = Vec::new();
    let mut prev_close = 0.0f64;
    let mut loaded_days = 0i64;
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        for day in &days {
            let raw = store::load_minutes(&conn, &symbol, day)?;
            if raw.is_empty() {
                continue;
            }
            let pc = if prev_close > 0.0 {
                prev_close
            } else {
                raw.first().map(|b| b.price).unwrap_or(0.0)
            };
            let bars = features::build(&raw, pc);
            if let Some(last) = raw.last() {
                prev_close = last.price;
            }
            results.push(backtest::run_day(
                &symbol,
                day,
                &bars,
                &cfg,
                &backtest::FeeModel::default(),
                cfg.cash,
            ));
            loaded_days += 1;
        }
    }
    if results.is_empty() {
        return Err(format!("{symbol} 在给定日期范围内没有缓存分钟数据"));
    }
    let mut card = backtest::scorecard(&results);
    card.days = loaded_days;
    Ok(card)
}

#[tauri::command]
pub fn backtest_research(
    state: tauri::State<'_, AppState>,
    symbol: String,
    days: Vec<String>,
) -> Result<research::ResearchReport, String> {
    let cfg = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        config::load_for(&conn, &symbol)
    };
    let mut days: Vec<String> = days.iter().map(|d| day_key(d)).collect();
    days.sort();
    days.dedup();

    let mut day_bars = Vec::new();
    let mut prev_close = 0.0f64;
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        for day in &days {
            let raw = store::load_minutes(&conn, &symbol, day)?;
            if raw.is_empty() {
                continue;
            }
            let pc = if prev_close > 0.0 {
                prev_close
            } else {
                raw.first().map(|b| b.price).unwrap_or(0.0)
            };
            let bars = features::build(&raw, pc);
            if let Some(last) = raw.last() {
                prev_close = last.price;
            }
            day_bars.push(research::DayBars {
                day: day.clone(),
                bars,
            });
        }
    }
    if day_bars.is_empty() {
        return Err(format!("{symbol} 在给定日期范围内没有缓存分钟数据"));
    }
    let demo_days = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        store::demo_day_count(&conn, &symbol, &days)?
    };
    let mut report = research::research(&symbol, &day_bars, &cfg, &backtest::FeeModel::default());
    report.demo_days = demo_days;
    if demo_days > 0 {
        report.note = format!(
            "含 {demo_days} 天合成示例数据，仅用于流程演示，不能作为策略结论。{}",
            report.note
        );
    }
    Ok(report)
}

#[tauri::command]
pub fn journal_upsert(
    state: tauri::State<'_, AppState>,
    input: JournalInput,
) -> Result<JournalEntry, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    journal::upsert(&conn, &input)
}

#[tauri::command]
pub fn journal_list(
    state: tauri::State<'_, AppState>,
    day: String,
    symbol: String,
) -> Result<Vec<JournalEntry>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    journal::list(&conn, &day, &symbol)
}

#[tauri::command]
pub fn journal_delete(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    journal::delete(&conn, id)
}

#[tauri::command]
pub fn journal_stats(
    state: tauri::State<'_, AppState>,
    day: String,
    symbol: String,
) -> Result<JournalStats, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    journal::stats(&conn, &day, &symbol)
}

/// 风控过滤：阻塞时把当日已通过的信号改成否决，并返回风控状态。
pub fn risk_filter(
    state: &AppState,
    cfg: &StrategyConfig,
    symbol: &str,
    day: &str,
    price: f64,
    day_amount: f64,
    base_cost: f64,
    signals: &mut Vec<Signal>,
) -> RiskState {
    let ledger = state
        .db
        .lock()
        .ok()
        .and_then(|conn| ledger::day(&conn, day, symbol, cfg.base_qty, cfg.base_cost).ok());
    let cost = if base_cost > 0.0 { base_cost } else { price };
    let equity = cfg.cash + cfg.base_qty as f64 * cost;
    let risk = match ledger {
        Some(day) => risk::evaluate(cfg, &day, equity, day_amount, price, base_cost),
        None => RiskState {
            blocked: true,
            reasons: vec!["无法读取账本，风控降级为禁止开仓".to_string()],
            ..Default::default()
        },
    };
    if risk.blocked {
        for signal in signals.iter_mut().filter(|s| s.accepted) {
            signal.accepted = false;
            signal
                .veto
                .push(format!("风控禁止开仓：{}", risk.reasons.join("；")));
        }
    }
    risk
}

#[tauri::command]
pub fn risk_state(
    state: tauri::State<'_, AppState>,
    symbol: String,
    day: String,
    price: Option<f64>,
) -> Result<RiskState, String> {
    let day = day_key(&day);
    let cfg = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        config::load_for(&conn, &symbol)
    };
    let (last_price, day_amount) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let raw = store::load_minutes(&conn, &symbol, &day)?;
        let last = raw.last().map(|b| b.price).unwrap_or(0.0);
        let amount = raw.last().map(|b| b.cum_amt).unwrap_or(0.0);
        (last, amount)
    };
    let price = price.filter(|p| *p > 0.0).unwrap_or(last_price);
    let ledger_day = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        ledger::day(&conn, &day, &symbol, cfg.base_qty, cfg.base_cost)?
    };
    let base_cost = {
        let stored = *state.base_cost.lock().map_err(|e| e.to_string())?;
        if cfg.base_cost > 0.0 {
            cfg.base_cost
        } else if stored > 0.0 {
            stored
        } else {
            price
        }
    };
    let equity = cfg.cash + cfg.base_qty as f64 * base_cost;
    Ok(risk::evaluate(
        &cfg,
        &ledger_day,
        equity,
        day_amount,
        price,
        base_cost,
    ))
}

/// 手工回填/冲正/导入后，用当前缓存的分时重算信号与风控，避免顶栏状态过期。
#[tauri::command]
pub fn refresh_signals(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    let symbol = cfg.symbol.clone();
    let (day, bars) = {
        let cache = state.series.lock().map_err(|e| e.to_string())?;
        match cache.as_ref() {
            Some(c) => (c.day.clone(), c.bars.clone()),
            None => return Err("分时缓存为空，请先刷新行情".to_string()),
        }
    };
    let day_fmt = fmt_day(&day);
    let price = bars.last().map(|b| b.price).unwrap_or(0.0);
    let day_amount = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        store::load_minutes(&conn, &symbol, &day)?
            .last()
            .map(|b| b.cum_amt)
            .unwrap_or(0.0)
    };

    let mut signals = strategy::scan(&bars, &day_fmt, &cfg);
    let base_cost = *state.base_cost.lock().map_err(|e| e.to_string())?;
    let risk_state = risk_filter(
        &state,
        &cfg,
        &symbol,
        &day_fmt,
        price,
        day_amount,
        base_cost,
        &mut signals,
    );
    {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        let _ = store::save_signals(&mut conn, &day_fmt, &signals);
    }
    let stats = stats_for(&signals);
    let accepted = signals.iter().filter(|s| s.accepted).count();
    let conclusion = if accepted > 0 {
        format!(
            "今日 {} 个候选，{} 个通过，按纪律执行。",
            signals.len(),
            accepted
        )
    } else {
        format!(
            "今日 {} 个候选全部否决：回归空间不足以覆盖 {:.2}% 往返成本。",
            signals.len(),
            strategy::COST_PCT * 100.0
        )
    };
    Ok(json!({
        "signals": signals,
        "stats": stats,
        "conclusion": conclusion,
        "risk": risk_state
    }))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketHealth {
    pub phase: String,
    pub trading_day: bool,
    pub last_bar: String,
    pub age_secs: i64,
    pub stale: bool,
    pub healthy: bool,
    pub emergency: bool,
    pub source: String,
    pub message: String,
}

fn minute_of(t: &str) -> Option<i64> {
    let mut it = t.split(':');
    let h: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    Some(h * 60 + m)
}

pub fn health_for(state: &AppState) -> MarketHealth {
    use chrono::{Datelike, Timelike};
    let now = session::now_cn();
    let phase = session::phase(&now).to_string();
    let (last_bar, age_secs) = {
        let cache = state.series.lock().ok();
        match cache.as_ref().and_then(|c| c.as_ref()) {
            Some(c) => {
                let bar = c.bars.last().map(|b| b.t.clone()).unwrap_or_default();
                let today = now.format("%Y%m%d").to_string();
                let age = if c.day == today {
                    minute_of(&bar)
                        .map(|m| ((now.hour() as i64 * 60 + now.minute() as i64) - m) * 60)
                        .map(|v| v.max(0))
                        .unwrap_or(-1)
                } else {
                    -1
                };
                (bar, age)
            }
            None => (String::new(), -1),
        }
    };
    let emergency = state.emergency.load(Ordering::SeqCst);
    let stale = phase == "交易" && (age_secs < 0 || age_secs > 90);
    let healthy = !emergency && !stale;
    let message = if emergency {
        "全局急停".to_string()
    } else if stale {
        format!("行情断流 {age_secs}s")
    } else {
        match phase.as_str() {
            "交易" => "行情正常".to_string(),
            "午休" => "午休".to_string(),
            "盘前" => "盘前".to_string(),
            "收盘" => "收盘".to_string(),
            _ => "休市".to_string(),
        }
    };
    MarketHealth {
        phase,
        trading_day: session::is_trading_day(&now),
        last_bar,
        age_secs,
        stale,
        healthy,
        emergency,
        source: state
            .feed
            .lock()
            .map(|f| f.source.clone())
            .unwrap_or_else(|_| "unknown".to_string()),
        message,
    }
}

#[tauri::command]
pub fn market_health(state: tauri::State<'_, AppState>) -> MarketHealth {
    health_for(&state)
}

#[tauri::command]
pub fn set_emergency_stop(state: tauri::State<'_, AppState>, active: bool) -> bool {
    state.emergency.store(active, Ordering::SeqCst);
    active
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrecheckInput {
    pub symbol: String,
    pub day: String,
    pub side: String,
    pub price: f64,
    pub qty: i64,
    #[serde(default)]
    pub signal_id: Option<String>,
    #[serde(default)]
    pub signal_t: Option<String>,
    #[serde(default)]
    pub signal_price: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrecheckCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrecheckResult {
    pub ok: bool,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub checks: Vec<PrecheckCheck>,
}

fn price_limit_ratio(symbol: &str) -> f64 {
    if symbol.starts_with("sz30") || symbol.starts_with("sh688") {
        0.2
    } else if symbol.starts_with("bj") {
        0.3
    } else {
        0.1
    }
}

#[tauri::command]
pub fn precheck_order(
    state: tauri::State<'_, AppState>,
    input: PrecheckInput,
) -> Result<PrecheckResult, String> {
    let cfg = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        config::load_for(&conn, &input.symbol)
    };
    let (cache_day, bars, prev_close) = {
        let cache = state.series.lock().map_err(|e| e.to_string())?;
        match cache.as_ref() {
            Some(c) => (c.day.clone(), c.bars.clone(), c.prev_close),
            None => return Err("分时缓存为空，请先刷新行情".to_string()),
        }
    };
    let day = if input.day.trim().is_empty() {
        cache_day
    } else {
        day_key(&input.day)
    };
    let health = health_for(&state);
    let base_cost = {
        let stored = *state.base_cost.lock().map_err(|e| e.to_string())?;
        if cfg.base_cost > 0.0 {
            cfg.base_cost
        } else if stored > 0.0 {
            stored
        } else {
            input.price
        }
    };
    let ledger_day = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        ledger::day(&conn, &day, &input.symbol, cfg.base_qty, cfg.base_cost)?
    };
    let day_amount = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        store::load_minutes(&conn, &input.symbol, &day)?
            .last()
            .map(|b| b.cum_amt)
            .unwrap_or(0.0)
    };
    let risk = risk::evaluate(
        &cfg,
        &ledger_day,
        cfg.cash + cfg.base_qty as f64 * base_cost,
        day_amount,
        input.price,
        base_cost,
    );

    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    let mut push = |name: &str, ok: bool, detail: String| {
        if !ok {
            blockers.push(format!("{name}：{detail}"));
        }
        checks.push(PrecheckCheck {
            name: name.to_string(),
            ok,
            detail,
        });
    };

    push(
        "全局急停",
        !health.emergency,
        if health.emergency {
            "已开启，禁止开仓".to_string()
        } else {
            "未开启".to_string()
        },
    );
    push(
        "交易时段",
        health.phase == "交易",
        format!("当前 {}", health.phase),
    );
    push(
        "行情新鲜度",
        !health.stale,
        format!("最后分钟 {}，延迟 {}s", health.last_bar, health.age_secs),
    );
    push(
        "价格合法",
        input.price > 0.0,
        format!("限价 {:.2}", input.price),
    );
    push(
        "数量整手",
        input.qty > 0 && input.qty % 100 == 0,
        format!("{} 股", input.qty),
    );

    let side = input.side.to_uppercase();
    let ratio = price_limit_ratio(&input.symbol);
    let up = (prev_close * (1.0 + ratio) * 100.0).round() / 100.0;
    let down = (prev_close * (1.0 - ratio) * 100.0).round() / 100.0;
    let limit_ok = if side == "BUY" {
        input.price < up - 0.001
    } else {
        input.price > down + 0.001
    };
    push(
        if side == "BUY" {
            "涨停限制"
        } else {
            "跌停限制"
        },
        limit_ok,
        format!("昨收 {:.2}，涨停 {:.2}，跌停 {:.2}", prev_close, up, down),
    );
    push(
        "停牌/无成交",
        day_amount > 0.0,
        format!("当日成交额 {:.0}", day_amount),
    );

    if side == "BUY" {
        let need = input.price * input.qty as f64;
        push(
            "可用资金",
            cfg.cash >= need,
            format!("需要 {:.2}，可用 {:.2}", need, cfg.cash),
        );
    } else {
        push(
            "T+1 可用数量",
            ledger_day.position.available >= input.qty,
            format!("需要 {}，可用 {}", input.qty, ledger_day.position.available),
        );
    }

    push(
        "风控",
        !risk.blocked,
        if risk.blocked {
            risk.reasons.join("；")
        } else {
            format!("日亏 {}，连亏 {}", risk.daily_pnl, risk.consecutive_losses)
        },
    );

    if let Some(t) = input.signal_t.as_deref() {
        let latest = bars.last().map(|b| b.t.as_str()).unwrap_or("");
        let age = match (minute_of(t), minute_of(latest)) {
            (Some(a), Some(b)) => b - a,
            _ => 0,
        };
        push(
            "信号时效",
            age <= 3,
            format!("信号 {}，最新 {}，约 {} 分钟", t, latest, age),
        );
    }

    if let (Some(sp), Some(id)) = (input.signal_price, input.signal_id.as_deref()) {
        if sp > 0.0 {
            let dev = (input.price - sp).abs() / sp;
            if dev > 0.005 {
                blockers.push(format!("价格偏离信号价 {:.2}%（>0.5%）", dev * 100.0));
            } else if dev > 0.002 {
                warnings.push(format!("价格偏离信号价 {:.2}%", dev * 100.0));
            }
            checks.push(PrecheckCheck {
                name: "偏离信号价".to_string(),
                ok: dev <= 0.005,
                detail: format!("信号 {}，偏离 {:.2}%", id, dev * 100.0),
            });
        }
    }

    Ok(PrecheckResult {
        ok: blockers.is_empty(),
        blockers,
        warnings,
        checks,
    })
}

#[tauri::command]
pub fn audit_report(state: tauri::State<'_, AppState>) -> Result<Vec<AuditIssue>, String> {
    Ok(state.audit.lock().map_err(|e| e.to_string())?.clone())
}

/// 单日回测：读取本地缓存的分钟数据，按当前标的参数跑一遍。
#[tauri::command]
pub fn backtest_day(
    state: tauri::State<'_, AppState>,
    symbol: String,
    day: String,
    prev_close: Option<f64>,
) -> Result<backtest::BacktestResult, String> {
    let day = day_key(&day);
    let (cfg, raw) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let cfg = config::load_for(&conn, &symbol);
        let raw = store::load_minutes(&conn, &symbol, &day)?;
        (cfg, raw)
    };
    if raw.is_empty() {
        return Err(format!("{symbol} {day} 没有历史分钟数据，请先导入/缓存"));
    }
    let pc = prev_close.unwrap_or(0.0);
    let prev_close = if pc > 0.0 { pc } else { raw[0].price };
    let bars = features::build(&raw, prev_close);
    Ok(backtest::run_day(
        &symbol,
        &day,
        &bars,
        &cfg,
        &backtest::FeeModel::default(),
        cfg.cash,
    ))
}

#[tauri::command]
pub fn get_config(state: tauri::State<'_, AppState>) -> Result<StrategyConfig, String> {
    Ok(state.config.lock().map_err(|e| e.to_string())?.clone())
}

#[tauri::command]
pub fn test_alert(app: tauri::AppHandle) -> Result<(), String> {
    let payload = json!({
        "symbol": "test",
        "name": "测试提醒",
        "accepted": 1,
        "side": "正T",
        "t": "10:30",
        "target": 0.0,
        "at": crate::session::now_cn().format("%H:%M:%S").to_string(),
        "test": true
    });
    app.emit("signals:alert", payload)
        .map_err(|e| format!("发送测试提醒失败: {e}"))
}

#[tauri::command]
pub fn test_notification(app: tauri::AppHandle) -> Result<(), String> {
    app.notification()
        .builder()
        .title("做T信号 · 测试")
        .body("系统通知可用。真实信号通过时会这样提醒。")
        .show()
        .map_err(|e| format!("发送通知失败: {e}"))
}

#[tauri::command]
pub fn update_config(
    state: tauri::State<'_, AppState>,
    next: StrategyConfig,
) -> Result<StrategyConfig, String> {
    config::validate(&next)?;
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        config::save(&conn, &next)?;
    }
    *state.config.lock().map_err(|e| e.to_string())? = next.clone();
    *state.symbol.lock().map_err(|e| e.to_string())? = next.symbol.clone();
    *state.series.lock().map_err(|e| e.to_string())? = None;
    Ok(next)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchItem {
    pub symbol: String,
    pub name: String,
    pub price: f64,
    pub change_pct: f64,
    pub base_qty: i64,
    pub paused: bool,
}

fn activate(state: &AppState, symbol: &str) -> Result<StrategyConfig, String> {
    let cfg = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        if !config::exists(&conn, symbol)? {
            return Err("该标的尚未加入自选".to_string());
        }
        config::load_for(&conn, symbol)
    };
    *state.config.lock().map_err(|e| e.to_string())? = cfg.clone();
    *state.symbol.lock().map_err(|e| e.to_string())? = cfg.symbol.clone();
    *state.series.lock().map_err(|e| e.to_string())? = None;
    Ok(cfg)
}

#[tauri::command]
pub fn set_symbol(
    state: tauri::State<'_, AppState>,
    symbol: String,
) -> Result<StrategyConfig, String> {
    activate(&state, symbol.trim())
}

#[tauri::command]
pub fn add_symbol(
    state: tauri::State<'_, AppState>,
    symbol: String,
) -> Result<StrategyConfig, String> {
    let sym = symbol.trim().to_lowercase();
    let mut probe = StrategyConfig::default();
    probe.symbol = sym.clone();
    config::validate(&probe)?;
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        if !config::exists(&conn, &sym)? {
            config::save(&conn, &probe)?;
        }
    }
    activate(&state, &sym)
}

#[tauri::command]
pub fn remove_symbol(state: tauri::State<'_, AppState>, symbol: String) -> Result<(), String> {
    let active = state
        .config
        .lock()
        .map_err(|e| e.to_string())?
        .symbol
        .clone();
    let remaining = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let symbols = config::list(&conn)?;
        if symbols.len() <= 1 {
            return Err("至少保留一只自选".to_string());
        }
        conn.execute(
            "DELETE FROM symbol_configs WHERE symbol = ?1",
            params![symbol.trim()],
        )
        .map_err(|e| format!("删除自选失败: {e}"))?;
        config::list(&conn)?
    };
    if active == symbol.trim() {
        if let Some(next) = remaining.first() {
            activate(&state, next)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn list_watchlist(state: tauri::State<'_, AppState>) -> Result<Vec<WatchItem>, String> {
    let (symbols, meta) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let symbols = config::list(&conn)?;
        let mut meta = Vec::new();
        for symbol in &symbols {
            let cfg = config::load_for(&conn, symbol);
            meta.push((symbol.clone(), cfg.base_qty, cfg.paused));
        }
        (symbols, meta)
    };

    let quotes = market::fetch_quotes(&state.client, &symbols)
        .await
        .unwrap_or_default();
    let map: HashMap<String, market::Quote> =
        quotes.into_iter().map(|q| (q.symbol.clone(), q)).collect();

    Ok(meta
        .into_iter()
        .map(|(symbol, base_qty, paused)| {
            let quote = map.get(&symbol);
            WatchItem {
                symbol,
                name: quote.map(|q| q.name.clone()).unwrap_or_default(),
                price: quote.map(|q| q.price).unwrap_or(0.0),
                change_pct: quote
                    .map(|q| {
                        if q.prev_close > 0.0 {
                            q.price / q.prev_close - 1.0
                        } else {
                            0.0
                        }
                    })
                    .unwrap_or(0.0),
                base_qty,
                paused,
            }
        })
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchScan {
    pub symbol: String,
    pub name: String,
    pub price: f64,
    pub change_pct: f64,
    pub regime: String,
    pub candidates: i64,
    pub accepted: i64,
    pub paused: bool,
    pub signal_side: Option<String>,
    pub signal_t: Option<String>,
    pub signal_target: Option<f64>,
    pub cached: bool,
    pub stale: bool,
}

const SCAN_CONCURRENCY: usize = 4;

fn backoff_delay(failures: u32) -> Duration {
    let exp = failures.clamp(1, 5) - 1;
    let secs = 30u64.saturating_mul(1u64 << exp).min(300);
    Duration::from_secs(secs)
}

/// 报价时间截断到分钟：同一分钟内分时不会变，可直接复用缓存。
fn minute_key(quote: Option<&market::Quote>) -> String {
    quote
        .map(|q| {
            let end = q.time.len().min(12);
            q.time[..end].to_string()
        })
        .unwrap_or_default()
}

/// 批量扫描自选：每只票独立参数；缓存命中直接复用，未命中并发拉取。
#[tauri::command]
pub async fn scan_watchlist(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<WatchScan>, String> {
    let (symbols, configs) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let symbols = config::list(&conn)?;
        let mut configs: HashMap<String, StrategyConfig> = HashMap::new();
        for symbol in &symbols {
            configs.insert(symbol.clone(), config::load_for(&conn, symbol));
        }
        (symbols, configs)
    };

    let quotes = market::fetch_quotes(&state.client, &symbols)
        .await
        .unwrap_or_default();
    let map: HashMap<String, market::Quote> =
        quotes.into_iter().map(|q| (q.symbol.clone(), q)).collect();

    let now = Instant::now();
    let mut bars_by_symbol: HashMap<String, Vec<Bar>> = HashMap::new();
    let mut cached_symbols: HashMap<String, bool> = HashMap::new();
    let mut stale_symbols: HashMap<String, bool> = HashMap::new();
    let mut misses: Vec<String> = Vec::new();

    {
        let cache = state.scan_cache.lock().map_err(|e| e.to_string())?;
        let fail = state.scan_fail.lock().map_err(|e| e.to_string())?;
        for symbol in &symbols {
            let key = minute_key(map.get(symbol));
            let hit = !key.is_empty()
                && cache
                    .get(symbol)
                    .map(|entry| entry.minute_key == key && !entry.bars.is_empty())
                    .unwrap_or(false);
            if hit {
                if let Some(entry) = cache.get(symbol) {
                    bars_by_symbol.insert(symbol.clone(), entry.bars.clone());
                }
                cached_symbols.insert(symbol.clone(), true);
                continue;
            }

            // 退避期内不发请求；有旧缓存就降级展示。
            if let Some(scan_fail) = fail.get(symbol) {
                if scan_fail.next_retry > now {
                    stale_symbols.insert(symbol.clone(), true);
                    if let Some(entry) = cache.get(symbol) {
                        bars_by_symbol.insert(symbol.clone(), entry.bars.clone());
                        cached_symbols.insert(symbol.clone(), true);
                    }
                    continue;
                }
            }
            misses.push(symbol.clone());
        }
    }

    let client = state.client.clone();
    let fetches = misses.into_iter().map(|symbol| {
        let client = client.clone();
        async move {
            let result = market::fetch_minute(&client, &symbol).await;
            (symbol, result)
        }
    });
    let fetched: Vec<_> = stream::iter(fetches)
        .buffer_unordered(SCAN_CONCURRENCY)
        .collect()
        .await;

    {
        let mut cache = state.scan_cache.lock().map_err(|e| e.to_string())?;
        let mut fail = state.scan_fail.lock().map_err(|e| e.to_string())?;
        for (symbol, result) in fetched {
            let ok = match result {
                Ok((_day, raw)) if !raw.is_empty() => {
                    let prev_close = map.get(&symbol).map(|q| q.prev_close).unwrap_or(0.0);
                    let bars = features::build(&raw, prev_close);
                    let key = minute_key(map.get(&symbol));
                    cache.insert(
                        symbol.clone(),
                        ScanCacheEntry {
                            minute_key: key,
                            bars: bars.clone(),
                        },
                    );
                    bars_by_symbol.insert(symbol.clone(), bars);
                    true
                }
                _ => false,
            };

            if ok {
                fail.remove(&symbol);
                cached_symbols.insert(symbol, false);
            } else {
                let entry = fail.entry(symbol.clone()).or_insert(ScanFail {
                    failures: 0,
                    next_retry: now,
                });
                entry.failures += 1;
                entry.next_retry = now + backoff_delay(entry.failures);
                stale_symbols.insert(symbol.clone(), true);
                if let Some(old) = cache.get(&symbol) {
                    bars_by_symbol.insert(symbol.clone(), old.bars.clone());
                    cached_symbols.insert(symbol.clone(), true);
                } else {
                    cached_symbols.insert(symbol.clone(), false);
                }
            }
        }
    }

    let mut out = Vec::new();
    for symbol in &symbols {
        let cfg = configs.get(symbol).cloned().unwrap_or_default();
        let quote = map.get(symbol);
        let bars = bars_by_symbol.get(symbol);
        let mut regime = "震荡".to_string();
        let mut candidates = 0i64;
        let mut accepted = 0i64;
        let mut signal_side = None;
        let mut signal_t = None;
        let mut signal_target = None;
        let mut cached = cached_symbols.get(symbol).copied().unwrap_or(false);

        if cfg.paused {
            cached = false;
        } else if let Some(bars) = bars {
            if !bars.is_empty() {
                if let Some(last) = bars.last() {
                    regime = last.regime.clone();
                }
                let signals = strategy::scan(bars, "live", &cfg);
                candidates = signals.len() as i64;
                accepted = signals.iter().filter(|s| s.accepted).count() as i64;
                if let Some(sig) = signals.iter().rev().find(|s| s.accepted) {
                    signal_side = Some(sig.side.clone());
                    signal_t = Some(sig.t.clone());
                    signal_target = Some(sig.target);
                }
            }
        }

        out.push(WatchScan {
            symbol: symbol.clone(),
            name: quote.map(|q| q.name.clone()).unwrap_or_default(),
            price: quote.map(|q| q.price).unwrap_or(0.0),
            change_pct: quote
                .map(|q| {
                    if q.prev_close > 0.0 {
                        q.price / q.prev_close - 1.0
                    } else {
                        0.0
                    }
                })
                .unwrap_or(0.0),
            regime,
            candidates,
            accepted,
            paused: cfg.paused,
            signal_side,
            signal_t,
            signal_target,
            cached,
            stale: stale_symbols.get(symbol).copied().unwrap_or(false),
        });
    }

    // 只在“通过数增加”时发系统通知；首轮只建立基线，避免启动即轰炸。
    {
        let mut last = state.last_alert.lock().map_err(|e| e.to_string())?;
        if !last.is_empty() {
            for item in &out {
                let prev = last.get(&item.symbol).copied().unwrap_or(0);
                if item.paused || item.accepted <= prev {
                    continue;
                }
                let label = if item.name.is_empty() {
                    item.symbol.clone()
                } else {
                    item.name.clone()
                };
                let side = item.signal_side.clone().unwrap_or_default();
                let t = item.signal_t.clone().unwrap_or_default();
                let body = match item.signal_target {
                    Some(target) => format!(
                        "{label} {} 个信号 {side} {t} 目标 {target:.2}",
                        item.accepted
                    ),
                    None => format!("{label} {} 个信号 {side} {t}", item.accepted),
                };
                let _ = app
                    .notification()
                    .builder()
                    .title("做T信号")
                    .body(body)
                    .show();
                let payload = json!({
                    "symbol": item.symbol,
                    "name": label,
                    "accepted": item.accepted,
                    "side": side,
                    "t": t,
                    "target": item.signal_target,
                    "at": crate::session::now_cn().format("%H:%M:%S").to_string(),
                    "test": false
                });
                let _ = app.emit("signals:alert", payload);
                if let Some(tray) = app.tray_by_id("main") {
                    let _ =
                        tray.set_tooltip(Some(format!("做T信号 · {label} {} 个", item.accepted)));
                }
            }
        }
        last.clear();
        for item in &out {
            last.insert(item.symbol.clone(), item.accepted);
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{backoff_delay, minute_key};
    use crate::market::Quote;
    use std::time::Duration;

    fn quote(time: &str) -> Quote {
        Quote {
            symbol: "sh600519".to_string(),
            name: "贵州茅台".to_string(),
            price: 1300.0,
            prev_close: 1300.0,
            open: 1300.0,
            high: 1300.0,
            low: 1300.0,
            time: time.to_string(),
        }
    }

    #[test]
    fn minute_key_truncates_to_minute() {
        assert_eq!(minute_key(Some(&quote("20261019150003"))), "202610191500");
        assert_eq!(minute_key(None), "");
    }

    #[test]
    fn backoff_grows_then_caps() {
        assert_eq!(backoff_delay(1), Duration::from_secs(30));
        assert_eq!(backoff_delay(2), Duration::from_secs(60));
        assert_eq!(backoff_delay(3), Duration::from_secs(120));
        assert_eq!(backoff_delay(4), Duration::from_secs(240));
        assert_eq!(backoff_delay(5), Duration::from_secs(300));
        assert_eq!(backoff_delay(9), Duration::from_secs(300));
        assert_eq!(backoff_delay(0), Duration::from_secs(30));
    }
}
