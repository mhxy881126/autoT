use encoding_rs::GB18030;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub symbol: String,
    pub name: String,
    pub price: f64,
    pub prev_close: f64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub time: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawBar {
    pub t: String,
    pub price: f64,
    pub cum_vol: i64,
    pub cum_amt: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayBar {
    pub d: String,
    pub o: f64,
    pub c: f64,
    pub h: f64,
    pub l: f64,
    pub v: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub t: String,
    pub price: f64,
    pub avg: f64,
    pub vwap: f64,
    pub z: f64,
    pub slope: f64,
    pub vol_ratio: f64,
    pub regime: String,
    pub latency_ms: u64,
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .build()
        .expect("failed to build http client")
}

/// 腾讯快照接口返回 GB18030，必须显式解码；
/// 用 from_utf8_lossy 会把中文名和部分字段变成替换字符，价格解析随之失败。
pub async fn fetch_quote(client: &reqwest::Client, symbol: &str) -> Result<Quote, String> {
    let url = format!("https://qt.gtimg.cn/q={symbol}");
    let bytes = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("读取失败: {e}"))?;

    let (text, _, had_errors) = GB18030.decode(&bytes);
    if had_errors {
        return Err("GB18030 解码失败".to_string());
    }

    let body = text
        .split("=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .ok_or_else(|| "行情格式异常".to_string())?;

    let f: Vec<&str> = body.split('~').collect();
    if f.len() < 35 {
        return Err(format!("行情字段不足: {}", f.len()));
    }

    let parse = |v: &str, name: &str| -> Result<f64, String> {
        v.parse::<f64>()
            .map_err(|_| format!("{name}解析失败: {v:?}"))
    };

    Ok(Quote {
        symbol: symbol.to_string(),
        name: f[1].to_string(),
        price: parse(f[3], "现价")?,
        prev_close: parse(f[4], "昨收")?,
        open: parse(f[5], "开盘")?,
        high: parse(f[33], "最高")?,
        low: parse(f[34], "最低")?,
        time: f[30].to_string(),
    })
}

/// 当日 1 分钟分时（09:30-11:30 / 13:00-15:00），返回 (交易日, 原始累计量)。
pub async fn fetch_minute(
    client: &reqwest::Client,
    symbol: &str,
) -> Result<(String, Vec<RawBar>), String> {
    let url = format!("https://web.ifzq.gtimg.cn/appstock/app/minute/query?code={symbol}");
    let body: serde_json::Value = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("分时请求失败: {e}"))?
        .json()
        .await
        .map_err(|e| format!("分时解析失败: {e}"))?;

    let inner = body
        .get("data")
        .and_then(|v| v.get(symbol))
        .and_then(|v| v.get("data"))
        .ok_or_else(|| "分时结构异常".to_string())?;

    let date = inner
        .get("date")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let arr = inner
        .get("data")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "分时数组缺失".to_string())?;

    let mut out: Vec<RawBar> = Vec::with_capacity(arr.len());
    let mut seen = std::collections::HashSet::new();

    for item in arr {
        let line = match item.as_str() {
            Some(s) => s,
            None => continue,
        };
        let p: Vec<&str> = line.split_whitespace().collect();
        if p.len() < 4 {
            continue;
        }
        let t = p[0];
        let in_session = (t >= "0930" && t <= "1130") || (t >= "1300" && t <= "1500");
        if !in_session || !seen.insert(t.to_string()) {
            continue;
        }
        let price = p[1]
            .parse::<f64>()
            .map_err(|_| format!("分时价格解析失败: {line}"))?;
        let cum_vol = p[2]
            .parse::<f64>()
            .map_err(|_| format!("分时成交量解析失败: {line}"))?;
        let cum_amt = p[3]
            .parse::<f64>()
            .map_err(|_| format!("分时成交额解析失败: {line}"))?;

        out.push(RawBar {
            t: format!("{}:{}", &t[..2], &t[2..]),
            price,
            cum_vol: cum_vol as i64,
            cum_amt,
        });
    }

    out.sort_by(|a, b| a.t.cmp(&b.t));
    Ok((date, out))
}

/// 前复权日 K。腾讯字段顺序：date, open, close, high, low, volume。
pub async fn fetch_daily(
    client: &reqwest::Client,
    symbol: &str,
    limit: usize,
) -> Result<Vec<DayBar>, String> {
    let url = format!(
        "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get?param={symbol},day,,,{limit},qfq"
    );
    let body: serde_json::Value = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("日K请求失败: {e}"))?
        .json()
        .await
        .map_err(|e| format!("日K解析失败: {e}"))?;

    let node = body
        .get("data")
        .and_then(|v| v.get(symbol))
        .ok_or_else(|| "日K结构异常".to_string())?;

    let arr = node
        .get("qfqday")
        .or_else(|| node.get("day"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| "日K数组缺失".to_string())?;

    let mut out = Vec::with_capacity(arr.len());
    for row in arr {
        let a = match row.as_array() {
            Some(a) if a.len() >= 6 => a,
            _ => continue,
        };
        let num = |idx: usize| -> f64 {
            a[idx]
                .as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0)
        };
        out.push(DayBar {
            d: a[0].as_str().unwrap_or("").to_string(),
            o: num(1),
            c: num(2),
            h: num(3),
            l: num(4),
            v: num(5),
        });
    }
    Ok(out)
}

/// "20261019161457" -> "16:14"
pub fn clock(ts: &str) -> String {
    if ts.len() >= 12 {
        format!("{}:{}", &ts[8..10], &ts[10..12])
    } else {
        "--:--".to_string()
    }
}

/// 批量快照：qt.gtimg.cn/q=sh600519,sz000858
pub async fn fetch_quotes(
    client: &reqwest::Client,
    symbols: &[String],
) -> Result<Vec<Quote>, String> {
    if symbols.is_empty() {
        return Ok(Vec::new());
    }
    let url = format!("https://qt.gtimg.cn/q={}", symbols.join(","));
    let bytes = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("批量行情请求失败: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("批量行情读取失败: {e}"))?;

    let (text, _, _) = GB18030.decode(&bytes);
    let mut out = Vec::new();

    for chunk in text.split(';') {
        let line = chunk.trim();
        if !line.starts_with("v_") {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let symbol = key.trim_start_matches("v_").trim();
        let body = value.trim().trim_matches('"');
        let f: Vec<&str> = body.split('~').collect();
        if f.len() < 35 {
            continue;
        }
        let num = |v: &str| v.parse::<f64>().unwrap_or(0.0);
        out.push(Quote {
            symbol: symbol.to_string(),
            name: f[1].to_string(),
            price: num(f[3]),
            prev_close: num(f[4]),
            open: num(f[5]),
            high: num(f[33]),
            low: num(f[34]),
            time: f[30].to_string(),
        });
    }

    Ok(out)
}
