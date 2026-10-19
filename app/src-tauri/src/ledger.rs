use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fill {
    pub id: i64,
    pub client_id: String,
    pub day: String,
    pub t: String,
    pub side: String,
    pub price: f64,
    pub qty: i64,
    pub fee: f64,
    pub signal_id: Option<String>,
    pub source: String,
    pub note: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FillInput {
    pub client_id: String,
    pub day: String,
    pub t: String,
    pub side: String,
    pub price: f64,
    pub qty: i64,
    #[serde(default)]
    pub fee: Option<f64>,
    #[serde(default)]
    pub signal_id: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerDay {
    pub day: String,
    pub fills: Vec<Fill>,
    pub buy_qty: i64,
    pub sell_qty: i64,
    pub buy_amount: f64,
    pub sell_amount: f64,
    pub fees: f64,
    pub cash_flow: f64,
    pub unresolved: i64,
}

const SELECT: &str = "SELECT id, client_id, trade_day, ts, side, price_cents, qty, fee_cents, signal_id, source, note, created_at FROM fills";

fn map_fill(row: &rusqlite::Row<'_>) -> rusqlite::Result<Fill> {
    Ok(Fill {
        id: row.get(0)?,
        client_id: row.get(1)?,
        day: row.get(2)?,
        t: row.get(3)?,
        side: row.get(4)?,
        price: row.get::<_, i64>(5)? as f64 / 100.0,
        qty: row.get(6)?,
        fee: row.get::<_, i64>(7)? as f64 / 100.0,
        signal_id: row.get(8)?,
        source: row.get(9)?,
        note: row.get(10)?,
        created_at: row.get(11)?,
    })
}

/// 回填成交。幂等：
/// 1. 同一 clientId 重复提交 → 返回原记录；
/// 2. 不同 clientId 但自然键相同 → 返回原记录（防重复导入/重复点击）。
pub fn submit(conn: &Connection, input: &FillInput) -> Result<Fill, String> {
    if input.qty <= 0 {
        return Err("数量必须大于 0".to_string());
    }
    if input.price <= 0.0 {
        return Err("价格必须大于 0".to_string());
    }
    if input.side != "BUY" && input.side != "SELL" {
        return Err(format!("非法方向: {}", input.side));
    }

    // 金额一律整数分，避免浮点对账误差。
    let price_cents = (input.price * 100.0).round() as i64;
    let fee_cents = (input.fee.unwrap_or(0.0) * 100.0).round() as i64;
    let now = Utc::now().to_rfc3339();

    let changed = conn
        .execute(
            "INSERT OR IGNORE INTO fills
             (client_id, trade_day, ts, side, price_cents, qty, fee_cents, signal_id, source, note, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'manual', ?9, ?10)",
            params![
                &input.client_id,
                &input.day,
                &input.t,
                &input.side,
                price_cents,
                input.qty,
                fee_cents,
                &input.signal_id,
                &input.note,
                &now
            ],
        )
        .map_err(|e| format!("写入成交失败: {e}"))?;

    if changed == 1 {
        return conn
            .query_row(
                &format!("{SELECT} WHERE client_id = ?1"),
                params![&input.client_id],
                map_fill,
            )
            .map_err(|e| format!("读取成交失败: {e}"));
    }

    let by_client = conn
        .query_row(
            &format!("{SELECT} WHERE client_id = ?1"),
            params![&input.client_id],
            map_fill,
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(fill) = by_client {
        // 同一个 clientId 但内容变了：不能静默返回旧记录，用户会以为改成功了。
        // 修改成交必须走冲正（red 单），保留审计链。
        let same_content = fill.day == input.day
            && fill.t == input.t
            && fill.side == input.side
            && ((fill.price * 100.0).round() as i64) == price_cents
            && fill.qty == input.qty
            && ((fill.fee * 100.0).round() as i64) == fee_cents;
        if !same_content {
            return Err("clientId 已存在但内容不同；修改成交需走冲正".to_string());
        }
        return Ok(fill);
    }

    conn.query_row(
        &format!(
            "{SELECT} WHERE trade_day = ?1 AND ts = ?2 AND side = ?3 AND price_cents = ?4 AND qty = ?5"
        ),
        params![&input.day, &input.t, &input.side, price_cents, input.qty],
        map_fill,
    )
    .optional()
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "重复成交但无法匹配原记录".to_string())
}

pub fn list_by_day(conn: &Connection, day: &str) -> Result<Vec<Fill>, String> {
    let mut stmt = conn
        .prepare(&format!("{SELECT} WHERE trade_day = ?1 ORDER BY ts, id"))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![day], map_fill)
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

pub fn day(conn: &Connection, day: &str) -> Result<LedgerDay, String> {
    let fills = list_by_day(conn, day)?;

    let mut buy_qty = 0i64;
    let mut sell_qty = 0i64;
    let mut buy_amount = 0.0f64;
    let mut sell_amount = 0.0f64;
    let mut fees = 0.0f64;

    for f in &fills {
        let amount = f.price * f.qty as f64;
        match f.side.as_str() {
            "BUY" => {
                buy_qty += f.qty;
                buy_amount += amount;
            }
            "SELL" => {
                sell_qty += f.qty;
                sell_amount += amount;
            }
            _ => {}
        }
        fees += f.fee;
    }

    let unresolved: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM t_pairs WHERE trade_day = ?1 AND status IN ('OPEN','UNWOUND')",
            params![day],
            |row| row.get(0),
        )
        .unwrap_or(0);

    Ok(LedgerDay {
        day: day.to_string(),
        fills,
        buy_qty,
        sell_qty,
        buy_amount,
        sell_amount,
        fees,
        cash_flow: sell_amount - buy_amount - fees,
        unresolved,
    })
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

    fn sample(client: &str, side: &str, price: f64, qty: i64) -> FillInput {
        FillInput {
            client_id: client.to_string(),
            day: "2026-10-19".to_string(),
            t: "09:47".to_string(),
            side: side.to_string(),
            price,
            qty,
            fee: Some(5.0),
            signal_id: None,
            note: None,
        }
    }

    #[test]
    fn idempotent_by_client_id() {
        let c = conn();
        let a = submit(&c, &sample("c1", "BUY", 1339.99, 100)).expect("first");
        let b = submit(&c, &sample("c1", "BUY", 1339.99, 100)).expect("second");
        assert_eq!(a.id, b.id);
        assert_eq!(list_by_day(&c, "2026-10-19").unwrap().len(), 1);
    }

    #[test]
    fn idempotent_by_natural_key() {
        let c = conn();
        let a = submit(&c, &sample("c1", "BUY", 1339.99, 100)).expect("first");
        let b = submit(&c, &sample("c2", "BUY", 1339.99, 100)).expect("duplicate content");
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn rejects_same_client_with_different_content() {
        let c = conn();
        submit(&c, &sample("c1", "BUY", 1339.99, 100)).expect("first");
        let err = submit(&c, &sample("c1", "BUY", 1340.0, 100)).expect_err("must reject");
        assert!(err.contains("冲正"));
    }

    #[test]
    fn rejects_bad_input() {
        let c = conn();
        assert!(submit(&c, &sample("c1", "HOLD", 100.0, 100)).is_err());
        assert!(submit(&c, &sample("c2", "BUY", 0.0, 100)).is_err());
        assert!(submit(&c, &sample("c3", "BUY", 100.0, 0)).is_err());
    }

    #[test]
    fn day_totals_match_fills() {
        let c = conn();
        submit(&c, &sample("c1", "BUY", 100.0, 100)).unwrap();
        submit(&c, &sample("c2", "SELL", 101.0, 100)).unwrap();
        let d = day(&c, "2026-10-19").unwrap();
        assert_eq!(d.buy_qty, 100);
        assert_eq!(d.sell_qty, 100);
        assert!((d.buy_amount - 10_000.0).abs() < 0.001);
        assert!((d.sell_amount - 10_100.0).abs() < 0.001);
        assert!((d.fees - 10.0).abs() < 0.001);
        assert!((d.cash_flow - 90.0).abs() < 0.001);
    }
}
