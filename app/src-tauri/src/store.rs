use std::path::Path;

use rusqlite::{params, Connection};

use crate::market::{MinuteOhlc, RawBar};
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
    migrate_minutes_symbol(&conn);
    let _ =
        conn.execute_batch("ALTER TABLE minute_raw ADD COLUMN source TEXT NOT NULL DEFAULT 'real'");
    let _ = conn.execute_batch("ALTER TABLE trade_journal ADD COLUMN plan_target_cents INTEGER");
    let _ = conn.execute_batch("ALTER TABLE trade_journal ADD COLUMN plan_stop_cents INTEGER");
    Ok(conn)
}

/// 旧库的 minute_raw 主键只有 (trade_day,t)，多标的分时会互相覆盖。
/// SQLite 不能改主键，只能重建表；旧数据没有 symbol，统一标成空串。
fn migrate_minutes_symbol(conn: &Connection) {
    let has_symbol = conn
        .prepare("PRAGMA table_info(minute_raw)")
        .and_then(|mut stmt| {
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            let mut found = false;
            for row in rows {
                if row? == "symbol" {
                    found = true;
                }
            }
            Ok(found)
        })
        .unwrap_or(true);
    if has_symbol {
        return;
    }

    let migrated = conn.execute_batch(
        "BEGIN IMMEDIATE;
         ALTER TABLE minute_raw RENAME TO minute_raw_old;
         CREATE TABLE minute_raw (
           symbol        TEXT    NOT NULL,
           trade_day     TEXT    NOT NULL,
           source        TEXT    NOT NULL DEFAULT 'real',
           t             TEXT    NOT NULL,
           price_cents   INTEGER NOT NULL,
           cum_vol       INTEGER NOT NULL,
           cum_amt_cents INTEGER NOT NULL,
           PRIMARY KEY (symbol, trade_day, t)
         );
         INSERT INTO minute_raw (symbol, trade_day, source, t, price_cents, cum_vol, cum_amt_cents)
           SELECT '', trade_day, 'real', t, price_cents, cum_vol, cum_amt_cents FROM minute_raw_old;
         DROP TABLE minute_raw_old;
         COMMIT;",
    );
    if migrated.is_err() {
        let _ = conn.execute_batch("ROLLBACK;");
    }
}

/// 覆盖写入某标的某日真实分时。先删后插，保证重算时没有残留旧点。
pub fn save_minutes(
    conn: &mut Connection,
    symbol: &str,
    day: &str,
    raw: &[RawBar],
) -> Result<(), String> {
    save_minutes_source(conn, symbol, day, raw, "real")
}

/// 合成示例数据，单独标记，避免和真实数据混在一起。
pub fn save_demo_minutes(
    conn: &mut Connection,
    symbol: &str,
    day: &str,
    raw: &[RawBar],
) -> Result<(), String> {
    save_minutes_source(conn, symbol, day, raw, "demo")
}

fn save_minutes_source(
    conn: &mut Connection,
    symbol: &str,
    day: &str,
    raw: &[RawBar],
    source: &str,
) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|e| format!("开启事务失败: {e}"))?;
    tx.execute(
        "DELETE FROM minute_raw WHERE symbol = ?1 AND trade_day = ?2",
        params![symbol, day],
    )
    .map_err(|e| format!("清理分时失败: {e}"))?;

    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO minute_raw (symbol, trade_day, source, t, price_cents, cum_vol, cum_amt_cents)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(|e| format!("准备分时写入失败: {e}"))?;
        for bar in raw {
            stmt.execute(params![
                symbol,
                day,
                source,
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

pub fn clear_demo_minutes(conn: &Connection, symbol: &str) -> Result<usize, String> {
    conn.execute(
        "DELETE FROM minute_raw WHERE symbol = ?1 AND source = 'demo'",
        params![symbol],
    )
    .map_err(|e| format!("清理示例数据失败: {e}"))
}

pub fn demo_day_count(conn: &Connection, symbol: &str, days: &[String]) -> Result<i64, String> {
    if days.is_empty() {
        return Ok(0);
    }
    let mut count = 0i64;
    for day in days {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM minute_raw WHERE symbol = ?1 AND trade_day = ?2 AND source = 'demo'",
                params![symbol, day],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if n > 0 {
            count += 1;
        }
    }
    Ok(count)
}

pub fn load_minutes(conn: &Connection, symbol: &str, day: &str) -> Result<Vec<RawBar>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT t, price_cents, cum_vol, cum_amt_cents
             FROM minute_raw WHERE symbol = ?1 AND trade_day = ?2 ORDER BY t",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![symbol, day], |row| {
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

pub fn save_ohlc(
    conn: &mut Connection,
    symbol: &str,
    day: &str,
    bars: &[MinuteOhlc],
) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|e| format!("开启 OHLC 事务失败: {e}"))?;
    tx.execute(
        "DELETE FROM minute_ohlc WHERE symbol = ?1 AND trade_day = ?2",
        params![symbol, day],
    )
    .map_err(|e| format!("清理 OHLC 失败: {e}"))?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO minute_ohlc
                 (symbol, trade_day, t, open_cents, high_cents, low_cents, close_cents)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(|e| e.to_string())?;
        for bar in bars {
            stmt.execute(params![
                symbol,
                day,
                &bar.t,
                (bar.open * 100.0).round() as i64,
                (bar.high * 100.0).round() as i64,
                (bar.low * 100.0).round() as i64,
                (bar.close * 100.0).round() as i64
            ])
            .map_err(|e| format!("写入 OHLC 失败: {e}"))?;
        }
    }
    tx.commit().map_err(|e| format!("提交 OHLC 失败: {e}"))
}

pub fn load_ohlc(conn: &Connection, symbol: &str, day: &str) -> Result<Vec<MinuteOhlc>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT t, open_cents, high_cents, low_cents, close_cents
             FROM minute_ohlc WHERE symbol = ?1 AND trade_day = ?2 ORDER BY t",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![symbol, day], |row| {
            Ok(MinuteOhlc {
                t: row.get(0)?,
                open: row.get::<_, i64>(1)? as f64 / 100.0,
                high: row.get::<_, i64>(2)? as f64 / 100.0,
                low: row.get::<_, i64>(3)? as f64 / 100.0,
                close: row.get::<_, i64>(4)? as f64 / 100.0,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(t: &str, price: f64, cum_vol: i64) -> RawBar {
        RawBar {
            t: t.to_string(),
            price,
            cum_vol,
            cum_amt: price * cum_vol as f64 * 100.0,
        }
    }

    #[test]
    fn minutes_are_scoped_by_symbol() {
        let mut c = Connection::open_in_memory().expect("memory db");
        c.execute_batch(include_str!("../../../contracts/schema.sql"))
            .expect("schema");

        save_minutes(&mut c, "sh600519", "2026-10-19", &[raw("09:30", 10.0, 100)]).expect("save a");
        save_minutes(&mut c, "sz000858", "2026-10-19", &[raw("09:30", 20.0, 200)]).expect("save b");

        let a = load_minutes(&c, "sh600519", "2026-10-19").expect("load a");
        let b = load_minutes(&c, "sz000858", "2026-10-19").expect("load b");
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert!((a[0].price - 10.0).abs() < 1e-9);
        assert!((b[0].price - 20.0).abs() < 1e-9);
    }

    #[test]
    fn legacy_minute_table_is_rebuilt_with_symbol() {
        let c = Connection::open_in_memory().expect("memory db");
        c.execute_batch(
            "CREATE TABLE minute_raw (
               trade_day TEXT NOT NULL,
               t TEXT NOT NULL,
               price_cents INTEGER NOT NULL,
               cum_vol INTEGER NOT NULL,
               cum_amt_cents INTEGER NOT NULL,
               PRIMARY KEY (trade_day, t)
             );
             INSERT INTO minute_raw VALUES ('2026-10-18', '09:30', 1000, 100, 100000);",
        )
        .expect("legacy schema");

        migrate_minutes_symbol(&c);

        let rows = load_minutes(&c, "", "2026-10-18").expect("load migrated");
        assert_eq!(rows.len(), 1);
        assert!((rows[0].price - 10.0).abs() < 1e-9);

        let mut c = c;
        save_minutes(&mut c, "sh600519", "2026-10-18", &[raw("09:30", 11.0, 100)])
            .expect("save after migration");
        assert_eq!(
            load_minutes(&c, "sh600519", "2026-10-18")
                .expect("load new")
                .len(),
            1
        );
    }
}
