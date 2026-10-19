use std::collections::HashMap;
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
use crate::ledger::{self, Fill, FillInput, LedgerDay};
use crate::market::{self, DayBar, Quote};
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
) -> Result<LedgerDay, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let cfg = config::load_for(&conn, &symbol);
    ledger::day(&conn, &day, &symbol, cfg.base_qty, cfg.base_cost)
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

    let day_fmt = fmt_day(&day);
    let signals = strategy::scan(&bars, &day_fmt, &cfg);
    {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        let _ = store::save_signals(&mut conn, &day_fmt, &signals);
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

#[tauri::command]
pub fn list_minute_days(
    state: tauri::State<'_, AppState>,
    symbol: String,
) -> Result<Vec<String>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT DISTINCT trade_day FROM minute_raw WHERE symbol = ?1 ORDER BY trade_day")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![symbol], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
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
