use chrono::{DateTime, Datelike, FixedOffset, Timelike, Utc};

use crate::calendar;

/// 所有交易日判断统一走 Asia/Shanghai，不依赖系统时区。
pub fn now_cn() -> DateTime<FixedOffset> {
    let offset = FixedOffset::east_opt(8 * 3600).expect("UTC+8");
    Utc::now().with_timezone(&offset)
}

pub fn is_trading_day(dt: &DateTime<FixedOffset>) -> bool {
    calendar::is_trading_day(dt.date_naive())
}

pub fn is_trading_time(dt: &DateTime<FixedOffset>) -> bool {
    phase(dt) == "交易"
}

pub fn phase(dt: &DateTime<FixedOffset>) -> &'static str {
    if !is_trading_day(dt) {
        return "休市";
    }
    let minutes = dt.hour() * 60 + dt.minute();
    if minutes < 570 {
        "盘前"
    } else if (570..=690).contains(&minutes) {
        "交易"
    } else if (690..780).contains(&minutes) {
        "午休"
    } else if (780..=900).contains(&minutes) {
        "交易"
    } else {
        "收盘"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32, m: u32) -> DateTime<FixedOffset> {
        let offset = FixedOffset::east_opt(8 * 3600).unwrap();
        offset
            .with_ymd_and_hms(2026, 10, 19, h, m, 0)
            .single()
            .unwrap()
    }

    #[test]
    fn phases_cover_the_session() {
        assert_eq!(phase(&at(9, 0)), "盘前");
        assert_eq!(phase(&at(10, 0)), "交易");
        assert_eq!(phase(&at(12, 0)), "午休");
        assert_eq!(phase(&at(14, 0)), "交易");
        assert_eq!(phase(&at(15, 30)), "收盘");
    }

    #[test]
    fn weekend_is_closed() {
        let offset = FixedOffset::east_opt(8 * 3600).unwrap();
        let sat = offset
            .with_ymd_and_hms(2026, 10, 17, 10, 0, 0)
            .single()
            .unwrap();
        assert_eq!(phase(&sat), "休市");
        assert!(!is_trading_time(&sat));
    }
}
