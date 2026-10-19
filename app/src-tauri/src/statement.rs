use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use encoding_rs::GB18030;
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::ledger::{self, FillInput};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub imported: i64,
    pub duplicated: i64,
    pub mismatched: i64,
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

pub fn import(
    conn: &mut Connection,
    day: &str,
    file_name: &str,
    bytes: &[u8],
) -> Result<ImportResult, String> {
    let text = decode(bytes);
    let (rows, errors, total) = parse(&text);

    let tx = conn
        .transaction()
        .map_err(|e| format!("开启导入事务失败: {e}"))?;

    let mut imported = 0i64;
    let mut duplicated = 0i64;

    for row in &rows {
        let price_cents = (row.price * 100.0).round() as i64;
        let input = FillInput {
            client_id: format!(
                "imp:{day}:{}:{}:{}:{}",
                row.t, row.side, price_cents, row.qty
            ),
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

    ledger::rebuild(&tx, day)?;

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
        format!(
            "识别 {} 行，导入 {} 行，重复 {} 行，异常 {} 行",
            rows.len(),
            imported,
            duplicated,
            errors.len()
        )
    };

    Ok(ImportResult {
        imported,
        duplicated,
        mismatched: errors.len() as i64,
        rows: total,
        errors,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
