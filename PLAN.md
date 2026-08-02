# Dimi Rust 化完整迁移规划

> 状态：规划基准（v1）。所有迁移工作以此为准，不要做一步想一步。
> 分支：`rust/wire`（worktree `.worktrees/rust`）。主工作区/main 继续承载 TS 产品演进。

## 0. 总纲

- **形态**：strangler 模式——每个模块 Rust 化后通过 napi-rs 桥换进运行中的 TS 产品，整个系统带 Rust 模块跑 e2e；模块逐个绞杀，TS 壳萎缩，最终单 Rust 二进制接管。
- **桥的定位**：迁移插座（运行时依赖）+ 差分测试台（验证机制）。终局退役。
- **原则**：
  1. 按自己的架构重设计，不翻译 kimi 的 TS（无 DI 容器、事件源核心、纯核心 + 效果边界）；
  2. **产品一致性（parity）是硬要求**：除明确删除的功能外，Rust 版必须与 TS 版产品逐功能一致——不存在"首发范围"或"后期再做"的范围缩减；切片只决定实现顺序，不决定范围；
  3. wire 契约（现有 /api/v1 + transcript 契约）是 Rust 与 JS 面的接缝，必须逐字节兼容；
  4. 每个模块换入的标准验收：现有测试套件全绿 + 日常 dogfood 正常 + 可回退（开关）+ 逐功能对照表无遗漏；
  5. JS 面保留（dimi-web、node-sdk/klient、transcript 浏览器 reducer），其行为同样以 TS 版为基准保持一致；
  6. 无用户、历史会清理：不做兼容仪式（changeset 等仅按需）。
- **完成标准**：Rust 二进制成为主产品，功能面与 TS 版一致（除明确删除项），TS 引擎/服务/TUI 退役删除，桥退役。

## 1. 现状基线（TS 侧，39 万行）

| 模块 | 行数 | 迁移状态 | 对应 Rust crate |
|---|---|---|---|
| apps/dimi（CLI/TUI） | ~10 万 | M5 | dimi-cli + dimi-tui |
| apps/dimi-web（Vue） | ~3.6 万 | **保留 JS**（连 Rust server） | — |
| packages/agent-core-v2 | ~21 万 | M1/M2/M3 | dimi-store / dimi-exec / dimi-engine |
| packages/kap-server | ~4.7 万 | M1/M4 | dimi-store(内嵌) / dimi-server |
| packages/pi-tui | ~2.7 万 | M5（被 ratatui 替代） | dimi-tui |
| packages/node-sdk | ~1.2 万 | **保留 JS**（协议驱动 Rust 引擎） | — |
| packages/protocol | ~1 万 | 保留为契约单一事实源（后期 schema 化） | dimi-wire 镜像 |
| packages/klient | ~1 万 | **保留 JS** | — |
| packages/transcript | ~0.5 万 | 服务端部分 M1；浏览器 reducer **保留 JS** | dimi-store |
| packages/oauth / telemetry | ~0.3 万 | 后期（可 Rust 化） | dimi-oauth / dimi-telemetry |

已删除（前期清理完成）：tree-sitter-bash、dimi-inspect、搜索+minidb、ACP+kaos、vis、死代码、Legacy 模块。

## 2. 目标架构（Rust 产品）

```
dimi-cli（二进制入口）
 ├── dimi-tui（ratatui）
 ├── dimi-server（axum：REST + WS，讲 /api/v1 协议）
 │     └── dimi-engine（回合循环 / LLM / 工具 / 权限 / 上下文）
 │           ├── dimi-store（事件源：wire 记录 + 冷重建）
 │           └── dimi-exec（进程 / pty / 文件 / 环境探测）
 └── dimi-wire（契约类型，纯数据）
```

架构原则：
- **事件源**：所有会话状态从 wire 记录重建（enum Op + append-only log）；
- **无 DI 容器**：Session/Agent 是 tokio 任务，消息传递；
- **纯核心 + 效果边界**：回合编排是纯函数，LLM/进程/文件经 trait 注入；
- **权限简单**：glob 规则 + 用户交互（不搞解析式安全）。

## 3. 迁移序列总览

```
M0  dimi-wire（契约镜像）＋ Cargo workspace
  ↓
M0.5 dimi-bridge（napi-rs 桥）＋ packages/dimi-native ＋ 差分测试基建
  ↓
M1  dimi-store（事件源存储）──┐
M2  dimi-exec（执行层）────────┤
                              ↓
M3  dimi-engine（引擎核心，垂直切片 ×N）
                              ↓
M4  dimi-server（axum 服务）→ 里程碑：dimi-web 连 Rust 服务
                              ↓
M5  dimi-tui + dimi-cli（ratatui + CLI 壳）
                              ↓
M6  切换与退役（删 TS、清历史、桥退役）
```

依赖：M3 依赖 M1+M2；M4 依赖 M3；M5 依赖 M3（TUI 框架可先用 mock engine）；M1/M2 相互独立可并行。

## 4. 模块规格（每模块：边界 / 契约 / 验证 / 换入 / 风险）

### M0 — 工具链 + dimi-wire
- **交付**：Cargo workspace（edition 2024、workspace lints）、`crates/dimi-wire`、`.gitignore`（/target）、fmt/clippy 规范。
- **范围**：镜像 `packages/transcript/src/contract/schema.ts` 核心对话模型——ID（newtype + is_plain_agent_id）、TurnOrigin、Usage/StepTiming/StepRetry、TurnState/StepState、Frame（text/thinking/tool/notice）、Step/Turn/Marker/TaskRef/Task、AgentPhase。
- **纪律**：字段名与 JSON 形状逐字节对齐 zod（snake_case + optional 语义一致）。
- **验证**：cargo test + fixtures 解析（fixtures 从 TS 测试与真实 wire 文件提取）。
- **完成标准（DoD）**：dimi-wire 与 zod schema 逐字段人工核对通过 + fixtures 全绿。

### M0.5 — dimi-bridge + packages/dimi-native
- **交付**：`crates/dimi-bridge`（napi-rs：parse/validate 函数）、`packages/dimi-native`（TS 包装，正式 pnpm workspace 成员 + flake.nix）、差分测试基建（fixtures 目录 + TS↔Rust 对拍 runner）。
- **验证**：vitest 差分测试——同一 JSON → zod 解析 vs serde 解析，接受/拒绝一致 + 字段值一致。
- **风险**：napi-rs 与 Node 24 兼容（低，需实测）。

### M1 — dimi-store（事件源存储）✅ 已完成
- **TS 对应物**：transcript 服务端（L1 store/L2 reducer/持久化）＋ kap-server TranscriptService 存储底座 ＋ persistence wire 记录。
- **边界**：wire.jsonl 追加日志、冷重建、状态快照、增量读、op-batch 序列（transcriptSeqSchema：seq/watermark/since cursor）。
- **验证**：同一 wire.jsonl → TS 冷重建 vs Rust 冷重建状态树对比；kap-server transcript 测试换底后全绿。
- **换入**：kap-server TranscriptService 经桥用 dimi-store；**Rust 为默认后端**，`dimi --legacy`（或 `DIMI_LEGACY_STORE=1`）退回 TS。
- **风险**：wire 记录边角（migration v1.2、unknown record 跳过语义）。
- **不迁移**：浏览器侧 reducer（dimi-web 用，保留 TS）。

**M1 落地记录（rust/wire 分支）**：
- 交付物：`crates/dimi-store`（state/apply/reduce/group/fold/wire/paginate，14 种 op reducer）、`crates/dimi-wire` 扩展（op/entity/snapshot/record/phase）、dimi-bridge 新增 `coldRebuild`/`paginateTurns`/`readWireRecords`/`RustAgentTranscript`、`packages/kap-server/src/services/transcript/rustTranscriptStore.ts`（Rust 适配器）。
- 差分防线：dimi-native 17 测试（apply 6 场景 + pagination + 契约）+ kap-server `store-cold-differential.test.ts`（4 场景，TS 三阶段 vs Rust 冷重建，JSON 字节级对比）。
- **架构决策（已定案）**：
  1. **op-batch 序列（seq/watermark/journal）留在 TS** —— 协议层决策（会话级排序、bound journal 存活期与 live store 同生命周期），Rust 只管存储语义；journal 由 TranscriptService 的 `opsJournals` 持有，不迁移。
  2. **接口抽象**：`@dimi-agent/transcript` 新增 `TranscriptStoreLike`/`AgentTranscriptLike`（TS 类带 `#private` 字段无法结构兼容，消费方必须面向接口）；kap-server 的 coreBinding/broadcaster/routes 已全部改为接口类型。
  3. **适配器读方法从 snapshot() 派生**（getItems/getTurn/getTasks/…/listPendingInteractions），pendingInteractions 由 interactions 过滤派生（与 TS applyReset 同源）。
  4. **AppliedOps.gap 省略而非 null**（TS 消费方判 `!== undefined`）；`RustAgentTranscriptAdapter.apply` 再归一化一次防御。
- **换入验证**：默认模式（Rust）与 `DIMI_LEGACY_STORE=1`（TS）均为 744/744 全绿（boot auth 测试已做环境隔离并改用 deepseek）。
- 换入期间修掉的 parity bug：RecordTime 接受 string time、冷重建快照无 hasMoreOlder 键（live 有）、tool.call 事件字段是 `args` 非 `arguments`、纯 user turn 的 prompt 丢失（start_turn 签名）、tasks/interactions 保插入序、`Ended.phase.at` 可选、`plan_revision ?? {}` 语义。

### M2 — dimi-exec（执行层）
- **TS 对应物**：os/ backends（hostFs/hostProcess/hostEnvironment）、ISessionProcessRunner + node-pty、execEnv helpers。
- **边界**：进程（spawn/pty/kill/wait/信号）、文件（read/write/list/watch）、环境探测（login shell PATH）。
- **验证**：agent-core-v2 os 域测试 + Bash 工具 e2e + 真实命令集成测试。
- **换入**：ISessionProcessRunner 换桥实现；开关 `DIMI_RUST_EXEC=1`。
- **风险**：pty 语义（node-pty vs portable-pty）、Windows（parity 要求：TS 产品支持 Windows，Rust 版必须支持；实现顺序可后置，M6 前完成）。

### M3 — dimi-engine（引擎核心，最大风险区）
- **TS 对应物**：agent-core-v2 agent/ 域全量（loop、llmRequester、toolExecutor、contextMemory、contextProjector、permissionPolicy/Rules/Mode、profile、plan、swarm、cron、mcp、media、fullCompaction、undo、usage、faultInjection、stepRetry、wait…）。
- **策略**：垂直切片，每片有独立 DoD；切片 1 立住新架构（事件源 + 纯核心 + 效果边界）。**所有切片均为必需（parity），顺序即实现顺序，不按切片缩减范围**：
  1. 最小闭环：会话创建 → 用户输入 → LLM（OpenAI 兼容 SSE）→ 工具执行（Bash via dimi-exec）→ 回合完成 → wire 落盘；
  2. 权限/审批（glob 规则 + 交互）；
  3. 上下文管理（memory/projector/fullCompaction）；
  4. 子代理（Agent/AgentSwarm）与 swarm 模式；
  5. MCP 服务器 / 插件 / skills；
  6. cron / plan 模式 / undo / 媒体输入。
  每个切片换入前，对照 TS 版逐功能核对（功能清单对照表），无遗漏才算完成。
- **验证**：脚本化差分（mock LLM + 固定工具结果 → 同输入序列对比 wire 输出）；shadow mode（真 LLM 双跑对比）。
- **换入**：TS 壳（TUI/server/SDK）经桥驱动 dimi-engine 作为会话后端；开关 `DIMI_RUST_ENGINE=1`。
- **风险**：功能面过宽 → 切片严格排序、每片 DoD、影子模式兜底。

### M4 — dimi-server（axum）
- **TS 对应物**：kap-server（routes、WS、auth、transcript 面、debug 面、config、snapshot、guiStore…）。
- **边界**：/api/v1 REST + /api/v1/ws 协议面 + 全局事件广播（event.session.* 等）。
- **验证**：kap-server 测试套件协议断言对拍；**dimi-web 直连 Rust 服务（迁移过半里程碑）**。
- **换入**：`dimi web` 指向 Rust 服务；开关 `DIMI_RUST_SERVER=1`。
- **风险**：WS 协议细节（transcript.ops 订阅语义、事件广播时序、suppression 规则）。

### M5 — dimi-tui + dimi-cli
- **TS 对应物**：apps/dimi（TUI + CLI 壳）+ pi-tui。
- **边界**：ratatui TUI、CLI 命令面（config.toml 兼容、slash commands、快捷键）、插件市场入口。
- **验证**：dogfood（日常工作流 100% 可用）+ 功能清单对照表逐项核对（parity 硬要求，无首发范围）。
- **风险**：TUI 打磨时间（10 万行 TS 的交互细节）。

### M6 — 切换与退役
- Rust 二进制成为主产品；删除 agent-core-v2、kap-server、transcript 服务端、pi-tui、apps/dimi TS 壳；桥退役。
- 保留：dimi-web、node-sdk/klient、packages/protocol（或 schema 化）。
- 发布管线切换到原生二进制发布（现有 GitHub Release 管线改造）；历史清理（用户已确认）。

## 5. 横切事项

1. **测试基建**：fixtures 库（wire 样本、JSON 文档）、差分 runner、shadow-mode runner——M0.5 建立，全迁移复用。
2. **契约一致性**：packages/protocol 为单一事实源（TS），Rust 侧镜像 + 差分守卫；后期 schema 化（JSON Schema 双端生成）。
3. **config.toml**：Rust 用 toml crate 解析，格式兼容（**决策点：建议兼容**）。
4. **LLM provider**：首发 OpenAI 兼容 + 现有主要 providers（**决策点**）。
5. **日志/遥测**：tracing 替代 pino；telemetry 事件格式兼容。
6. **文档**：gen-docs 流程沿用；docs 随模块迁移更新。
7. **性能目标**：启动 <100ms、日常 dogfood 基准；在 M5 前后测。

## 6. 风险矩阵

| 风险 | 等级 | 对策 |
|---|---|---|
| 引擎范围失控 | 高 | 切片制 + 每片 DoD + 影子模式 |
| 契约漂移（TS/Rust 双实现） | 高 | 差分测试守卫（M0.5 基建） |
| **parity 遗漏（功能被悄悄丢掉）** | 高 | 每模块"功能清单对照表"：换入前逐功能核对 TS 行为 |
| TUI 打磨时间 | 中 | 放最后，dogfood 验收 |
| 并发会话冲突 | 中 | worktree 隔离（已运行） |
| 桥边界性能 | 低 | 桥只交换数据，不做热路径对象图 |
| napi-rs/Node 兼容 | 低 | M0.5 实测 |

## 7. 待确认事项（不再是"范围"决策，而是明确清单）

1. **删除清单确认**：随迁移明确删除的功能需逐项列出并确认（已删：tree-sitter-bash、dimi-inspect、搜索+minidb、ACP、kaos、vis；后续如有新增删除项，同样明确列出）。删除清单之外的一切功能，Rust 版必须 parity；
2. **实现顺序**：Windows 等平台、MCP/插件等切片——顺序可后置，但切换（M6）前必须完成 parity，不存在"不做"；
3. 技术选型（tokio 版本、序列化库等）由实现决定，不构成范围决策。

## 8. 执行纪律

- 所有 Rust 工作只在 `rust/wire` worktree；TS 侧清理类改动走独立 worktree；
- 每个模块按"边界 → Rust 实现 → 桥暴露 → 差分验证 → 换入（开关）→ e2e/dogfood → 默认打开 → 删 TS 副本"的流程；
- **每模块换入前必须建立"功能清单对照表"**：列出 TS 版该模块的全部功能点，逐项核对 Rust 版行为一致；表随模块提交；
- 开关命名：`DIMI_RUST_<MODULE>=1`；
- 模块 DoD 未过不进入下一模块；parity 无遗漏是 DoD 的组成部分。
