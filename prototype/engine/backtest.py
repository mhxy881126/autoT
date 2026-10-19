"""撮合与绩效：用下一分钟价成交，含双边成本、滑点、收盘平T。"""
from __future__ import annotations

from typing import Dict, List, Tuple


def trade_cost(buy_amt: float, sell_amt: float) -> Dict[str, float]:
    commission = max(5.0, buy_amt * 0.00025) + max(5.0, sell_amt * 0.00025)
    stamp = sell_amt * 0.0005
    transfer = (buy_amt + sell_amt) * 0.00001
    slip = (buy_amt + sell_amt) * 0.0002
    return {
        "commission": round(commission, 2),
        "stamp": round(stamp, 2),
        "transfer": round(transfer, 2),
        "slip": round(slip, 2),
        "total": round(commission + stamp + transfer + slip, 2),
    }


def run(rows: List[dict], signals: List[dict]) -> Tuple[List[dict], List[dict], Dict]:
    index = {r["t"]: i for i, r in enumerate(rows)}
    trades: List[dict] = []
    equity: List[dict] = []
    busy_until = -1
    cum = 0.0

    for s in signals:
        if not s["accepted"]:
            continue
        i = index.get(s["t"])
        if i is None or i + 1 >= len(rows) or i <= busy_until:
            s["filled"] = False
            s["fill_note"] = "错过执行窗口"
            continue

        nxt = rows[i + 1]
        if abs(nxt["price"] / s["entry"] - 1) > 0.0015:
            s["filled"] = False
            s["fill_note"] = "下一分钟跳价，未成交"
            continue

        entry = nxt["price"]
        qty = s["qty"]
        exit_price = None
        exit_t = None
        reason = ""

        for j in range(i + 1, len(rows)):
            r = rows[j]
            if s["side"] == "正T":
                if r["price"] >= s["target"]:
                    exit_price, exit_t, reason = s["target"], r["t"], "目标价"
                    break
                if r["price"] <= s["stop"]:
                    exit_price, exit_t, reason = s["stop"], r["t"], "止损"
                    break
            else:
                if r["price"] <= s["target"]:
                    exit_price, exit_t, reason = s["target"], r["t"], "目标价"
                    break
                if r["price"] >= s["stop"]:
                    exit_price, exit_t, reason = s["stop"], r["t"], "止损"
                    break
            if r["t"] >= "14:55":
                exit_price, exit_t, reason = r["price"], r["t"], "收盘平T"
                break

        if exit_price is None:
            exit_price, exit_t, reason = rows[-1]["price"], rows[-1]["t"], "收盘平T"

        if s["side"] == "正T":
            buy_amt, sell_amt = entry * qty, exit_price * qty
            gross_pnl = (exit_price - entry) * qty
        else:
            sell_amt, buy_amt = entry * qty, exit_price * qty
            gross_pnl = (entry - exit_price) * qty

        cost = trade_cost(buy_amt, sell_amt)
        net_pnl = gross_pnl - cost["total"]
        cum += net_pnl

        trade = {
            "id": s["id"],
            "side": s["side"],
            "entry_t": nxt["t"],
            "exit_t": exit_t,
            "entry": round(entry, 2),
            "exit": round(exit_price, 2),
            "qty": qty,
            "gross_pct": round(gross_pnl / (entry * qty), 5),
            "net_pct": round(net_pnl / (entry * qty), 5),
            "gross_pnl": round(gross_pnl, 2),
            "cost": cost["total"],
            "net_pnl": round(net_pnl, 2),
            "reason": reason,
            "hold_min": _minutes_between(nxt["t"], exit_t),
        }
        trades.append(trade)
        equity.append({"t": exit_t, "pnl": round(cum, 2)})
        busy_until = index.get(exit_t, i)
        s["filled"] = True

    return trades, equity, summarize(trades, signals, equity)


def _minutes_between(a: str, b: str) -> int:
    def to_min(t: str) -> int:
        h, m = t.split(":")
        return int(h) * 60 + int(m)

    return max(0, to_min(b) - to_min(a))


def summarize(trades: List[dict], signals: List[dict], equity: List[dict]) -> Dict:
    filled = trades
    wins = [t for t in filled if t["net_pnl"] > 0]
    losses = [t for t in filled if t["net_pnl"] <= 0]
    total_net = round(sum(t["net_pnl"] for t in filled), 2)
    total_cost = round(sum(t["cost"] for t in filled), 2)
    total_gross = round(sum(t["gross_pnl"] for t in filled), 2)

    peak, max_dd = 0.0, 0.0
    for e in equity:
        peak = max(peak, e["pnl"])
        max_dd = min(max_dd, e["pnl"] - peak)

    veto_reasons: Dict[str, int] = {}
    for s in signals:
        for v in s.get("veto", []):
            key = v.split("（")[0]
            veto_reasons[key] = veto_reasons.get(key, 0) + 1

    return {
        "candidates": len(signals),
        "accepted": len([s for s in signals if s["accepted"]]),
        "filled": len(filled),
        "wins": len(wins),
        "losses": len(losses),
        "win_rate": round(len(wins) / len(filled), 4) if filled else 0.0,
        "avg_net_pct": round(sum(t["net_pct"] for t in filled) / len(filled), 5) if filled else 0.0,
        "avg_win": round(sum(t["net_pnl"] for t in wins) / len(wins), 2) if wins else 0.0,
        "avg_loss": round(sum(t["net_pnl"] for t in losses) / len(losses), 2) if losses else 0.0,
        "total_net": total_net,
        "total_cost": total_cost,
        "total_gross": total_gross,
        "cost_ratio": round(total_cost / total_gross, 3) if total_gross > 0 else None,
        "max_dd": round(max_dd, 2),
        "veto_reasons": veto_reasons,
    }
