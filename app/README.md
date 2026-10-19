# 做T决策台 · Tauri 2 桌面端

Tauri 2（Rust）+ Vue 3 + TypeScript + Pinia + ECharts。
一套代码面向 Windows x64 与 macOS（Apple Silicon / Intel 通用二进制）。

> 当前是 **D1–D8 增量**：前端工作台完整可用；Rust 侧行情、SQLite 账本、分时缓存、regime、
> 信号引擎、T 对配对与冲正、CSV 交割单导入与对账均已接入，并在 Windows MSVC 与 macOS arm64 双平台通过 `cargo test` / `cargo check`。
> 防休眠、崩溃恢复与安装包尚未接入（D9–D10）。

![工作台](./shot.png)

## 快速开始

```bash
cd app
pnpm install

# 浏览器模式（用内置回放数据，不需要 Rust）
pnpm dev          # http://127.0.0.1:1425

# 构建前端（vue-tsc + vite）
pnpm build

# 桌面模式（需要 MSVC Build Tools）
# Windows 若提示找不到 link.exe，先在同一个终端执行：
#   call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
pnpm tauri dev
```

> 端口用 **1425**，不是 Tauri 模板默认的 1420。
> 本机 1420 被另一个股票应用占用，`strictPort` 会直接报错。

## 状态（已验证 / 未验证）

| 项 | 状态 | 说明 |
|---|---|---|
| Vue 3 + TS 应用 | ✅ 已验证 | `vue-tsc --noEmit` + `vite build` 通过，产出 `dist/` |
| 工作台 UI | ✅ 已验证 | 分时 / 日K / 决策卡片 / 信号流水 / 门槛压测 / 账务回填，见 `shot.png`、`shot-test.png` |
| Pinia 数据层 | ✅ 已验证 | 浏览器用 `src/data/demo.json`；Tauri 模式走 IPC 注入 |
| IPC 桥 | ✅ 已验证（前端） | `src/lib/ipc.ts`，`__TAURI_INTERNALS__` 检测 + 事件订阅 |
| Tauri 运行 | ✅ 本机验证 | 2026-10-19 19:37 启动，LIVE 行情；`desk.db` 落盘 242 根分时 + 9 条信号 |
| Rust 行情骨架 | ✅ CI 通过 | Windows MSVC + macOS arm64 `cargo check` 通过 |
| SQLite 账本 | ✅ CI 通过 | 幂等/冲突/配对/部分成交/未闭环/冲正重放 |
| 回填幂等 | ✅ CI 通过 | `submit_fill` 双唯一键；前端回填表单已接通 |
| 分时链路 | ✅ CI 通过 | Rust 拉 242 点 → SQLite 缓存 → 特征/regime → `bootstrap` 直出图表 |
| 信号引擎 | ✅ CI 通过 | 4 个单测：回归通过 / 下行禁正T / 薄价差否决 / 冷却 |
| 信号落库 | ✅ CI 通过 | `signals` 表按 `(trade_day, id)` 去重，否决理由落库可复盘 |
| 跨语言一致性 | ✅ CI 通过 | 真实 242 点数据，Rust 与 Python 信号逐条对齐 |
| T 对配对 / 冲正 | ✅ CI 通过 | FIFO 配对、部分成交拆分、未闭环、void 重放 |
| 交割单导入 | ✅ CI 通过 | CSV/TXT、GB18030 回退、表头识别、自然键去重、手工单差异提示 |
| 配置与操作 | ✅ 本机验证 | 设置弹窗保存成功，`meta.config` 落库并重载；操作指引可打开 |
| CI 编译门 | ✅ 全绿 | [run 39245093718](https://github.com/mhxy881126/autoT/actions/runs/39245093718)（17 单测） |
| 安装包 / macOS 产物 | ❌ 未产出 | 需 Windows MSVC 与 macOS + Xcode，见根目录技术方案 §7 |

**验证边界**：本机用 `vcvars64` 初始化 MSVC 后已实际启动窗口，看到 LIVE 行情与真实信号；
CI 负责 Windows/macOS 双平台编译与单测。安装包、签名与公证仍未做。

## 本机为什么编不了 Rust

```
error: error calling dlltool 'dlltool.exe': program not found
error: could not compile `parking_lot_core`
```

- `rustc` 1.96 存在，但 `link.exe` / `cl.exe` 不存在；
- GNU 工具链能链接 Hello World，但 `windows` crate 的导入库需要 `dlltool.exe`；
- crates.io 可访问，依赖能下载，卡在工具链而不是网络；
- macOS 通用二进制必须在 macOS 上构建，Windows 无法交叉编译。

在有 Visual Studio Build Tools（C++ 桌面开发）的机器上：

```bash
cd app/src-tauri
cargo check          # 先过类型检查
cd ..
pnpm tauri dev       # 再起窗口
```

## 目录

```
app/
├─ src/
│  ├─ App.vue                 # 布局：顶栏 / 账户条 / 图表 / 卡片 / 状态栏
│  ├─ components/
│  │  ├─ IntradayChart.vue    # 分时 + 成交量 + regime 背景 + 信号标记
│  │  ├─ KLineChart.vue       # 日K + MA5/10/20
│  │  ├─ DecisionCard.vue     # 通过 / 拒绝 / 等待信号 三态
│  │  └─ Panels.vue           # 信号流水 / 门槛压测 / 账务回填
│  ├─ stores/desk.ts          # Pinia：payload + live tick + 模式
│  ├─ lib/ipc.ts              # invoke / event / 浏览器降级
│  ├─ lib/types.ts            # 复用 ../../contracts/ipc.ts
│  └─ data/demo.json          # 2026-10-19 真实回放数据（68KB）
├─ src-tauri/
│  ├─ src/lib.rs              # Builder + 3 秒轮询 + market:tick
│  ├─ src/market.rs           # GB18030 解码 + 腾讯快照解析
│  ├─ src/session.rs          # 交易日 / 时段（Asia/Shanghai）
│  ├─ src/commands.rs         # app_info / fetch_quote / feed_status
│  └─ tauri.conf.json
├─ contracts/                 # 根目录：ipc.ts / schema.sql
└─ shot.png / shot-test.png
```

## 配置与操作

- 右上角 **设置**：标的代码、底仓数量、底仓成本、可用资金、单次 T 仓上限、
  z 入场阈值、目标价差上下限、止损、每日次数、冷却、交易时段、暂停策略。
- 保存后写入 SQLite `meta.config`，并立即重载该标的的分时、日 K 与信号。
- 右上角 **怎么用**：6 步操作说明（配置 → 看 regime → 读卡片 → 券商下单 → 回填/导入 → 复盘）。

## 已实现的交互

- 浏览器模式：完整工作台，数据来自 `demo.json`（600519，2026-10-19）。
- Tauri 模式：`bootstrap` 拉真实分时 + 日K + 信号，图表与决策卡片都是当日数据；
  订阅 `market:tick` 与 `signals:update` 实时更新。
- 三张卡片状态：`通过` / `拒绝交易` / `等待信号`。没有信号时不猜方向。
- 门槛压测：严格 0.45% vs 放宽 0.25% 的对照，以及 0.20%–0.45% 敏感性柱状图。
- 账务页：回填成交、冲正、配对列表（正T/倒T）、已实现/未闭环汇总。无券商，回填是唯一成交来源。

## 下一步（对应技术方案 §8）

| 天 | 任务 | 状态 |
|---|---|---|
| D3 | SQLite 迁移 + `submit_fill` 幂等 + 账本 | ✅ CI 通过（5 单测） |
| D4 | 分时落盘 + `series` 命令 + 图表切实时 | ✅ CI 通过 |
| D5 | 特征 + regime + `market:tick` 全字段 | ✅ CI 通过 |
| D6 | 信号引擎 + 否决理由落库 | ✅ CI 通过 |
| D7 | T 对配对 + 未闭环识别 + 冲正 | ✅ CI 通过 |
| D8 | 交割单导入 + 对账 | ✅ CI 通过 |
| D9 | 防休眠 + 崩溃恢复 | 下一步 |
| D10 | Windows 安装包 + macOS universal dmg | 待做 |

## 已知问题

- **tick 的 regime 每 60 秒才刷新一次**：分时缓存按分钟重算，3 秒 tick 只更新价格与 z。
- **冲正后要换新 clientId 重新录入**；冲正保留原记录并标记 `voided`，配对按当日全量重放。
- **`position.available` 是近似值**：未闭环倒T时会偏保守；`unrealized` 暂为 0，接行情后补。
- **交割单仅支持 CSV/TXT**：Excel 需先另存为 CSV；表头靠关键词识别，券商自定义模板可能要在 `statement.rs` 里补映射。
- `src-tauri/icons/` 缺 `icon.icns`；macOS 打包前需在 macOS 上执行 `pnpm tauri icon icons/icon.png`。
- Tauri 模式没有单实例插件，先开两个窗口会重复轮询（P3 加 `tauri-plugin-single-instance`）。
- 前端包 1.29MB（ECharts 占大头）；桌面端可接受，若做 Web 分发需要按需引入。
