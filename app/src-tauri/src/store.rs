use std::path::Path;

use rusqlite::{params, Connection};

use crate::market::RawBar;
use crate::strategy::Signal;

/// 打开账本并执行 schema。路径由调用方决定（app_data_dir/desk.db）。
pub fn open(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| format!("打开数据库失败: {e}"))?;
    conn.execute_batch(include_str!("../../../contracts/schema.sql"))
        .map_err(|e| format!("初始化数据库失败: {e}"))?;
    // 兼容早期库：CREATE TABLE IF NOT EXISTS 不会补列。
    let _ = conn.execute_batch("ALTER TABLE fills ADD COLUMN voided INTEGER NOT NULL DEFAULT 0");
    let _ = conn.execute_batch("ALTER TABLE fills ADD COLUMN symbol TEXT NOT NULL DEFAULT ''");
    let _ = conn.execute_batch("ALTER TABLE t_pairs ADD COLUMN symbol TEXT NOT NULL DEFAULT ''");
    // 唯一键/索引升级为含 symbol（旧库同样重建）。
    let _ = conn.execute_batch(
        "DROP INDEX IF EXISTS uq_fills_active;
         DROP INDEX IF EXISTS idx_fills_day;
         DROP INDEX IF EXISTS idx_pairs_day;
         CREATE UNIQUE INDEX IF NOT EXISTS uq_fills_active
           ON fills (symbol, trade_day, ts, side, price_cents, qty)
           WHERE voided = 0;
         CREATE INDEX IF NOT EXISTS idx_fills_day ON fills (symbol, trade_day, ts);
         CREATE INDEX IF NOT EXISTS idx_pairs_day ON t_pairs (trade_day, symbol, status);",
    );
    Ok(conn)
}

/// 覆盖写入某日原始分时。先删后插，保证重算时没有残留旧点。
pub fn save_minutes(conn: &mut Connection, day: &str, raw: &[RawBar]) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|e| format!("开启事务失败: {e}"))?;
    tx.execute("DELETE FROM minute_raw WHERE trade_day = ?1", params![day])
        .map_err(|e| format!("清理分时失败: {e}"))?;

    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO minute_raw (trade_day, t, price_cents, cum_vol, cum_amt_cents)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .map_err(|e| format!("准备分时写入失败: {e}"))?;
        for bar in raw {
            stmt.execute(params![
                day,
                &bar.t,
                (bar.price * 100.0).round() as i64,
                bar.cum_vol,
                (bar.cum_amt * 100.0).round() as i64
            ])
            .map_err(|e| format!("写入分时失败: {e}"))?;
        }
    }

    tx.commit().map_err(|e| format!("提交分时失败: {e}"))
}

pub fn load_minutes(conn: &Connection, day: &str) -> Result<Vec<RawBar>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT t, price_cents, cum_vol, cum_amt_cents
             FROM minute_raw WHERE trade_day = ?1 ORDER BY t",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![day], |row| {
            Ok(RawBar {
                t: row.get(0)?,
                price: row.get::<_, i64>(1)? as f64 / 100.0,
                cum_vol: row.get(2)?,
                cum_amt: row.get::<_, i64>(3)? as f64 / 100.0,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// 覆盖写入某日信号。信号是派生结果，重算后整体替换，避免旧否决理由残留。
pub fn save_signals(conn: &mut Connection, day: &str, signals: &[Signal]) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|e| format!("开启信号事务失败: {e}"))?;
    tx.execute("DELETE FROM signals WHERE trade_day = ?1", params![day])
        .map_err(|e| format!("清理信号失败: {e}"))?;

    let now = chrono::Utc::now().to_rfc3339();
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO signals
                 (id, trade_day, ts, side, price_cents, target_cents, stop_cents, qty, z, regime, conf,
                  accepted, veto_json, why_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            )
            .map_err(|e| format!("准备信号写入失败: {e}"))?;

        for s in signals {
            let veto = serde_json::to_string(&s.veto).unwrap_or_else(|_| "[]".to_string());
            let why = serde_json::to_string(&s.why).unwrap_or_else(|_| "[]".to_string());
            stmt.execute(params![
                &s.id,
                day,
                &s.t,
                &s.side,
                (s.price * 100.0).round() as i64,
                (s.target * 100.0).round() as i64,
                (s.stop * 100.0).round() as i64,
                s.qty,
                s.z,
                &s.regime,
                s.conf,
                if s.accepted { 1 } else { 0 },
                veto,
                why,
                &now
            ])
            .map_err(|e| format!("写入信号失败: {e}"))?;
        }
    }

    tx.commit().map_err(|e| format!("提交信号失败: {e}"))
}
