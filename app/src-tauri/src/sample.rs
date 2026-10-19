//! 合成示例数据：只用于演示回测/研究流程，不是真实行情。
//! 确定性生成：同一 seed 必须得到同一份数据。

use chrono::{Datelike, Duration, NaiveDate, Weekday};

use crate::market::RawBar;

pub fn generate(
    days: usize,
    start_price: f64,
    seed: u64,
    end_day: &str,
) -> Result<Vec<(String, Vec<RawBar>)>, String> {
    if days == 0 || days > 60 {
        return Err("示例天数应在 1–60 之间".to_string());
    }
    if start_price <= 0.0 {
        return Err("起始价格必须大于 0".to_string());
    }
    let end = NaiveDate::parse_from_str(end_day, "%Y-%m-%d")
        .map_err(|e| format!("结束日期格式应为 YYYY-MM-DD: {e}"))?;

    let mut dates = Vec::new();
    let mut cursor = end;
    while dates.len() < days {
        if !matches!(cursor.weekday(), Weekday::Sat | Weekday::Sun) {
            dates.push(cursor);
        }
        cursor -= Duration::days(1);
    }
    dates.reverse();

    let mut rng = Rng::new(seed);
    let mut prev_close = start_price;
    let mut out = Vec::new();

    for date in dates {
        let day = date.format("%Y%m%d").to_string();
        let times = minute_times();
        let gap = rng.gauss() * 0.003;
        let mut price = prev_close * (1.0 + gap);
        let anchor = price;
        let mut cum_vol = 0i64;
        let mut cum_amt = 0.0f64;
        let mut bars = Vec::with_capacity(times.len());

        for (i, t) in times.iter().enumerate() {
            let u = i as f64 / (times.len().max(2) - 1) as f64;
            let vol_factor = 0.7 + 1.6 * (u - 0.5).powi(2) * 4.0;
            let vol = (400.0 * vol_factor * (0.6 + rng.next_f64() * 0.9)).round() as i64;
            let vol = vol.max(1);

            let shock = rng.gauss() * 0.0009;
            let revert = (anchor - price) * 0.015;
            price = (price + revert + shock * price).max(0.5);
            let clamp = prev_close * 1.05;
            let floor = prev_close * 0.95;
            price = price.clamp(floor, clamp);
            price = (price * 100.0).round() / 100.0;

            cum_vol += vol;
            cum_amt += price * vol as f64 * 100.0;
            bars.push(RawBar {
                t: t.clone(),
                price,
                cum_vol,
                cum_amt,
            });
        }

        prev_close = bars.last().map(|b| b.price).unwrap_or(prev_close);
        out.push((day, bars));
    }

    Ok(out)
}

fn minute_times() -> Vec<String> {
    let mut out = Vec::with_capacity(242);
    for m in 0..=120 {
        out.push(fmt_time(9 * 60 + 30 + m));
    }
    for m in 0..=120 {
        out.push(fmt_time(13 * 60 + m));
    }
    out
}

fn fmt_time(minutes: i32) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn gauss(&mut self) -> f64 {
        let sum: f64 = (0..12).map(|_| self.next_f64()).sum();
        sum - 6.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_ten_days_of_242_bars() {
        let data = generate(10, 72.0, 42, "2026-10-19").expect("generate");
        assert_eq!(data.len(), 10);
        for (day, bars) in &data {
            assert_eq!(day.len(), 8);
            assert_eq!(bars.len(), 242);
            assert_eq!(bars[0].t, "09:30");
            assert_eq!(bars[120].t, "11:30");
            assert_eq!(bars[121].t, "13:00");
            assert_eq!(bars[241].t, "15:00");
            assert!(bars.iter().all(|b| b.price > 0.0));
            for pair in bars.windows(2) {
                assert!(pair[1].cum_vol >= pair[0].cum_vol);
                assert!(pair[1].cum_amt >= pair[0].cum_amt);
            }
        }
    }

    #[test]
    fn deterministic_for_same_seed() {
        let a = generate(3, 50.0, 7, "2026-10-19").unwrap();
        let b = generate(3, 50.0, 7, "2026-10-19").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn different_seed_changes_prices() {
        let a = generate(2, 50.0, 1, "2026-10-19").unwrap();
        let b = generate(2, 50.0, 2, "2026-10-19").unwrap();
        assert_ne!(a[0].1[10].price, b[0].1[10].price);
    }
}
