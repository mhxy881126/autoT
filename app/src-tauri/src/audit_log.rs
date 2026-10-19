//! 操作审计：急停与配置修改的旧值/新值/理由。

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionAudit {
    pub id: i64,
    pub action: String,
    pub target: String,
    pub changed_at: String,
    pub reason: Option<String>,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

pub fn log(
    conn: &Connection,
    action: &str,
    target: &str,
    reason: Option<&str>,
    before: Option<Value>,
    after: Option<Value>,
) -> Result<(), String> {
    let before_json = before
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| e.to_string())?;
    let after_json = after
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| e.to_string())?;
    let reason: Option<String> = reason
        .map(|r| r.trim().chars().take(200).collect::<String>())
        .filter(|r| !r.is_empty());
    conn.execute(
        "INSERT INTO action_audit (action, target, changed_at, reason, before_json, after_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            action,
            target,
            Utc::now().to_rfc3339(),
            reason,
            before_json,
            after_json
        ],
    )
    .map_err(|e| format!("写入操作审计失败: {e}"))?;
    Ok(())
}

pub fn list(conn: &Connection, limit: i64) -> Result<Vec<ActionAudit>, String> {
    let limit = limit.clamp(1, 500);
    let mut stmt = conn
        .prepare(
            "SELECT id, action, target, changed_at, reason, before_json, after_json
             FROM action_audit ORDER BY id DESC LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![limit], |row| {
            let before_json: Option<String> = row.get(5)?;
            let after_json: Option<String> = row.get(6)?;
            Ok(ActionAudit {
                id: row.get(0)?,
                action: row.get(1)?,
                target: row.get(2)?,
                changed_at: row.get(3)?,
                reason: row.get(4)?,
                before: before_json.and_then(|s| serde_json::from_str(&s).ok()),
                after: after_json.and_then(|s| serde_json::from_str(&s).ok()),
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
    fn logs_and_lists_actions() {
        let c = conn();
        log(
            &c,
            "emergency",
            "global",
            Some("手动急停"),
            Some(serde_json::json!(false)),
            Some(serde_json::json!(true)),
        )
        .expect("log");
        log(
            &c,
            "config",
            "sz000858",
            Some("调整阈值"),
            Some(serde_json::json!({"zEntry": 1.2})),
            Some(serde_json::json!({"zEntry": 1.5})),
        )
        .expect("log");
        let rows = list(&c, 10).expect("list");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].action, "config");
        assert_eq!(rows[1].reason.as_deref(), Some("手动急停"));
    }
}
