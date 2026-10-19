# Tauri 2 + Vue 3 + TypeScript 技术方案

> 目标：一套代码发布 Windows（x64）与 macOS（Apple Silicon / Intel 通用二进制）
> 版本：v0.1 · 2026-10-19
> 前置：产品约束见《做T交易系统_审查与路线图.md》v1.1（无券商 / 半自动 / 单票 / T仓≤50% / L1）

---

## 0. 本机环境核验

| 项 | 状态 | 说明 |
|---|---|---|
| rustc / cargo | ✅ 1.96.0 | host `x86_64-pc-windows-msvc` |
| Rust targets | ✅ msvc + gnu | 已有 `x86_64-pc-windows-msvc` |
| Node / pnpm | ✅ v24.13.0 / 11.28.5 | 前端可构建 |
| WebView2 Runtime | ✅ 155.0.4283.62 | Windows 侧运行前提已满足 |
| MSVC 链接器 | ❌ 未找到 `link.exe` / `cl.exe` | **无法链接出 exe**，`cargo check` 可试、`tauri build` 不行 |
| crates.io | ⚠️ 未验证通 | `cargo search` 无返回，需要代理或镜像 |
| macOS 工具链 | ❌ 不存在 | 通用二进制必须在 macOS + Xcode 上构建 |

**结论**：本机不能产出可运行的 Tauri 二进制，也不应假装验证过。下面给出的是**可执行的工程结构与契约**，编译验证放在你的开发机或 CI（§7）。

---

## 1. 结论：栈合适，但五个默认行为会坑交易系统

Tauri 2 的卖点是体积小、内存低、Rust 侧可信。但对"盘中不能停"的盯盘系统，下面五点是**设计约束，不是优化项**：

| # | 默认行为 | 后果 | 必须怎么做 |
|---|---|---|---|
| 1 | WebView 在窗口失焦/最小化时节流定时器 | 行情停更、止损不触发、信号延迟几分钟 | **行情、策略、计时全部放 Rust**；前端只做渲染 |
| 2 | 前端状态（Pinia）易被当成事实源 | 崩溃/刷新后持仓、次数、盈亏错乱 | Rust + SQLite 是唯一事实源；前端每次从 IPC 拉 |
| 3 | `qt.gtimg.cn` 返回 **GB18030** | `from_utf8_lossy` 会乱码，价格解析成 0 | Rust 用 `encoding_rs::GB18030` 解码 |
| 4 | macOS App Nap / Windows 睡眠 | 后台线程被挂起，行情断流 | 盘中 power assertion + 恢复后补数 + 时钟校验 |
| 5 | 允许开多个实例 | 重复回填、重复记账 | `tauri-plugin-single-instance` + 启动时账本完整性检查 |

补充两条容易被忽略的：

- **时区**：所有 `trade_day` 按 `Asia/Shanghai` 计算，时间戳存 epoch 毫秒，不依赖系统时区。
- **安装路径**：SQLite 放 `app_data_dir`，**不要放 OneDrive/坚果云同步目录**（WAL 文件被同步会损坏）。

---

## 2. 进程模型

```
┌──────────────────────────── Tauri 进程 ────────────────────────────┐
│  Rust Core（tokio，独立于窗口生命周期）                              │
│  ├─ market feed      3s 快照轮询 / 60s 分时刷新 / 双源降级           │
│  ├─ session clock    交易日历、集合竞价、午休、收盘                    │
│  ├─ features         VWAP / z / 量比 / 斜率 / ATR / regime           │
│  ├─ strategy         信号 + 否决理由（确定性，可回放）                │
│  ├─ ledger           SQLite WAL，事务，T 对配对，未闭环识别           │
│  ├─ power guard      Win: SetThreadExecutionState / macOS: IOPMAssertion │
│  └─ event bus        emit("market:tick" | "signal:new" | "risk:alert") │
│           │                                                          │
│           ▼  invoke（请求/响应） + event（单向推送）                   │
│  WebView（Vue 3 + TS）                                               │
│  ├─ Pinia：仅视图状态（选中标的、tab、缩放），不存账务                 │
│  ├─ ECharts：分时 / 日K / 敏感性                                     │
│  └─ 交互：看卡片、回填成交、导入交割单                                 │
└──────────────────────────────────────────────────────────────────────┘
```

**硬规则**：Rust 不发"建议买"的模糊事件，只发结构化 `Signal`；前端不做任何阈值判断。

---

## 3. 仓库结构

```
app/
├─ package.json
├─ vite.config.ts
├─ tsconfig.json
├─ index.html
├─ src/                        # Vue 3 + TS
│  ├─ main.ts
│  ├─ App.vue
│  ├─ styles/terminal.css
│  ├─ components/
│  │  ├─ TopBar.vue
│  │  ├─ AccountStrip.vue
│  │  ├─ IntradayChart.vue
│  │  ├─ KLineChart.vue
│  │  ├─ DecisionCard.vue
│  │  └─ LedgerPanel.vue
│  ├─ stores/
│  │  ├─ market.ts
│  │  └─ view.ts
│  └─ lib/
│     ├─ ipc.ts                # invoke/event 封装 + 浏览器降级
│     ├─ types.ts              # 与 contracts/ipc.ts 同源
│     └─ format.ts
├─ src-tauri/
│  ├─ Cargo.toml
│  ├─ build.rs
│  ├─ tauri.conf.json
│  ├─ capabilities/default.json
│  ├─ icons/                   # pnpm tauri icon 生成
│  └─ src/
│     ├─ main.rs
│     ├─ lib.rs                # Builder、插件、状态
│     ├─ commands.rs           # IPC 入口（薄）
│     ├─ state.rs              # AppState（Arc<RwLock<...>>）
│     ├─ market/
│     │  ├─ mod.rs
│     │  ├─ feed.rs            # reqwest + GB18030 + 重试
│     │  ├─ session.rs         # 交易日历 / 时段
│     │  ├─ features.rs
│     │  └─ strategy.rs
│     ├─ store/
│     │  ├─ mod.rs             # rusqlite + migrations
│     │  ├─ ledger.rs          # fills / t_pairs / 对账
│     │  └─ schema.sql
│     └─ power.rs              # 防休眠（cfg 分平台）
├─ contracts/
│  ├─ ipc.ts                   # 前后端共享类型（单一来源）
│  └─ schema.sql
└─ tests/
   ├─ golden/                  # 与 Python 原型对齐的黄金样本
   └─ e2e/                     # tauri-driver（可选）
```

---

## 4. IPC 契约

### 4.1 Commands（请求/响应）

| 命令 | 入参 | 返回 | 说明 |
|---|---|---|---|
| `bootstrap` | — | `Bootstrap` | 启动拉一次：配置、账户、底仓、今日信号、最后快照 |
| `snapshot` | — | `Snapshot` | 手动刷新（正常靠事件） |
| `series` | `{day}` | `MinuteBar[]` | 分时，按需 |
| `kline` | `{period, limit}` | `KLine[]` | 日/60m/15m |
| `submit_fill` | `FillInput` | `Fill` | 回填成交，幂等 |
| `list_fills` | `{day}` | `Fill[]` | |
| `ledger_day` | `{day}` | `LedgerDay` | 持仓、T 对、未闭环 |
| `import_statement` | `{path}` | `ImportResult` | 券商交割单 CSV/Excel |
| `update_config` | `Partial<Config>` | `Config` | 写库并热更新 |
| `feed_status` | — | `FeedStatus` | 连接、延迟、数据源 |

### 4.2 Events（单向推送）

| 事件 | 频率 | 载荷 |
|---|---|---|
| `market:tick` | 3s | `{t, price, avg, vwap, z, regime, vol, latency_ms}` |
| `signal:new` | 事件驱动 | `Signal`（含 `accepted`、`veto[]`） |
| `risk:alert` | 事件驱动 | `{level, code, message}` |
| `feed:status` | 状态变化 | `{connected, source, latency_ms}` |
| `ledger:changed` | 回填/导入后 | `{day}` |

### 4.3 三条契约规则

1. **类型单一来源**：`contracts/ipc.ts` 由前端直接 import；Rust 侧用 `serde` 对齐字段名（`rename_all = "camelCase"`）。改契约先改这个文件。
2. **业务逻辑不跨 IPC**：不在前端算盈亏、算可用数量、算次数。前端只格式化。
3. **写操作幂等**：`submit_fill` 必须带 `clientId`（UUID），Rust 侧唯一索引去重；导入交割单按 `(trade_day, ts, side, price, qty)` 去重。

---

## 5. 数据层

- `rusqlite`（bundled）或 `sqlx` + SQLite；`journal_mode=WAL`，`synchronous=NORMAL`。
- 所有写操作在事务里；账本变更与 `position_snapshots` 同事务。
- 金额一律用 **整数分** 或 `f64` + 四舍五入到 0.01；推荐整数分避免浮点误差。
- 表：`meta` / `fills` / `t_pairs` / `signals` / `position_snapshots` / `statements`。
- 关键约束：
  - `fills` 唯一键 `(trade_day, ts, side, price, qty)` 防重复回填；
  - `t_pairs.status ∈ OPEN/CLOSED/UNWOUND`，`UNWOUND` 就是"未闭环"，必须在次日开盘提示；
  - `signals.accepted` 与 `veto_json` 必须落库，否则无法复盘"为什么没做"。

完整 DDL 见 `contracts/schema.sql`。

---

## 6. 行情接入（Rust）

```rust
// 关键：GB18030，不要 from_utf8_lossy
let (text, _, had_errors) = encoding_rs::GB18030.decode(&bytes);
if had_errors { return Err(FeedError::Decode); }
```

| 项 | 设计 |
|---|---|
| 轮询 | 快照 3s；分时整表 60s；日 K 每日一次 |
| 超时 | connect 3s / total 5s，失败退避 1s→2s→4s，上限 30s |
| 双源 | 腾讯主、Sina 备；主源连续 3 次失败切换，30 分钟后回切 |
| 限流 | 单标的 QPS ≤ 1；加 0–300ms 抖动，避免固定节奏 |
| 交易日历 | 内置 JSON（节假日 + 调休），启动时校验，过期告警 |
| 时钟 | 每次快照记录本地单调时钟与交易所时间差；> 2s 告警 |
| 落盘 | 原始快照按日 gzip，供回测与事故复盘 |

**不要**在 WebView 里 `fetch` 行情：CORS、节流、无持久化，三个问题一次踩全。

---

## 7. 打包、签名与分发

### 7.1 Windows

```bash
pnpm tauri build --target x86_64-pc-windows-msvc
# 产物：NSIS .exe + .msi
```

- `bundle.windows.webviewInstallMode`: `downloadBootstrapper`（默认）或 `embedBootstrapper`（离线安装包）。
- 代码签名可选（EV 证书可减少 SmartScreen 拦截），内部使用可先不签。
- 安装到 per-user 目录，避免 UAC。

### 7.2 macOS（通用二进制）

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
pnpm tauri build --target universal-apple-darwin
```

- 必须在 macOS 上构建；Windows 无法交叉编译。
- 需要 Apple Developer ID（$99/年）+ 公证，否则用户首次打开会被 Gatekeeper 拦。
- `tauri.conf.json`：`bundle.macOS.signingIdentity`、`hardenedRuntime: true`、`entitlements`。
- 公证：`xcrun notarytool submit ... --wait`，然后 `xcrun stapler staple`。
- Intel 机器需要 `x86_64` 切片，通用二进制体积约翻倍（可接受）。

### 7.3 CI（GitHub Actions 矩阵）

```yaml
jobs:
  build:
    strategy:
      matrix:
        include:
          - os: windows-latest
            target: x86_64-pc-windows-msvc
          - os: macos-14          # Apple Silicon runner，产 universal
            target: universal-apple-darwin
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with: { node-version: 22, cache: pnpm }
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - uses: swatinem/rust-cache@v2
      - run: pnpm install --frozen-lockfile
      - run: pnpm tauri build --target ${{ matrix.target }}
        env:
          APPLE_ID: ${{ secrets.APPLE_ID }}
          APPLE_PASSWORD: ${{ secrets.APPLE_PASSWORD }}
          APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
      - uses: actions/upload-artifact@v4
        with: { name: ${{ matrix.os }}, path: src-tauri/target/**/bundle/** }
```

### 7.4 更新策略

- `tauri-plugin-updater` + minisign 公钥。
- **交易时段禁止自动更新**；只在收盘后检查，且默认"询问"而非静默安装。
- 更新包与当前版本账本 schema 必须兼容；不兼容时先迁移再启动。

---

## 8. P1 十天计划（Walking Skeleton → 可用）

| 天 | 任务 | 当日出口 |
|---|---|---|
| D1 | Tauri 2 脚手架、单实例、日志、窗口尺寸持久化 | `pnpm tauri dev` 起窗，重启保留布局 |
| D2 | Rust `session` + `feed`（GB18030、超时、退避）、一个 `snapshot` 命令 | 前端能显示实时价，断网有状态 |
| D3 | SQLite 迁移、`meta`/`fills` 表、`submit_fill` 幂等 | 回填 10 笔，重复提交不重复记 |
| D4 | 分时落盘、`series` 命令、ECharts 分时组件 | 收盘后能回放当日 |
| D5 | 特征 + regime（Rust），`market:tick` 事件推送到卡片 | 卡片随行情更新，不依赖前端定时器 |
| D6 | 信号 + 否决理由落库，信号流水 UI | 每条否决可读 |
| D7 | `t_pairs` 配对、未闭环识别、`ledger_day` | 收盘后能看到"未闭环 1 笔" |
| D8 | 交割单导入 + 对账 + 差异清单 | 导入 1 天交割单，差异为 0 |
| D9 | 防休眠、崩溃恢复、异常态（断网/停牌/涨跌停） | 杀进程重启不重复记账 |
| D10 | Windows 安装包 + macOS universal dmg + 公证 dry run | 两台机器各装一次 |

**并行**：D1 就要把 CI 建起来，否则 macOS 产物会拖到最后一天才发现签名问题。

---

## 9. 验收标准（P1）

| 类别 | 指标 | 目标 |
|---|---|---|
| 稳定性 | 窗口最小化 4 小时，tick 到达率 | ≥ 99%（证明未被节流） |
| 稳定性 | 杀进程重启，重复记账 | 0 |
| 账务 | 100 笔回填，与手工台账差异 | 0 |
| 账务 | 导入交割单对账差异 | 0 |
| 性能 | tick → UI 渲染 P99 | < 200ms |
| 性能 | 冷启动到首屏 | < 2s |
| 兼容 | Windows 10/11 125% 缩放 | 布局不破 |
| 兼容 | macOS 14/15，Apple Silicon + Intel | 均能启动 |
| 分发 | 未签名 macOS 包 | 明确提示安装步骤，不静默失败 |

---

## 10. 从 Python 原型迁移

原型 `prototype/` 不废弃，转为**规格与黄金样本**：

1. 用原型生成 `tests/golden/*.json`：同一份分钟数据 → 特征、regime、信号、成交。
2. Rust 实现逐字段对齐；CI 里跑 golden 对比，差异 > 1e-6 即失败。
3. UI 按 §3 的组件拆分迁移，视觉稿以 `prototype/web/shot_signals.png` 为准。
4. Python 侧保留 `build_demo.py` 作为"离线重放器"，用于构造边界样本（涨跌停、停牌、跳水）。

**不要**在 Rust 里重写一遍再和 Python 对拍之前就上线——两套实现的偏差会以"策略失灵"的形式出现，很难查。

---

## 11. 风险

| 风险 | 触发 | 影响 | 对策 |
|---|---|---|---|
| 无 MSVC / 无 macOS 机器 | 开发环境不齐 | 无法出包 | CI 必须从 D1 建；本地只做前端与 `cargo check` |
| crates.io 不可达 | 网络策略 | 依赖拉不下来 | 配镜像/代理；vendor 关键 crate |
| macOS 公证失败 | 证书/ entitlements | 用户打不开 | D1 就跑一次 dry run，不要留到发布 |
| WebView 差异 | WKWebView vs WebView2 | 图表/字体错位 | 两端各跑一遍截图对比；避免 `backdrop-filter` 等重特效 |
| 行情接口变更 | 非官方接口 | 数据断流 | 双源 + 解析层隔离 + 每日健康检查 |
| 时区/交易日历错误 | 节假日调休 | 空跑或漏跑 | 日历 JSON 版本化，启动校验 |
| 账本浮点误差 | 用 f64 记金额 | 对账差几分钱 | 金额用整数分 |

---

## 12. 下一步

**D1–D2 的 walking skeleton** 是唯一正确的起点：一个窗口、一个 Rust 命令、一条真实行情、一次 SQLite 写入。跑通之后，后面都是加模块，不再是"赌架构"。

在当前机器上我能做的验证只有前端构建；Rust 侧需要一台带 MSVC 的 Windows 或 macOS。建议先用 CI 的 `cargo check` 当第一道门。
