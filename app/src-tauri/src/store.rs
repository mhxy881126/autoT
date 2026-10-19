use std::path::Path;

use rusqlite::{params, Connection};

use crate::market::RawBar;

/// 打开账本并执行 schema。路径由调用方决定（app_data_dir/desk.db）。
pub fn open(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| format!("打开数据库失败: {e}"))?;
    conn.execute_batch(include_str!("../../../contracts/schema.sql"))
        .map_err(|e| format!("初始化数据库失败: {e}"))?;
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
