# M3 dimi-engine 切片 1 — 最小闭环设计

> 状态：实现规格（v1）。本文件是切片 1 实现者的唯一规格输入；行为点全部来自
> TS 侧 loopService/llmRequester/toolExecutor 的代码事实（2026-08-02 调研）。
> 参考：PLAN.md §M3。

## 1. 目标

Rust 侧实现"一个回合的最小闭环"：会话上下文（wire 历史）→ 用户输入 →
LLM（OpenAI 兼容 SSE 流式）→ 工具调用（Bash via dimi-exec）→ 回合完成 →
wire op 落盘。产出 wire op 序列与 TS 版（loopService + llmRequester +
toolExecutor 在相同输入/mock 响应下）**字节级一致**。

## 2. 边界与 trait（效果边界）

```rust
// 引擎核心：纯编排，不直接碰 IO
pub struct Engine { /* 配置: max_steps_per_turn 等 */ }

// LLM 效果边界
pub trait LlmClient {
    /// 流式 chat 完成；事件顺序: text/thinking/tool_call delta → usage → finish
    fn stream_chat(&self, req: ChatRequest) -> impl Stream<Item = Result<LlmEvent>>;
}

// 工具效果边界
pub trait ToolExecutor {
    fn execute(&self, call: ToolCall, ctx: &ToolContext) -> impl Future<Output = ToolResult>;
}

// 落盘边界（wire op 序列；实现复用 dimi-store 的 apply/序列化语义）
pub trait TranscriptSink {
    fn apply_ops(&self, ops: &[Operation]) -> Result<()>;
}
```

- 切片 1 的 `LlmClient` 实现：OpenAI 兼容 `/chat/completions` SSE（reqwest +
  eventsource 解析），支持 thinking（reasoning_content / delta 约定，按
  TS llmRequester 的 stream 解析行为）；mock 实现用于差分。
- 切片 1 的 `ToolExecutor` 实现：Bash（经 dimi-exec 的进程/pty），参数
  schema 校验（command 必填、cwd?/timeout?/description?），超时 60s/上限
  300s，50k/2k 截断，错误文案与 TS 完全一致。
- 上下文组装：切片 1 从 wire 历史（dimi-store coldRebuild/snapshot 面）
  重建 messages（user/assistant/tool 历史 + 新 prompt）；projector/memory/
  compaction 属切片 3。

## 3. 回合编排（复刻 TS loopService.run 的行为）

TS 主循环（loopService.ts `run()`）行为点：

1. **循环结构**：`while true`：`beginLoopStep` → `executeLoopStep` →
   `completeLoopStep`；队列无 pending 请求 → 返回 completed。
2. **maxSteps**：`maxStepsPerTurn`（config `loop_control.max_steps_per_turn`，
   默认未设/0 = 不限）；`steps >= maxSteps` → 失败 `max_steps`（wire 上
   step.interrupted reason=max_steps，turn failed）。
3. **step 内部**：
   a. 组装 messages（历史 + 本轮请求）→ LLM 流式请求；
   b. 流式事件 → frame 写盘（text → frame.upsert + append；thinking →
      thinking frame；tool_call → tool frame 的 input 累积）；
   c. 响应结束 → 解析 finishReason（completed/tool_calls/…）与 usage；
   d. 有 tool_calls → 执行工具（executeStepTools）→ 结果回填（tool 消息）；
      `stopTurn=true`（后台化）→ finishReason="completed" 提前收尾；
   e. finishStep：写 step 完成（usage、finishReason、timing）。
4. **turn 落盘**：`turn.upsert`（started：state=running、prompt、origin、
   started_at；ended：state、usage、duration_ms、ended_at）、`step.upsert`
   （number、state、usage、finishReason…）、`frame.upsert`（text/thinking/
   tool 帧）、`append`（流式文本增量）。
5. **结束原因**：completed / cancelled（用户中断）/ failed（错误）/
   blocked（折进 failed）；turn 结束发 `turn.ended`。

## 4. wire 形状（dimi-wire 已有类型，逐字段对齐）

- `TurnHeader`：kind="turn"、turnId、ordinal、state、origin、prompt、
  attachmentIds、startedAt、endedAt、usage、durationMs、error?（model.rs）。
- `StepHeader`：kind="step"、stepId、number、state、usage?、finishReason?、
  providerFinishReason?、rawFinishReason?、timing?、error?、retry?。
- `Frame`：kind text/thinking/tool；tool 帧含 toolCallId、name、state
  running/done/error、input（累积）、output、display?、inputText?。
- op 序列（turn.upsert → step.upsert → frame.upsert/append…→ step.upsert
  完成 → … → turn.upsert 完成）。精确序列以 TS turnOps/context 实现为准
  （实现者必须对照 `contextService` 的 append/frame 写入点逐条核对）。

## 5. 输出面与 TS 换入（修订 v2：事件流，非 op 序列）

**架构事实（2026-08-02 确认）**：transcript op（turn.upsert/step.upsert/
frame.upsert/append）不是 loop 直接写的——agent-core-v2 的 loop 发
**事件**（IEventBus），kap-server 的 `coreEventMap`（transcript 投影层）把
事件折叠成 op。因此：

- **dimi-engine 的输出 = 事件流**（与 TS loop 的事件形状逐字段对齐）：
  `turn.started`、`turn.step.started`、`assistant.delta`、`thinking.delta`、
  `tool.call.delta`、`tool.call.started`、`tool.progress`、`tool.result`、
  `turn.step.completed`、`turn.step.interrupted`、`turn.ended`
  （形状见 `agent/loop/turnEvents.ts` + `toolExecutorEvents.ts`）。
- **投影层保留 TS**（coreEventMap/transcriptService/广播/telemetry 不动）。
- **上下文组装保留 TS**（切片 1）：TS 把已组装的 messages（含历史）传给
  引擎；引擎不读 transcript、不做投影/压缩（切片 3 再做引擎内组装）。
- **换入**：`RustEngine`（napi 类）`start_turn(input, eventCallback)`——
  eventCallback（ThreadsafeFunction）把引擎事件推给 TS；TS 适配器
  `rustEngineLoopService.ts`（DIMI_RUST_ENGINE=1）把事件 publish 到
  IEventBus，其余（投影/落盘/广播）原样走既有链路。
- `EngineTurnInput`：{ messages: <LLM messages JSON>, tools: [{name,
  argsSchema, description}], model, maxStepsPerTurn, provider: {baseUrl,
  apiKey, model, thinkingEffort?} }。
- 差分验证基准：**事件序列对拍**（同输入 + 同 mock 响应 → TS loop 事件 vs
  Rust 引擎事件，逐事件 JSON 对比）；投影后 op 一致性由 TS 侧既有测试兜底。

## 6. 差分验证（DoD 核心）

1. **事件对拍**：mock LlmClient（固定事件序列）+ 固定工具结果 → 同一输入
   序列 → Rust 引擎事件流 vs TS loop 事件流（mock provider + fake bash），
   逐事件 JSON 对比。
2. **换入测试**：DIMI_RUST_ENGINE=1 时 agent-core-v2 最小闭环用例全绿
   （现有 loop 测试选最小子集跑双模式）。
3. **e2e**：真实 CLI `dimi -p "..."`（mock/真 LLM）走 Rust 引擎回合完成，
   transcript 正常。

## 7. 切片 1 不做（后续切片）

- 权限/审批（切片 2）：切片 1 引擎内工具**全部自动执行**（manual 白名单
  之外的行为在切片 2 对齐）。
- 上下文管理/compaction（切片 3）。
- 子代理/swarm（切片 4）、MCP/插件/skills（切片 5）、cron/plan/undo/媒体
  （切片 6）。
- 事件总线/telemetry 细节：切片 1 只保证 wire 一致 + 最小事件发布。

## 8. 功能清单对照表（切片 1 范围）

| # | 功能点（TS 行为） | Rust 状态 |
|---|---|---|
| 1 | turn 启动：turn.upsert(running, prompt, origin) | ☑ |
| 2 | step 启动：step.upsert(running, number) | ☑ |
| 3 | 流式 text → frame.upsert + append（delta 累积） | ☑ |
| 4 | 流式 thinking → thinking frame | ☑ |
| 5 | tool_call delta → tool frame（input 累积） | ☑ |
| 6 | finishReason 归一化（completed/tool_calls/…） | ☑ |
| 7 | usage 统计（prompt/completion/total + 细项） | ☑ |
| 8 | Bash 工具：参数校验/超时/截断/错误文案 | ☑ |
| 9 | 工具结果回填（tool 消息 + step 续接） | ☑ |
| 10 | stopTurn（后台化）→ completed 提前收尾 | ☑ |
| 11 | maxSteps 超限 → failed(max_steps) | ☑ |
| 12 | turn 结束：turn.upsert(ended, usage, duration) | ☑ |
| 13 | 用户中断 → cancelled | ☑ |

> 实现者：每完成一项在表中勾选；DoD = 13 项全勾 + 差分全绿 + 换入测试全绿。
