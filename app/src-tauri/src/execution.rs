//! 执行质量：信号价到成交价的滑点，以及持仓期间 MAE/MFE。

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::ledger::{self, Fill, TradePair};
use crate::store;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlippageRow {
    pub fill_id: i64,
    pub signal_id: String,
    pub side: String,
    pub signal_price: f64,
    pub fill_price: f64,
    pub qty: i64,
    pub bps: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlippageStats {
    pub count: i64,
    pub avg_bps: f64,
    pub worst_bps: f64,
    pub rows: Vec<SlippageRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairQuality {
    pub pair_id: i64,
    pub side: String,
    pub qty: i64,
    pub open_t: String,
    pub close_t: String,
    pub open_price: f64,
    pub close_price: f64,
    pub net: f64,
    pub mfe_pct: f64,
    pub mae_pct: f64,
    pub capture_pct: f64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionQuality {
    pub day: String,
    pub symbol: String,
    pub slippage: SlippageStats,
    pub pairs: Vec<PairQuality>,
    pub avg_mfe_pct: f64,
    pub avg_mae_pct: f64,
}

fn signal_price(conn: &Connection, day: &str, id: &str) -> Option<f64> {
    conn.query_row(
        "SELECT price_cents FROM signals WHERE trade_day = ?1 AND id = ?2",
        params![day, id],
        |row| row.get::<_, i64>(0),
    )
    .ok()
    .map(|c| c as f64 / 100.0)
}

fn excursions(side: &str, open: f64, prices: &[f64]) -> (f64, f64) {
    let mut mfe = f64::MIN;
    let mut mae = f64::MAX;
    for price in prices {
        let move_ = if side == "正T" {
            price - open
        } else {
            open - price
        };
        mfe = mfe.max(move_);
        mae = mae.min(move_);
    }
    if prices.is_empty() {
        (0.0, 0.0)
    } else {
        (mfe, mae)
    }
}

fn ohlc_excursions(side: &str, open: f64, bars: &[crate::market::MinuteOhlc]) -> (f64, f64) {
    let mut mfe = f64::MIN;
    let mut mae = f64::MAX;
    for bar in bars {
        let (fav, adv) = if side == "正T" {
            (bar.high - open, bar.low - open)
        } else {
            (open - bar.low, open - bar.high)
        };
        mfe = mfe.max(fav);
        mae = mae.min(adv);
    }
    if bars.is_empty() {
        (0.0, 0.0)
    } else {
        (mfe, mae)
    }
}

fn fill_by_id<'a>(fills: &'a [Fill], id: i64) -> Option<&'a Fill> {
    fills.iter().find(|f| f.id == id && !f.voided)
}

fn pair_quality(
    pair: &TradePair,
    fills: &[Fill],
    ohlc: &[crate::market::MinuteOhlc],
    closes: &[crate::market::RawBar],
) -> Option<PairQuality> {
    let open_fill = fill_by_id(fills, pair.open_fill_id)?;
    let close_id = pair.close_fill_id?;
    let close_fill = fill_by_id(fills, close_id)?;
    let ohlc_bars: Vec<crate::market::MinuteOhlc> = ohlc
        .iter()
        .filter(|b| b.t >= open_fill.t && b.t <= close_fill.t)
        .cloned()
        .collect();
    let close_prices: Vec<f64> = closes
        .iter()
        .filter(|b| b.t >= open_fill.t && b.t <= close_fill.t)
        .map(|b| b.price)
        .collect();
    let (mfe, mae, source) = if ohlc_bars.is_empty() {
        let (mfe, mae) = excursions(&pair.side, open_fill.price, &close_prices);
        (mfe, mae, "CLOSE")
    } else {
        let (mfe, mae) = ohlc_excursions(&pair.side, open_fill.price, &ohlc_bars);
        (mfe, mae, "OHLC")
    };
    let direction = if pair.side == "正T" { 1.0 } else { -1.0 };
    let gross_per_share = direction * (close_fill.price - open_fill.price);
    let capture = if mfe > 0.0 {
        (gross_per_share / mfe * 100.0).clamp(-999.0, 999.0)
    } else {
        0.0
    };
    Some(PairQuality {
        pair_id: pair.id,
        side: pair.side.clone(),
        qty: pair.qty,
        open_t: open_fill.t.clone(),
        close_t: close_fill.t.clone(),
        open_price: open_fill.price,
        close_price: close_fill.price,
        net: pair.net.unwrap_or(0.0),
        mfe_pct: if open_fill.price > 0.0 {
            mfe / open_fill.price * 100.0
        } else {
            0.0
        },
        mae_pct: if open_fill.price > 0.0 {
            mae / open_fill.price * 100.0
        } else {
            0.0
        },
        capture_pct: capture,
        source: source.to_string(),
    })
}

pub fn compute(conn: &Connection, day: &str, symbol: &str) -> Result<ExecutionQuality, String> {
    let ledger_day = ledger::day(conn, day, symbol, 0, 0.0)?;
    let pairs = ledger_day.pairs;
    let fills = ledger_day.fills;

    let mut slip_rows = Vec::new();
    for fill in fills.iter().filter(|f| !f.voided) {
        let Some(signal_id) = fill.signal_id.as_deref() else {
            continue;
        };
        let Some(sig_price) = signal_price(conn, day, signal_id) else {
            continue;
        };
        let direction = if fill.side == "BUY" { 1.0 } else { -1.0 };
        let bps = if sig_price > 0.0 {
            direction * (fill.price - sig_price) / sig_price * 10_000.0
        } else {
            0.0
        };
        slip_rows.push(SlippageRow {
            fill_id: fill.id,
            signal_id: signal_id.to_string(),
            side: fill.side.clone(),
            signal_price: sig_price,
            fill_price: fill.price,
            qty: fill.qty,
            bps: (bps * 100.0).round() / 100.0,
        });
    }
    slip_rows.sort_by(|a, b| {
        b.bps
            .partial_cmp(&a.bps)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let count = slip_rows.len() as i64;
    let avg_bps = if count > 0 {
        slip_rows.iter().map(|r| r.bps).sum::<f64>() / count as f64
    } else {
        0.0
    };
    let worst_bps = slip_rows.first().map(|r| r.bps).unwrap_or(0.0);

    let key: String = day.chars().filter(|c| c.is_ascii_digit()).collect();
    let bars = store::load_minutes(conn, symbol, &key).unwrap_or_default();
    let ohlc = store::load_ohlc(conn, symbol, &key).unwrap_or_default();
    let mut pair_rows = Vec::new();
    for pair in pairs.iter().filter(|p| p.status == "CLOSED") {
        if let Some(row) = pair_quality(pair, &fills, &ohlc, &bars) {
            pair_rows.push(row);
        }
    }
    let avg_mfe = if pair_rows.is_empty() {
        0.0
    } else {
        pair_rows.iter().map(|p| p.mfe_pct).sum::<f64>() / pair_rows.len() as f64
    };
    let avg_mae = if pair_rows.is_empty() {
        0.0
    } else {
        pair_rows.iter().map(|p| p.mae_pct).sum::<f64>() / pair_rows.len() as f64
    };

    Ok(ExecutionQuality {
        day: day.to_string(),
        symbol: symbol.to_string(),
        slippage: SlippageStats {
            count,
            avg_bps: (avg_bps * 100.0).round() / 100.0,
            worst_bps,
            rows: slip_rows,
        },
        pairs: pair_rows,
        avg_mfe_pct: (avg_mfe * 100.0).round() / 100.0,
        avg_mae_pct: (avg_mae * 100.0).round() / 100.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excursions_for_long_and_short() {
        let (mfe, mae) = excursions("正T", 100.0, &[99.0, 101.5, 100.2]);
        assert!((mfe - 1.5).abs() < 1e-9);
        assert!((mae + 1.0).abs() < 1e-9);
        let (mfe, mae) = excursions("倒T", 100.0, &[101.0, 98.5, 99.8]);
        assert!((mfe - 1.5).abs() < 1e-9);
        assert!((mae + 1.0).abs() < 1e-9);
    }

    #[test]
    fn capture_is_capped_and_safe() {
        let q = pair_quality(
            &TradePair {
                id: 1,
                day: "2026-10-19".to_string(),
                symbol: "sz000858".to_string(),
                side: "正T".to_string(),
                open_fill_id: 1,
                close_fill_id: Some(2),
                qty: 100,
                gross: Some(50.0),
                cost: Some(10.0),
                net: Some(40.0),
                status: "CLOSED".to_string(),
            },
            &[
                Fill {
                    id: 1,
                    client_id: "a".to_string(),
                    symbol: "sz000858".to_string(),
                    day: "2026-10-19".to_string(),
                    t: "09:40".to_string(),
                    side: "BUY".to_string(),
                    price: 100.0,
                    qty: 100,
                    fee: 0.0,
                    signal_id: None,
                    source: "manual".to_string(),
                    note: None,
                    created_at: "x".to_string(),
                    voided: false,
                },
                Fill {
                    id: 2,
                    client_id: "b".to_string(),
                    symbol: "sz000858".to_string(),
                    day: "2026-10-19".to_string(),
                    t: "10:10".to_string(),
                    side: "SELL".to_string(),
                    price: 100.5,
                    qty: 100,
                    fee: 0.0,
                    signal_id: None,
                    source: "manual".to_string(),
                    note: None,
                    created_at: "x".to_string(),
                    voided: false,
                },
            ],
            &[],
            &[crate::market::RawBar {
                t: "10:00".to_string(),
                price: 101.0,
                cum_vol: 1,
                cum_amt: 101.0,
            }],
        )
        .expect("pair");
        assert!((q.mfe_pct - 1.0).abs() < 1e-9);
        assert!((q.mae_pct - 1.0).abs() < 1e-9);
        assert!((q.capture_pct - 50.0).abs() < 1e-9);
        assert_eq!(q.source, "CLOSE");
    }

    #[test]
    fn ohlc_uses_high_low() {
        let bars = vec![crate::market::MinuteOhlc {
            t: "10:00".to_string(),
            open: 100.0,
            high: 101.5,
            low: 99.0,
            close: 100.2,
        }];
        let (mfe, mae) = ohlc_excursions("正T", 100.0, &bars);
        assert!((mfe - 1.5).abs() < 1e-9);
        assert!((mae + 1.0).abs() < 1e-9);
        let (mfe, mae) = ohlc_excursions("倒T", 100.0, &bars);
        assert!((mfe - 1.0).abs() < 1e-9);
        assert!((mae + 1.5).abs() < 1e-9);
    }
}
