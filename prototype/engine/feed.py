"""行情适配层：腾讯免费接口（L1，3 秒级快照 / 1 分钟线）。

说明：这是非官方公开接口，适合个人研究与原型；生产环境应换成
有 SLA 的行情源（Level-1 授权或 Level-2），并做双源热备。
"""
from __future__ import annotations

import json
import urllib.request
from typing import Any, Dict, List

BASE = "https://web.ifzq.gtimg.cn"
HEADERS = {"User-Agent": "Mozilla/5.0", "Referer": "https://gu.qq.com/"}


def _get_json(url: str) -> dict:
    req = urllib.request.Request(url, headers=HEADERS)
    with urllib.request.urlopen(req, timeout=15) as resp:
        return json.loads(resp.read().decode("utf-8"))


def fetch_minute(symbol: str) -> Dict[str, Any]:
    """取当日 1 分钟分时。返回 09:30-11:30 / 13:00-15:00 共 242 点。"""
    data = _get_json(f"{BASE}/appstock/app/minute/query?code={symbol}")
    node = data["data"][symbol]
    date = node["data"]["date"]
    rows: List[dict] = []
    seen = set()
    for line in node["data"]["data"]:
        parts = line.split()
        if len(parts) < 4:
            continue
        t, price, vol, amt = parts[0], float(parts[1]), int(float(parts[2])), float(parts[3])
        if t in seen:
            continue
        if not (("0930" <= t <= "1130") or ("1300" <= t <= "1500")):
            continue
        seen.add(t)
        rows.append(
            {"t": f"{t[:2]}:{t[2:]}", "price": price, "cum_vol": vol, "cum_amt": amt}
        )
    rows.sort(key=lambda r: r["t"])
    return {"symbol": symbol, "date": date, "rows": rows}


def fetch_daily(symbol: str, n: int = 120) -> List[dict]:
    """前复权日 K。字段顺序：date, open, close, high, low, volume。"""
    data = _get_json(
        f"{BASE}/appstock/app/fqkline/get?param={symbol},day,,,{n},qfq"
    )
    node = data["data"][symbol]
    bars = node.get("qfqday") or node.get("day") or []
    out: List[dict] = []
    for b in bars:
        if len(b) < 6:
            continue
        out.append(
            {
                "d": b[0],
                "o": float(b[1]),
                "c": float(b[2]),
                "h": float(b[3]),
                "l": float(b[4]),
                "v": float(b[5]),
            }
        )
    return out


def fetch_quote(symbol: str) -> Dict[str, Any]:
    """实时快照（含五档盘口），GBK 编码。"""
    url = f"https://qt.gtimg.cn/q={symbol}"
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    with urllib.request.urlopen(req, timeout=10) as resp:
        raw = resp.read()
    for enc in ("gb18030", "utf-8"):
        try:
            text = raw.decode(enc)
            break
        except UnicodeDecodeError:
            continue
    else:
        raise ValueError("无法解码行情快照")
    body = text.split('="', 1)[1].rsplit('"', 1)[0]
    f = body.split("~")
    return {
        "name": f[1],
        "price": float(f[3]),
        "prev_close": float(f[4]),
        "open": float(f[5]),
        "high": float(f[33]),
        "low": float(f[34]),
        "time": f[30],
    }
