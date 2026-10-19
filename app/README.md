# 做T决策台 · Tauri 2 桌面端

Tauri 2（Rust）+ Vue 3 + TypeScript + Pinia + ECharts。
一套代码面向 Windows x64 与 macOS（Apple Silicon / Intel 通用二进制）。

> 当前是 **D1–D6 增量**：前端工作台完整可用；Rust 侧行情、SQLite 账本、分时缓存、regime、
> 信号引擎与否决理由落库均已接入，并在 Windows MSVC 与 macOS arm64 双平台通过 `cargo test` / `cargo check`。
> T 对配对与对账尚未接入（D7–D8）。

![工作台](./shot.png)

## 快速开始

```bash
cd app
pnpm install

# 浏览器模式（用内置回放数据，不需要 Rust）
pnpm dev          # http://127.0.0.1:1425

# 构建前端（vue-tsc + vite）
pnpm build

# 桌面模式（需要 MSVC Build Tools，见下）
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
| Tauri 配置 | ⚠️ 未打包 | `tauri.conf.json` / capabilities / icons 已就位；`cargo check` 通过 |
| Rust 行情骨架 | ✅ CI 通过 | Windows MSVC + macOS arm64 `cargo check` 通过 |
| SQLite 账本 | ✅ CI 通过 | `store.rs` + `ledger.rs`，5 个单测全绿（幂等/自然键/内容冲突/非法输入/汇总） |
| 回填幂等 | ✅ CI 通过 | `submit_fill` 双唯一键；前端回填表单已接通 |
| 分时链路 | ✅ CI 通过 | Rust 拉 242 点 → SQLite 缓存 → 特征/regime → `bootstrap` 直出图表 |
| 信号引擎 | ✅ CI 通过 | 4 个单测：回归通过 / 下行禁正T / 薄价差否决 / 冷却 |
| 信号落库 | ✅ CI 通过 | `signals` 表按 `(trade_day, id)` 去重，否决理由落库可复盘 |
| 跨语言一致性 | ✅ CI 通过 | 真实 242 点数据，Rust 与 Python 信号逐条对齐 |
| CI 编译门 | ✅ 全绿 | [run 39241919794](https://github.com/mhxy881126/autoT/actions/runs/39241919794)（12 单测） |
| 安装包 / macOS 产物 | ❌ 未产出 | 需 Windows MSVC 与 macOS + Xcode，见根目录技术方案 §7 |

**没有伪造验证**：Rust 代码是按 Tauri 2 API 写的，但没有在本机编译过。
第一次在有 MSVC 的机器上执行 `cargo check` 时，预计会有少量类型/API 修正。

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

## 已实现的交互

- 浏览器模式：完整工作台，数据来自 `demo.json`（600519，2026-10-19）。
- Tauri 模式：启动调用 `app_info` + `fetch_quote`，顶栏显示实时价；订阅 `market:tick` 更新；
  图表仍为回放样本（**这是刻意的**，分时序列命令在 D4）。
- 三张卡片状态：`通过` / `拒绝交易` / `等待信号`。没有信号时不猜方向。
- 门槛压测：严格 0.45% vs 放宽 0.25% 的对照，以及 0.20%–0.45% 敏感性柱状图。
- 账务页展示成本模型与回填表单（禁用），明确「无券商，回填是唯一成交来源」。

## 下一步（对应技术方案 §8）

| 天 | 任务 | 状态 |
|---|---|---|
| D3 | SQLite 迁移 + `submit_fill` 幂等 + 账本 | ✅ CI 通过（5 单测） |
| D4 | 分时落盘 + `series` 命令 + 图表切实时 | ✅ CI 通过 |
| D5 | 特征 + regime + `market:tick` 全字段 | ✅ CI 通过 |
| D6 | 信号引擎 + 否决理由落库 | ✅ CI 通过（12 单测） |
| D7 | T 对配对 + 未闭环识别 + 冲正 | 下一步 |
| D8 | 交割单导入 + 对账 | 待做 |
| D9 | 防休眠 + 崩溃恢复 | 待做 |
| D10 | Windows 安装包 + macOS universal dmg | 待做 |

## 已知问题

- **tick 的 regime 每 60 秒才刷新一次**：分时缓存按分钟重算，3 秒 tick 只更新价格与 z；D5 会把 regime 跟到分钟级。
- **修改成交只能新增、不能改。** 同一 `clientId` 内容变化会显式报错，需走冲正（red 单）；`amend_fill` 与审计链在 D7 与配对一起做。
- `src-tauri/icons/` 缺 `icon.icns`；macOS 打包前需在 macOS 上执行 `pnpm tauri icon icons/icon.png`。
- Tauri 模式没有单实例插件，先开两个窗口会重复轮询（P3 加 `tauri-plugin-single-instance`）。
- `market:tick` 目前只推快照（`Quote`），不含 z / regime；D5 换成完整 `Snapshot`。
- 前端包 1.29MB（ECharts 占大头）；桌面端可接受，若做 Web 分发需要按需引入。
