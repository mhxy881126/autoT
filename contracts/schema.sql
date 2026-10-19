-- 做T决策台 · SQLite schema v1
-- 约定：
--   * 金额一律整数分（*_cents），避免浮点对账误差
--   * 时间戳 epoch 毫秒；trade_day 为 Asia/Shanghai 的 YYYY-MM-DD
--   * 所有写操作在事务内完成

PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- 每票独立参数：自选列表即这张表
CREATE TABLE IF NOT EXISTS symbol_configs (
  symbol      TEXT PRIMARY KEY,
  config_json TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);

-- 成交：无券商回报，全部来自人工回填或交割单导入
CREATE TABLE IF NOT EXISTS fills (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  client_id   TEXT    NOT NULL UNIQUE,             -- 幂等键（UUID）
  symbol      TEXT    NOT NULL,                    -- "sh600519"，多票账务隔离
  trade_day   TEXT    NOT NULL,
  ts          TEXT    NOT NULL,                    -- "HH:MM"
  side        TEXT    NOT NULL CHECK (side IN ('BUY','SELL')),
  price_cents INTEGER NOT NULL CHECK (price_cents > 0),
  qty         INTEGER NOT NULL CHECK (qty > 0),
  fee_cents   INTEGER NOT NULL DEFAULT 0,
  signal_id   TEXT,
  source      TEXT    NOT NULL DEFAULT 'manual' CHECK (source IN ('manual','import')),
  note        TEXT,
  created_at  TEXT    NOT NULL,
  voided      INTEGER NOT NULL DEFAULT 0           -- 冲正标记：1 表示已作废，配对时跳过
);

-- 只对未冲正的成交做自然键去重：冲正后允许用正确内容重新录入。
CREATE UNIQUE INDEX IF NOT EXISTS uq_fills_active
  ON fills (symbol, trade_day, ts, side, price_cents, qty)
  WHERE voided = 0;

CREATE INDEX IF NOT EXISTS idx_fills_day ON fills (symbol, trade_day, ts);

-- D4：原始分时缓存。存原始累计量，特征始终由代码重算，避免缓存派生值后口径漂移。
CREATE TABLE IF NOT EXISTS minute_raw (
  symbol        TEXT    NOT NULL,
  trade_day     TEXT    NOT NULL,
  source        TEXT    NOT NULL DEFAULT 'real',  -- real / demo
  t             TEXT    NOT NULL,          -- "09:30"
  price_cents   INTEGER NOT NULL,
  cum_vol       INTEGER NOT NULL,          -- 手，累计
  cum_amt_cents INTEGER NOT NULL,          -- 元 * 100，累计
  PRIMARY KEY (symbol, trade_day, t)
);

-- 做T配对：正T=先买后卖，倒T=先卖后买
CREATE TABLE IF NOT EXISTS t_pairs (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  trade_day      TEXT    NOT NULL,
  symbol         TEXT    NOT NULL,                 -- 与 fills.symbol 对齐
  side           TEXT    NOT NULL CHECK (side IN ('正T','倒T')),
  open_fill_id   INTEGER NOT NULL REFERENCES fills(id),
  close_fill_id  INTEGER          REFERENCES fills(id),
  qty            INTEGER NOT NULL CHECK (qty > 0),
  gross_cents    INTEGER,
  cost_cents     INTEGER,
  net_cents      INTEGER,
  status         TEXT    NOT NULL CHECK (status IN ('OPEN','CLOSED','UNWOUND')),
  opened_at      TEXT    NOT NULL,
  closed_at      TEXT
);

CREATE INDEX IF NOT EXISTS idx_pairs_day ON t_pairs (trade_day, symbol, status);

-- 交易日志 v1：每笔 T 的计划、理由、情绪、错误标签
CREATE TABLE IF NOT EXISTS trade_journal (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  symbol         TEXT    NOT NULL,
  trade_day      TEXT    NOT NULL,
  open_fill_id   INTEGER NOT NULL REFERENCES fills(id),
  close_fill_id  INTEGER          REFERENCES fills(id),
  plan_target_cents INTEGER,
  plan_stop_cents   INTEGER,
  planned        INTEGER NOT NULL DEFAULT 1,
  reason         TEXT,
  emotion        TEXT,
  mistakes_json  TEXT    NOT NULL DEFAULT '[]',
  note           TEXT,
  created_at     TEXT    NOT NULL,
  updated_at     TEXT    NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_journal_pair
  ON trade_journal (symbol, trade_day, open_fill_id, IFNULL(close_fill_id, 0));

-- 信号与否决理由：没有这张表，就无法复盘"为什么没做"
CREATE TABLE IF NOT EXISTS signals (
  id          TEXT NOT NULL,
  trade_day   TEXT NOT NULL,
  ts          TEXT NOT NULL,
  side        TEXT NOT NULL,
  price_cents INTEGER NOT NULL,
  target_cents INTEGER,
  stop_cents   INTEGER,
  qty          INTEGER,
  z            REAL,
  regime       TEXT,
  conf         INTEGER,
  accepted     INTEGER NOT NULL CHECK (accepted IN (0,1)),
  veto_json    TEXT,                               -- string[]
  why_json     TEXT,                               -- string[]
  created_at   TEXT NOT NULL,
  PRIMARY KEY (trade_day, id)                      -- 每日内唯一，S01 不会跨日冲突
);

CREATE INDEX IF NOT EXISTS idx_signals_day ON signals (trade_day, ts);

-- 每日持仓快照：崩了也能恢复
CREATE TABLE IF NOT EXISTS position_snapshots (
  trade_day   TEXT PRIMARY KEY,
  base_qty    INTEGER NOT NULL,
  available   INTEGER NOT NULL,
  frozen      INTEGER NOT NULL,
  t_qty       INTEGER NOT NULL,
  base_cost_cents INTEGER NOT NULL,
  updated_at  TEXT NOT NULL,
  CHECK (base_qty = available + frozen + t_qty)
);

-- 交割单导入记录：可追溯到文件哈希
CREATE TABLE IF NOT EXISTS statements (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  trade_day   TEXT NOT NULL,
  file_name   TEXT NOT NULL,
  file_hash   TEXT NOT NULL,
  imported_at TEXT NOT NULL,
  rows        INTEGER NOT NULL,
  mismatched  INTEGER NOT NULL DEFAULT 0,
  UNIQUE (trade_day, file_hash)
);

-- 未闭环 T：收盘时 open 的配对，次日开盘必须处理
CREATE VIEW IF NOT EXISTS v_unresolved AS
SELECT p.*, f.ts AS open_ts, f.price_cents AS open_price_cents
FROM t_pairs p
JOIN fills f ON f.id = p.open_fill_id
WHERE p.status IN ('OPEN','UNWOUND');

-- 当日净收益（分）
CREATE VIEW IF NOT EXISTS v_daily_net AS
SELECT trade_day,
       SUM(COALESCE(net_cents, 0)) AS net_cents,
       COUNT(*) FILTER (WHERE status = 'CLOSED') AS closed_pairs,
       COUNT(*) FILTER (WHERE status <> 'CLOSED') AS unresolved_pairs
FROM t_pairs
GROUP BY trade_day;
