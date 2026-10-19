//! 样本外与参数敏感性研究。
//!
//! - 70/30 切分：前 70% 交易日做样本内参数选择，后 30% 做样本外验证。
//! - 参数网格：z_entry × target_lo，按样本内净收益选最优。
//! - walk-forward：扩张窗口，滚动选择参数并在下一段测试。
//!
//! 注意：这里只做“参数选择”，不做未来函数；样本外从不参与选参。

use serde::Serialize;

use crate::backtest::{self, BacktestResult, FeeModel, Scorecard};
use crate::config::StrategyConfig;
use crate::features::Bar;

pub const Z_GRID: [f64; 5] = [0.8, 1.0, 1.2, 1.5, 1.8];
pub const TARGET_GRID: [f64; 4] = [0.0025, 0.0035, 0.0045, 0.0055];

#[derive(Debug, Clone)]
pub struct DayBars {
    pub day: String,
    pub bars: Vec<Bar>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamCell {
    pub z_entry: f64,
    pub target_lo: f64,
    pub trades: i64,
    pub net: f64,
    pub win_rate: f64,
    pub max_drawdown: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedParams {
    pub z_entry: f64,
    pub target_lo: f64,
    pub in_sample_net: f64,
    pub in_sample_trades: i64,
    pub out_sample_net: f64,
    pub out_sample_trades: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoldResult {
    pub train_days: i64,
    pub test_days: i64,
    pub z_entry: f64,
    pub target_lo: f64,
    pub train_net: f64,
    pub test_net: f64,
    pub test_trades: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchReport {
    pub symbol: String,
    pub total_days: i64,
    pub in_sample_days: Vec<String>,
    pub out_sample_days: Vec<String>,
    pub note: String,
    pub demo_days: i64,
    pub selection: Option<SelectedParams>,
    pub current_out_sample: Option<Scorecard>,
    pub best_out_sample: Option<Scorecard>,
    pub sensitivity: Vec<ParamCell>,
    pub walk_forward: Vec<FoldResult>,
}

pub fn split_days(days: &[DayBars]) -> (Vec<DayBars>, Vec<DayBars>) {
    let n = days.len();
    if n < 4 {
        return (days.to_vec(), Vec::new());
    }
    let split = ((n as f64) * 0.7).floor() as usize;
    let split = split.clamp(1, n - 1);
    (days[..split].to_vec(), days[split..].to_vec())
}

fn run(days: &[DayBars], symbol: &str, cfg: &StrategyConfig, fees: &FeeModel) -> Scorecard {
    let results: Vec<BacktestResult> = days
        .iter()
        .map(|d| backtest::run_day(symbol, &d.day, &d.bars, cfg, fees, cfg.cash))
        .collect();
    backtest::scorecard(&results)
}

fn with_params(cfg: &StrategyConfig, z: f64, lo: f64) -> StrategyConfig {
    let mut next = cfg.clone();
    next.z_entry = z;
    next.target_lo = lo;
    if next.target_hi < lo {
        next.target_hi = lo + 0.002;
    }
    next
}

/// 在给定交易日上跑网格，返回全部单元格与最优参数。
fn grid(
    days: &[DayBars],
    symbol: &str,
    cfg: &StrategyConfig,
    fees: &FeeModel,
) -> (Vec<ParamCell>, Option<(ParamCell, StrategyConfig)>) {
    let mut cells = Vec::new();
    let mut best: Option<(ParamCell, StrategyConfig)> = None;

    for z in Z_GRID {
        for lo in TARGET_GRID {
            let next = with_params(cfg, z, lo);
            let sc = run(days, symbol, &next, fees);
            let cell = ParamCell {
                z_entry: z,
                target_lo: lo,
                trades: sc.trades,
                net: sc.net,
                win_rate: sc.win_rate,
                max_drawdown: sc.max_drawdown,
            };
            let better = match &best {
                None => true,
                Some((current, _)) => {
                    cell.net > current.net
                        || (cell.net == current.net && cell.trades > current.trades)
                }
            };
            if better {
                best = Some((cell.clone(), next));
            }
            cells.push(cell);
        }
    }

    (cells, best)
}

pub fn research(
    symbol: &str,
    days: &[DayBars],
    cfg: &StrategyConfig,
    fees: &FeeModel,
) -> ResearchReport {
    let (in_sample, out_sample) = split_days(days);
    let total_days = days.len() as i64;
    let in_names: Vec<String> = in_sample.iter().map(|d| d.day.clone()).collect();
    let out_names: Vec<String> = out_sample.iter().map(|d| d.day.clone()).collect();

    let (cells, best) = grid(&in_sample, symbol, cfg, fees);
    let mut selection = None;
    let mut current_out_sample = None;
    let mut best_out_sample = None;

    if let Some((_, best_cfg)) = &best {
        let is_cell = best.as_ref().map(|(c, _)| c).unwrap();
        let oos_best = if out_sample.is_empty() {
            None
        } else {
            Some(run(&out_sample, symbol, best_cfg, fees))
        };
        let oos_current = if out_sample.is_empty() {
            None
        } else {
            Some(run(&out_sample, symbol, cfg, fees))
        };
        selection = Some(SelectedParams {
            z_entry: is_cell.z_entry,
            target_lo: is_cell.target_lo,
            in_sample_net: is_cell.net,
            in_sample_trades: is_cell.trades,
            out_sample_net: oos_best.as_ref().map(|s| s.net).unwrap_or(0.0),
            out_sample_trades: oos_best.as_ref().map(|s| s.trades).unwrap_or(0),
        });
        current_out_sample = oos_current;
        best_out_sample = oos_best;
    }

    let note = if out_sample.is_empty() {
        format!("样本不足：共 {total_days} 天，至少需要 4 天才能切样本外；当前只做敏感性。")
    } else {
        format!(
            "样本内 {} 天 / 样本外 {} 天；参数只在样本内选择，样本外不参与调参。",
            in_sample.len(),
            out_sample.len()
        )
    };

    ResearchReport {
        symbol: symbol.to_string(),
        total_days,
        in_sample_days: in_names,
        out_sample_days: out_names,
        note,
        demo_days: 0,
        selection,
        current_out_sample,
        best_out_sample,
        sensitivity: cells,
        walk_forward: walk_forward(symbol, days, cfg, fees),
    }
}

fn walk_forward(
    symbol: &str,
    days: &[DayBars],
    cfg: &StrategyConfig,
    fees: &FeeModel,
) -> Vec<FoldResult> {
    let n = days.len();
    if n < 8 {
        return Vec::new();
    }
    let initial = ((n as f64) * 0.4).floor() as usize;
    let initial = initial.clamp(2, n - 1);
    let remaining = n - initial;
    let folds = 3usize;
    let chunk = (remaining / folds).max(1);

    let mut out = Vec::new();
    for f in 0..folds {
        let train_end = initial + f * chunk;
        let test_end = (train_end + chunk).min(n);
        if train_end >= n || test_end <= train_end {
            break;
        }
        let train = &days[..train_end];
        let test = &days[train_end..test_end];
        let (_, best) = grid(train, symbol, cfg, fees);
        let Some((cell, best_cfg)) = best else {
            continue;
        };
        let test_sc = run(test, symbol, &best_cfg, fees);
        out.push(FoldResult {
            train_days: train.len() as i64,
            test_days: test.len() as i64,
            z_entry: cell.z_entry,
            target_lo: cell.target_lo,
            train_net: cell.net,
            test_net: test_sc.net,
            test_trades: test_sc.trades,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn day(name: &str) -> DayBars {
        DayBars {
            day: name.to_string(),
            bars: vec![bar("09:30", 100.0), bar("09:31", 100.0)],
        }
    }

    #[test]
    fn splits_70_30_and_handles_small_samples() {
        let days: Vec<DayBars> = (0..10).map(|i| day(&format!("202610{:02}", i))).collect();
        let (train, test) = split_days(&days);
        assert_eq!(train.len(), 7);
        assert_eq!(test.len(), 3);

        let small: Vec<DayBars> = (0..3).map(|i| day(&format!("202610{:02}", i))).collect();
        let (train, test) = split_days(&small);
        assert_eq!(train.len(), 3);
        assert!(test.is_empty());
    }

    #[test]
    fn grid_returns_all_combinations() {
        let days = vec![day("20261019")];
        let (cells, best) = grid(
            &days,
            "sh600519",
            &StrategyConfig::default(),
            &FeeModel::default(),
        );
        assert_eq!(cells.len(), Z_GRID.len() * TARGET_GRID.len());
        assert!(best.is_some());
    }

    #[test]
    fn report_notes_small_sample() {
        let days = vec![day("20261019"), day("20261020")];
        let r = research(
            "sh600519",
            &days,
            &StrategyConfig::default(),
            &FeeModel::default(),
        );
        assert_eq!(r.total_days, 2);
        assert!(r.out_sample_days.is_empty());
        assert!(r.note.contains("样本不足"));
        assert_eq!(r.sensitivity.len(), 20);
        assert!(r.walk_forward.is_empty());
    }
}
