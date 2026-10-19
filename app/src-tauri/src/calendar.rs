//! A 股交易日历（静态快照）。
//! 数据来源：exchange_calendars XSHG，覆盖 2025-01-01 ~ 2026-12-31。
//! 覆盖范围外退化为“周一至周五开市”，并在健康提示里标注未覆盖。

use chrono::{Datelike, Duration, NaiveDate, Weekday};

/// 覆盖范围内的交易日休市日（仅工作日，周末天然休市）。
const HOLIDAYS: &[&str] = &[
    // 2025
    "2025-01-01",
    "2025-01-28",
    "2025-01-29",
    "2025-01-30",
    "2025-01-31",
    "2025-02-03",
    "2025-02-04",
    "2025-04-04",
    "2025-05-01",
    "2025-05-02",
    "2025-05-05",
    "2025-06-02",
    "2025-10-01",
    "2025-10-02",
    "2025-10-03",
    "2025-10-06",
    "2025-10-07",
    "2025-10-08",
    // 2026
    "2026-01-01",
    "2026-01-02",
    "2026-02-16",
    "2026-02-17",
    "2026-02-18",
    "2026-02-19",
    "2026-02-20",
    "2026-02-23",
    "2026-04-06",
    "2026-05-01",
    "2026-05-04",
    "2026-05-05",
    "2026-06-19",
    "2026-09-25",
    "2026-10-01",
    "2026-10-02",
    "2026-10-05",
    "2026-10-06",
    "2026-10-07",
];

pub const COVERAGE_START: &str = "2025-01-01";
pub const COVERAGE_END: &str = "2026-12-31";

pub fn is_covered(day: NaiveDate) -> bool {
    let start = NaiveDate::parse_from_str(COVERAGE_START, "%Y-%m-%d").expect("start");
    let end = NaiveDate::parse_from_str(COVERAGE_END, "%Y-%m-%d").expect("end");
    day >= start && day <= end
}

pub fn is_trading_day(day: NaiveDate) -> bool {
    if matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
        return false;
    }
    if !is_covered(day) {
        return true;
    }
    let key = day.format("%Y-%m-%d").to_string();
    !HOLIDAYS.contains(&key.as_str())
}

/// 今天如果是交易日就返回今天，否则返回下一个交易日。
pub fn current_or_next(day: NaiveDate) -> NaiveDate {
    let mut cursor = day;
    for _ in 0..15 {
        if is_trading_day(cursor) {
            return cursor;
        }
        cursor += Duration::days(1);
    }
    cursor
}

/// 下一个交易日（严格大于 day）。
pub fn next_trading_day(day: NaiveDate) -> NaiveDate {
    current_or_next(day + Duration::days(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn national_day_is_closed() {
        assert!(!is_trading_day(date("2026-10-01")));
        assert!(!is_trading_day(date("2026-10-07")));
        assert!(is_trading_day(date("2026-10-08")));
    }

    #[test]
    fn spring_festival_is_closed() {
        assert!(!is_trading_day(date("2026-02-17")));
        assert!(is_trading_day(date("2026-02-24")));
    }

    #[test]
    fn next_session_skips_holiday_week() {
        assert_eq!(next_trading_day(date("2026-09-30")), date("2026-10-08"));
        assert_eq!(current_or_next(date("2026-10-02")), date("2026-10-08"));
    }

    #[test]
    fn weekend_is_closed() {
        assert!(!is_trading_day(date("2026-10-17")));
    }
}
