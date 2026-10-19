mod commands;
mod config;
mod features;
mod ledger;
mod market;
mod session;
mod state;
mod statement;
mod store;
mod strategy;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{Emitter, Manager};

use state::{AppState, FeedStatus, SeriesCache};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::fetch_quote,
            commands::feed_status,
            commands::submit_fill,
            commands::void_fill,
            commands::list_fills,
            commands::ledger_day,
            commands::bootstrap,
            commands::series,
            commands::import_statement,
            commands::get_config,
            commands::update_config,
            commands::test_notification,
            commands::list_watchlist,
            commands::scan_watchlist,
            commands::add_symbol,
            commands::set_symbol,
            commands::remove_symbol
        ])
        .setup(|app| {
            let dir = app.path().app_data_dir().expect("app data dir");
            std::fs::create_dir_all(&dir).expect("create app data dir");
            let conn = store::open(&dir.join("desk.db")).expect("open ledger db");
            let cfg = config::ensure_default(&conn).expect("load config");

            app.manage(AppState {
                client: market::http_client(),
                symbol: Mutex::new(cfg.symbol.clone()),
                config: Mutex::new(cfg),
                feed: Mutex::new(FeedStatus {
                    connected: false,
                    source: "tencent".to_string(),
                    latency_ms: 0,
                    last_error: None,
                }),
                db: Mutex::new(conn),
                series: Mutex::new(None),
                scan_cache: Mutex::new(std::collections::HashMap::new()),
                scan_fail: Mutex::new(std::collections::HashMap::new()),
                last_alert: Mutex::new(std::collections::HashMap::new()),
            });

            let handle = app.handle().clone();

            // 行情时钟在 Rust，不在 WebView。
            // 窗口最小化/失焦时 WebView 定时器会被节流，这条链路不受影响。
            tauri::async_runtime::spawn(async move {
                let mut last_refresh = Instant::now() - Duration::from_secs(120);

                loop {
                    let paused = handle
                        .state::<AppState>()
                        .config
                        .lock()
                        .map(|c| c.paused)
                        .unwrap_or(false);

                    if !paused && session::is_trading_time(&session::now_cn()) {
                        let (symbol, client) = {
                            let st = handle.state::<AppState>();
                            let symbol = st
                                .symbol
                                .lock()
                                .map(|s| s.clone())
                                .unwrap_or_else(|_| "sh600519".to_string());
                            (symbol, st.client.clone())
                        };

                        // 每 60 秒刷新分时缓存：特征是派生值，始终从原始分钟重算。
                        if last_refresh.elapsed() >= Duration::from_secs(60) {
                            last_refresh = Instant::now();
                            if let Ok((day, raw)) = market::fetch_minute(&client, &symbol).await {
                                if !raw.is_empty() {
                                    let prev_close = {
                                        let st = handle.state::<AppState>();
                                        st.series
                                            .lock()
                                            .ok()
                                            .and_then(|c| c.as_ref().map(|s| s.prev_close))
                                    };
                                    if let Some(pc) = prev_close {
                                        let bars = features::build(&raw, pc);
                                        let day_fmt = commands::fmt_day(&day);
                                        let cfg = handle
                                            .state::<AppState>()
                                            .config
                                            .lock()
                                            .map(|c| c.clone())
                                            .unwrap_or_default();
                                        let signals = strategy::scan(&bars, &day_fmt, &cfg);
                                        let stats = commands::stats_for(&signals);

                                        if let Ok(mut conn) = handle.state::<AppState>().db.lock() {
                                            let _ = store::save_minutes(&mut conn, &day, &raw);
                                            let _ =
                                                store::save_signals(&mut conn, &day_fmt, &signals);
                                        }

                                        if let Ok(mut cache) =
                                            handle.state::<AppState>().series.lock()
                                        {
                                            *cache = Some(SeriesCache {
                                                day,
                                                prev_close: pc,
                                                bars,
                                            });
                                        }

                                        let accepted =
                                            signals.iter().filter(|s| s.accepted).count();
                                        let conclusion = if accepted > 0 {
                                            format!(
                                                "{} 个候选，{} 个通过。",
                                                signals.len(),
                                                accepted
                                            )
                                        } else {
                                            format!(
                                                "{} 个候选全部否决：回归空间不足。",
                                                signals.len()
                                            )
                                        };
                                        let _ = handle.emit(
                                            "signals:update",
                                            serde_json::json!({
                                                "signals": signals,
                                                "stats": stats,
                                                "conclusion": conclusion
                                            }),
                                        );
                                    }
                                }
                            }
                        }

                        let started = Instant::now();
                        match market::fetch_quote(&client, &symbol).await {
                            Ok(q) => {
                                let (avg, vwap, sigma, slope, vol_ratio, regime) = {
                                    let st = handle.state::<AppState>();
                                    let guard = st.series.lock().ok();
                                    let last = guard
                                        .as_ref()
                                        .and_then(|g| g.as_ref())
                                        .and_then(|c| c.bars.last());
                                    match last {
                                        Some(b) => (
                                            b.avg,
                                            b.vwap,
                                            b.sigma,
                                            b.slope,
                                            b.vol_ratio,
                                            b.regime.clone(),
                                        ),
                                        None => {
                                            (q.price, q.price, 0.0008, 0.0, 1.0, "震荡".to_string())
                                        }
                                    }
                                };

                                let latency = started.elapsed().as_millis() as u64;
                                let z = if vwap > 0.0 {
                                    (q.price / vwap - 1.0) / sigma.max(0.0008)
                                } else {
                                    0.0
                                };

                                let snap = market::Snapshot {
                                    t: market::clock(&q.time),
                                    price: q.price,
                                    avg,
                                    vwap,
                                    z,
                                    slope,
                                    vol_ratio,
                                    regime,
                                    latency_ms: latency,
                                };

                                if let Ok(mut feed) = handle.state::<AppState>().feed.lock() {
                                    feed.connected = true;
                                    feed.latency_ms = latency;
                                    feed.last_error = None;
                                }
                                let _ = handle.emit("market:tick", &snap);
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
