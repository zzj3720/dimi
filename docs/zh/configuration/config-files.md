# 配置文件

Dimi CLI 把长期运行时偏好写进 TOML（一种结构清晰的纯文本配置格式）文件，例如使用哪个已连接的供应商与模型、请求多少推理，以及 Agent 每轮最多运行几步。供应商凭据由 provider runtime 单独保存。Agent 与运行时设置放在 `config.toml`，终端界面与客户端偏好（主题、编辑器、通知、自动更新）放在配套的 `tui.toml`；用户拥有的供应商定义和覆盖层使用 `models.json`。

默认位置：`~/.dimi/config.toml`，首次运行时自动创建。

## 配置文件位置

CLI 从 `~/.dimi/config.toml` 读取配置。如需把数据目录迁移到别处，可用 `DIMI_CODE_HOME` 环境变量覆盖：

```sh
export DIMI_CODE_HOME=/path/to/dimi-home
```

此时配置文件路径变为 `$DIMI_CODE_HOME/config.toml`。无论目录在哪里，文件名固定是 `config.toml`。

::: tip
TOML 字段名一律用下划线（snake_case），例如 `default_model` 和 `max_completion_tokens`。
:::

## 完整示例

以下示例覆盖最常用的配置项，可直接复制后按需修改：

```toml
default_provider = "kimi-coding"
default_model = "kimi-for-coding"
default_permission_mode = "manual"
default_plan_mode = false
merge_all_available_skills = true
telemetry = true

[thinking]
enabled = true
effort = "high"
keep = "all"

[model_catalog]
refresh_on_start = true
refresh_interval_ms = 21600000

[model_overrides]
temperature = 0.3
max_completion_tokens = 8192

[loop_control]
max_retries_per_step = 10
reserved_context_size = 50000

[task]
max_running_tasks = 4

[services.moonshot_search]
base_url = "https://api.kimi.com/coding/v1/search"
api_key = ""

[services.moonshot_fetch]
base_url = "https://api.kimi.com/coding/v1/fetch"
api_key = ""

[[permission.rules]]
decision = "allow"
pattern = "Read"

[[permission.rules]]
decision = "deny"
pattern = "Bash(rm -rf*)"

[[hooks]]
event = "PreToolUse"
matcher = "Bash"
command = "node ~/.dimi/hooks/check-bash.mjs"
timeout = 5
```

## 顶层字段

配置文件里的字段分两类：**顶层标量**直接控制默认行为，**嵌套表**（`thinking`、`model_catalog`、`loop_control` 等）各有独立结构，在下文各节单独说明。

| 字段                         | 类型            | 默认值   | 说明                                                                                                                                          |
| ---------------------------- | --------------- | -------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `default_provider`           | `string`        | —        | 默认模型的供应商 ID                                                                                                                           |
| `default_model`              | `string`        | —        | `default_provider` 中的模型 ID                                                                                                                |
| `default_permission_mode`    | `string`        | `manual` | 新会话的默认权限模式，可选 `manual`（逐次询问）、`yolo`（自动批准工具操作，Agent 仍可能提问）、`auto`（完全自主，Agent 自己做决定，不再提问）。可在 TUI 中用 `/permission <mode>` 设置 |
| `default_plan_mode`          | `boolean`       | `false`  | 新会话是否默认以 Plan 模式（先出计划再执行）启动                                                                                              |
| `merge_all_available_skills` | `boolean`       | `true`   | 是否合并所有目录中的 Agent Skills                                                                                                             |
| `extra_skill_dirs`           | `array<string>` | —        | 额外 Skill 搜索目录，叠加到默认目录之上                                                                                                       |
| `extra_agent_dirs`           | `array<string>` | —        | 额外自定义 Agent 搜索目录，叠加到默认目录之上                                                                                                 |
| `telemetry`                  | `boolean`       | `true`   | 是否启用匿名遥测；显式设为 `false` 时关闭                                                                                                     |
| `model_catalog`              | `table`         | —        | 动态模型目录的后台刷新 → [`model_catalog`](#model-catalog)                                                                                    |
| `model_overrides`            | `table`         | —        | 应用于所选模型的请求默认值 → [`model_overrides`](#model-overrides)                                                                            |
| `secondary_model`            | `table`         | —        | 新子 Agent 使用的供应商与模型 → [`secondary_model`](#secondary-model)                                                                         |
| `thinking`                   | `table`         | —        | Thinking 模式默认参数 → [`thinking`](#thinking)                                                                                               |
| `loop_control`               | `table`         | —        | Agent 循环控制参数 → [`loop_control`](#loop-control)                                                                                          |
| `task`                       | `table`         | —        | 任务运行参数 → [`task`](#task)                                                                                                                |
| `tools`                      | `table`         | —        | 全局工具开关 → [`tools`](#tools)                                                                                                              |
| `image`                      | `table`         | —        | 图片压缩参数 → [`image`](#image)                                                                                                              |
| `services`                   | `table`         | —        | 内置外部服务配置 → [`services`](#services)                                                                                                    |
| `permission`                 | `table`         | —        | 初始权限规则 → [`permission`](#permission)                                                                                                    |
| `hooks`                      | `array<table>`  | —        | 生命周期 hook，详见 [Hooks](../customization/hooks.md)                                                                                        |

供应商凭据不是配置小节。使用 `vp run dev:cli -- login` 连接供应商，使用 `vp run dev:cli -- provider models` 查看模型。用 `$DIMI_CODE_HOME/models.json` 添加自定义供应商或覆盖内置模型；它是 JSONC 层，不是 `config.toml` 表。详见[供应商与模型](./providers.md#使用-modelsjson-添加或覆盖供应商)。

## `model_catalog`

`model_catalog` 控制长期运行 server 对拥有远程模型端点的已认证供应商进行后台刷新。provider runtime 还会保留最近一次成功的缓存，并遵守上游返回的新鲜度信息。它不会改变本地 `models.json` 声明的限制或能力。

| 字段                  | 类型      | 默认值     | 说明                                              |
| --------------------- | --------- | ---------- | ------------------------------------------------- |
| `refresh_interval_ms` | `integer` | `21600000` | server 两次刷新尝试之间的毫秒数；`0` 关闭周期刷新 |
| `refresh_on_start`    | `boolean` | `true`     | server 启动后是否进行一次尽力刷新                 |

## `model_overrides`

`model_overrides` 设置请求默认值，不替换供应商拥有的模型元数据。

| 字段                    | 类型      | 默认值 | 说明                            |
| ----------------------- | --------- | ------ | ------------------------------- |
| `temperature`           | `number`  | —      | 采样温度                        |
| `top_p`                 | `number`  | —      | 核采样概率                      |
| `thinking_keep`         | `string`  | —      | 供应商特定的历史推理保留指令    |
| `max_completion_tokens` | `integer` | —      | 向供应商请求的最大输出 token 数 |

## `secondary_model`

次主力模型是主模型 `default_model` 之外的第二个模型指针——通常是一个更便宜的模型，供不需要主模型的功能绑定使用。目前的消费者是子 Agent 派生：设置后，新派生的子 Agent（`Agent` / `AgentSwarm`）默认绑定该模型，而不再继承主 Agent 的模型；主 Agent 会被告知每次派生可在 `"secondary"`（该模型）与 `"primary"`（主模型）之间选择。未设置时，子 Agent 继承主 Agent 的模型。

该功能目前是实验功能，默认关闭。通过 `DIMI_CODE_EXPERIMENTAL_SECONDARY_MODEL=1` 启用，或使用 master `DIMI_CODE_EXPERIMENTAL_FLAG=1`。它在包括交互式 TUI 在内的所有启动方式下生效。

在交互式 TUI 中，可以使用 [`/secondary_model`](../reference/slash-commands.md) 命令打开模型选择器来设置该配置：选择后会写入本小节配置，并在当前会话立即生效——之后派生的子 Agent 会直接绑定新的第二模型。

| 字段             | 类型     | 默认值 | 说明                                                                                                                                                                                                                                                                                         |
| ---------------- | -------- | ------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `provider`       | `string` | —      | 次主力模型的供应商 ID                                                                                                                                                                                                                                                                        |
| `model`          | `string` | —      | `provider` 中的模型 ID                                                                                                                                                                                                                                                                       |
| `default_effort` | `string` | —      | 子代理绑定次主力模型时使用的 thinking effort。未设置时按"全局 `[thinking]` 配置 → 模型默认 effort"的链路解析，不再继承主 Agent 的 effort。与主模型的 thinking effort 语义一致：严格校验 effort 的模型（如 dimi 模型）在不支持该取值时回退到模型默认 effort，其他供应商的模型按原样发送给后端 |

```toml
[secondary_model]
provider = "anthropic"
model = "claude-sonnet-4-6"
default_effort = "low"
```

`provider`、`model` 和 `default_effort` 可被环境变量 `DIMI_SECONDARY_PROVIDER`、`DIMI_SECONDARY_MODEL` 和 `DIMI_SECONDARY_EFFORT` 覆盖，优先级均高于配置文件。

实验功能启用后，会话启动时会校验该配置：供应商或模型无法解析，或 `default_effort` 不受支持时，会在启动时显示警告（并通过会话警告 API 返回）。该检查仅为提示——配置有误的次主力模型仍会在派生子 Agent 时失败，派生错误中同样附带配置来源提示。

## `thinking`

`thinking` 设置 Thinking 模式的全局默认行为。

| 字段      | 类型      | 默认值  | 说明                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| --------- | --------- | ------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `enabled` | `boolean` | `true`  | 新会话是否默认开启 Thinking，设为 `false` 会在所选模型支持时请求关闭。没有 `reasoning` 能力的模型会忽略该偏好；始终 Thinking 且 `thinkingLevelMap.off` 为 `null` 的模型会保留其声明的默认档位。 |
| `effort`  | `string`  | —       | 首选 Thinking 档位（如 `low`、`medium`、`high`、`xhigh`、`max`）。所选模型的 `thinkingLevelMap` 是权威来源：已映射档位会转换为供应商 wire 值；映射为 `null` 代表不可用；推理模型没有映射时只有开/关。配置档位不可用时，运行时优先使用模型声明的默认档位，否则采用其正常支持模式。 |
| `keep`    | `string`  | `"all"` | 保留思考透传。在 `dimi` 上以 `thinking.keep` 发送；在 `anthropic`（Claude 以及 Dimi 的 Anthropic 兼容模式）上以 `context_management` 的 `clear_thinking_20251015` 编辑发送（开启 keep 会让 Anthropic 请求走 beta Messages API；关值可禁用 keep 并回到标准端点）。`"all"` 会保留历史轮次的思考内容（`reasoning_content` / Anthropic thinking blocks）；传入关值（`false`/`0`/`no`/`off`/`none`/`null`）可禁用。可被 `DIMI_MODEL_THINKING_KEEP` 覆盖；仅在 Thinking 开启时注入 |

### 已废弃字段

| 字段               | 废弃版本 | 描述                                                                                                                                                                                |
| ------------------ | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `default_thinking` | 0.21.0   | 顶层布尔值，由 `[thinking] enabled` 取代。将 `default_thinking = true` 迁移为 `enabled = true`，`default_thinking = false` 迁移为 `enabled = false`。                               |
| `thinking.mode`    | 0.21.0   | 可选值 `auto` / `on` / `off`，由 `[thinking] enabled` 取代。`mode = "off"` 改为 `enabled = false`；`mode = "on"` 和 `mode = "auto"` 等价于 `enabled = true`（默认值），可删除该行。 |

## `model_efforts`

`model_efforts` 按模型记忆你上次选择的 Thinking 档位（`"<provider>/<model>" -> effort`）。切换到有记录的模型时，Dimi 会恢复该档位而不是使用全局 `[thinking]` 默认；在选档器中按 `Enter`（或 `/effort <level>`）会写入记录，`Alt+S` 仅应用到当前会话。没有记录的模型回退到全局 `[thinking]`。

```toml
[model_efforts]
"anthropic/claude-sonnet-4-5" = "high"
"kimi-coding/kimi-k2.5" = "off"
```

## `loop_control`

`loop_control` 控制 Agent 执行循环的步数上限、单步重试次数，以及触发上下文自动压缩的阈值。

| 字段                     | 类型      | 默认值 | 说明                                                                 |
| ------------------------ | --------- | ------ | -------------------------------------------------------------------- |
| `max_steps_per_turn`     | `integer` | —      | 单轮最大步数；不设或设为 `0` 则无上限                                |
| `max_retries_per_step`   | `integer` | `10`   | 单步失败后的最大重试次数                                             |
| `reserved_context_size`  | `integer` | —      | 预留给模型输出的 token 数；上下文窗口剩余量低于此值时触发自动压缩    |
| `context_size_percent`   | `integer` | `100`  | 有效上下文窗口占模型默认窗口的百分比；按 5% 档位取值（5–100），最低保留 200k token。模型窗口本身低于 200k 时不可调整，保持原窗口。可在 TUI 设置（`/config` → Context size）中修改 |

`max_steps_per_turn` 可被环境变量 `DIMI_LOOP_MAX_STEPS_PER_TURN` 覆盖，`max_retries_per_step` 可被 `DIMI_LOOP_MAX_RETRIES_PER_STEP` 覆盖，优先级均高于配置文件。

重试仅针对瞬时故障——连接错误、超时、HTTP 429 限流和 5xx 服务端错误。账户额度耗尽或余额不足导致的 429 不会重试，会立即失败：在充值之前重试不可能成功。

## `task`

`task` 控制任务生命周期与并发行为，包括后台 `Bash` 命令，以及使用 `run_in_background=true` 启动的 `Agent` 调用。

| 字段                              | 类型                           | 默认值      | 说明                                                                                                                                                                                                                                                                                                                |
| --------------------------------- | ------------------------------ | ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `max_running_tasks`               | `integer`                      | —           | 同时运行的最大后台任务数                                                                                                                                                                                                                                                                                            |
| `kill_grace_period_ms`            | `integer`                      | `5000`      | 会话关闭、手动停止或任务超时请求正常终止后，等待任务自行结束的宽限时间（毫秒）。超过该时间仍在运行时，Dimi 会尝试强制停止该任务                                                                                                                                                                                |
| `bash_auto_background_on_timeout` | `boolean`                      | `true`      | 前台 `Bash` 命令触及超时时间时，将其转为后台任务而不是直接终止：命令完成时 agent 会收到通知，转入后台的命令受 `bash_task_timeout_s` 默认后台超时约束。设为 `false` 则恢复超时即终止的行为                                                                                                                           |
| `bash_task_timeout_s`             | `integer`                      | `600`       | 后台 `Bash` 任务在调用未传 `timeout` 时的默认超时（秒）；前台命令超时转后台后也按此值重新计时。`0` 表示无超时——任务一直运行到自行结束或被模型手动停止。显式传入的 `timeout` 不受影响。在 print 模式（`dimi -p`）下未显式设置时默认为 `0`                                                                            |
| `print_background_mode`           | `"exit" \| "drain" \| "steer"` | `"steer"`   | 仅 print 模式（`dimi -p`）生效，决定主 agent 的 turn 结束后如何处理未返回的后台任务：`"exit"` 立即退出；`"drain"` 退出前等待所有后台任务进入终态（结果不回馈给主 agent）；`"steer"` 不退出，让后台任务完成时像后台子代理一样以合成 user 消息 steer 主 agent 进入新 turn，直到某 turn 结束时无未决后台任务或触及上限 |
| `print_wait_ceiling_s`            | `integer`                      | `315360000` | print 模式（`dimi -p`）下，`print_background_mode` 为 `"drain"` 或 `"steer"` 时，等待/steer 循环的墙钟上限（秒；默认 10 年，近似不设限）。在非 print 模式或 `"exit"` 时无效                                                                                                                                         |
| `print_max_turns`                 | `integer`                      | `100000`    | print 模式（`dimi -p`）且 `print_background_mode = "steer"` 时，允许由后台任务完成触发的新 turn 的最大数量，防止 steer 循环失控（默认值近似不设限）                                                                                                                                                                 |

`max_running_tasks` 可被环境变量 `DIMI_CODE_BACKGROUND_MAX_RUNNING_TASKS` 覆盖，优先级高于配置文件。

在 print 模式（`dimi -p "<prompt>"`）下，只要还有未决的后台任务，Dimi 在主 agent 的 turn 结束后不会退出：每个任务完成都会以合成 user 消息回馈给主 agent，steer 出新的 turn（默认 `print_background_mode = "steer"`），直到某 turn 结束时没有任何未决任务才退出。该循环受 `print_wait_ceiling_s` 与 `print_max_turns` 约束，默认值都近似不设限。print 模式下后台工作也不会被墙钟超时杀掉：后台 `Bash` 任务默认无超时（`bash_task_timeout_s = 0`），子代理默认无超时（`[subagent] timeout_ms = 0`），只有模型自己能停止任务。将 `print_background_mode` 设为 `"drain"` 可等待任务结束但不回馈结果，设为 `"exit"` 则在主 agent 结束后立即退出。

## `subagent`

| 字段         | 类型      | 默认值              | 说明                                                                                                                                                                                                                                                                                                                                                                      |
| ------------ | --------- | ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `timeout_ms` | `integer` | `7200000`（2 小时） | 单个子代理（`Agent` / `AgentSwarm`）允许运行的最长时间（毫秒）。超时后子代理以 `timed_out` 收尾。`0` 表示无超时——子代理一直运行到自行结束或被模型手动停止。该值是后台任务管理器对每个子代理任务的 per-task timeout，因此对前台与后台子代理同时生效。在 print 模式（`dimi -p`）下未显式设置时默认为 `0`。注意：超过 `2147483647`（约 24.8 天）的值会被运行时钳到约 24.8 天 |

`timeout_ms` 可被环境变量 `DIMI_SUBAGENT_TIMEOUT_MS` 覆盖，优先级高于配置文件。

## `mcp`

| 字段                 | 类型      | 默认值           | 说明                                                                                                                                                                                      |
| -------------------- | --------- | ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `startup_timeout_ms` | `integer` | `30000`（30 秒） | 所有 MCP server 的全局默认连接（启动 + 工具发现）超时（毫秒），取值范围为 `1`–`2147483647`。`mcp.json` 中单个 server 的 `startupTimeoutMs` 始终优先于本节与环境变量；都未设置时使用默认值 |
| `tool_timeout_ms`    | `integer` | `60000`（60 秒） | 所有 MCP server 的全局默认单次工具调用超时（毫秒），取值范围为 `1`–`2147483647`。`mcp.json` 中单个 server 的 `toolTimeoutMs` 始终优先于本节与环境变量；都未设置时使用客户端内置默认值     |

`startup_timeout_ms` 和 `tool_timeout_ms` 可分别被环境变量 `DIMI_MCP_STARTUP_TIMEOUT_MS` 和 `DIMI_MCP_TOOL_TIMEOUT_MS` 覆盖，优先级高于配置文件。MCP server 的完整配置方式见 [MCP](../customization/mcp.md)。

## `tools`

`tools` 设置全局工具开关，对所有会话中的每个 Agent 生效，并在 Agent 自身的 `tools` / `disallowedTools` 策略之上再取一次交集。

| 字段       | 类型            | 默认值 | 说明                                                               |
| ---------- | --------------- | ------ | ------------------------------------------------------------------ |
| `enabled`  | `array<string>` | —      | 全局允许列表：非空时仅列出的工具可用；省略或设为空数组均表示不约束 |
| `disabled` | `array<string>` | —      | 全局禁止列表，在 `enabled` 之后应用                                |

工具名匹配规则与 Agent 文件中的同名字段一致：内置工具按名称精确匹配（如 `Read`），MCP 工具用 glob 匹配（如 `mcp__github__*`）。有三种写法永远匹配不到任何工具，出现时会给出警告：`mcp__` 模式之外使用通配符（`enabled = ["*"]` 会禁用所有工具，而 `disabled = ["*"]` 什么也禁不掉）；缺少工具段的 `mcp__` 字面量（`mcp__github` —— 匹配整个服务器要用 `mcp__github__*`）；以及任何已注册或内置工具都没有的名字（匹配区分大小写）。

```toml
[tools]
disabled = ["EnterPlanMode", "ExitPlanMode", "mcp__github__*"]
```

::: warning 注意
与 Agent 文件中的 `tools` / `disallowedTools` 一样，本节不仅决定模型能"看到"哪些工具，还会在执行前再次强制检查。[权限规则](#permission)仍是独立的控制层，用于决定哪些操作需要审批。
:::

## `image`

`image` 控制图片发送给模型前的压缩行为，对所有图片入口生效（粘贴图片、`ReadMediaFile` 读图、MCP 工具结果里的图片等）。

| 字段               | 类型      | 默认值             | 说明                                                                                                                                                                                                           |
| ------------------ | --------- | ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `max_edge_px`      | `integer` | `2000`             | 图片最长边上限（像素）。超过时按比例缩小到该值以内；调大可保留更多细节，代价是更大的请求体积                                                                                                                   |
| `read_byte_budget` | `integer` | `262144`（256 KB） | 模型自行读取的图片（`ReadMediaFile` 默认读取）的单图字节预算。会话中模型反复截图、读图时，累计请求体大小由它控制；细节可通过 `region` 参数按原图坐标全保真回读（`region` 与 `full_resolution` 不受此预算限制） |

`max_edge_px` 可被环境变量 `DIMI_IMAGE_MAX_EDGE_PX` 覆盖，`read_byte_budget` 可被 `DIMI_IMAGE_READ_BYTE_BUDGET` 覆盖，优先级均高于配置文件。

<!--
## `experimental`

`experimental` 存放实验功能 flag 的持久化覆盖。目前 `micro_compaction` 是唯一用户可见的字段，默认值为 `false`；如需自动清理较旧的大型工具结果，把它设为 `true`。

| 字段 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `micro_compaction` | `boolean` | `false` | 清理较旧的大型工具结果内容，同时保留最近对话 |
-->

## `services`

`services` 配置网页搜索（`moonshot_search`）和网页抓取（`moonshot_fetch`）两项内置服务。只识别这两个固定 key，其他 key 会被忽略。两项字段相同：

| 字段             | 类型                    | 必填 | 说明                       |
| ---------------- | ----------------------- | ---- | -------------------------- |
| `base_url`       | `string`                | 否   | 服务 API URL               |
| `api_key`        | `string`                | 否   | API 密钥                   |
| `custom_headers` | `table<string, string>` | 否   | 请求时附加的自定义 HTTP 头 |

`base_url` 和 `api_key` 也可由环境变量提供，环境变量优先于配置文件：`DIMI_WEB_SEARCH_BASE_URL` / `DIMI_WEB_SEARCH_API_KEY` 对应 `moonshot_search`，`DIMI_WEB_FETCH_BASE_URL` / `DIMI_WEB_FETCH_API_KEY` 对应 `moonshot_fetch`。`DIMI_WEB_SEARCH_BASE_URL` 和 `DIMI_WEB_FETCH_BASE_URL` 定义的是独立服务端点，因此文件中持久化的 API 密钥和自定义 header 都不会发送给它；该端点需要鉴权时，请同时设置对应的环境变量 API 密钥。只设置环境变量 API 密钥时，配置中的端点和自定义 header 保持不变，但配置密钥会被替换。不写配置段、只通过环境变量设置 base URL 和 API 密钥，也可以启用对应服务。

```toml
[services.moonshot_search]
base_url = "https://api.moonshot.cn/v1/search"
api_key = "YOUR_API_KEY"

[services.moonshot_fetch]
base_url = "https://api.moonshot.cn/v1/fetch"
api_key = "YOUR_API_KEY"
```

## `permission`

`permission` 设置会话启动时自动加载的权限规则，控制 Agent 调用工具时是否需要用户确认。规则用 `[[permission.rules]]` 数组表写出，按顺序匹配，第一条命中即生效。

| 字段       | 类型     | 必填 | 说明                                                                             |
| ---------- | -------- | ---- | -------------------------------------------------------------------------------- |
| `decision` | `string` | 是   | 匹配后的处置：`allow`（直接放行）、`deny`（直接拒绝）、`ask`（每次询问）         |
| `scope`    | `string` | 否   | 规则有效范围：`turn-override`、`session-runtime`、`project`、`user`；默认 `user` |
| `pattern`  | `string` | 是   | 匹配模式，格式为 `工具名` 或 `工具名(参数模式)`，如 `Read`、`Bash(rm -rf*)`      |
| `reason`   | `string` | 否   | 规则说明，仅用于调试和审计                                                       |

内置工具名见[内置工具](../reference/tools.md)。大多数支持规则参数的内置工具会定义自己的匹配对象，例如 `Bash(command-pattern)` 或 `Read(path-pattern)`。`AgentSwarm`、MCP 工具和自定义工具只能按工具名匹配，不支持参数模式。

```toml
[[permission.rules]]
decision = "allow"
pattern = "Read"

[[permission.rules]]
decision = "allow"
pattern = "Grep"

[[permission.rules]]
decision = "deny"
pattern = "Bash(rm -rf*)"

[[permission.rules]]
decision = "ask"
pattern = "Bash"
```

::: tip
MCP server 的声明配置写在 `~/.dimi/mcp.json` 或项目内 `.dimi/mcp.json` 中，不在 `config.toml` 里。交互式配置入口是 `/mcp-config`，详见 [Model Context Protocol](../customization/mcp.md)。
:::

## `tui.toml`

除了 `config.toml`，CLI 还在同一目录下用一份配套的 `tui.toml` 保存终端界面与客户端偏好（`~/.dimi/tui.toml`，或覆盖后的 `$DIMI_CODE_HOME/tui.toml`）。它在首次运行时以默认值创建，交互式命令 `/config`、`/theme`、`/editor` 会自动写入，通常无需手动编辑。文件格式有误时，CLI 会回退到默认值并给出提示，而不是启动失败。

| 字段                                     | 类型                 | 默认值      | 说明                                                                                                                                                                                                           |
| ---------------------------------------- | -------------------- | ----------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `theme`                                  | `string`             | `auto`      | 配色主题：`auto`（跟随终端）、`dark`、`light`，或[自定义主题](../customization/themes.md)的名字                                                                                                                |
| `disable_paste_burst`                    | `boolean`            | `false`     | 禁用非 bracketed paste 的粘贴突发兜底；默认开启，避免快速多行粘贴被逐行提交                                                                                                                                    |
| `busy_input_mode`                        | `"queue" \| "steer"` | `steer`     | Agent 忙碌时 Enter 的行为：`steer` 时 Enter 直接注入当前 turn；`queue` 排队等当前任务结束后再发（Ctrl-S 仍可立即 steer）。可在 `/settings` → Busy input 切换                                                   |
| `[editor].command`                       | `string`             | `""`        | 编写长输入用的外部编辑器命令；留空则回退到 `$VISUAL` / `$EDITOR`                                                                                                                                               |
| `[notifications].enabled`                | `boolean`            | `true`      | 是否发送桌面通知                                                                                                                                                                                               |
| `[notifications].notification_condition` | `string`             | `unfocused` | 何时通知：`unfocused`（仅终端失去焦点时）或 `always`（总是）                                                                                                                                                   |
| `[upgrade].auto_install`                 | `boolean`            | `true`      | 保存的更新器偏好。此源码构建没有配置更新通道，因此该项不生效。                                                                                                                                                  |
| `[status_line].items`                    | `string[]`           | `[]`        | 底部状态栏第一行展示哪些内置槽位及其顺序：`mode`、`model`、`tasks`、`cwd`、`git`、`tips`。缺省保持默认布局；未知 id 跳过并告警                                                                         |
| `[status_line].command`                  | `string`             | `""`        | 自定义状态栏命令。其 stdout 第一行替换状态栏第一行，stdin 会收到 JSON 快照（model、cwd、git 分支、permission 模式、plan 模式、上下文用量、session id、版本）。运行上限 300ms、每秒最多一次；失败时回退内置布局 |

```toml
# ~/.dimi/tui.toml
theme = "auto" # "auto" | "dark" | "light" | 自定义主题名
disable_paste_burst = false # true 表示禁用非 bracketed paste 的粘贴突发兜底
busy_input_mode = "steer" # "steer"（Enter 直接注入当前 turn）| "queue"（Enter 排队，Ctrl-S 立即 steer）

[editor]
command = "" # 留空则使用 $VISUAL / $EDITOR

[notifications]
enabled = true
notification_condition = "unfocused" # "unfocused" | "always"

[upgrade]
# 此源码构建只保存该偏好；请用 git pull 更新 checkout。
auto_install = true

# [status_line]
# items = ["mode", "model", "tasks", "cwd", "git", "tips"]
# command = "~/.dimi/statusline.sh"
```

修改在下次启动时生效，或用 `/reload-tui` 立即生效（只重载 `tui.toml`）；`/reload` 会同时重载 `config.toml` 和 `tui.toml`。

## 项目级本地配置

除了 `~/.dimi` 下的用户级文件，Dimi 还会读取位于 `<项目根目录>/.dimi/local.toml` 的项目级本地配置文件。它保存的是与某一个项目检出相关、通常不应与队友共享的设置。

该文件会在你通过 [`/add-dir`](../reference/slash-commands.md) 添加额外工作目录并选择记入项目时自动创建，通常无需手动编辑。

### `[workspace]`

`[workspace]` 表用于存放项目级的工作区设置：

| 字段             | 类型            | 必填 | 说明                                                                                                                           |
| ---------------- | --------------- | ---- | ------------------------------------------------------------------------------------------------------------------------------ |
| `additional_dir` | `array<string>` | 否   | 额外工作目录列表，以绝对路径存储。在 `/add-dir` 中确认"记住此目录"时自动写入；启动时读回，使这些目录在该项目的每个会话中都可用 |

```toml
[workspace]
additional_dir = ["/absolute/path/to/shared"]
```

目录以绝对路径存储，与具体机器相关。因此建议把 `.dimi/local.toml` 加入项目的 `.gitignore`，避免被提交。

## 下一步

- [平台与模型](./providers.md) — 内置供应商、认证方式与动态模型目录
- [配置覆盖](./overrides.md) — CLI 选项、配置文件、环境变量的优先级规则
- [环境变量](./env-vars.md) — `DIMI_CODE_HOME` 等运行时变量的完整列表
