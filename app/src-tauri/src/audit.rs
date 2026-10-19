//! 启动完整性检查：列出跨日仍未闭环的 T，避免崩溃/断网后“假装没发生”。

use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditIssue {
    pub day: String,
    pub symbol: String,
    pub count: i64,
}

pub fn unresolved(conn: &Connection) -> Result<Vec<AuditIssue>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT trade_day, symbol, COUNT(*)
             FROM t_pairs
             WHERE status <> 'CLOSED'
             GROUP BY trade_day, symbol
             ORDER BY trade_day DESC
             LIMIT 20",
        )
        .map_err(|e| format!("读取未闭环失败: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(AuditIssue {
                day: row.get(0)?,
                symbol: row.get(1)?,
                count: row.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().expect("memory db");
        c.execute_batch(include_str!("../../../contracts/schema.sql"))
            .expect("schema");
        c
    }

    #[test]
    fn groups_open_pairs_by_day_and_symbol() {
        let c = conn();
        c.execute_batch(
            "INSERT INTO fills (client_id, symbol, trade_day, ts, side, price_cents, qty, created_at)
               VALUES ('c1', 'sh600519', '2026-10-18', '09:40', 'BUY', 10000, 100, 'x');
             INSERT INTO fills (client_id, symbol, trade_day, ts, side, price_cents, qty, created_at)
               VALUES ('c2', 'sz000858', '2026-10-19', '09:45', 'SELL', 20000, 100, 'x');
             INSERT INTO t_pairs
               (trade_day, symbol, side, open_fill_id, close_fill_id, qty, status, opened_at)
               VALUES ('2026-10-18', 'sh600519', '正T', 1, NULL, 100, 'OPEN', '09:40');
             INSERT INTO t_pairs
               (trade_day, symbol, side, open_fill_id, close_fill_id, qty, status, opened_at)
               VALUES ('2026-10-19', 'sz000858', '倒T', 2, NULL, 100, 'OPEN', '09:45');
             INSERT INTO t_pairs
               (trade_day, symbol, side, open_fill_id, close_fill_id, qty, status, opened_at)
               VALUES ('2026-10-19', 'sz000858', '倒T', 2, 2, 100, 'CLOSED', '09:50');",
        )
        .expect("fixtures");

        let issues = unresolved(&c).expect("audit");
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].day, "2026-10-19");
        assert_eq!(issues[0].count, 1);
        assert_eq!(issues[1].day, "2026-10-18");
    }
}
