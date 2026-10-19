use serde::Serialize;
use serde_json::json;

use crate::features::{self, Bar};
use crate::ledger::{self, Fill, FillInput, LedgerDay};
use crate::market::{self, DayBar, Quote};
use crate::session;
use crate::state::{AppState, FeedStatus, SeriesCache};
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
pub fn list_fills(state: tauri::State<'_, AppState>, day: String) -> Result<Vec<Fill>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    ledger::list_by_day(&conn, &day)
}

#[tauri::command]
pub fn ledger_day(state: tauri::State<'_, AppState>, day: String) -> Result<LedgerDay, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    ledger::day(&conn, &day, 1000, 0.0)
}

pub(crate) fn fmt_day(d: &str) -> String {
    if d.len() == 8 {
        format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..])
    } else {
        d.to_string()
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
    let symbol = state.symbol.lock().map_err(|e| e.to_string())?.clone();
    let quote = market::fetch_quote(&state.client, &symbol).await?;
    let (day, raw) = market::fetch_minute(&state.client, &symbol).await?;
    if raw.is_empty() {
        return Err("分时数据为空（可能非交易日）".to_string());
    }

    {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        let _ = store::save_minutes(&mut conn, &day, &raw);
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
    let signals = strategy::scan(&bars, &day_fmt, 1000);
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

    let base_cost = if daily.len() >= 20 {
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
            "baseQty": 1000,
            "baseCost": base_cost,
            "available": 1000,
            "cash": 800000.0,
            "lot": 200,
            "maxTRatio": 0.5
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

    let symbol = state.symbol.lock().map_err(|e| e.to_string())?.clone();
    let quote = market::fetch_quote(&state.client, &symbol).await?;
    let (d, raw) = market::fetch_minute(&state.client, &symbol).await?;
    if raw.is_empty() {
        return Err("分时数据为空".to_string());
    }

    {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        let _ = store::save_minutes(&mut conn, &d, &raw);
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
