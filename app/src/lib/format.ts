export const fmt = (n: number, d = 0): string =>
  Number(n).toLocaleString("zh-CN", {
    minimumFractionDigits: d,
    maximumFractionDigits: d
  });

export const pct = (n: number, d = 2): string =>
  (n >= 0 ? "+" : "") + (n * 100).toFixed(d) + "%";

export const money = (n: number): string =>
  n === 0 ? "¥0" : (n > 0 ? "+" : "−") + "¥" + fmt(Math.abs(n), 0);
