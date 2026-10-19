use std::sync::Mutex;

use rusqlite::Connection;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedStatus {
    pub connected: bool,
    pub source: String,
    pub latency_ms: u64,
    pub last_error: Option<String>,
}

pub struct AppState {
    pub client: reqwest::Client,
    pub symbol: Mutex<String>,
    pub feed: Mutex<FeedStatus>,
    pub db: Mutex<Connection>,
}
