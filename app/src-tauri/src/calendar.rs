//! A 股交易日历（静态快照）。
//! 数据来源：exchange_calendars XSHG（2025–2026）+ 2027 预排。
//! 2027 年国务院/交易所安排尚未发布，2027 为按农历与近年放假规律预排，
//! 健康状态会标注“2027 预排”，发布后需要替换。

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
    // 2027 预排（官方安排未发布）
    "2027-01-01",
    "2027-02-05",
    "2027-02-08",
    "2027-02-09",
    "2027-02-10",
    "2027-02-11",
    "2027-02-12",
    "2027-04-05",
    "2027-05-03",
    "2027-05-04",
    "2027-05-05",
    "2027-06-09",
    "2027-06-10",
    "2027-06-11",
    "2027-09-15",
    "2027-09-16",
    "2027-09-17",
    "2027-10-01",
    "2027-10-04",
    "2027-10-05",
    "2027-10-06",
    "2027-10-07",
];

pub const COVERAGE_START: &str = "2025-01-01";
pub const COVERAGE_END: &str = "2027-12-31";

pub fn is_covered(day: NaiveDate) -> bool {
    let start = NaiveDate::parse_from_str(COVERAGE_START, "%Y-%m-%d").expect("start");
    let end = NaiveDate::parse_from_str(COVERAGE_END, "%Y-%m-%d").expect("end");
    day >= start && day <= end
}

/// 2027 为预排表，不是交易所正式公告。
pub fn is_provisional(day: NaiveDate) -> bool {
    day.year() == 2027 && is_covered(day)
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

    #[test]
    fn provisional_2027_is_covered_and_marked() {
        assert!(is_covered(date("2027-06-01")));
        assert!(is_provisional(date("2027-06-01")));
        assert!(!is_provisional(date("2026-06-01")));
        assert!(!is_trading_day(date("2027-02-08")));
        assert!(!is_trading_day(date("2027-10-01")));
        assert!(is_trading_day(date("2027-10-08")));
    }
}
