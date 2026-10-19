use encoding_rs::GB18030;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub name: String,
    pub price: f64,
    pub prev_close: f64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub time: String,
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
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
        name: f[1].to_string(),
        price: parse(f[3], "现价")?,
        prev_close: parse(f[4], "昨收")?,
        open: parse(f[5], "开盘")?,
        high: parse(f[33], "最高")?,
        low: parse(f[34], "最低")?,
        time: f[30].to_string(),
    })
}
