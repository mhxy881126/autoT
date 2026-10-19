use serde::Serialize;

use crate::ledger::{self, Fill, FillInput, LedgerDay};
use crate::market::{self, Quote};
use crate::state::{AppState, FeedStatus};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub stack: String,
    pub mode: String,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        stack: "tauri2 + vue3 + ts".to_string(),
        mode: "skeleton".to_string(),
    }
}

#[tauri::command]
pub async fn fetch_quote(
    state: tauri::State<'_, AppState>,
    symbol: Option<String>,
) -> Result<Quote, String> {
    let sym = match symbol {
        Some(s) => s,
        None => state.symbol.lock().map_err(|e| e.to_string())?.clone(),
    };
    market::fetch_quote(&state.client, &sym).await
}

#[tauri::command]
pub fn feed_status(state: tauri::State<'_, AppState>) -> Result<FeedStatus, String> {
    Ok(state.feed.lock().map_err(|e| e.to_string())?.clone())
}

#[tauri::command]
pub fn submit_fill(state: tauri::State<'_, AppState>, input: FillInput) -> Result<Fill, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    ledger::submit(&conn, &input)
}

#[tauri::command]
pub fn list_fills(state: tauri::State<'_, AppState>, day: String) -> Result<Vec<Fill>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    ledger::list_by_day(&conn, &day)
}

#[tauri::command]
pub fn ledger_day(state: tauri::State<'_, AppState>, day: String) -> Result<LedgerDay, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    ledger::day(&conn, &day)
}
