use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StrategyConfig {
    pub symbol: String,
    pub base_qty: i64,
    pub base_cost: f64,
    pub cash: f64,
    pub max_t_ratio: f64,
    pub z_entry: f64,
    pub target_lo: f64,
    pub target_hi: f64,
    pub stop_pct: f64,
    pub max_signals: i64,
    pub cooldown_min: i64,
    pub session_start: String,
    pub session_end: String,
    pub paused: bool,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            symbol: "sh600519".to_string(),
            base_qty: 1000,
            base_cost: 0.0,
            cash: 800_000.0,
            max_t_ratio: 0.5,
            z_entry: 1.2,
            target_lo: 0.0045,
            target_hi: 0.0090,
            stop_pct: 0.0035,
            max_signals: 4,
            cooldown_min: 15,
            session_start: "09:45".to_string(),
            session_end: "14:50".to_string(),
            paused: false,
        }
    }
}

pub fn load_for(conn: &Connection, symbol: &str) -> StrategyConfig {
    conn.query_row(
        "SELECT config_json FROM symbol_configs WHERE symbol = ?1",
        params![symbol],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
    .and_then(|text| serde_json::from_str::<StrategyConfig>(&text).ok())
    .map(|mut cfg| {
        cfg.symbol = symbol.to_string();
        cfg
    })
    .unwrap_or_else(|| {
        let mut cfg = StrategyConfig::default();
        cfg.symbol = symbol.to_string();
        cfg
    })
}

pub fn save(conn: &Connection, cfg: &StrategyConfig) -> Result<(), String> {
    let text = serde_json::to_string(cfg).map_err(|e| format!("序列化配置失败: {e}"))?;
    conn.execute(
        "INSERT INTO symbol_configs (symbol, config_json, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(symbol) DO UPDATE SET
           config_json = excluded.config_json,
           updated_at = excluded.updated_at",
        params![
            &cfg.symbol,
            text,
            chrono::Utc::now().to_rfc3339()
        ],
    )
    .map_err(|e| format!("保存配置失败: {e}"))?;
    Ok(())
}

pub fn list(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT symbol FROM symbol_configs ORDER BY updated_at DESC, symbol")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

pub fn exists(conn: &Connection, symbol: &str) -> Result<bool, String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbol_configs WHERE symbol = ?1",
            params![symbol],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(count > 0)
}

/// 启动时取活跃配置：优先自选列表，其次迁移旧的 meta.config，最后默认。
pub fn ensure_default(conn: &Connection) -> Result<StrategyConfig, String> {
    let symbols = list(conn)?;
    if let Some(first) = symbols.first() {
        return Ok(load_for(conn, first));
    }
    let legacy = conn
        .query_row("SELECT value FROM meta WHERE key = 'config'", [], |row| {
            row.get::<_, String>(0)
        })
        .optional()
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str::<StrategyConfig>(&text).ok())
        .unwrap_or_default();
    save(conn, &legacy)?;
    Ok(legacy)
}

fn valid_hm(value: &str) -> bool {
    let parts: Vec<&str> = value.split(':').collect();
    if parts.len() != 2 {
        return false;
    }
    let h: u32 = match parts[0].parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let m: u32 = match parts[1].parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    h < 24 && m < 60
}

pub fn validate(cfg: &StrategyConfig) -> Result<(), String> {
    let symbol = cfg.symbol.as_str();
    let prefix_ok =
        symbol.starts_with("sh") || symbol.starts_with("sz") || symbol.starts_with("bj");
    if symbol.len() != 8 || !prefix_ok || !symbol[2..].chars().all(|c| c.is_ascii_digit()) {
        return Err("标的代码格式应为 sh600519 / sz000858 / bj430047".to_string());
    }
    if cfg.base_qty < 0 {
        return Err("底仓数量不能为负".to_string());
    }
    if cfg.base_cost < 0.0 {
        return Err("底仓成本不能为负".to_string());
    }
    if cfg.cash < 0.0 {
        return Err("可用资金不能为负".to_string());
    }
    if !(0.1..=1.0).contains(&cfg.max_t_ratio) {
        return Err("T 仓上限应在 10%–100% 之间".to_string());
    }
    if !(0.5..=5.0).contains(&cfg.z_entry) {
        return Err("z 入场阈值应在 0.5–5.0 之间".to_string());
    }
    if !(0.001..=0.05).contains(&cfg.target_lo) {
        return Err("目标价差下限应在 0.1%–5% 之间".to_string());
    }
    if cfg.target_hi < cfg.target_lo || cfg.target_hi > 0.05 {
        return Err("目标价差上限应不小于下限且不超过 5%".to_string());
    }
    if !(0.001..=0.05).contains(&cfg.stop_pct) {
        return Err("止损应在 0.1%–5% 之间".to_string());
    }
    if !(1..=20).contains(&cfg.max_signals) {
        return Err("每日做T次数应在 1–20 之间".to_string());
    }
    if !(1..=120).contains(&cfg.cooldown_min) {
        return Err("冷却时间应在 1–120 分钟之间".to_string());
    }
    if !valid_hm(&cfg.session_start) || !valid_hm(&cfg.session_end) {
        return Err("交易时段格式应为 HH:MM".to_string());
    }
    if cfg.session_start >= cfg.session_end {
        return Err("开始时间必须早于结束时间".to_string());
    }
    Ok(())
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
    fn per_symbol_configs_roundtrip() {
        let c = conn();

        let mut a = StrategyConfig::default();
        a.symbol = "sh600519".to_string();
        a.base_qty = 1000;
        save(&c, &a).expect("save a");

        let mut b = StrategyConfig::default();
        b.symbol = "sz000858".to_string();
        b.base_qty = 500;
        b.z_entry = 1.5;
        b.paused = true;
        save(&c, &b).expect("save b");

        let symbols = list(&c).expect("list");
        assert_eq!(symbols.len(), 2);
        assert!(symbols.contains(&"sh600519".to_string()));
        assert!(symbols.contains(&"sz000858".to_string()));

        let loaded = load_for(&c, "sz000858");
        assert_eq!(loaded.base_qty, 500);
        assert!((loaded.z_entry - 1.5).abs() < 1e-9);
        assert!(loaded.paused);

        // 覆盖保存不应新增行
        b.base_qty = 800;
        save(&c, &b).expect("resave b");
        assert_eq!(list(&c).unwrap().len(), 2);
        assert_eq!(load_for(&c, "sz000858").base_qty, 800);
    }

    #[test]
    fn default_is_valid() {
        validate(&StrategyConfig::default()).expect("default valid");
    }

    #[test]
    fn rejects_bad_symbol_and_range() {
        let mut c = StrategyConfig::default();
        c.symbol = "600519".to_string();
        assert!(validate(&c).is_err());

        c.symbol = "sh600519".to_string();
        c.max_t_ratio = 3.0;
        assert!(validate(&c).is_err());
    }

    #[test]
    fn ensure_default_migrates_legacy_meta() {
        let c = conn();
        let mut legacy = StrategyConfig::default();
        legacy.symbol = "sh601318".to_string();
        legacy.base_qty = 300;
        c.execute(
            "INSERT INTO meta (key, value) VALUES ('config', ?1)",
            params![serde_json::to_string(&legacy).unwrap()],
        )
        .expect("legacy");

        let cfg = ensure_default(&c).expect("ensure");
        assert_eq!(cfg.symbol, "sh601318");
        assert_eq!(cfg.base_qty, 300);
        assert_eq!(list(&c).unwrap().len(), 1);
    }
}
