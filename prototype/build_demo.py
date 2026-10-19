"""构建可离线打开的演示数据：真实行情 → 特征 → 信号 → 模拟成交 → demo_data.js"""
from __future__ import annotations

import json
import os
import sys
from datetime import datetime

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from engine import backtest, feed, features, strategy  # noqa: E402

STRICT = {}
LOOSE = {
    "target_lo": 0.0025,
    "stop_pct": 0.0020,
    "z_entry": 1.0,
    "max_signals": 6,
    "min_net": 0.0005,
}


def run_variant(rows, cfg):
    signals = strategy.scan(rows, base_qty=1000, cfg=cfg)
    trades, equity, stats = backtest.run(rows, signals)
    return signals, trades, equity, stats


def main(symbol: str = "sh600519") -> None:
    quote = feed.fetch_quote(symbol)
    minute = feed.fetch_minute(symbol)
    daily = feed.fetch_daily(symbol, 120)

    prev_close = quote["prev_close"]
    today = minute["date"]
    today_fmt = f"{today[:4]}-{today[4:6]}-{today[6:]}"

    hist = [b for b in daily if b["d"] != today_fmt] or daily
    atr_abs = features.atr(hist, 14)
    atr_pct = atr_abs / prev_close if prev_close else 0.0

    rows = features.build(minute["rows"], prev_close, atr_pct)
    signals, trades, equity, stats = run_variant(rows, STRICT)
    _, loose_trades, _, loose_stats = run_variant(rows, LOOSE)

    closes = [b["c"] for b in daily]
    ma5 = features.moving_average(closes, 5)
    ma10 = features.moving_average(closes, 10)
    ma20 = features.moving_average(closes, 20)
    kline = [
        {
            "d": daily[i]["d"],
            "o": daily[i]["o"],
            "c": daily[i]["c"],
            "h": daily[i]["h"],
            "l": daily[i]["l"],
            "v": daily[i]["v"],
            "ma5": ma5[i],
            "ma10": ma10[i],
            "ma20": ma20[i],
        }
        for i in range(max(0, len(daily) - 60), len(daily))
    ]

    sensitivity = []
    for th in (0.0020, 0.0025, 0.0030, 0.0035, 0.0040, 0.0045):
        cfg = {
            "target_lo": th,
            "stop_pct": max(0.0015, round(th * 0.8, 4)),
            "z_entry": 1.0,
            "max_signals": 8,
            "min_net": 0.0002,
        }
        _, tr, _, st = run_variant(rows, cfg)
        sensitivity.append(
            {
                "threshold": round(th * 100, 2),
                "accepted": st["accepted"],
                "filled": st["filled"],
                "net": st["total_net"],
                "win_rate": st["win_rate"],
            }
        )

    base_cost = round(sum(closes[-20:]) / min(20, len(closes)), 2)
    last = rows[-1]
    accepted = [s for s in signals if s["accepted"]]

    if accepted:
        conclusion = f"今日通过 {len(accepted)} 个候选，按纪律执行。"
    else:
        conclusion = (
            f"今日 {len(signals)} 个候选全部否决：日内偏离不足以覆盖 0.14% 的往返成本。"
            "系统的结论是——不做。"
        )

    payload = {
        "meta": {
            "symbol": quote["name"],
            "code": symbol[2:],
            "market": symbol[:2].upper(),
            "date": today_fmt,
            "prev_close": prev_close,
            "open": quote["open"],
            "high": quote["high"],
            "low": quote["low"],
            "close": quote["price"],
            "atr_pct": round(atr_pct, 5),
            "generated_at": datetime.now().strftime("%Y-%m-%d %H:%M:%S"),
            "mode": "replay",
            "conclusion": conclusion,
        },
        "account": {
            "base_qty": 1000,
            "base_cost": base_cost,
            "available": 1000,
            "cash": 800000.0,
            "lot": 200,
            "max_t_ratio": 0.5,
        },
        "minutes": rows,
        "kline": kline,
        "signals": signals,
        "trades": trades,
        "equity": equity,
        "stats": stats,
        "scenarios": [
            {
                "key": "strict",
                "label": "严格门槛 0.45%",
                "desc": "价差必须覆盖成本后仍有 0.10% 净利",
                "stats": stats,
                "trades": trades,
            },
            {
                "key": "loose",
                "label": "放宽到 0.25%",
                "desc": "多数人的实际做法：看到两三个点波动就想做",
                "stats": loose_stats,
                "trades": loose_trades,
            },
        ],
        "sensitivity": sensitivity,
        "last": {
            "t": last["t"],
            "regime": last["regime"],
            "z": last["z"],
            "price": last["price"],
            "vwap": last["vwap"],
            "slope": last["slope"],
        },
    }

    out_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "web")
    os.makedirs(out_dir, exist_ok=True)
    out_path = os.path.join(out_dir, "demo_data.js")
    with open(out_path, "w", encoding="utf-8") as f:
        f.write("window.DEMO = ")
        json.dump(payload, f, ensure_ascii=False, separators=(",", ":"))
        f.write(";\n")

    print(f"OK {out_path}")
    print(f"date={today_fmt} prev_close={prev_close} atr_pct={atr_pct:.4%}")
    print(f"minutes={len(rows)} kline={len(kline)} signals={len(signals)} accepted={len(accepted)}")
    print(f"strict: trades={len(trades)} net={stats['total_net']}")
    print(f"loose : trades={len(loose_trades)} net={loose_stats['total_net']} cost={loose_stats['total_cost']}")
    print("sensitivity:", json.dumps(sensitivity, ensure_ascii=False))


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "sh600519")
