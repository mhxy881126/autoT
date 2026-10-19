use serde::Serialize;

use crate::features::Bar;

/// 往返成本：佣金双边 + 印花税 + 过户费双边 + 滑点双边。
pub const COST_PCT: f64 = 0.00025 * 2.0 + 0.0005 + 0.00001 * 2.0 + 0.0002 * 2.0;

const Z_ENTRY: f64 = 1.2;
const COOLDOWN_MIN: usize = 15;
const MAX_SIGNALS: usize = 4;
const TARGET_LO: f64 = 0.0045;
const TARGET_HI: f64 = 0.0090;
const STOP_PCT: f64 = 0.0035;
const MIN_NET: f64 = 0.0010;
const LOT: i64 = 200;
const MAX_T_RATIO: f64 = 0.5;
const START: &str = "09:45";
const END: &str = "14:50";
const VOL_CAP: f64 = 3.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Signal {
    pub id: String,
    pub day: String,
    pub t: String,
    pub side: String,
    pub price: f64,
    pub entry: f64,
    pub target: f64,
    pub stop: f64,
    pub qty: i64,
    pub gross: f64,
    pub net: f64,
    pub conf: i64,
    pub regime: String,
    pub z: f64,
    pub accepted: bool,
    pub veto: Vec<String>,
    pub why: Vec<String>,
}

/// 与 Python 原型 `strategy.scan` 对齐：先过 regime 门，再过成本门。
pub fn scan(bars: &[Bar], day: &str, base_qty: i64) -> Vec<Signal> {
    let mut out: Vec<Signal> = Vec::new();
    let mut last_i: isize = -999;
    let mut accepted_count = 0usize;

    for i in 1..bars.len() {
        let bar = &bars[i];
        let prev = &bars[i - 1];

        if bar.t.as_str() < START || bar.t.as_str() > END {
            continue;
        }
        if (i as isize - last_i) < COOLDOWN_MIN as isize {
            continue;
        }

        let side = if prev.z > -Z_ENTRY && bar.z <= -Z_ENTRY {
            "正T"
        } else if prev.z < Z_ENTRY && bar.z >= Z_ENTRY {
            "倒T"
        } else {
            continue;
        };

        last_i = i as isize;

        let price = bar.price;
        let mut veto: Vec<String> = Vec::new();

        if bar.regime == "极端" {
            veto.push("极端行情（接近涨跌停/大幅异动）".to_string());
        } else if side == "正T" && bar.regime == "下行" {
            veto.push("下行趋势禁止正T（防止越买越套）".to_string());
        } else if side == "倒T" && bar.regime == "上行" {
            veto.push("上行趋势禁止倒T（防止卖飞）".to_string());
        }

        let raw_gross = if side == "正T" {
            bar.vwap / price - 1.0
        } else {
            1.0 - bar.vwap / price
        };

        if raw_gross < TARGET_LO {
            veto.push(format!(
                "回归空间不足（{:.2}% < {:.2}%）",
                raw_gross * 100.0,
                TARGET_LO * 100.0
            ));
        }

        let gross = raw_gross.max(0.0).min(TARGET_HI);
        let target = if side == "正T" {
            price * (1.0 + gross)
        } else {
            price * (1.0 - gross)
        };
        let stop = if side == "正T" {
            price * (1.0 - STOP_PCT)
        } else {
            price * (1.0 + STOP_PCT)
        };
        let net = gross - COST_PCT;

        if net < MIN_NET {
            veto.push(format!("扣成本后无优势（净 {:.2}%）", net * 100.0));
        }
        if bar.vol_ratio > VOL_CAP {
            veto.push(format!("放量异动（量比 {:.1}）", bar.vol_ratio));
        }
        if accepted_count >= MAX_SIGNALS {
            veto.push("今日做T次数已满".to_string());
        }

        let qty = ((base_qty as f64 * MAX_T_RATIO).min(LOT as f64) as i64) / 100 * 100;
        if qty <= 0 {
            veto.push("可做数量不足".to_string());
        }

        let accepted = veto.is_empty();
        let conf = if accepted {
            let raw = 52.0
                + bar.z.abs() * 9.0
                + if bar.regime == "震荡" { 6.0 } else { 0.0 }
                + if bar.vol_ratio < 1.2 { 5.0 } else { 0.0 };
            (raw as i64).min(92)
        } else {
            0
        };

        if accepted {
            accepted_count += 1;
        }

        out.push(Signal {
            id: format!("S{:02}", out.len() + 1),
            day: day.to_string(),
            t: bar.t.clone(),
            side: side.to_string(),
            price,
            entry: (price * 100.0).round() / 100.0,
            target: (target * 100.0).round() / 100.0,
            stop: (stop * 100.0).round() / 100.0,
            qty,
            gross,
            net,
            conf,
            regime: bar.regime.clone(),
            z: bar.z,
            accepted,
            veto,
            why: vec![
                format!("偏离滚动VWAP {:+.2}σ", bar.z),
                format!("量比 {:.2}", bar.vol_ratio),
                format!("regime={}", bar.regime),
                format!("斜率 {:+.3}%/min", bar.slope),
            ],
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(t: &str, price: f64, vwap: f64, z: f64, regime: &str) -> Bar {
        Bar {
            t: t.to_string(),
            price,
            avg: vwap,
            vwap,
            dev: 0.0,
            sigma: 0.001,
            z,
            slope: 0.0,
            vol: 100,
            vol_ratio: 1.0,
            high: price,
            low: price,
            day_ret: 0.0,
            amp: 0.0,
            regime: regime.to_string(),
        }
    }

    #[test]
    fn accepts_reversion_in_range() {
        let bars = vec![
            bar("09:50", 100.0, 100.0, 0.5, "震荡"),
            bar("09:51", 99.4, 100.0, -1.5, "震荡"),
        ];
        let signals = scan(&bars, "2026-10-19", 1000);
        assert_eq!(signals.len(), 1);
        let s = &signals[0];
        assert!(s.accepted);
        assert_eq!(s.side, "正T");
        assert!(s.target > s.entry);
        assert!(s.stop < s.entry);
        assert!(s.qty <= 500);
        assert!(s.net > 0.0);
    }

    #[test]
    fn blocks_buy_in_downtrend() {
        let bars = vec![
            bar("09:50", 100.0, 100.0, 0.5, "下行"),
            bar("09:51", 99.4, 100.0, -1.5, "下行"),
        ];
        let signals = scan(&bars, "2026-10-19", 1000);
        assert_eq!(signals.len(), 1);
        assert!(!signals[0].accepted);
        assert!(signals[0].veto.iter().any(|v| v.contains("下行")));
    }

    #[test]
    fn rejects_thin_spread() {
        let bars = vec![
            bar("09:50", 100.0, 100.0, 0.5, "震荡"),
            bar("09:51", 99.8, 100.0, -1.5, "震荡"),
        ];
        let signals = scan(&bars, "2026-10-19", 1000);
        assert!(!signals[0].accepted);
        assert!(signals[0].veto.iter().any(|v| v.contains("回归空间不足")));
    }

    #[test]
    fn cooldown_skips_clustered_crossings() {
        let mut bars = vec![bar("09:50", 100.0, 100.0, 0.5, "震荡")];
        bars.push(bar("09:51", 99.4, 100.0, -1.5, "震荡"));
        bars.push(bar("09:52", 100.0, 100.0, 0.6, "震荡"));
        bars.push(bar("09:53", 99.4, 100.0, -1.5, "震荡"));
        let signals = scan(&bars, "2026-10-19", 1000);
        assert_eq!(signals.len(), 1);
    }
}
