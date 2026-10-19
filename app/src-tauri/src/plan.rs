//! 盘前计划 v1：方向偏好、关键价位、最大次数、风险预算。
//! 开盘后修改必须给出理由，并保留最后一次理由。

use chrono::{NaiveDate, Timelike, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::session;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanLevel {
    pub label: String,
    pub price: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyPlan {
    pub day: String,
    pub symbol: String,
    pub bias: String,
    pub max_trades: i64,
    pub risk_budget_pct: f64,
    pub levels: Vec<PlanLevel>,
    pub note: Option<String>,
    pub locked: bool,
    pub locked_reason: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanInput {
    pub day: String,
    pub symbol: String,
    pub bias: String,
    pub max_trades: i64,
    pub risk_budget_pct: f64,
    #[serde(default)]
    pub levels: Vec<PlanLevel>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanAudit {
    pub id: i64,
    pub day: String,
    pub symbol: String,
    pub changed_at: String,
    pub reason: Option<String>,
    pub before: Option<DailyPlan>,
    pub after: DailyPlan,
}

const SELECT: &str = "SELECT trade_day, symbol, bias, max_trades, risk_budget_pct, levels_json, note, locked, locked_reason, updated_at FROM daily_plans";

fn map(row: &rusqlite::Row<'_>) -> rusqlite::Result<DailyPlan> {
    let levels_json: String = row.get(5)?;
    Ok(DailyPlan {
        day: row.get(0)?,
        symbol: row.get(1)?,
        bias: row.get(2)?,
        max_trades: row.get(3)?,
        risk_budget_pct: row.get(4)?,
        levels: serde_json::from_str(&levels_json).unwrap_or_default(),
        note: row.get(6)?,
        locked: row.get::<_, i64>(7)? != 0,
        locked_reason: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

pub fn get(conn: &Connection, day: &str, symbol: &str) -> Result<Option<DailyPlan>, String> {
    Ok(conn
        .query_row(
            &format!("{SELECT} WHERE trade_day = ?1 AND symbol = ?2"),
            params![day, symbol],
            map,
        )
        .ok())
}

pub fn audit_list(conn: &Connection, day: &str, symbol: &str) -> Result<Vec<PlanAudit>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, trade_day, symbol, changed_at, reason, before_json, after_json
             FROM plan_audit WHERE trade_day = ?1 AND symbol = ?2 ORDER BY id DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![day, symbol], |row| {
            let before_json: Option<String> = row.get(5)?;
            let after_json: String = row.get(6)?;
            Ok(PlanAudit {
                id: row.get(0)?,
                day: row.get(1)?,
                symbol: row.get(2)?,
                changed_at: row.get(3)?,
                reason: row.get(4)?,
                before: before_json.and_then(|text| serde_json::from_str::<DailyPlan>(&text).ok()),
                after: serde_json::from_str::<DailyPlan>(&after_json).unwrap_or_else(|_| {
                    DailyPlan {
                        day: String::new(),
                        symbol: String::new(),
                        bias: "双向".to_string(),
                        max_trades: 0,
                        risk_budget_pct: 0.0,
                        levels: vec![],
                        note: None,
                        locked: false,
                        locked_reason: None,
                        updated_at: String::new(),
                    }
                }),
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

fn clean(value: Option<String>, max: usize) -> Option<String> {
    value.and_then(|v| {
        let s = v.trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s.chars().take(max).collect())
        }
    })
}

fn opened(day: &str) -> bool {
    let now = session::now_cn();
    let today = now.format("%Y%m%d").to_string();
    if day != today {
        return false;
    }
    let minutes = now.hour() as i64 * 60 + now.minute() as i64;
    minutes >= 570
}

fn same_plan(a: &DailyPlan, input: &PlanInput) -> bool {
    a.bias == input.bias
        && a.max_trades == input.max_trades
        && (a.risk_budget_pct - input.risk_budget_pct).abs() < 1e-9
        && a.levels == input.levels
        && a.note == clean(input.note.clone(), 500)
}

pub fn save(conn: &Connection, input: &PlanInput) -> Result<DailyPlan, String> {
    let day = input.day.trim();
    let symbol = input.symbol.trim();
    if NaiveDate::parse_from_str(day, "%Y-%m-%d").is_err()
        && NaiveDate::parse_from_str(day, "%Y%m%d").is_err()
    {
        return Err("计划日期格式不正确".to_string());
    }
    if symbol.len() != 8 {
        return Err("标的代码格式应为 sh600519 / sz000858".to_string());
    }
    if !["正T", "倒T", "双向", "不做"].contains(&input.bias.as_str()) {
        return Err("方向偏好应为 正T / 倒T / 双向 / 不做".to_string());
    }
    if !(0..=20).contains(&input.max_trades) {
        return Err("最大次数应在 0–20 之间".to_string());
    }
    if !(0.0..=10.0).contains(&input.risk_budget_pct) {
        return Err("风险预算应在 0–10% 之间".to_string());
    }
    if input.levels.len() > 10 {
        return Err("关键价位最多 10 个".to_string());
    }
    for level in &input.levels {
        if level.label.trim().is_empty() || level.label.chars().count() > 20 {
            return Err("关键价位名称应为 1–20 字".to_string());
        }
        if level.price <= 0.0 {
            return Err("关键价位必须大于 0".to_string());
        }
    }

    let levels_json = serde_json::to_string(&input.levels).map_err(|e| e.to_string())?;
    let note = clean(input.note.clone(), 500);
    let reason = clean(input.reason.clone(), 200);
    let existing = get(conn, day, symbol)?;
    let is_open = opened(day);
    let changed = existing
        .as_ref()
        .map(|p| !same_plan(p, input))
        .unwrap_or(true);
    let must_reason = is_open || existing.as_ref().map(|p| p.locked).unwrap_or(false);

    if must_reason && changed && reason.is_none() {
        return Err("计划已锁定，修改必须填写理由".to_string());
    }

    let locked = is_open || existing.as_ref().map(|p| p.locked).unwrap_or(false);
    let locked_reason = if changed {
        reason
            .clone()
            .or_else(|| existing.as_ref().and_then(|p| p.locked_reason.clone()))
    } else {
        existing.as_ref().and_then(|p| p.locked_reason.clone())
    };
    let now = Utc::now().to_rfc3339();

    let before_json = existing
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| e.to_string())?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("开启计划事务失败: {e}"))?;

    tx.execute(
        "INSERT INTO daily_plans
         (trade_day, symbol, bias, max_trades, risk_budget_pct, levels_json, note,
          locked, locked_reason, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
         ON CONFLICT(trade_day, symbol) DO UPDATE SET
           bias = excluded.bias,
           max_trades = excluded.max_trades,
           risk_budget_pct = excluded.risk_budget_pct,
           levels_json = excluded.levels_json,
           note = excluded.note,
           locked = excluded.locked,
           locked_reason = excluded.locked_reason,
           updated_at = excluded.updated_at",
        params![
            day,
            symbol,
            &input.bias,
            input.max_trades,
            input.risk_budget_pct,
            levels_json,
            note,
            if locked { 1 } else { 0 },
            locked_reason,
            now
        ],
    )
    .map_err(|e| format!("保存计划失败: {e}"))?;

    let after = get(&tx, day, symbol)?.ok_or_else(|| "计划保存后读取失败".to_string())?;

    if changed {
        let after_json = serde_json::to_string(&after).map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO plan_audit
             (trade_day, symbol, changed_at, reason, before_json, after_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![day, symbol, now, reason, before_json, after_json],
        )
        .map_err(|e| format!("写入计划审计失败: {e}"))?;
    }

    tx.commit().map_err(|e| format!("提交计划失败: {e}"))?;
    Ok(after)
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

    fn input() -> PlanInput {
        PlanInput {
            day: "2026-10-19".to_string(),
            symbol: "sz000858".to_string(),
            bias: "正T".to_string(),
            max_trades: 3,
            risk_budget_pct: 0.8,
            levels: vec![PlanLevel {
                label: "VWAP".to_string(),
                price: 72.8,
            }],
            note: Some("只做回踩".to_string()),
            reason: None,
        }
    }

    #[test]
    fn saves_and_reads_plan() {
        let c = conn();
        let p = save(&c, &input()).expect("save");
        assert_eq!(p.bias, "正T");
        assert_eq!(p.levels.len(), 1);
        assert_eq!(
            get(&c, "2026-10-19", "sz000858")
                .unwrap()
                .unwrap()
                .max_trades,
            3
        );
    }

    #[test]
    fn rejects_bad_direction() {
        let c = conn();
        let mut i = input();
        i.bias = "追涨".to_string();
        assert!(save(&c, &i).is_err());
    }

    #[test]
    fn audit_records_each_change() {
        let c = conn();
        save(&c, &input()).expect("create");
        let first = audit_list(&c, "2026-10-19", "sz000858").expect("audit");
        assert_eq!(first.len(), 1);
        assert!(first[0].before.is_none());
        assert_eq!(first[0].after.max_trades, 3);

        let mut second = input();
        second.max_trades = 2;
        save(&c, &second).expect("update");
        let list = audit_list(&c, "2026-10-19", "sz000858").expect("audit");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].before.as_ref().unwrap().max_trades, 3);
        assert_eq!(list[0].after.max_trades, 2);

        c.execute(
            "UPDATE daily_plans SET locked = 1 WHERE trade_day = ?1 AND symbol = ?2",
            params!["2026-10-19", "sz000858"],
        )
        .expect("lock");
        let mut third = input();
        third.max_trades = 1;
        third.reason = Some("盘面转弱".to_string());
        save(&c, &third).expect("reason update");
        let list = audit_list(&c, "2026-10-19", "sz000858").expect("audit");
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].reason.as_deref(), Some("盘面转弱"));

        save(&c, &third).expect("same plan");
        assert_eq!(audit_list(&c, "2026-10-19", "sz000858").unwrap().len(), 3);
    }

    #[test]
    fn open_modification_requires_reason() {
        let c = conn();
        let mut i = input();
        i.day = "2026-10-19".to_string();
        save(&c, &i).expect("first save");
        c.execute(
            "UPDATE daily_plans SET locked = 1 WHERE trade_day = ?1 AND symbol = ?2",
            params![i.day, i.symbol],
        )
        .expect("lock");
        i.max_trades = 1;
        assert!(save(&c, &i).is_err());
        i.reason = Some("临时调整".to_string());
        let p = save(&c, &i).expect("with reason");
        assert!(p.locked);
        assert_eq!(p.locked_reason.as_deref(), Some("临时调整"));
    }
}
