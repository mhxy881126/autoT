//! 风控 v1：日亏损、连亏、交易次数、流动性。
//! 只做“是否允许新开 T”的判断，不修改账本。

use serde::Serialize;

use crate::config::StrategyConfig;
use crate::ledger::LedgerDay;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskState {
    pub blocked: bool,
    pub reasons: Vec<String>,
    pub daily_pnl: f64,
    pub daily_limit: f64,
    pub consecutive_losses: i64,
    pub trades_today: i64,
    pub max_trades: i64,
    pub position_notional: f64,
    pub liquidity_cap: f64,
}

pub fn evaluate(
    cfg: &StrategyConfig,
    ledger: &LedgerDay,
    equity: f64,
    day_amount: f64,
    price: f64,
) -> RiskState {
    let mut reasons = Vec::new();

    let daily_limit = (equity * cfg.daily_loss_limit).max(0.0);
    let daily_pnl = ledger.realized_net;
    if daily_limit > 0.0 && daily_pnl <= -daily_limit {
        reasons.push(format!(
            "日亏损 {:.2} 已达上限 -{:.2}",
            daily_pnl, daily_limit
        ));
    }

    let mut consecutive_losses = 0i64;
    for pair in ledger.pairs.iter().rev() {
        if pair.status != "CLOSED" {
            continue;
        }
        match pair.net {
            Some(n) if n < 0.0 => consecutive_losses += 1,
            _ => break,
        }
    }
    if cfg.max_consecutive_losses > 0 && consecutive_losses >= cfg.max_consecutive_losses {
        reasons.push(format!(
            "连续亏损 {} 笔，达到上限 {}",
            consecutive_losses, cfg.max_consecutive_losses
        ));
    }

    let trades_today = ledger.pairs.iter().filter(|p| p.status == "CLOSED").count() as i64;
    if cfg.max_signals > 0 && trades_today >= cfg.max_signals {
        reasons.push(format!(
            "今日已做 {} 笔，达到上限 {}",
            trades_today, cfg.max_signals
        ));
    }

    let qty = ((cfg.base_qty as f64 * cfg.max_t_ratio) as i64 / 100 * 100).max(0);
    let position_notional = qty as f64 * price;
    let liquidity_cap = day_amount * cfg.liquidity_cap;
    if liquidity_cap > 0.0 && position_notional > liquidity_cap {
        reasons.push(format!(
            "单笔名义 {:.0} 超过当日成交额 {:.2}% 上限",
            position_notional,
            cfg.liquidity_cap * 100.0
        ));
    }

    RiskState {
        blocked: !reasons.is_empty(),
        reasons,
        daily_pnl: round2(daily_pnl),
        daily_limit: round2(daily_limit),
        consecutive_losses,
        trades_today,
        max_trades: cfg.max_signals,
        position_notional: round2(position_notional),
        liquidity_cap: round2(liquidity_cap),
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{Position, TradePair};

    fn pair(net: Option<f64>, status: &str) -> TradePair {
        TradePair {
            id: 1,
            day: "2026-10-19".to_string(),
            symbol: "sh600519".to_string(),
            side: "正T".to_string(),
            open_fill_id: 1,
            close_fill_id: Some(2),
            qty: 100,
            gross: net,
            cost: None,
            net,
            status: status.to_string(),
        }
    }

    fn ledger(pairs: Vec<TradePair>) -> LedgerDay {
        let realized = pairs.iter().filter_map(|p| p.net).sum();
        LedgerDay {
            day: "2026-10-19".to_string(),
            symbol: "sh600519".to_string(),
            position: Position {
                base_qty: 1000,
                available: 1000,
                frozen: 0,
                t_qty: 0,
                base_cost: 90.0,
                updated_at: "x".to_string(),
            },
            fills: vec![],
            pairs,
            realized_net: realized,
            unrealized: 0.0,
            unresolved: 0,
        }
    }

    #[test]
    fn daily_loss_blocks() {
        let mut cfg = StrategyConfig::default();
        cfg.daily_loss_limit = 0.01;
        let l = ledger(vec![pair(Some(-1200.0), "CLOSED")]);
        let r = evaluate(&cfg, &l, 100_000.0, 0.0, 100.0);
        assert!(r.blocked);
        assert!(r.reasons.iter().any(|m| m.contains("日亏损")));
    }

    #[test]
    fn consecutive_losses_block() {
        let mut cfg = StrategyConfig::default();
        cfg.max_consecutive_losses = 3;
        cfg.daily_loss_limit = 0.1;
        let l = ledger(vec![
            pair(Some(-10.0), "CLOSED"),
            pair(Some(-20.0), "CLOSED"),
            pair(Some(-30.0), "CLOSED"),
        ]);
        let r = evaluate(&cfg, &l, 100_000.0, 0.0, 100.0);
        assert!(r.blocked);
        assert_eq!(r.consecutive_losses, 3);
    }

    #[test]
    fn liquidity_blocks_large_order() {
        let mut cfg = StrategyConfig::default();
        cfg.max_signals = 10;
        cfg.daily_loss_limit = 0.1;
        let l = ledger(vec![]);
        let r = evaluate(&cfg, &l, 100_000.0, 10_000.0, 100.0);
        assert!(r.blocked);
        assert!(r.reasons.iter().any(|m| m.contains("成交额")));
    }

    #[test]
    fn clean_state_is_not_blocked() {
        let mut cfg = StrategyConfig::default();
        cfg.daily_loss_limit = 0.1;
        let l = ledger(vec![pair(Some(10.0), "CLOSED")]);
        let r = evaluate(&cfg, &l, 100_000.0, 100_000_000.0, 100.0);
        assert!(!r.blocked);
        assert_eq!(r.trades_today, 1);
    }
}
