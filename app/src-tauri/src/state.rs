use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use rusqlite::Connection;

use crate::audit::AuditIssue;
use crate::config::StrategyConfig;
use crate::features::Bar;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedStatus {
    pub connected: bool,
    pub source: String,
    pub latency_ms: u64,
    pub last_error: Option<String>,
}

#[derive(Clone)]
pub struct SeriesCache {
    pub day: String,
    pub prev_close: f64,
    pub bars: Vec<Bar>,
}

#[derive(Clone)]
pub struct ScanCacheEntry {
    /// 报价时间截断到分钟（YYYYMMDDHHMM）。同一分钟内复用分时。
    pub minute_key: String,
    pub bars: Vec<Bar>,
}

#[derive(Clone, Copy)]
pub struct ScanFail {
    pub failures: u32,
    pub next_retry: Instant,
}

pub struct AppState {
    pub client: reqwest::Client,
    pub symbol: Mutex<String>,
    pub config: Mutex<StrategyConfig>,
    pub feed: Mutex<FeedStatus>,
    pub db: Mutex<Connection>,
    pub series: Mutex<Option<SeriesCache>>,
    pub scan_cache: Mutex<HashMap<String, ScanCacheEntry>>,
    pub scan_fail: Mutex<HashMap<String, ScanFail>>,
    /// 当前标的的有效底仓成本（bootstrap 后写入，用于浮动盈亏）。
    pub base_cost: Mutex<f64>,
    /// 上一轮扫描每票的通过数，用于只在“新增通过”时提醒。
    pub last_alert: Mutex<HashMap<String, i64>>,
    pub audit: Mutex<Vec<AuditIssue>>,
}
