//! 交易日志 v1：每笔 T 的计划、理由、情绪、错误标签与统计。

use std::collections::HashMap;

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalInput {
    pub symbol: String,
    pub day: String,
    pub open_fill_id: i64,
    #[serde(default)]
    pub close_fill_id: Option<i64>,
    pub planned: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub emotion: Option<String>,
    #[serde(default)]
    pub mistakes: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    pub id: i64,
    pub symbol: String,
    pub day: String,
    pub open_fill_id: i64,
    pub close_fill_id: Option<i64>,
    pub planned: bool,
    pub reason: Option<String>,
    pub emotion: Option<String>,
    pub mistakes: Vec<String>,
    pub note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CountItem {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PnlBucket {
    pub trades: i64,
    pub net: f64,
    pub avg: f64,
    pub wins: i64,
    pub win_rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MistakePnl {
    pub name: String,
    pub count: i64,
    pub net: f64,
    pub avg: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalStats {
    pub entries: i64,
    pub planned: i64,
    pub unplanned: i64,
    pub adherence: f64,
    pub mistakes: Vec<CountItem>,
    pub emotions: Vec<CountItem>,
    pub planned_pnl: PnlBucket,
    pub unplanned_pnl: PnlBucket,
    pub pnl_delta_avg: f64,
    pub mistake_pnl: Vec<MistakePnl>,
}

const SELECT: &str = "SELECT id, symbol, trade_day, open_fill_id, close_fill_id, planned, reason, emotion, mistakes_json, note, created_at, updated_at FROM trade_journal";

fn map_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<JournalEntry> {
    let mistakes_json: String = row.get(8)?;
    Ok(JournalEntry {
        id: row.get(0)?,
        symbol: row.get(1)?,
        day: row.get(2)?,
        open_fill_id: row.get(3)?,
        close_fill_id: row.get(4)?,
        planned: row.get::<_, i64>(5)? != 0,
        reason: row.get(6)?,
        emotion: row.get(7)?,
        mistakes: serde_json::from_str(&mistakes_json).unwrap_or_default(),
        note: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn clean_text(value: Option<String>, max: usize) -> Option<String> {
    value.and_then(|v| {
        let trimmed = v.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.chars().take(max).collect())
        }
    })
}

pub fn upsert(conn: &Connection, input: &JournalInput) -> Result<JournalEntry, String> {
    if input.symbol.trim().is_empty() || input.day.trim().is_empty() {
        return Err("symbol/day 不能为空".to_string());
    }
    if input.open_fill_id <= 0 {
        return Err("open_fill_id 非法".to_string());
    }
    if input.mistakes.len() > 10 {
        return Err("错误标签最多 10 个".to_string());
    }
    if input.mistakes.iter().any(|m| m.chars().count() > 20) {
        return Err("单个错误标签最多 20 字".to_string());
    }

    let mistakes: Vec<String> = input
        .mistakes
        .iter()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .collect();
    let mistakes_json = serde_json::to_string(&mistakes).map_err(|e| e.to_string())?;
    let reason = clean_text(input.reason.clone(), 500);
    let emotion = clean_text(input.emotion.clone(), 20);
    let note = clean_text(input.note.clone(), 1000);
    let now = Utc::now().to_rfc3339();

    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM trade_journal
             WHERE symbol = ?1 AND trade_day = ?2 AND open_fill_id = ?3
               AND IFNULL(close_fill_id, 0) = IFNULL(?4, 0)",
            params![
                &input.symbol,
                &input.day,
                input.open_fill_id,
                input.close_fill_id
            ],
            |row| row.get(0),
        )
        .ok();

    if let Some(id) = existing {
        conn.execute(
            "UPDATE trade_journal SET planned = ?1, reason = ?2, emotion = ?3,
             mistakes_json = ?4, note = ?5, updated_at = ?6 WHERE id = ?7",
            params![
                if input.planned { 1 } else { 0 },
                reason,
                emotion,
                mistakes_json,
                note,
                now,
                id
            ],
        )
        .map_err(|e| format!("更新日志失败: {e}"))?;
        return get(conn, id);
    }

    conn.execute(
        "INSERT INTO trade_journal
         (symbol, trade_day, open_fill_id, close_fill_id, planned, reason, emotion,
          mistakes_json, note, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
        params![
            &input.symbol,
            &input.day,
            input.open_fill_id,
            input.close_fill_id,
            if input.planned { 1 } else { 0 },
            reason,
            emotion,
            mistakes_json,
            note,
            now
        ],
    )
    .map_err(|e| format!("写入日志失败: {e}"))?;

    get(conn, conn.last_insert_rowid())
}

fn get(conn: &Connection, id: i64) -> Result<JournalEntry, String> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), params![id], map_entry)
        .map_err(|e| format!("读取日志失败: {e}"))
}

pub fn list(conn: &Connection, day: &str, symbol: &str) -> Result<Vec<JournalEntry>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "{SELECT} WHERE trade_day = ?1 AND symbol = ?2 ORDER BY id"
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![day, symbol], map_entry)
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

pub fn delete(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute("DELETE FROM trade_journal WHERE id = ?1", params![id])
        .map_err(|e| format!("删除日志失败: {e}"))?;
    Ok(())
}

#[derive(Default)]
struct BucketAcc {
    trades: i64,
    net: f64,
    wins: i64,
}

impl BucketAcc {
    fn add(&mut self, net: f64) {
        self.trades += 1;
        self.net += net;
        if net > 0.0 {
            self.wins += 1;
        }
    }

    fn finish(self) -> PnlBucket {
        PnlBucket {
            trades: self.trades,
            net: round2(self.net),
            avg: if self.trades > 0 {
                round2(self.net / self.trades as f64)
            } else {
                0.0
            },
            wins: self.wins,
            win_rate: if self.trades > 0 {
                self.wins as f64 / self.trades as f64
            } else {
                0.0
            },
        }
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn pair_net(conn: &Connection, entry: &JournalEntry) -> Result<Option<f64>, String> {
    let cents: Option<i64> = conn
        .query_row(
            "SELECT net_cents FROM t_pairs
             WHERE symbol = ?1 AND trade_day = ?2 AND open_fill_id = ?3
               AND IFNULL(close_fill_id, 0) = IFNULL(?4, 0)
               AND status = 'CLOSED'
             LIMIT 1",
            params![
                &entry.symbol,
                &entry.day,
                entry.open_fill_id,
                entry.close_fill_id
            ],
            |row| row.get(0),
        )
        .ok()
        .flatten();
    Ok(cents.map(|c| c as f64 / 100.0))
}

pub fn stats(conn: &Connection, day: &str, symbol: &str) -> Result<JournalStats, String> {
    let entries = list(conn, day, symbol)?;
    let mut mistakes: HashMap<String, i64> = HashMap::new();
    let mut mistake_pnl_map: HashMap<String, (i64, f64)> = HashMap::new();
    let mut emotions: HashMap<String, i64> = HashMap::new();
    let mut planned = 0i64;
    let mut planned_acc = BucketAcc::default();
    let mut unplanned_acc = BucketAcc::default();

    for entry in &entries {
        if entry.planned {
            planned += 1;
        }
        let net = pair_net(conn, entry)?;
        if let Some(n) = net {
            if entry.planned {
                planned_acc.add(n);
            } else {
                unplanned_acc.add(n);
            }
        }
        for m in &entry.mistakes {
            *mistakes.entry(m.clone()).or_default() += 1;
            let slot = mistake_pnl_map.entry(m.clone()).or_insert((0, 0.0));
            slot.0 += 1;
            if let Some(n) = net {
                slot.1 += n;
            }
        }
        if let Some(e) = &entry.emotion {
            *emotions.entry(e.clone()).or_default() += 1;
        }
    }

    let mut mistake_items: Vec<CountItem> = mistakes
        .into_iter()
        .map(|(name, count)| CountItem { name, count })
        .collect();
    mistake_items.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));

    let mut mistake_pnl: Vec<MistakePnl> = mistake_pnl_map
        .into_iter()
        .map(|(name, (count, net))| MistakePnl {
            name,
            count,
            net: round2(net),
            avg: if count > 0 {
                round2(net / count as f64)
            } else {
                0.0
            },
        })
        .collect();
    mistake_pnl.sort_by(|a, b| {
        a.net
            .partial_cmp(&b.net)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut emotion_items: Vec<CountItem> = emotions
        .into_iter()
        .map(|(name, count)| CountItem { name, count })
        .collect();
    emotion_items.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));

    let total = entries.len() as i64;
    let planned_pnl = planned_acc.finish();
    let unplanned_pnl = unplanned_acc.finish();
    let pnl_delta_avg = planned_pnl.avg - unplanned_pnl.avg;

    Ok(JournalStats {
        entries: total,
        planned,
        unplanned: total - planned,
        adherence: if total > 0 {
            planned as f64 / total as f64
        } else {
            0.0
        },
        mistakes: mistake_items,
        emotions: emotion_items,
        planned_pnl,
        unplanned_pnl,
        pnl_delta_avg,
        mistake_pnl,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().expect("memory db");
        c.execute_batch(include_str!("../../../contracts/schema.sql"))
            .expect("schema");
        c.execute_batch(
            "INSERT INTO fills (id, client_id, symbol, trade_day, ts, side, price_cents, qty, created_at)
               VALUES (1, 'c1', 'sh600519', '2026-10-19', '09:40', 'BUY', 10000, 100, 'x');
             INSERT INTO fills (id, client_id, symbol, trade_day, ts, side, price_cents, qty, created_at)
               VALUES (2, 'c2', 'sh600519', '2026-10-19', '10:00', 'SELL', 10100, 100, 'x');
             INSERT INTO fills (id, client_id, symbol, trade_day, ts, side, price_cents, qty, created_at)
               VALUES (3, 'c3', 'sh600519', '2026-10-19', '13:30', 'BUY', 10100, 100, 'x');
             INSERT INTO fills (id, client_id, symbol, trade_day, ts, side, price_cents, qty, created_at)
               VALUES (4, 'c4', 'sh600519', '2026-10-19', '14:00', 'SELL', 10070, 100, 'x');
             INSERT INTO t_pairs (trade_day, symbol, side, open_fill_id, close_fill_id, qty,
               gross_cents, cost_cents, net_cents, status, opened_at, closed_at)
               VALUES ('2026-10-19', 'sh600519', '正T', 1, 2, 100, 5000, 700, 4300, 'CLOSED', '09:40', '10:00');
             INSERT INTO t_pairs (trade_day, symbol, side, open_fill_id, close_fill_id, qty,
               gross_cents, cost_cents, net_cents, status, opened_at, closed_at)
               VALUES ('2026-10-19', 'sh600519', '正T', 3, 4, 100, -3000, 700, -3000, 'CLOSED', '13:30', '14:00');",
        )
        .expect("fills");
        c
    }

    fn input_for(open_fill_id: i64, close_fill_id: i64, planned: bool) -> JournalInput {
        JournalInput {
            symbol: "sh600519".to_string(),
            day: "2026-10-19".to_string(),
            open_fill_id,
            close_fill_id: Some(close_fill_id),
            planned,
            reason: Some("VWAP 回归".to_string()),
            emotion: Some("冷静".to_string()),
            mistakes: vec!["追高".to_string()],
            note: None,
        }
    }

    fn input(planned: bool) -> JournalInput {
        input_for(1, 2, planned)
    }

    #[test]
    fn upsert_updates_same_pair_instead_of_duplicating() {
        let c = conn();
        let a = upsert(&c, &input(true)).expect("insert");
        let b = upsert(&c, &input(false)).expect("update");
        assert_eq!(a.id, b.id);
        assert!(!b.planned);
        assert_eq!(list(&c, "2026-10-19", "sh600519").unwrap().len(), 1);
    }

    #[test]
    fn stats_counts_adherence_and_mistakes() {
        let c = conn();
        upsert(&c, &input(true)).expect("insert");
        let s = stats(&c, "2026-10-19", "sh600519").expect("stats");
        assert_eq!(s.entries, 1);
        assert_eq!(s.planned, 1);
        assert_eq!(s.adherence, 1.0);
        assert_eq!(s.mistakes[0].name, "追高");
        assert_eq!(s.emotions[0].name, "冷静");
        assert_eq!(s.planned_pnl.net, 43.0);
        assert_eq!(s.planned_pnl.trades, 1);
    }

    #[test]
    fn attribution_splits_planned_and_unplanned_pnl() {
        let c = conn();
        upsert(&c, &input_for(1, 2, true)).expect("planned");
        upsert(&c, &input_for(3, 4, false)).expect("unplanned");
        let s = stats(&c, "2026-10-19", "sh600519").expect("stats");
        assert_eq!(s.planned_pnl.trades, 1);
        assert_eq!(s.planned_pnl.net, 43.0);
        assert_eq!(s.unplanned_pnl.trades, 1);
        assert_eq!(s.unplanned_pnl.net, -30.0);
        assert!((s.pnl_delta_avg - 73.0).abs() < 1e-9);
        assert_eq!(s.mistake_pnl[0].count, 2);
        assert_eq!(s.mistake_pnl[0].net, 13.0);
    }

    #[test]
    fn delete_removes_entry() {
        let c = conn();
        let e = upsert(&c, &input(true)).expect("insert");
        delete(&c, e.id).expect("delete");
        assert!(list(&c, "2026-10-19", "sh600519").unwrap().is_empty());
    }
}
