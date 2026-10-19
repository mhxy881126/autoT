"""引擎单元测试：python -m unittest discover -s tests"""
from __future__ import annotations

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import backtest, features, strategy  # noqa: E402


def mk(t, price, vwap, z, regime="震荡", vol_ratio=1.0, slope=0.0, dev=0.0):
    return {
        "t": t,
        "price": price,
        "vwap": vwap,
        "z": z,
        "regime": regime,
        "vol_ratio": vol_ratio,
        "slope": slope,
        "dev": dev,
    }


class FeatureTest(unittest.TestCase):
    def test_cumulative_to_per_minute(self):
        minutes = [
            {"t": "09:30", "price": 100.0, "cum_vol": 10, "cum_amt": 100000.0},
            {"t": "09:31", "price": 101.0, "cum_vol": 30, "cum_amt": 301000.0},
        ]
        rows = features.build(minutes, prev_close=100.0, atr_pct=0.02)
        self.assertEqual(rows[0]["avg"], 100.0)
        self.assertEqual(rows[1]["vol"], 20)
        self.assertAlmostEqual(rows[1]["avg"], 301000 / 3000, places=3)
        self.assertIn(rows[0]["regime"], {"震荡", "上行", "下行", "极端"})

    def test_regime_classification(self):
        self.assertEqual(features.classify({"day_ret": 0.01, "slope": 0.05, "z": 1.0, "amp": 0.02}, 0.015), "上行")
        self.assertEqual(features.classify({"day_ret": -0.01, "slope": -0.05, "z": -1.0, "amp": 0.02}, 0.015), "下行")
        self.assertEqual(features.classify({"day_ret": 0.0, "slope": 0.001, "z": 0.1, "amp": 0.01}, 0.015), "震荡")
        self.assertEqual(features.classify({"day_ret": 0.09, "slope": 0.0, "z": 0.0, "amp": 0.09}, 0.015), "极端")

    def test_regime_smoothing_rejects_blip(self):
        rows = ([{"regime": "震荡"} for _ in range(5)] + [{"regime": "上行"}]
                + [{"regime": "震荡"} for _ in range(5)])
        features._smooth_regime(rows, min_run=3)
        self.assertTrue(all(r["regime"] == "震荡" for r in rows))

    def test_regime_smoothing_accepts_persistent(self):
        rows = ([{"regime": "震荡"} for _ in range(3)]
                + [{"regime": "上行"} for _ in range(5)])
        features._smooth_regime(rows, min_run=3)
        self.assertEqual(rows[3]["regime"], "震荡")
        self.assertEqual(rows[5]["regime"], "上行")


class StrategyTest(unittest.TestCase):
    def test_blocks_buy_in_downtrend(self):
        rows = [
            mk("09:50", 100.0, 100.0, 0.5, regime="下行", slope=-0.03),
            mk("09:51", 99.4, 100.0, -1.5, regime="下行", slope=-0.03),
        ]
        sigs = strategy.scan(rows, base_qty=1000)
        self.assertEqual(len(sigs), 1)
        self.assertFalse(sigs[0]["accepted"])
        self.assertTrue(any("下行" in v for v in sigs[0]["veto"]))

    def test_accepts_reversion_in_range(self):
        rows = [
            mk("09:50", 100.0, 100.0, 0.5),
            mk("09:51", 99.4, 100.0, -1.5),
        ]
        sigs = strategy.scan(rows, base_qty=1000)
        self.assertEqual(len(sigs), 1)
        sig = sigs[0]
        self.assertTrue(sig["accepted"])
        self.assertEqual(sig["side"], "正T")
        self.assertGreater(sig["target"], sig["entry"])
        self.assertLess(sig["stop"], sig["entry"])
        self.assertLessEqual(sig["qty"], 500)
        self.assertGreater(sig["net"], 0)

    def test_rejects_thin_spread(self):
        rows = [
            mk("09:50", 100.0, 100.0, 0.5),
            mk("09:51", 99.8, 100.0, -1.5),  # 仅 0.2% 空间，低于 0.45% 门槛
        ]
        sigs = strategy.scan(rows, base_qty=1000)
        self.assertFalse(sigs[0]["accepted"])
        self.assertTrue(any("回归空间不足" in v for v in sigs[0]["veto"]))


class BacktestTest(unittest.TestCase):
    def test_cost_model(self):
        c = backtest.trade_cost(100000, 100000)
        self.assertEqual(c["commission"], 50.0)
        self.assertEqual(c["stamp"], 50.0)
        self.assertAlmostEqual(c["total"], 142.0, places=2)

    def test_min_commission(self):
        c = backtest.trade_cost(10000, 10000)
        self.assertEqual(c["commission"], 10.0)

    def test_target_hit_is_profitable(self):
        rows = [
            {"t": "09:50", "price": 99.4},
            {"t": "09:51", "price": 99.4},
            {"t": "09:52", "price": 100.0},
        ]
        sig = {
            "id": "S01", "accepted": True, "t": "09:50", "side": "正T",
            "entry": 99.4, "target": 100.0, "stop": 99.0, "qty": 100,
        }
        trades, equity, stats = backtest.run(rows, [sig])
        self.assertEqual(len(trades), 1)
        self.assertEqual(trades[0]["exit"], 100.0)
        self.assertGreater(trades[0]["net_pnl"], 0)
        self.assertEqual(stats["win_rate"], 1.0)
        self.assertEqual(stats["filled"], 1)

    def test_stop_loss_is_negative(self):
        rows = [
            {"t": "09:50", "price": 99.4},
            {"t": "09:51", "price": 99.4},
            {"t": "09:52", "price": 98.9},
        ]
        sig = {
            "id": "S01", "accepted": True, "t": "09:50", "side": "正T",
            "entry": 99.4, "target": 100.0, "stop": 99.0, "qty": 100,
        }
        trades, _, stats = backtest.run(rows, [sig])
        self.assertEqual(trades[0]["reason"], "止损")
        self.assertLess(trades[0]["net_pnl"], 0)
        self.assertEqual(stats["win_rate"], 0.0)


if __name__ == "__main__":
    unittest.main()
