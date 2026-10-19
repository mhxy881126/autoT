use std::collections::VecDeque;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// 单边滑点假设：2bp。与信号引擎的成本口径保持一致。
pub const SLIP_PER_LEG: f64 = 0.0002;

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
    pub voided: bool,
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
pub struct TradePair {
    pub id: i64,
    pub day: String,
    pub side: String,
    pub open_fill_id: i64,
    pub close_fill_id: Option<i64>,
    pub qty: i64,
    pub gross: Option<f64>,
    pub cost: Option<f64>,
    pub net: Option<f64>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub base_qty: i64,
    pub available: i64,
    pub frozen: i64,
    pub t_qty: i64,
    pub base_cost: f64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerDay {
    pub day: String,
    pub position: Position,
    pub fills: Vec<Fill>,
    pub pairs: Vec<TradePair>,
    pub realized_net: f64,
    pub unrealized: f64,
    pub unresolved: i64,
}

const SELECT: &str = "SELECT id, client_id, trade_day, ts, side, price_cents, qty, fee_cents, signal_id, source, note, created_at, voided FROM fills";

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
        voided: row.get::<_, i64>(12)? != 0,
    })
}

fn get_fill(conn: &Connection, id: i64) -> Result<Fill, String> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), params![id], map_fill)
        .map_err(|e| format!("读取成交失败: {e}"))
}

fn find_existing(
    conn: &Connection,
    input: &FillInput,
    price_cents: i64,
    fee_cents: i64,
) -> Result<Option<Fill>, String> {
    let by_client = conn
        .query_row(
            &format!("{SELECT} WHERE client_id = ?1"),
            params![&input.client_id],
            map_fill,
        )
        .optional()
        .map_err(|e| e.to_string())?;

    if let Some(fill) = by_client {
        if fill.voided {
            return Err("该 clientId 已冲正，不能复用".to_string());
        }
        let same_content = fill.day == input.day
            && fill.t == input.t
            && fill.side == input.side
            && ((fill.price * 100.0).round() as i64) == price_cents
            && fill.qty == input.qty
            && ((fill.fee * 100.0).round() as i64) == fee_cents;
        if !same_content {
            return Err("clientId 已存在但内容不同；修改成交需走冲正".to_string());
        }
        return Ok(Some(fill));
    }

    let by_natural = conn
        .query_row(
            &format!(
                "{SELECT} WHERE trade_day = ?1 AND ts = ?2 AND side = ?3 AND price_cents = ?4 AND qty = ?5 AND voided = 0"
            ),
            params![&input.day, &input.t, &input.side, price_cents, input.qty],
            map_fill,
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(fill) = by_natural {
        if fill.voided {
            return Err("该成交已冲正".to_string());
        }
        return Ok(Some(fill));
    }

    Ok(None)
}

/// 回填成交。幂等 + 当日全量重配对（增量配对在乱序回填时会错）。
pub fn submit(conn: &mut Connection, input: &FillInput) -> Result<Fill, String> {
    if input.qty <= 0 {
        return Err("数量必须大于 0".to_string());
    }
    if input.price <= 0.0 {
        return Err("价格必须大于 0".to_string());
    }
    if input.side != "BUY" && input.side != "SELL" {
        return Err(format!("非法方向: {}", input.side));
    }

    let price_cents = (input.price * 100.0).round() as i64;
    let fee_cents = (input.fee.unwrap_or(0.0) * 100.0).round() as i64;

    if let Some(fill) = find_existing(conn, input, price_cents, fee_cents)? {
        return Ok(fill);
    }

    let now = Utc::now().to_rfc3339();
    let tx = conn
        .transaction()
        .map_err(|e| format!("开启事务失败: {e}"))?;

    tx.execute(
        "INSERT INTO fills
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

    let id = tx.last_insert_rowid();
    rebuild_pairs(&tx, &input.day)?;
    tx.commit().map_err(|e| format!("提交成交失败: {e}"))?;

    get_fill(conn, id)
}

/// 冲正：作废成交并重放当日配对。修改成交只能走这里，保留审计链。
pub fn void_fill(conn: &mut Connection, id: i64) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|e| format!("开启冲正事务失败: {e}"))?;

    let day: String = tx
        .query_row(
            "SELECT trade_day FROM fills WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .map_err(|_| format!("成交不存在: {id}"))?;

    tx.execute("UPDATE fills SET voided = 1 WHERE id = ?1", params![id])
        .map_err(|e| format!("冲正失败: {e}"))?;
    rebuild_pairs(&tx, &day)?;
    tx.commit().map_err(|e| format!("提交冲正失败: {e}"))
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

pub fn list_pairs(conn: &Connection, day: &str) -> Result<Vec<TradePair>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, trade_day, side, open_fill_id, close_fill_id, qty,
                    gross_cents, cost_cents, net_cents, status
             FROM t_pairs WHERE trade_day = ?1 ORDER BY id",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![day], |row| {
            let gross: Option<i64> = row.get(6)?;
            let cost: Option<i64> = row.get(7)?;
            let net: Option<i64> = row.get(8)?;
            Ok(TradePair {
                id: row.get(0)?,
                day: row.get(1)?,
                side: row.get(2)?,
                open_fill_id: row.get(3)?,
                close_fill_id: row.get(4)?,
                qty: row.get(5)?,
                gross: gross.map(|v| v as f64 / 100.0),
                cost: cost.map(|v| v as f64 / 100.0),
                net: net.map(|v| v as f64 / 100.0),
                status: row.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

struct Leg {
    fill_id: i64,
    side: String,
    price_cents: i64,
    qty: i64,
    fee_per_share: f64,
    t: String,
}

struct PairRow {
    side: String,
    open_fill_id: i64,
    close_fill_id: Option<i64>,
    qty: i64,
    gross: Option<i64>,
    cost: Option<i64>,
    net: Option<i64>,
    status: String,
    opened_at: String,
    closed_at: Option<String>,
}

/// 删除当日配对，按成交时间重放。FIFO：新成交优先平掉最早的反向未平腿。
fn rebuild_pairs(conn: &Connection, day: &str) -> Result<(), String> {
    let fills: Vec<Fill> = {
        let mut stmt = conn
            .prepare(&format!(
                "{SELECT} WHERE trade_day = ?1 AND voided = 0 ORDER BY ts, id"
            ))
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![day], map_fill)
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        out
    };

    conn.execute("DELETE FROM t_pairs WHERE trade_day = ?1", params![day])
        .map_err(|e| format!("清理配对失败: {e}"))?;

    let mut open: VecDeque<Leg> = VecDeque::new();
    let mut pairs: Vec<PairRow> = Vec::new();

    for f in &fills {
        let price_cents = (f.price * 100.0).round() as i64;
        let fee_per_share = if f.qty > 0 { f.fee / f.qty as f64 } else { 0.0 };
        let mut remaining = f.qty;

        while remaining > 0 {
            // BUY 平掉 SELL 开的腿（倒T）；SELL 平掉 BUY 开的腿（正T）。
            let need_sell = f.side == "BUY";
            let idx = open
                .iter()
                .position(|leg| (leg.side == "SELL") == need_sell);
            let Some(idx) = idx else { break };

            let mut leg = open.remove(idx).expect("index checked");
            let matched = leg.qty.min(remaining);
            let side = if leg.side == "BUY" { "正T" } else { "倒T" };

            let open_price = leg.price_cents as f64 / 100.0;
            let close_price = price_cents as f64 / 100.0;
            let gross = if side == "正T" {
                (close_price - open_price) * matched as f64
            } else {
                (open_price - close_price) * matched as f64
            };
            let slip = (open_price + close_price) * matched as f64 * SLIP_PER_LEG;
            let fee = (leg.fee_per_share + fee_per_share) * matched as f64;
            let cost = slip + fee;
            let net = gross - cost;

            pairs.push(PairRow {
                side: side.to_string(),
                open_fill_id: leg.fill_id,
                close_fill_id: Some(f.id),
                qty: matched,
                gross: Some((gross * 100.0).round() as i64),
                cost: Some((cost * 100.0).round() as i64),
                net: Some((net * 100.0).round() as i64),
                status: "CLOSED".to_string(),
                opened_at: leg.t.clone(),
                closed_at: Some(f.t.clone()),
            });

            if leg.qty > matched {
                leg.qty -= matched;
                open.push_front(leg);
            }
            remaining -= matched;
        }

        if remaining > 0 {
            open.push_back(Leg {
                fill_id: f.id,
                side: f.side.clone(),
                price_cents,
                qty: remaining,
                fee_per_share,
                t: f.t.clone(),
            });
        }
    }

    for leg in open {
        pairs.push(PairRow {
            side: if leg.side == "BUY" {
                "正T".to_string()
            } else {
                "倒T".to_string()
            },
            open_fill_id: leg.fill_id,
            close_fill_id: None,
            qty: leg.qty,
            gross: None,
            cost: None,
            net: None,
            status: "OPEN".to_string(),
            opened_at: leg.t,
            closed_at: None,
        });
    }

    for row in pairs {
        conn.execute(
            "INSERT INTO t_pairs
             (trade_day, side, open_fill_id, close_fill_id, qty,
              gross_cents, cost_cents, net_cents, status, opened_at, closed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                day,
                row.side,
                row.open_fill_id,
                row.close_fill_id,
                row.qty,
                row.gross,
                row.cost,
                row.net,
                row.status,
                row.opened_at,
                row.closed_at
            ],
        )
        .map_err(|e| format!("写入配对失败: {e}"))?;
    }

    Ok(())
}

pub fn day(
    conn: &Connection,
    day: &str,
    base_qty: i64,
    base_cost: f64,
) -> Result<LedgerDay, String> {
    let fills = list_by_day(conn, day)?;
    let pairs = list_pairs(conn, day)?;

    let realized_cents: i64 = pairs
        .iter()
        .filter_map(|p| p.net)
        .map(|n| (n * 100.0).round() as i64)
        .sum();

    let unresolved = pairs.iter().filter(|p| p.status != "CLOSED").count() as i64;
    let t_open_qty: i64 = pairs
        .iter()
        .filter(|p| p.status != "CLOSED")
        .map(|p| p.qty)
        .sum();

    let net_change: i64 = fills
        .iter()
        .filter(|f| !f.voided)
        .map(|f| if f.side == "BUY" { f.qty } else { -f.qty })
        .sum();
    let net_qty = base_qty + net_change;
    let available = (net_qty - t_open_qty).max(0);

    Ok(LedgerDay {
        day: day.to_string(),
        position: Position {
            base_qty: net_qty,
            available,
            frozen: 0,
            t_qty: t_open_qty,
            base_cost,
            updated_at: Utc::now().to_rfc3339(),
        },
        fills,
        pairs,
        realized_net: realized_cents as f64 / 100.0,
        unrealized: 0.0,
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

    fn sample(client: &str, side: &str, price: f64, qty: i64, t: &str) -> FillInput {
        FillInput {
            client_id: client.to_string(),
            day: "2026-10-19".to_string(),
            t: t.to_string(),
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
        let mut c = conn();
        let a = submit(&mut c, &sample("c1", "BUY", 100.0, 100, "09:40")).expect("first");
        let b = submit(&mut c, &sample("c1", "BUY", 100.0, 100, "09:40")).expect("second");
        assert_eq!(a.id, b.id);
        assert_eq!(list_by_day(&c, "2026-10-19").unwrap().len(), 1);
    }

    #[test]
    fn conflicts_rejected() {
        let mut c = conn();
        submit(&mut c, &sample("c1", "BUY", 100.0, 100, "09:40")).unwrap();
        assert!(submit(&mut c, &sample("c1", "BUY", 101.0, 100, "09:40")).is_err());
    }

    #[test]
    fn buy_then_sell_closes_pair() {
        let mut c = conn();
        submit(&mut c, &sample("b1", "BUY", 100.0, 100, "09:40")).unwrap();
        submit(&mut c, &sample("s1", "SELL", 101.0, 100, "10:00")).unwrap();

        let d = day(&c, "2026-10-19", 1000, 90.0).unwrap();
        assert_eq!(d.pairs.len(), 1);
        assert_eq!(d.pairs[0].status, "CLOSED");
        assert_eq!(d.pairs[0].side, "正T");
        assert!(d.pairs[0].net.unwrap() > 0.0);
        assert_eq!(d.unresolved, 0);
        assert_eq!(d.position.base_qty, 1000);
    }

    #[test]
    fn partial_fills_split_into_pairs() {
        let mut c = conn();
        submit(&mut c, &sample("b1", "BUY", 100.0, 300, "09:40")).unwrap();
        submit(&mut c, &sample("s1", "SELL", 101.0, 100, "10:00")).unwrap();
        submit(&mut c, &sample("s2", "SELL", 102.0, 200, "10:30")).unwrap();

        let d = day(&c, "2026-10-19", 1000, 90.0).unwrap();
        assert_eq!(d.pairs.len(), 2);
        assert!(d.pairs.iter().all(|p| p.status == "CLOSED"));
        let total: i64 = d.pairs.iter().map(|p| p.qty).sum();
        assert_eq!(total, 300);
    }

    #[test]
    fn unclosed_t_is_unresolved() {
        let mut c = conn();
        submit(&mut c, &sample("b1", "BUY", 100.0, 200, "09:40")).unwrap();

        let d = day(&c, "2026-10-19", 1000, 90.0).unwrap();
        assert_eq!(d.pairs.len(), 1);
        assert_eq!(d.pairs[0].status, "OPEN");
        assert_eq!(d.unresolved, 1);
        assert_eq!(d.position.t_qty, 200);
    }

    #[test]
    fn void_replays_pairs() {
        let mut c = conn();
        submit(&mut c, &sample("b1", "BUY", 100.0, 100, "09:40")).unwrap();
        let sell = submit(&mut c, &sample("s1", "SELL", 101.0, 100, "10:00")).unwrap();
        assert_eq!(day(&c, "2026-10-19", 1000, 90.0).unwrap().unresolved, 0);

        void_fill(&mut c, sell.id).unwrap();
        let d = day(&c, "2026-10-19", 1000, 90.0).unwrap();
        assert_eq!(d.pairs.len(), 1);
        assert_eq!(d.pairs[0].status, "OPEN");
        assert_eq!(d.unresolved, 1);
        assert!(d.fills.iter().any(|f| f.voided));
    }
}
