use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use encoding_rs::GB18030;
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::ledger::{self, FillInput};
use crate::store;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatementRowCheck {
    pub line: i64,
    pub t: String,
    pub side: String,
    pub price: f64,
    pub qty: i64,
    pub already: bool,
    pub ok: bool,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatementPrecheck {
    pub ok: bool,
    pub total: i64,
    pub rows: Vec<StatementRowCheck>,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub imported: i64,
    pub duplicated: i64,
    pub mismatched: i64,
    pub manual_only: i64,
    pub rows: i64,
    pub errors: Vec<String>,
    pub message: String,
}

struct ParsedRow {
    t: String,
    side: String,
    price: f64,
    qty: i64,
    fee: f64,
}

fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => {
            let (text, _, _) = GB18030.decode(bytes);
            text.to_string()
        }
    }
}

fn norm_side(raw: &str) -> Option<&'static str> {
    let s = raw.trim();
    if s.contains('买') || s.eq_ignore_ascii_case("B") || s.eq_ignore_ascii_case("BUY") {
        Some("BUY")
    } else if s.contains('卖') || s.eq_ignore_ascii_case("S") || s.eq_ignore_ascii_case("SELL") {
        Some("SELL")
    } else {
        None
    }
}

fn norm_time(raw: &str) -> Option<String> {
    let token = raw.split_whitespace().last()?;
    let parts: Vec<&str> = token.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let h: u32 = parts[0].trim().parse().ok()?;
    let m: u32 = parts[1].trim().parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some(format!("{h:02}:{m:02}"))
}

fn parse_num(raw: &str) -> Option<f64> {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    cleaned.parse::<f64>().ok()
}

fn find_col(rec: &csv::StringRecord, keys: &[&str]) -> Option<usize> {
    rec.iter()
        .position(|cell| keys.iter().any(|k| cell.contains(k)))
}

fn is_header(rec: &csv::StringRecord) -> bool {
    rec.iter().any(|cell| {
        cell.contains("时间")
            || cell.contains("成交")
            || cell.contains("买卖")
            || cell.contains("数量")
            || cell.contains("价格")
    })
}

/// 解析 CSV。返回 (有效行, 错误列表, 原始行数)。
fn parse(text: &str) -> (Vec<ParsedRow>, Vec<String>, i64) {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());

    let records: Vec<csv::StringRecord> = rdr.records().filter_map(|r| r.ok()).collect();
    if records.is_empty() {
        return (Vec::new(), vec!["文件为空".to_string()], 0);
    }

    let has_header = is_header(&records[0]);
    let header = if has_header {
        Some(records[0].clone())
    } else {
        None
    };

    let (time_idx, side_idx, price_idx, qty_idx, fee_idx) = match &header {
        Some(h) => (
            find_col(h, &["时间"]).unwrap_or(0),
            find_col(h, &["买卖", "方向", "操作", "业务"]).unwrap_or(1),
            find_col(h, &["成交价格", "成交价", "价格"]).unwrap_or(2),
            find_col(h, &["成交数量", "数量", "成交量"]).unwrap_or(3),
            find_col(h, &["手续费", "佣金", "费用"]),
        ),
        None => (0, 1, 2, 3, Some(4)),
    };

    let mut rows: Vec<ParsedRow> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let start = if has_header { 1 } else { 0 };
    let mut total = 0i64;

    for (offset, rec) in records.iter().enumerate().skip(start) {
        if rec.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        total += 1;
        let raw_line = rec.iter().collect::<Vec<_>>().join(" | ");

        let t = rec.get(time_idx).and_then(norm_time);
        let side = rec.get(side_idx).and_then(norm_side);
        let price = rec.get(price_idx).and_then(parse_num);
        let qty = rec.get(qty_idx).and_then(parse_num);
        let fee = fee_idx
            .and_then(|i| rec.get(i))
            .and_then(parse_num)
            .unwrap_or(0.0);

        match (t, side, price, qty) {
            (Some(t), Some(side), Some(price), Some(qty)) if price > 0.0 && qty > 0.0 => {
                rows.push(ParsedRow {
                    t,
                    side: side.to_string(),
                    price,
                    qty: qty as i64,
                    fee,
                });
            }
            _ => {
                if errors.len() < 20 {
                    errors.push(format!("第 {} 行无法识别: {}", offset + 1, raw_line));
                }
            }
        }
    }

    (rows, errors, total)
}

pub fn precheck(
    conn: &rusqlite::Connection,
    day: &str,
    symbol: &str,
    bytes: &[u8],
    cash: f64,
    base_qty: i64,
    base_cost: f64,
) -> Result<StatementPrecheck, String> {
    let text = decode(bytes);
    let (rows, parse_errors, total) = parse(&text);
    let existing = ledger::list_by_day(conn, day, symbol)?;
    let keys: HashSet<(String, String, i64, i64)> = existing
        .iter()
        .filter(|f| !f.voided)
        .map(|f| {
            (
                f.t.clone(),
                f.side.clone(),
                (f.price * 100.0).round() as i64,
                f.qty,
            )
        })
        .collect();
    let raw = store::load_minutes(conn, symbol, day).unwrap_or_default();
    let amount = raw.last().map(|b| b.cum_amt).unwrap_or(0.0);
    let low = raw.iter().map(|b| b.price).fold(f64::MAX, f64::min);
    let high = raw.iter().map(|b| b.price).fold(f64::MIN, f64::max);
    let mut available = ledger::day(conn, day, symbol, base_qty, base_cost)
        .map(|d| d.position.available)
        .unwrap_or(base_qty);
    let mut cash_left = cash;
    let mut blockers = Vec::new();
    let mut warnings: Vec<String> = parse_errors.clone();

    if rows.is_empty() {
        blockers.push("没有可导入的成交行".to_string());
    }
    if amount <= 0.0 {
        warnings.push(format!("{day} 无成交额或行情未缓存，跳过价格区间校验"));
    }

    let mut checks = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let line = i as i64 + 1;
        let price_cents = (row.price * 100.0).round() as i64;
        let key = (row.t.clone(), row.side.clone(), price_cents, row.qty);
        let already = keys.contains(&key);
        let mut row_blockers = Vec::new();
        let mut row_warnings = Vec::new();

        if row.price <= 0.0 {
            row_blockers.push(format!("价格非法 {:.2}", row.price));
        }
        if row.qty <= 0 || row.qty % 100 != 0 {
            row_blockers.push(format!("数量非整手 {}", row.qty));
        }
        if amount > 0.0 && raw.len() > 1 {
            let in_range = row.price >= low * 0.995 && row.price <= high * 1.005;
            if !in_range {
                row_blockers.push(format!(
                    "价格 {:.2} 不在当日区间 {:.2}–{:.2}",
                    row.price, low, high
                ));
            }
        }
        if !already {
            if row.side == "BUY" {
                let need = row.price * row.qty as f64;
                if cash_left < need {
                    row_warnings.push(format!(
                        "可用资金不足：需要 {:.2}，剩余 {:.2}",
                        need, cash_left
                    ));
                }
                cash_left -= need;
            } else {
                if available < row.qty {
                    row_warnings.push(format!(
                        "T+1 可用不足：需要 {}，剩余 {}",
                        row.qty, available
                    ));
                }
                available -= row.qty;
            }
        }

        checks.push(StatementRowCheck {
            line,
            t: row.t.clone(),
            side: row.side.clone(),
            price: row.price,
            qty: row.qty,
            already,
            ok: row_blockers.is_empty(),
            blockers: row_blockers,
            warnings: row_warnings,
        });
    }

    for check in &checks {
        for b in &check.blockers {
            blockers.push(format!("第 {} 条：{b}", check.line));
        }
    }
    if !parse_errors.is_empty() {
        warnings.push(format!(
            "解析告警 {} 条，已忽略无法识别的行",
            parse_errors.len()
        ));
    }

    Ok(StatementPrecheck {
        ok: blockers.is_empty(),
        total,
        rows: checks,
        blockers,
        warnings,
    })
}

pub fn import(
    conn: &mut Connection,
    day: &str,
    symbol: &str,
    file_name: &str,
    bytes: &[u8],
) -> Result<ImportResult, String> {
    let text = decode(bytes);
    let (rows, errors, total) = parse(&text);

    // 对账：手工录入但不在交割单里的成交，必须显式暴露。
    let existing = ledger::list_by_day(conn, day, symbol)?;
    let statement_keys: HashSet<(String, String, i64, i64)> = rows
        .iter()
        .map(|r| {
            (
                r.t.clone(),
                r.side.clone(),
                (r.price * 100.0).round() as i64,
                r.qty,
            )
        })
        .collect();
    let manual_only = existing
        .iter()
        .filter(|f| f.source == "manual" && !f.voided)
        .filter(|f| {
            !statement_keys.contains(&(
                f.t.clone(),
                f.side.clone(),
                (f.price * 100.0).round() as i64,
                f.qty,
            ))
        })
        .count() as i64;

    let tx = conn
        .transaction()
        .map_err(|e| format!("开启导入事务失败: {e}"))?;

    let mut imported = 0i64;
    let mut duplicated = 0i64;

    for row in &rows {
        let price_cents = (row.price * 100.0).round() as i64;
        let input = FillInput {
            client_id: format!(
                "imp:{symbol}:{day}:{}:{}:{}:{}",
                row.t, row.side, price_cents, row.qty
            ),
            symbol: symbol.to_string(),
            day: day.to_string(),
            t: row.t.clone(),
            side: row.side.clone(),
            price: row.price,
            qty: row.qty,
            fee: Some(row.fee),
            signal_id: None,
            note: Some(format!("import:{file_name}")),
        };
        if ledger::insert_imported(&tx, &input)? {
            imported += 1;
        } else {
            duplicated += 1;
        }
    }

    ledger::rebuild(&tx, day, symbol)?;

    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    let file_hash = format!("{:x}", hasher.finish());
    tx.execute(
        "INSERT OR REPLACE INTO statements
         (trade_day, file_name, file_hash, imported_at, rows, mismatched)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            day,
            file_name,
            file_hash,
            chrono::Utc::now().to_rfc3339(),
            total,
            errors.len() as i64
        ],
    )
    .map_err(|e| format!("记录导入失败: {e}"))?;

    tx.commit().map_err(|e| format!("提交导入失败: {e}"))?;

    let message = if rows.is_empty() {
        "没有识别到有效成交行，请检查表头与格式".to_string()
    } else {
        let mut text = format!(
            "识别 {} 行，导入 {} 行，重复 {} 行，异常 {} 行",
            rows.len(),
            imported,
            duplicated,
            errors.len()
        );
        if manual_only > 0 {
            text.push_str(&format!("；手工录入 {manual_only} 笔不在交割单中"));
        }
        text
    };

    Ok(ImportResult {
        imported,
        duplicated,
        mismatched: errors.len() as i64,
        manual_only,
        rows: total,
        errors,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().expect("memory db");
        c.execute_batch(include_str!("../../../contracts/schema.sql"))
            .expect("schema");
        c.execute_batch(
            "INSERT INTO minute_raw (symbol, trade_day, source, t, price_cents, cum_vol, cum_amt_cents)
             VALUES ('sz000858', '2026-10-19', 'real', '09:30', 10000, 1000, 100000000);
             INSERT INTO minute_raw (symbol, trade_day, source, t, price_cents, cum_vol, cum_amt_cents)
             VALUES ('sz000858', '2026-10-19', 'real', '10:30', 10100, 2000, 201000000);
             INSERT INTO minute_raw (symbol, trade_day, source, t, price_cents, cum_vol, cum_amt_cents)
             VALUES ('sz000858', '2026-10-19', 'real', '15:00', 10050, 3000, 301500000);",
        )
        .expect("minutes");
        c
    }

    #[test]
    fn prechecks_valid_statement_rows() {
        let c = db();
        let csv = "成交时间,买卖标志,成交价格,成交数量,手续费\n\
                   2026-10-19 10:00:00,买入,100.50,100,5\n\
                   2026-10-19 10:30:00,卖出,101.00,100,5\n";
        let check = precheck(
            &c,
            "2026-10-19",
            "sz000858",
            csv.as_bytes(),
            1_000_000.0,
            1000,
            100.0,
        )
        .expect("precheck");
        assert!(check.ok, "{:?}", check.blockers);
        assert_eq!(check.rows.len(), 2);
        assert!(check.rows.iter().all(|r| r.ok));
    }

    #[test]
    fn rejects_statement_price_outside_day_range() {
        let c = db();
        let csv = "成交时间,买卖标志,成交价格,成交数量,手续费\n\
                   2026-10-19 10:00:00,买入,200.00,100,5\n";
        let check = precheck(
            &c,
            "2026-10-19",
            "sz000858",
            csv.as_bytes(),
            1_000_000.0,
            1000,
            100.0,
        )
        .expect("precheck");
        assert!(!check.ok);
        assert!(check.blockers.iter().any(|b| b.contains("当日区间")));
    }

    #[test]
    fn parses_common_broker_header() {
        let csv = "成交时间,买卖标志,成交价格,成交数量,手续费\n\
                   2026-10-19 09:47:12,买入,100.00,100,5\n\
                   2026-10-19 10:12:03,卖出,101.00,100,5\n";
        let (rows, errors, total) = parse(csv);
        assert_eq!(total, 2);
        assert!(errors.is_empty());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].t, "09:47");
        assert_eq!(rows[0].side, "BUY");
        assert_eq!(rows[1].side, "SELL");
    }

    #[test]
    fn rejects_bad_rows() {
        let csv = "时间,方向,价格,数量\n09:47,买入,abc,100\n";
        let (rows, errors, _) = parse(csv);
        assert!(rows.is_empty());
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn decodes_gb18030() {
        let (encoded, _, _) = GB18030.encode("时间,方向,价格,数量\n09:47,买入,100,100\n");
        let text = decode(&encoded);
        assert!(text.contains("买入"));
    }

    #[test]
    fn flags_manual_fills_missing_from_statement() {
        let mut c = Connection::open_in_memory().expect("memory db");
        c.execute_batch(include_str!("../../../contracts/schema.sql"))
            .expect("schema");

        let manual = FillInput {
            client_id: "manual-1".to_string(),
            symbol: "sh600519".to_string(),
            day: "2026-10-19".to_string(),
            t: "09:40".to_string(),
            side: "BUY".to_string(),
            price: 100.0,
            qty: 100,
            fee: Some(5.0),
            signal_id: None,
            note: None,
        };
        ledger::submit(&mut c, &manual).expect("manual fill");

        let csv = "时间,方向,价格,数量\n10:00,卖出,101,100\n";
        let result =
            import(&mut c, "2026-10-19", "sh600519", "s.csv", csv.as_bytes()).expect("import");
        assert_eq!(result.manual_only, 1);
        assert!(result.message.contains("不在交割单中"));
    }
}
