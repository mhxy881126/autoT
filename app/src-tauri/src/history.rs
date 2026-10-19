//! 历史分钟数据导入。
//!
//! 目前先支持通用 CSV（券商/数据商导出后整理成统一列）：
//! `day,time,price,cum_vol,cum_amt`
//! 例如：`20261019,09:30,1324.00,1200,158880000`
//!
//! 约定：cum_vol 为累计手数，cum_amt 为累计成交额（元）。导入后按天覆盖写入 minute_raw。

use std::collections::BTreeMap;

use crate::market::RawBar;

#[derive(Debug, Clone)]
pub struct ParsedHistory {
    pub days: Vec<(String, Vec<RawBar>)>,
    pub rows: i64,
    pub skipped: i64,
}

pub fn parse_csv(text: &str) -> Result<ParsedHistory, String> {
    let mut days: BTreeMap<String, BTreeMap<String, RawBar>> = BTreeMap::new();
    let mut rows = 0i64;
    let mut skipped = 0i64;
    let mut first = true;

    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }
        if first {
            first = false;
            let lower = line.to_lowercase();
            if lower.contains("time") || line.contains("时间") || line.contains("日期") {
                continue;
            }
        }

        let cols: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if cols.len() < 5 {
            skipped += 1;
            continue;
        }

        let Some(day) = normalize_day(cols[0]) else {
            skipped += 1;
            continue;
        };
        let Some(t) = normalize_time(cols[1]) else {
            skipped += 1;
            continue;
        };
        let Ok(price) = cols[2].parse::<f64>() else {
            skipped += 1;
            continue;
        };
        let Ok(cum_vol) = cols[3].parse::<i64>() else {
            skipped += 1;
            continue;
        };
        let Ok(cum_amt) = cols[4].parse::<f64>() else {
            skipped += 1;
            continue;
        };
        if price <= 0.0 || cum_vol < 0 || cum_amt < 0.0 {
            skipped += 1;
            continue;
        }

        rows += 1;
        days.entry(day).or_default().insert(
            t.clone(),
            RawBar {
                t,
                price,
                cum_vol,
                cum_amt,
            },
        );
    }

    let mut out = Vec::new();
    for (day, map) in days {
        let bars: Vec<RawBar> = map.into_values().collect();
        let mut prev_vol = 0i64;
        let mut prev_amt = 0.0f64;
        for bar in &bars {
            if bar.cum_vol < prev_vol || bar.cum_amt + 1e-6 < prev_amt {
                return Err(format!("{day} 累计成交量/成交额不是单调递增，无法用于回测"));
            }
            prev_vol = bar.cum_vol;
            prev_amt = bar.cum_amt;
        }
        out.push((day, bars));
    }

    if out.is_empty() {
        return Err("没有解析到有效分钟数据".to_string());
    }

    Ok(ParsedHistory {
        days: out,
        rows,
        skipped,
    })
}

fn normalize_day(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != 8 {
        return None;
    }
    Some(digits)
}

fn normalize_time(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    let (h, m) = match digits.len() {
        3 => (
            digits[0..1].parse::<u32>().ok()?,
            digits[1..3].parse::<u32>().ok()?,
        ),
        4 => (
            digits[0..2].parse::<u32>().ok()?,
            digits[2..4].parse::<u32>().ok()?,
        ),
        _ => return None,
    };
    if h >= 24 || m >= 60 {
        return None;
    }
    Some(format!("{h:02}:{m:02}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_two_days_and_sorts() {
        let csv = "\u{feff}day,time,price,cum_vol,cum_amt\n\
                   2026-10-19,09:31,10.10,200,202000\n\
                   2026-10-19,09:30,10.00,100,100000\n\
                   20261020,09:30,10.20,100,102000\n";
        let parsed = parse_csv(csv).expect("parse");
        assert_eq!(parsed.days.len(), 2);
        assert_eq!(parsed.rows, 3);
        assert_eq!(parsed.skipped, 0);
        assert_eq!(parsed.days[0].0, "20261019");
        assert_eq!(parsed.days[0].1[0].t, "09:30");
        assert_eq!(parsed.days[0].1[1].t, "09:31");
        assert_eq!(parsed.days[1].0, "20261020");
    }

    #[test]
    fn skips_bad_rows_but_keeps_good_ones() {
        let csv = "20261019,09:30,abc,100,100000\n\
                   20261019,09:31,10.10,200,202000\n";
        let parsed = parse_csv(csv).expect("parse");
        assert_eq!(parsed.rows, 1);
        assert_eq!(parsed.skipped, 1);
    }

    #[test]
    fn rejects_non_monotonic_cumulative() {
        let csv = "20261019,09:30,10.00,200,200000\n\
                   20261019,09:31,10.10,100,101000\n";
        assert!(parse_csv(csv).is_err());
    }

    #[test]
    fn normalizes_time_formats() {
        assert_eq!(normalize_time("930").as_deref(), Some("09:30"));
        assert_eq!(normalize_time("0930").as_deref(), Some("09:30"));
        assert_eq!(normalize_time("9:30").as_deref(), Some("09:30"));
        assert_eq!(normalize_time("25:00"), None);
    }
}
