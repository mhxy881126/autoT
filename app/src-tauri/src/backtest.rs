//! 单日做 T 回测。
//!
//! 口径：
//! - 信号在分钟收盘生成，下一根分钟收盘价作为成交价（没有分钟 OHLC，用 close 代理）。
//! - 目标/止损用后续分钟收盘价判断，触发后按目标/止损价成交。
//! - 每笔都计入佣金、印花税、过户费、双边滑点。
//! - T+1：当天买入的股份不可再卖，卖出只能消耗底仓可卖额度；每做一次 T，
//!   可卖底仓减少 qty，恢复要等下一交易日。
//! - 同一时间只允许一笔 T；未平仓前的新信号跳过。

use serde::Serialize;

use crate::config::StrategyConfig;
use crate::features::Bar;
use crate::strategy::{self, Signal};

#[derive(Debug, Clone)]
pub struct FeeModel {
    pub commission_rate: f64,
    pub min_commission: f64,
    pub stamp_rate: f64,
    pub transfer_rate: f64,
    pub slip_bps: f64,
}

impl Default for FeeModel {
    fn default() -> Self {
        Self {
            commission_rate: 0.00025,
            min_commission: 5.0,
            stamp_rate: 0.0005,
            transfer_rate: 0.00001,
            slip_bps: 2.0,
        }
    }
}

impl FeeModel {
    fn buy_fee(&self, notional: f64) -> f64 {
        (notional * self.commission_rate).max(self.min_commission) + notional * self.transfer_rate
    }

    fn sell_fee(&self, notional: f64) -> f64 {
        (notional * self.commission_rate).max(self.min_commission)
            + notional * self.stamp_rate
            + notional * self.transfer_rate
    }

    fn slip_rate(&self) -> f64 {
        self.slip_bps / 10_000.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BtTrade {
    pub signal_id: String,
    pub side: String,
    pub entry_t: String,
    pub entry_price: f64,
    pub exit_t: String,
    pub exit_price: f64,
    pub qty: i64,
    pub gross: f64,
    pub fees: f64,
    pub slippage: f64,
    pub net: f64,
    pub exit_reason: String,
    pub bars_held: i64,
    pub mae_pct: f64,
    pub mfe_pct: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktestSummary {
    pub trades: i64,
    pub wins: i64,
    pub losses: i64,
    pub gross: f64,
    pub fees: f64,
    pub slippage: f64,
    pub net: f64,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub max_drawdown: f64,
    pub max_drawdown_pct: f64,
    pub avg_hold_min: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktestResult {
    pub symbol: String,
    pub day: String,
    pub trades: Vec<BtTrade>,
    pub summary: BacktestSummary,
    pub skipped: Vec<String>,
    pub cash_end: f64,
    pub available_base: i64,
}

pub fn run_day(
    symbol: &str,
    day: &str,
    bars: &[Bar],
    cfg: &StrategyConfig,
    fees: &FeeModel,
    cash: f64,
) -> BacktestResult {
    let signals = strategy::scan(bars, day, cfg);
    let accepted: Vec<Signal> = signals.into_iter().filter(|s| s.accepted).collect();
    execute_signals(symbol, day, bars, &accepted, cfg, fees, cash)
}

fn execute_signals(
    symbol: &str,
    day: &str,
    bars: &[Bar],
    signals: &[Signal],
    cfg: &StrategyConfig,
    fees: &FeeModel,
    cash: f64,
) -> BacktestResult {
    let mut trades: Vec<BtTrade> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut cash_balance = cash;
    let mut available_base = cfg.base_qty.max(0);
    let mut last_exit_idx: isize = -1;

    for sig in signals {
        let Some(sig_idx) = bars.iter().position(|b| b.t == sig.t) else {
            skipped.push(format!("{}: 找不到信号所在分钟", sig.id));
            continue;
        };
        let entry_idx = sig_idx + 1;
        if entry_idx >= bars.len() {
            skipped.push(format!("{}: 信号在收盘前最后一分钟，无法成交", sig.id));
            continue;
        }
        if (entry_idx as isize) <= last_exit_idx {
            skipped.push(format!("{}: 上一笔尚未结束", sig.id));
            continue;
        }
        if sig.qty <= 0 {
            skipped.push(format!("{}: 数量不足", sig.id));
            continue;
        }
        if available_base < sig.qty {
            skipped.push(format!(
                "{}: 可卖底仓不足（{} < {}）",
                sig.id, available_base, sig.qty
            ));
            continue;
        }
        if sig.side != "正T" && sig.side != "倒T" {
            skipped.push(format!("{}: 非法方向", sig.id));
            continue;
        }
        if sig.target <= 0.0 || sig.stop <= 0.0 {
            skipped.push(format!("{}: 目标/止损缺失", sig.id));
            continue;
        }

        let entry_raw = bars[entry_idx].price;
        if entry_raw <= 0.0 {
            skipped.push(format!("{}: 成交价异常", sig.id));
            continue;
        }

        let slip = fees.slip_rate();
        let is_long = sig.side == "正T";
        let entry_price = if is_long {
            entry_raw * (1.0 + slip)
        } else {
            entry_raw * (1.0 - slip)
        };

        if is_long {
            let notional = entry_price * sig.qty as f64;
            let fee = fees.buy_fee(notional);
            if cash_balance < notional + fee {
                skipped.push(format!("{}: 可用资金不足", sig.id));
                continue;
            }
        }

        let mut exit_idx = bars.len() - 1;
        let mut exit_raw = bars[bars.len() - 1].price;
        let mut exit_reason = "session_end".to_string();

        for j in (entry_idx + 1)..bars.len() {
            let px = bars[j].price;
            if is_long {
                if px <= sig.stop {
                    exit_idx = j;
                    exit_raw = sig.stop;
                    exit_reason = "stop".to_string();
                    break;
                }
                if px >= sig.target {
                    exit_idx = j;
                    exit_raw = sig.target;
                    exit_reason = "target".to_string();
                    break;
                }
            } else {
                if px >= sig.stop {
                    exit_idx = j;
                    exit_raw = sig.stop;
                    exit_reason = "stop".to_string();
                    break;
                }
                if px <= sig.target {
                    exit_idx = j;
                    exit_raw = sig.target;
                    exit_reason = "target".to_string();
                    break;
                }
            }
        }

        let exit_price = if is_long {
            exit_raw * (1.0 - slip)
        } else {
            exit_raw * (1.0 + slip)
        };

        let buy_price = if is_long { entry_price } else { exit_price };
        let sell_price = if is_long { exit_price } else { entry_price };
        let buy_notional = buy_price * sig.qty as f64;
        let sell_notional = sell_price * sig.qty as f64;
        let buy_fee = fees.buy_fee(buy_notional);
        let sell_fee = fees.sell_fee(sell_notional);
        let total_fees = buy_fee + sell_fee;

        let gross = if is_long {
            (exit_raw - entry_raw) * sig.qty as f64
        } else {
            (entry_raw - exit_raw) * sig.qty as f64
        };
        let slippage =
            ((entry_price - entry_raw).abs() + (exit_price - exit_raw).abs()) * sig.qty as f64;
        let net = gross - total_fees - slippage;

        let (mae_pct, mfe_pct) = excursion(bars, entry_idx, exit_idx, entry_raw, is_long);

        if is_long {
            cash_balance -= buy_notional + buy_fee;
            cash_balance += sell_notional - sell_fee;
        } else {
            cash_balance += sell_notional - sell_fee;
            cash_balance -= buy_notional + buy_fee;
        }
        available_base -= sig.qty;
        last_exit_idx = exit_idx as isize;

        trades.push(BtTrade {
            signal_id: sig.id.clone(),
            side: sig.side.clone(),
            entry_t: bars[entry_idx].t.clone(),
            entry_price: round2(entry_price),
            exit_t: bars[exit_idx].t.clone(),
            exit_price: round2(exit_price),
            qty: sig.qty,
            gross: round2(gross),
            fees: round2(total_fees),
            slippage: round2(slippage),
            net: round2(net),
            exit_reason,
            bars_held: (exit_idx - entry_idx) as i64,
            mae_pct: round4(mae_pct),
            mfe_pct: round4(mfe_pct),
        });
    }

    let summary = summarize(&trades, cash, cfg.base_qty, bars);
    BacktestResult {
        symbol: symbol.to_string(),
        day: day.to_string(),
        trades,
        summary,
        skipped,
        cash_end: round2(cash_balance),
        available_base,
    }
}

fn excursion(
    bars: &[Bar],
    entry_idx: usize,
    exit_idx: usize,
    entry_raw: f64,
    is_long: bool,
) -> (f64, f64) {
    if entry_raw <= 0.0 {
        return (0.0, 0.0);
    }
    let slice = &bars[entry_idx..=exit_idx.min(bars.len() - 1)];
    let min = slice.iter().map(|b| b.price).fold(f64::MAX, f64::min);
    let max = slice.iter().map(|b| b.price).fold(f64::MIN, f64::max);
    if is_long {
        ((min - entry_raw) / entry_raw, (max - entry_raw) / entry_raw)
    } else {
        ((entry_raw - max) / entry_raw, (entry_raw - min) / entry_raw)
    }
}

fn summarize(trades: &[BtTrade], start_cash: f64, base_qty: i64, bars: &[Bar]) -> BacktestSummary {
    let mut s = BacktestSummary {
        trades: trades.len() as i64,
        ..Default::default()
    };
    if trades.is_empty() {
        return s;
    }

    let mut cum = 0.0f64;
    let mut peak = 0.0f64;
    let mut max_dd = 0.0f64;
    let mut gross_win = 0.0f64;
    let mut gross_loss = 0.0f64;
    let mut hold_sum = 0.0f64;

    for t in trades {
        s.gross += t.gross;
        s.fees += t.fees;
        s.slippage += t.slippage;
        s.net += t.net;
        if t.net > 0.0 {
            s.wins += 1;
            gross_win += t.net;
        } else if t.net < 0.0 {
            s.losses += 1;
            gross_loss += t.net.abs();
        }
        hold_sum += t.bars_held as f64;

        cum += t.net;
        peak = peak.max(cum);
        max_dd = max_dd.max(peak - cum);
    }

    s.win_rate = if s.trades > 0 {
        s.wins as f64 / s.trades as f64
    } else {
        0.0
    };
    s.profit_factor = if gross_loss > 0.0 {
        gross_win / gross_loss
    } else if gross_win > 0.0 {
        99.0
    } else {
        0.0
    };
    s.max_drawdown = round2(max_dd);
    let first_price = bars.first().map(|b| b.price).unwrap_or(0.0);
    let initial_equity = start_cash + base_qty as f64 * first_price;
    s.max_drawdown_pct = if initial_equity > 0.0 {
        round4(max_dd / initial_equity)
    } else {
        0.0
    };
    s.avg_hold_min = round2(hold_sum / s.trades as f64);
    s.gross = round2(s.gross);
    s.fees = round2(s.fees);
    s.slippage = round2(s.slippage);
    s.net = round2(s.net);
    s
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::Bar;

    fn bar(t: &str, price: f64) -> Bar {
        Bar {
            t: t.to_string(),
            price,
            avg: price,
            vwap: price,
            dev: 0.0,
            sigma: 0.001,
            z: 0.0,
            slope: 0.0,
            vol: 100,
            vol_ratio: 1.0,
            high: price,
            low: price,
            day_ret: 0.0,
            amp: 0.0,
            regime: "震荡".to_string(),
        }
    }

    fn signal(id: &str, t: &str, side: &str, target: f64, stop: f64) -> Signal {
        Signal {
            id: id.to_string(),
            day: "2026-10-19".to_string(),
            t: t.to_string(),
            side: side.to_string(),
            price: 100.0,
            entry: 100.0,
            target,
            stop,
            qty: 100,
            gross: 0.0,
            net: 0.0,
            conf: 80,
            regime: "震荡".to_string(),
            z: 0.0,
            accepted: true,
            veto: vec![],
            why: vec![],
        }
    }

    fn cfg() -> StrategyConfig {
        let mut c = StrategyConfig::default();
        c.base_qty = 1000;
        c
    }

    #[test]
    fn long_target_hit_after_costs() {
        let bars = vec![
            bar("09:30", 100.0),
            bar("09:31", 100.0),
            bar("09:32", 101.0),
            bar("09:33", 102.0),
        ];
        let sig = signal("S01", "09:30", "正T", 101.0, 99.0);
        let r = execute_signals(
            "sh600519",
            "2026-10-19",
            &bars,
            &[sig],
            &cfg(),
            &FeeModel::default(),
            1_000_000.0,
        );
        assert_eq!(r.trades.len(), 1);
        let t = &r.trades[0];
        assert_eq!(t.exit_reason, "target");
        assert!(t.gross > 0.0);
        assert!(t.net > 0.0);
        assert!(t.net < t.gross);
        assert_eq!(t.entry_t, "09:31");
        assert_eq!(t.exit_t, "09:32");
    }

    #[test]
    fn short_stop_loses() {
        let bars = vec![
            bar("09:30", 100.0),
            bar("09:31", 100.0),
            bar("09:32", 100.5),
            bar("09:33", 102.0),
        ];
        let sig = signal("S01", "09:30", "倒T", 99.0, 101.0);
        let r = execute_signals(
            "sh600519",
            "2026-10-19",
            &bars,
            &[sig],
            &cfg(),
            &FeeModel::default(),
            1_000_000.0,
        );
        assert_eq!(r.trades.len(), 1);
        assert_eq!(r.trades[0].exit_reason, "stop");
        assert!(r.trades[0].net < 0.0);
        assert_eq!(r.summary.losses, 1);
    }

    #[test]
    fn t_plus_one_limits_sellable_base() {
        let bars = vec![
            bar("09:30", 100.0),
            bar("09:31", 100.0),
            bar("09:32", 101.0),
            bar("09:33", 101.0),
            bar("09:34", 102.0),
        ];
        let mut a = signal("S01", "09:30", "正T", 101.0, 99.0);
        a.qty = 600;
        let mut b = signal("S02", "09:32", "正T", 102.0, 99.0);
        b.qty = 600;
        let r = execute_signals(
            "sh600519",
            "2026-10-19",
            &bars,
            &[a, b],
            &cfg(),
            &FeeModel::default(),
            1_000_000.0,
        );
        assert_eq!(r.trades.len(), 1);
        assert_eq!(r.available_base, 400);
        assert!(r.skipped.iter().any(|m| m.contains("可卖底仓不足")));
    }

    #[test]
    fn session_end_forces_close() {
        let bars = vec![
            bar("09:30", 100.0),
            bar("09:31", 100.0),
            bar("09:32", 100.2),
            bar("09:33", 100.3),
        ];
        let sig = signal("S01", "09:30", "正T", 105.0, 95.0);
        let r = execute_signals(
            "sh600519",
            "2026-10-19",
            &bars,
            &[sig],
            &cfg(),
            &FeeModel::default(),
            1_000_000.0,
        );
        assert_eq!(r.trades[0].exit_reason, "session_end");
        assert_eq!(r.trades[0].exit_t, "09:33");
    }

    #[test]
    fn deterministic_replay() {
        let bars = vec![
            bar("09:30", 100.0),
            bar("09:31", 100.0),
            bar("09:32", 101.0),
        ];
        let sig = signal("S01", "09:30", "正T", 101.0, 99.0);
        let a = execute_signals(
            "sh600519",
            "2026-10-19",
            &bars,
            std::slice::from_ref(&sig),
            &cfg(),
            &FeeModel::default(),
            1_000_000.0,
        );
        let b = execute_signals(
            "sh600519",
            "2026-10-19",
            &bars,
            std::slice::from_ref(&sig),
            &cfg(),
            &FeeModel::default(),
            1_000_000.0,
        );
        assert_eq!(a, b);
    }
}
