"""信号引擎：VWAP 均值回归 + regime 门控 + 硬性否决。

设计原则：
1. 先判"能不能做"，再判"在哪做"；
2. 被否决的信号也要留痕，UI 必须能回答"为什么没做"；
3. 所有阈值可按标的覆盖，不做全局拍脑袋。
"""
from __future__ import annotations

from typing import Dict, List

DEFAULT: Dict = {
    "z_entry": 1.2,          # 偏离入场阈值（标准差）
    "cooldown": 15,          # 同票两次候选的最小间隔（分钟）
    "max_signals": 4,        # 每日最多执行的 T 次数
    "target_lo": 0.0045,     # 最小目标价差（覆盖成本 + 最低利润）
    "target_hi": 0.0090,     # 目标价差上限（不贪婪）
    "stop_pct": 0.0035,      # 硬止损
    "min_net": 0.0010,       # 扣成本后最低净收益
    "lot": 200,              # 单次 T 数量（股）
    "max_t_ratio": 0.5,      # T 仓 ≤ 底仓 50%
    "start_time": "09:45",   # 开盘 15 分钟不做（方向未明）
    "end_time": "14:50",     # 尾盘不做（流动性/隔夜风险）
    "vol_cap": 3.0,          # 放量异动阈值
}

COST_PCT = 0.00025 * 2 + 0.0005 + 0.00001 * 2 + 0.0002 * 2  # ≈ 0.142%


def scan(rows: List[dict], base_qty: int = 1000, cfg: Dict | None = None) -> List[dict]:
    c = {**DEFAULT, **(cfg or {})}
    out: List[dict] = []
    last_i = -999
    accepted_count = 0

    for i in range(1, len(rows)):
        r, prev = rows[i], rows[i - 1]
        if r["t"] < c["start_time"] or r["t"] > c["end_time"]:
            continue
        if i - last_i < c["cooldown"]:
            continue

        side = None
        if prev["z"] > -c["z_entry"] and r["z"] <= -c["z_entry"]:
            side = "正T"  # 低于 VWAP 过多，先买后卖
        elif prev["z"] < c["z_entry"] and r["z"] >= c["z_entry"]:
            side = "倒T"  # 高于 VWAP 过多，先卖后买
        if side is None:
            continue

        last_i = i
        price = r["price"]
        veto: List[str] = []

        if not c.get("ignore_regime"):
            if r["regime"] == "极端":
                veto.append("极端行情（接近涨跌停/大幅异动）")
            elif side == "正T" and r["regime"] == "下行":
                veto.append("下行趋势禁止正T（防止越买越套）")
            elif side == "倒T" and r["regime"] == "上行":
                veto.append("上行趋势禁止倒T（防止卖飞）")

        # 目标 = 回归到滚动 VWAP；空间不足则不做，绝不为了凑目标而拉高预期
        if side == "正T":
            raw_gross = r["vwap"] / price - 1
        else:
            raw_gross = 1 - r["vwap"] / price

        if raw_gross < c["target_lo"]:
            veto.append(f"回归空间不足（{raw_gross * 100:.2f}% < {c['target_lo'] * 100:.2f}%）")

        gross = min(max(raw_gross, 0.0), c["target_hi"])
        if side == "正T":
            target = price * (1 + gross)
            stop = price * (1 - c["stop_pct"])
        else:
            target = price * (1 - gross)
            stop = price * (1 + c["stop_pct"])
        net = gross - COST_PCT
        if net < c["min_net"]:
            veto.append(f"扣成本后无优势（净 {net * 100:.2f}%）")

        if r["vol_ratio"] > c["vol_cap"]:
            veto.append(f"放量异动（量比 {r['vol_ratio']:.1f}）")

        if accepted_count >= c["max_signals"]:
            veto.append("今日做T次数已满")

        qty = int(min(base_qty * c["max_t_ratio"], c["lot"]) // 100 * 100)
        if qty <= 0:
            veto.append("可做数量不足")

        accepted = len(veto) == 0
        conf = 0
        if accepted:
            conf = min(
                92,
                int(52 + abs(r["z"]) * 9 + (6 if r["regime"] == "震荡" else 0) + (5 if r["vol_ratio"] < 1.2 else 0)),
            )
            accepted_count += 1

        why = [
            f"偏离滚动VWAP {r['z']:+.2f}σ",
            f"量比 {r['vol_ratio']:.2f}",
            f"regime={r['regime']}",
            f"斜率 {r['slope']:+.3f}%/min",
        ]

        out.append(
            {
                "id": f"S{len(out) + 1:02d}",
                "t": r["t"],
                "side": side,
                "price": price,
                "entry": round(price, 2),
                "target": round(target, 2),
                "stop": round(stop, 2),
                "qty": qty,
                "gross": round(gross, 5),
                "net": round(net, 5),
                "conf": conf,
                "regime": r["regime"],
                "z": r["z"],
                "accepted": accepted,
                "veto": veto,
                "why": why,
                "filled": None,
            }
        )

    return out
