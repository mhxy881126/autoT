# autoT · 做T决策台

[![ci](https://github.com/mhxy881126/autoT/actions/workflows/ci.yml/badge.svg)](https://github.com/mhxy881126/autoT/actions/workflows/ci.yml)

面向 A 股短线散户的单票做T决策系统：**先判断今天该不该做，再谈在哪做**。

- 桌面端：Tauri 2（Rust）+ Vue 3 + TypeScript，一套代码发布 Windows 与 macOS（Apple Silicon / Intel）
- 约束：**无券商通道**，因此不做自动下单；系统负责决策、台账、对账，下单与成交由人工完成并回填
- 数据：L1 免费行情（3 秒快照 / 1 分钟线），不做盘口类策略

> 这不是“AI 自动交易机器人”。它解决的是：**大多数日子、大多数股票，不应该做 T。**

## 仓库结构

```
├─ app/                     # Tauri 2 + Vue 3 + TS 桌面端
│  ├─ src/                  # 工作台：分时 / 日K / 决策卡片 / 信号流水 / 压测 / 回填
│  ├─ src-tauri/            # Rust：行情、交易日历、SQLite 账本
│  └─ README.md             # 运行方式、已验证/未验证边界
├─ contracts/
│  ├─ ipc.ts                # 前后端共享类型（单一来源）
│  └─ schema.sql            # SQLite 账本 DDL
├─ prototype/               # Python 参考实现 + 可回放原型（规格与黄金样本）
├─ 做T交易系统_审查与路线图.md   # 产品/交易逻辑审查 + v1.1 路线图
├─ 技术方案_Tauri2_Vue3.md     # 进程模型、IPC、打包签名、10 天计划
└─ .github/workflows/ci.yml    # Windows MSVC + macOS arm64 双平台编译门
```

## 快速开始

```bash
cd app
pnpm install
pnpm dev            # 浏览器模式，http://127.0.0.1:1425（用内置回放数据）
pnpm build          # vue-tsc + vite build
pnpm tauri dev      # 桌面模式，需要 MSVC / Xcode
```

## 当前进度

| 阶段 | 内容 | 状态 |
|---|---|---|
| D1 | Tauri 脚手架、单实例、工作台 UI | 部分完成（单实例待加） |
| D2 | Rust 行情：GB18030 解码、3 秒轮询、事件推送 | 完成（CI 绿） |
| D3 | SQLite 账本、回填幂等、冲突检测 | 完成（CI 绿，5 单测） |
| D5 | 特征 + regime + tick 全字段 | 完成（CI 绿） |
| D6 | 信号引擎 + 否决理由落库 | 完成（CI 绿） |
| D7–D10 | T 对配对、交割单对账、防休眠、打包 | 待做 |

**验证**：前端 `vue-tsc + vite build` 通过；Rust 在 Windows MSVC 与 macOS arm64 双平台通过 `cargo test` + `cargo check`（12 个单测，含 Python–Rust 黄金样本对齐）。CI：[run 39241919794](https://github.com/mhxy881126/autoT/actions/runs/39241919794)。

## 关键设计

- **行情时钟在 Rust**：WebView 失焦会被节流，`setInterval` 不可信。
- **金额用整数分**：避免日终对账的浮点尾差。
- **回填双唯一键**：`clientId` 防重复提交，`(trade_day, ts, side, price, qty)` 防重复导入；改成交必须走冲正。
- **不做 LLM 实时决策**：延迟、不可回测、不可审计；LLM 只在环外做复盘。
- **不做盘口策略**：L1 的 3 秒切片看不到队列，盘口信号是伪信号。

## 文档

- [做T交易系统审查与路线图](做T交易系统_审查与路线图.md)
- [Tauri 2 + Vue 3 技术方案](技术方案_Tauri2_Vue3.md)
- [桌面端运行说明](app/README.md)
