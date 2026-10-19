"""特征引擎：把分时序列变成可计算、可解释的指标。

所有特征只用当前及历史数据，不引入未来函数。
"""
from __future__ import annotations

import statistics
from typing import List


def _ols_slope(ys: List[float]) -> float:
    """对等间隔序列做最小二乘斜率。"""
    n = len(ys)
    if n < 2:
        return 0.0
    mx = (n - 1) / 2.0
    my = sum(ys) / n
    den = sum((i - mx) ** 2 for i in range(n))
    if den == 0:
        return 0.0
    return sum((i - mx) * (y - my) for i, y in enumerate(ys)) / den


def build(
    minutes: List[dict],
    prev_close: float,
    atr_pct: float,
    vwap_window: int = 20,
    sigma_window: int = 30,
) -> List[dict]:
    """逐分钟计算：均价、滚动 VWAP、偏离 z 值、斜率、量比、日内高低。

    atr_pct: 日线 ATR14 / 前收，用于自适应阈值。
    """
    rows: List[dict] = []
    prices: List[float] = []
    devs: List[float] = []
    vols: List[float] = []
    amts: List[float] = []
    cum_v = 0
    cum_a = 0.0
    day_h = None
    day_l = None

    for m in minutes:
        vol = max(0, m["cum_vol"] - cum_v)
        amt = max(0.0, m["cum_amt"] - cum_a)
        cum_v, cum_a = m["cum_vol"], m["cum_amt"]

        avg = cum_a / (cum_v * 100) if cum_v > 0 else m["price"]

        prices.append(m["price"])
        vols.append(float(vol))
        amts.append(amt)
        if len(vols) > vwap_window:
            vols.pop(0)
            amts.pop(0)

        tot_v = sum(vols)
        vwap = sum(amts) / (tot_v * 100) if tot_v > 0 else avg

        dev = m["price"] / vwap - 1 if vwap > 0 else 0.0
        devs.append(dev)
        if len(devs) > sigma_window:
            devs.pop(0)
        sigma = statistics.pstdev(devs) if len(devs) >= 5 else 0.0
        sigma = max(sigma, 0.0008)  # 防止极静时段 z 值爆炸
        z = dev / sigma

        slope = 0.0
        if len(prices) >= 10:
            window = prices[-10:]
            slope = _ols_slope(window) / m["price"] * 100  # %/分钟

        base_vol = sum(vols[-20:-1]) / max(1, len(vols[-20:-1]))
        vol_ratio = vol / base_vol if base_vol > 0 else 1.0

        day_h = m["price"] if day_h is None else max(day_h, m["price"])
        day_l = m["price"] if day_l is None else min(day_l, m["price"])

        rows.append(
            {
                "t": m["t"],
                "price": m["price"],
                "avg": round(avg, 3),
                "vwap": round(vwap, 3),
                "dev": round(dev, 6),
                "sigma": round(sigma, 6),
                "z": round(z, 3),
                "slope": round(slope, 4),
                "vol": int(vol),
                "vol_ratio": round(vol_ratio, 3),
                "high": day_h,
                "low": day_l,
                "day_ret": round(m["price"] / prev_close - 1, 6),
                "amp": round((day_h - day_l) / prev_close, 6),
            }
        )

    for r in rows:
        r["regime"] = classify(r, atr_pct)
    return _smooth_regime(rows)


def _smooth_regime(rows: List[dict], min_run: int = 3) -> List[dict]:
    """状态机去抖：新状态必须连续出现 min_run 分钟才切换。

    分时 regime 如果逐分钟翻转，UI 会变成条纹，信号也会在门控边界来回横跳。
    """
    if not rows:
        return rows
    current = rows[0]["regime"]
    candidate = None
    run = 0
    for r in rows:
        if r["regime"] == current:
            candidate, run = None, 0
        else:
            if r["regime"] == candidate:
                run += 1
            else:
                candidate, run = r["regime"], 1
            if run >= min_run:
                current, candidate, run = candidate, None, 0
        r["regime"] = current
    return rows


def classify(r: dict, atr_pct: float) -> str:
    """四态 regime。宁可判错为震荡，也不要在趋势里放开逆势做T。"""
    ret = r["day_ret"]
    if abs(ret) >= 0.07:
        return "极端"
    s = r["slope"]
    z = r["z"]
    if s >= 0.020 and z >= 0.2:
        return "上行"
    if s <= -0.020 and z <= -0.2:
        return "下行"
    return "震荡"


def atr(daily: List[dict], n: int = 14) -> float:
    """日线 ATR（绝对价格）。"""
    trs = []
    for i in range(1, len(daily)):
        cur, prev = daily[i], daily[i - 1]
        tr = max(
            cur["h"] - cur["l"],
            abs(cur["h"] - prev["c"]),
            abs(cur["l"] - prev["c"]),
        )
        trs.append(tr)
    if not trs:
        return 0.0
    return sum(trs[-n:]) / len(trs[-n:])


def moving_average(values: List[float], n: int) -> List[float | None]:
    out: List[float | None] = []
    for i in range(len(values)):
        if i + 1 < n:
            out.append(None)
        else:
            out.append(round(sum(values[i + 1 - n : i + 1]) / n, 3))
    return out
