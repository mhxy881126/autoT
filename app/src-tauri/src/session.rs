use chrono::{DateTime, Datelike, FixedOffset, Timelike, Utc, Weekday};

/// 所有交易日判断统一走 Asia/Shanghai，不依赖系统时区。
pub fn now_cn() -> DateTime<FixedOffset> {
    let offset = FixedOffset::east_opt(8 * 3600).expect("UTC+8");
    Utc::now().with_timezone(&offset)
}

pub fn is_trading_day(dt: &DateTime<FixedOffset>) -> bool {
    !matches!(dt.weekday(), Weekday::Sat | Weekday::Sun)
}

pub fn is_trading_time(dt: &DateTime<FixedOffset>) -> bool {
    if !is_trading_day(dt) {
        return false;
    }
    let minutes = dt.hour() * 60 + dt.minute();
    // 09:30-11:30, 13:00-15:00
    (570..=690).contains(&minutes) || (780..=900).contains(&minutes)
}
