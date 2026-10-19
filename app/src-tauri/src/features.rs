use serde::Serialize;

use crate::market::{DayBar, RawBar};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bar {
    pub t: String,
    pub price: f64,
    pub avg: f64,
    pub vwap: f64,
    pub dev: f64,
    pub sigma: f64,
    pub z: f64,
    pub slope: f64,
    pub vol: i64,
    pub vol_ratio: f64,
    pub high: f64,
    pub low: f64,
    pub day_ret: f64,
    pub amp: f64,
    pub regime: String,
}

fn pstdev(xs: &[f64]) -> f64 {
    let n = xs.len() as f64;
    if n == 0.0 {
        return 0.0;
    }
    let mean = xs.iter().sum::<f64>() / n;
    (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt()
}

fn ols_slope(ys: &[f64]) -> f64 {
    let n = ys.len();
    if n < 2 {
        return 0.0;
    }
    let nf = n as f64;
    let mx = (nf - 1.0) / 2.0;
    let my = ys.iter().sum::<f64>() / nf;
    let den: f64 = (0..n).map(|i| (i as f64 - mx).powi(2)).sum();
    if den == 0.0 {
        return 0.0;
    }
    (0..n).map(|i| (i as f64 - mx) * (ys[i] - my)).sum::<f64>() / den
}

fn classify(day_ret: f64, slope: f64, z: f64) -> &'static str {
    if day_ret.abs() >= 0.07 {
        return "极端";
    }
    if slope >= 0.020 && z >= 0.2 {
        return "上行";
    }
    if slope <= -0.020 && z <= -0.2 {
        return "下行";
    }
    "震荡"
}

/// 与 Python 原型逐字段对齐。窗口：VWAP 20 分钟，sigma 30 分钟，斜率 10 分钟。
pub fn build(raw: &[RawBar], prev_close: f64) -> Vec<Bar> {
    let mut bars: Vec<Bar> = Vec::with_capacity(raw.len());
    let mut prices: Vec<f64> = Vec::new();
    let mut devs: Vec<f64> = Vec::new();
    let mut vols: Vec<f64> = Vec::new();
    let mut amts: Vec<f64> = Vec::new();

    let mut cum_v: i64 = 0;
    let mut cum_a: f64 = 0.0;
    let mut day_h = f64::MIN;
    let mut day_l = f64::MAX;

    for m in raw {
        let vol = (m.cum_vol - cum_v).max(0) as f64;
        let amt = (m.cum_amt - cum_a).max(0.0);
        cum_v = m.cum_vol;
        cum_a = m.cum_amt;

        let avg = if cum_v > 0 {
            cum_a / (cum_v as f64 * 100.0)
        } else {
            m.price
        };

        prices.push(m.price);
        vols.push(vol);
        amts.push(amt);
        if vols.len() > 20 {
            vols.remove(0);
            amts.remove(0);
        }

        let total_v: f64 = vols.iter().sum();
        let vwap = if total_v > 0.0 {
            amts.iter().sum::<f64>() / (total_v * 100.0)
        } else {
            avg
        };

        let dev = if vwap > 0.0 {
            m.price / vwap - 1.0
        } else {
            0.0
        };
        devs.push(dev);
        if devs.len() > 30 {
            devs.remove(0);
        }
        let sigma = if devs.len() >= 5 {
            pstdev(&devs).max(0.0008)
        } else {
            0.0008
        };
        let z = dev / sigma;

        let slope = if prices.len() >= 10 {
            ols_slope(&prices[prices.len() - 10..]) / m.price * 100.0
        } else {
            0.0
        };

        let base_len = vols.len().saturating_sub(1);
        let base_vol = if base_len > 0 {
            vols[..base_len].iter().sum::<f64>() / base_len as f64
        } else {
            vol
        };
        let vol_ratio = if base_vol > 0.0 { vol / base_vol } else { 1.0 };

        day_h = day_h.max(m.price);
        day_l = day_l.min(m.price);

        bars.push(Bar {
            t: m.t.clone(),
            price: m.price,
            avg,
            vwap,
            dev,
            sigma,
            z,
            slope,
            vol: vol as i64,
            vol_ratio,
            high: day_h,
            low: day_l,
            day_ret: if prev_close > 0.0 {
                m.price / prev_close - 1.0
            } else {
                0.0
            },
            amp: if prev_close > 0.0 {
                (day_h - day_l) / prev_close
            } else {
                0.0
            },
            regime: String::new(),
        });
    }

    for b in bars.iter_mut() {
        b.regime = classify(b.day_ret, b.slope, b.z).to_string();
    }
    smooth_regime(&mut bars, 3);
    bars
}

/// 状态机去抖：新状态连续出现 min_run 分钟才切换，避免门控在边界横跳。
fn smooth_regime(bars: &mut [Bar], min_run: usize) {
    if bars.is_empty() {
        return;
    }
    let mut current = bars[0].regime.clone();
    let mut candidate: Option<String> = None;
    let mut run = 0usize;

    for bar in bars.iter_mut() {
        if bar.regime == current {
            candidate = None;
            run = 0;
        } else {
            if candidate.as_deref() == Some(bar.regime.as_str()) {
                run += 1;
            } else {
                candidate = Some(bar.regime.clone());
                run = 1;
            }
            if run >= min_run {
                if let Some(next) = candidate.clone() {
                    current = next;
                }
                candidate = None;
                run = 0;
            }
        }
        bar.regime = current.clone();
    }
}

pub fn atr(daily: &[DayBar], n: usize) -> f64 {
    if daily.len() < 2 {
        return 0.0;
    }
    let mut trs: Vec<f64> = Vec::new();
    for i in 1..daily.len() {
        let cur = &daily[i];
        let prev = &daily[i - 1];
        let tr = (cur.h - cur.l)
            .max((cur.h - prev.c).abs())
            .max((cur.l - prev.c).abs());
        trs.push(tr);
    }
    let take = trs.len().min(n.max(1));
    let slice = &trs[trs.len() - take..];
    slice.iter().sum::<f64>() / take as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::RawBar;

    fn raw(t: &str, price: f64, cum_vol: i64, cum_amt: f64) -> RawBar {
        RawBar {
            t: t.to_string(),
            price,
            cum_vol,
            cum_amt,
        }
    }

    #[test]
    fn avg_uses_cumulative_amount() {
        let bars = build(
            &[
                raw("09:30", 100.0, 10, 100_000.0),
                raw("09:31", 101.0, 30, 301_000.0),
            ],
            100.0,
        );
        assert!((bars[0].avg - 100.0).abs() < 1e-9);
        assert!((bars[1].avg - 301_000.0 / 3000.0).abs() < 1e-9);
        assert_eq!(bars[1].vol, 20);
    }

    #[test]
    fn regime_smoothing_ignores_blip() {
        let mut bars: Vec<Bar> = (0..11)
            .map(|i| Bar {
                t: format!("09:{i:02}"),
                price: 100.0,
                avg: 100.0,
                vwap: 100.0,
                dev: 0.0,
                sigma: 0.001,
                z: 0.0,
                slope: 0.0,
                vol: 1,
                vol_ratio: 1.0,
                high: 100.0,
                low: 100.0,
                day_ret: 0.0,
                amp: 0.0,
                regime: if i == 5 {
                    "上行".into()
                } else {
                    "震荡".into()
                },
            })
            .collect();
        smooth_regime(&mut bars, 3);
        assert!(bars.iter().all(|b| b.regime == "震荡"));
    }
}
