mod commands;
mod ledger;
mod market;
mod session;
mod state;
mod store;

use std::sync::Mutex;
use std::time::Duration;

use tauri::{Emitter, Manager};

use state::{AppState, FeedStatus};

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::fetch_quote,
            commands::feed_status,
            commands::submit_fill,
            commands::list_fills,
            commands::ledger_day
        ])
        .setup(|app| {
            let dir = app.path().app_data_dir().expect("app data dir");
            std::fs::create_dir_all(&dir).expect("create app data dir");
            let conn = store::open(&dir.join("desk.db")).expect("open ledger db");

            app.manage(AppState {
                client: market::http_client(),
                symbol: Mutex::new("sh600519".to_string()),
                feed: Mutex::new(FeedStatus {
                    connected: false,
                    source: "tencent".to_string(),
                    latency_ms: 0,
                    last_error: None,
                }),
                db: Mutex::new(conn),
            });

            let handle = app.handle().clone();

            // 行情时钟在 Rust，不在 WebView。
            // 窗口最小化/失焦时 WebView 定时器会被节流，这条链路不受影响。
            tauri::async_runtime::spawn(async move {
                loop {
                    if session::is_trading_time(&session::now_cn()) {
                        let (symbol, client) = {
                            let st = handle.state::<AppState>();
                            let symbol = st
                                .symbol
                                .lock()
                                .map(|s| s.clone())
                                .unwrap_or_else(|_| "sh600519".to_string());
                            (symbol, st.client.clone())
                        };

                        let started = std::time::Instant::now();
                        match market::fetch_quote(&client, &symbol).await {
                            Ok(q) => {
                                if let Ok(mut feed) = handle.state::<AppState>().feed.lock() {
                                    feed.connected = true;
                                    feed.latency_ms = started.elapsed().as_millis() as u64;
                                    feed.last_error = None;
                                }
                                let _ = handle.emit("market:tick", &q);
                            }
                            Err(err) => {
                                if let Ok(mut feed) = handle.state::<AppState>().feed.lock() {
                                    feed.connected = false;
                                    feed.last_error = Some(err);
                                }
                            }
                        }
                    }

                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
