# Configuration files

Kimi Code CLI writes long-term runtime preferences — which connected provider/model to use, how much reasoning to request, and how many steps an agent can run per turn — into TOML (a plain-text configuration format with a clear structure) files. Provider credentials live in the provider runtime instead. Agent and runtime settings use `config.toml`; terminal-UI and client preferences (theme, editor, notifications, auto-update) use the companion `tui.toml`.

Default location: `~/.kimi-code/config.toml`, created automatically on first run.

## Config file location

The CLI reads configuration from `~/.kimi-code/config.toml`. To relocate the data directory, override it with the `KIMI_CODE_HOME` environment variable:

```sh
export KIMI_CODE_HOME=/path/to/kimi-home
```

The config file path then becomes `$KIMI_CODE_HOME/config.toml`. Regardless of where the directory lives, the file name is always `config.toml`.

::: tip
TOML field names always use snake_case, for example `default_model` and `max_completion_tokens`.
:::

## Complete example

The following example covers the most commonly used configuration fields. You can copy it and adjust as needed:

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
command = "node ~/.kimi-code/hooks/check-bash.mjs"
timeout = 5
```

## Top-level fields

Fields in the config file fall into two categories: **top-level scalars** that directly control default behavior, and **nested tables** (`thinking`, `model_catalog`, `loop_control`, etc.) that each have their own structure, described individually below.

| Field                        | Type            | Default  | Description                                                                                                                                                                                                                         |
| ---------------------------- | --------------- | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `default_provider`           | `string`        | —        | Provider ID for the default model                                                                                                                                                                                                   |
| `default_model`              | `string`        | —        | Model ID within `default_provider`                                                                                                                                                                                                  |
| `default_permission_mode`    | `string`        | `manual` | Default permission mode for new sessions; one of `manual` (prompt each time), `yolo` (auto-approve tool actions, but the agent may still ask questions), or `auto` (fully autonomous — the agent decides everything without asking) |
| `default_plan_mode`          | `boolean`       | `false`  | Whether new sessions start in Plan mode (produce a plan before executing) by default                                                                                                                                                |
| `merge_all_available_skills` | `boolean`       | `true`   | Whether to merge Agent Skills from all available directories                                                                                                                                                                        |
| `extra_skill_dirs`           | `array<string>` | —        | Extra skill search directories, layered on top of the default directories                                                                                                                                                           |
| `extra_agent_dirs`           | `array<string>` | —        | Extra custom agent search directories, layered on top of the default directories                                                                                                                                                    |
| `telemetry`                  | `boolean`       | `true`   | Whether anonymous telemetry is enabled; disabled only when explicitly set to `false`                                                                                                                                                |
| `model_catalog`              | `table`         | —        | Background dynamic-catalog refresh → [`model_catalog`](#model-catalog)                                                                                                                                                              |
| `model_overrides`            | `table`         | —        | Request defaults applied to every selected model → [`model_overrides`](#model-overrides)                                                                                                                                            |
| `secondary_model`            | `table`         | —        | Provider/model used by new subagents → [`secondary_model`](#secondary-model)                                                                                                                                                        |
| `thinking`                   | `table`         | —        | Default parameters for Thinking mode → [`thinking`](#thinking)                                                                                                                                                                      |
| `loop_control`               | `table`         | —        | Agent loop control parameters → [`loop_control`](#loop-control)                                                                                                                                                                     |
| `task`                       | `table`         | —        | Task runtime parameters → [`task`](#task)                                                                                                                                                                                           |
| `tools`                      | `table`         | —        | Global tool switch → [`tools`](#tools)                                                                                                                                                                                              |
| `image`                      | `table`         | —        | Image compression parameters → [`image`](#image)                                                                                                                                                                                    |
| `services`                   | `table`         | —        | Built-in external service configuration → [`services`](#services)                                                                                                                                                                   |
| `permission`                 | `table`         | —        | Initial permission rules → [`permission`](#permission)                                                                                                                                                                              |
| `hooks`                      | `array<table>`  | —        | Lifecycle hooks; see [Hooks](../customization/hooks.md)                                                                                                                                                                             |

Provider credentials and model metadata are not config sections. Connect providers with `kimi login` and inspect their models with `kimi provider models`; see [Providers and models](./providers.md).

## `model_catalog`

`model_catalog` controls background refresh in the long-running server. The provider runtime also keeps a last-known-good cache and respects each upstream catalog's refresh interval.

| Field                 | Type      | Default    | Description                                                             |
| --------------------- | --------- | ---------- | ----------------------------------------------------------------------- |
| `refresh_interval_ms` | `integer` | `21600000` | Milliseconds between server refresh attempts; `0` disables the interval |
| `refresh_on_start`    | `boolean` | `true`     | Whether the server starts a best-effort refresh after boot              |

## `model_overrides`

`model_overrides` sets request defaults without replacing provider-owned model metadata.

| Field                   | Type      | Default | Description                                                   |
| ----------------------- | --------- | ------- | ------------------------------------------------------------- |
| `temperature`           | `number`  | —       | Sampling temperature                                          |
| `top_p`                 | `number`  | —       | Nucleus-sampling probability                                  |
| `thinking_keep`         | `string`  | —       | Provider-specific instruction for retaining earlier reasoning |
| `max_completion_tokens` | `integer` | —       | Maximum output tokens requested from the provider             |

## `secondary_model`

The secondary model is a second model pointer next to the primary `default_model` — typically a cheaper model that features can bind to when they do not need the main model. Its consumer today is subagent spawning: when set, newly spawned subagents (`Agent` / `AgentSwarm`) bind to it by default instead of inheriting the main agent's model, and the main agent is told it can pick per spawn between `"secondary"` (this model) and `"primary"` (the main model). When unset, subagents inherit the main agent's model.

This feature is experimental and disabled by default. Enable it with `KIMI_CODE_EXPERIMENTAL_SECONDARY_MODEL=1`, or the master `KIMI_CODE_EXPERIMENTAL_FLAG=1`. It takes effect in every launch mode, including the interactive TUI.

In the interactive TUI, the [`/secondary_model`](../reference/slash-commands.md) command opens a model picker that writes this section and live-applies it to the current session, so newly spawned subagents bind the new secondary model right away.

| Field            | Type     | Default | Description                                                                                                                                                                                                                                                                                                                                                                                                                           |
| ---------------- | -------- | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `provider`       | `string` | —       | Provider ID for the secondary model                                                                                                                                                                                                                                                                                                                                                                                                   |
| `model`          | `string` | —       | Model ID within `provider`                                                                                                                                                                                                                                                                                                                                                                                                            |
| `default_effort` | `string` | —       | Thinking effort applied when subagents bind to the secondary model. Unset, the effort resolves naturally (global `[thinking]` config → the bound model's default effort) instead of inheriting the main agent's effort. Follows the main model's thinking-effort semantics: models with strict effort validation (e.g. Kimi models) fall back to their default effort for unsupported values; other providers receive the value as-is |

```toml
[secondary_model]
provider = "anthropic"
model = "claude-sonnet-4-6"
default_effort = "low"
```

`provider`, `model`, and `default_effort` can be overridden by `KIMI_SECONDARY_PROVIDER`, `KIMI_SECONDARY_MODEL`, and `KIMI_SECONDARY_EFFORT`, which take higher priority than `config.toml`.

When the experiment is enabled, the configuration is validated as the session starts: an unresolvable provider/model or an unsupported `default_effort` produces a startup warning (also returned by the session-warnings API). The check is advisory — a broken secondary model still fails at spawn time, with the same source hint attached to the spawn error.

## `thinking`

`thinking` sets the global default behavior for Thinking mode.

| Field     | Type      | Default | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| --------- | --------- | ------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `enabled` | `boolean` | `true`  | Whether Thinking is enabled by default for new sessions; set to `false` to force Thinking off                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `effort`  | `string`  | —       | Thinking effort level (for example `low`, `medium`, `high`, `xhigh`, `max`). Non-Kimi providers do not remap concrete effort values when the upstream protocol accepts them; if the provider rejects the value, choose one that the model supports. Protocols that expose only levels or token budgets still require format conversion. Kimi models with `support_efforts` fall back to their model default when this configured value is not listed; Kimi models without that list treat every enabled value as boolean `on`                                                                  |
| `keep`    | `string`  | `"all"` | Preserved Thinking passthrough. On `kimi` it is sent as `thinking.keep`; on `anthropic` (Claude and Kimi's Anthropic-compatible mode) it is sent as a `context_management` `clear_thinking_20251015` edit (enabling keep routes Anthropic requests to the beta Messages API; an off-value disables keep and returns to the standard endpoint). `"all"` preserves prior turns' reasoning (`reasoning_content` / Anthropic thinking blocks); set to an off-value (`false`/`0`/`no`/`off`/`none`/`null`) to disable. Overridden by `KIMI_MODEL_THINKING_KEEP`; only injected while Thinking is on |

### Deprecated fields

| Field              | Deprecated in | Description                                                                                                                                                                                                      |
| ------------------ | ------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `default_thinking` | 0.21.0        | Top-level boolean, replaced by `[thinking] enabled`. Migrate `default_thinking = true` to `enabled = true`, and `default_thinking = false` to `enabled = false`.                                                 |
| `thinking.mode`    | 0.21.0        | One of `auto` / `on` / `off`, replaced by `[thinking] enabled`. `mode = "off"` becomes `enabled = false`; `mode = "on"` and `mode = "auto"` are equivalent to `enabled = true` (the default) and can be removed. |

## `loop_control`

`loop_control` governs the step count limit, per-step retry count, and the threshold that triggers automatic context compaction in the Agent execution loop.

| Field                   | Type      | Default | Description                                                                                                                            |
| ----------------------- | --------- | ------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `max_steps_per_turn`    | `integer` | —       | Maximum steps per turn; unset or `0` means unlimited                                                                                   |
| `max_retries_per_step`  | `integer` | `10`    | Maximum retries after a step failure                                                                                                   |
| `reserved_context_size` | `integer` | —       | Number of tokens reserved for model output; automatic compaction is triggered when the remaining context window falls below this value |

`max_steps_per_turn` can be overridden by the `KIMI_LOOP_MAX_STEPS_PER_TURN` environment variable, and `max_retries_per_step` by `KIMI_LOOP_MAX_RETRIES_PER_STEP`; both take higher priority than the config file.

Retries only apply to transient failures — connection errors, timeouts, HTTP 429 rate limits, and 5xx server errors. A 429 caused by an exhausted quota or insufficient account balance is not retried and fails immediately, since it cannot succeed until the account is recharged.

## `task`

`task` controls the lifecycle and concurrency behavior of tasks (including background `Bash` commands and `Agent` calls with `run_in_background=true`).

| Field                             | Type                           | Default     | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| --------------------------------- | ------------------------------ | ----------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `max_running_tasks`               | `integer`                      | —           | Maximum number of background tasks running concurrently                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `kill_grace_period_ms`            | `integer`                      | `5000`      | Grace period in milliseconds after session close, a manual stop, or a task timeout requests graceful termination. If a task is still running after this period, Kimi Code attempts to force-stop it                                                                                                                                                                                                                                                                                                                        |
| `bash_auto_background_on_timeout` | `boolean`                      | `true`      | When a foreground `Bash` command hits its timeout, move it to a background task instead of killing it — the agent is notified when it completes, and the backgrounded command is bounded by the `bash_task_timeout_s` default background timeout. Set to `false` to kill timed-out foreground commands instead                                                                                                                                                                                                             |
| `bash_task_timeout_s`             | `integer`                      | `600`       | Default timeout (seconds) for background `Bash` tasks when the call omits `timeout`; also used to re-arm foreground commands moved to the background on timeout. `0` means no timeout — the task runs until it exits or the model stops it. Explicit per-call `timeout` values are unaffected. In print mode (`kimi -p`) the default is `0` unless explicitly set                                                                                                                                                          |
| `print_background_mode`           | `"exit" \| "drain" \| "steer"` | `"steer"`   | Print mode (`kimi -p`) only. Governs how pending background tasks are handled once the main agent's turn ends: `"exit"` exits immediately; `"drain"` waits for every background task to reach a terminal state before exiting (results are not fed back to the main agent); `"steer"` stays alive so a completing background task — like a background subagent — injects a synthetic user message that steers the main agent into a new turn, looping until a turn ends with no pending background tasks or a limit is hit |
| `print_wait_ceiling_s`            | `integer`                      | `315360000` | In print mode (`kimi -p`), the wall-clock ceiling (seconds) for the wait/steer loop when `print_background_mode` is `"drain"` or `"steer"` (the default is 10 years — effectively unbounded). Has no effect outside print mode or when it is `"exit"`                                                                                                                                                                                                                                                                      |
| `print_max_turns`                 | `integer`                      | `100000`    | In print mode (`kimi -p`) with `print_background_mode = "steer"`, the maximum number of new turns that may be triggered by background-task completions, to keep the steering loop bounded (the default is effectively unbounded)                                                                                                                                                                                                                                                                                           |

`max_running_tasks` can be overridden by `KIMI_CODE_BACKGROUND_MAX_RUNNING_TASKS`, which takes higher priority than `config.toml`.

In print mode (`kimi -p "<prompt>"`), Kimi Code stays alive after the main agent's turn as long as background tasks are still pending: each completion is fed back to the main agent as a synthetic user message, steering it into a new turn (`print_background_mode = "steer"` by default), and the run exits once a turn ends with nothing pending. The loop is bounded by `print_wait_ceiling_s` and `print_max_turns`, both effectively unbounded by default. Background work is never killed by a wall-clock cap in print mode either: background `Bash` tasks default to no timeout (`bash_task_timeout_s = 0`), and subagents run without a timeout (`[subagent] timeout_ms = 0`), so only the model itself stops a task. Set `print_background_mode` to `"drain"` to wait for tasks without feeding results back, or `"exit"` to end the run as soon as the main agent finishes.

## `subagent`

| Field        | Type      | Default             | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| ------------ | --------- | ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `timeout_ms` | `integer` | `7200000` (2 hours) | Maximum wall-clock time (milliseconds) a single subagent (`Agent` / `AgentSwarm`) is allowed to run before it is settled as `timed_out`. `0` means no timeout — the subagent runs until it finishes or the model stops it. This is the background-task manager's per-task timeout for each subagent task, so it applies to both foreground and background subagents. In print mode (`kimi -p`) the default is `0` unless explicitly set. Note: any value above `2147483647` (about 24.8 days) is clamped to roughly 24.8 days by the runtime |

`timeout_ms` can be overridden by the `KIMI_SUBAGENT_TIMEOUT_MS` environment variable, which takes higher priority than `config.toml`.

## `mcp`

| Field                | Type      | Default              | Description                                                                                                                                                                                                                                                                    |
| -------------------- | --------- | -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `startup_timeout_ms` | `integer` | `30000` (30 seconds) | Global default connection (startup + tool discovery) timeout in milliseconds for all MCP servers. Accepts `1`–`2147483647`. A per-server `startupTimeoutMs` in `mcp.json` always wins over this section and the environment variable; when neither is set, the default applies |
| `tool_timeout_ms`    | `integer` | `60000` (60 seconds) | Global default single tool-call timeout in milliseconds for all MCP servers. Accepts `1`–`2147483647`. A per-server `toolTimeoutMs` in `mcp.json` always wins over this section and the environment variable; when neither is set, the client built-in default applies         |

`startup_timeout_ms` and `tool_timeout_ms` can be overridden by the `KIMI_MCP_STARTUP_TIMEOUT_MS` and `KIMI_MCP_TOOL_TIMEOUT_MS` environment variables respectively, which take higher priority than `config.toml`. See [MCP](../customization/mcp.md) for the full MCP server configuration.

## `tools`

`tools` is the global tool switch: it applies to every agent in all sessions and intersects with each agent's own `tools` / `disallowedTools` policy.

| Field      | Type            | Default | Description                                                                                                                               |
| ---------- | --------------- | ------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `enabled`  | `array<string>` | —       | Global allowlist: when non-empty, only the listed tools are available; omitting the field or setting an empty array imposes no constraint |
| `disabled` | `array<string>` | —       | Global denylist, applied after `enabled`                                                                                                  |

Name matching follows the same rules as the same-named fields in an agent file: built-in tools match by exact name (such as `Read`), and MCP tools match with globs (such as `mcp__github__*`). Three entry shapes never match anything and are reported with a warning: a wildcard outside an `mcp__` pattern (`enabled = ["*"]` disables every tool, `disabled = ["*"]` disables none), an `mcp__` literal missing the tool segment (`mcp__github` — use `mcp__github__*` for a whole server), and a name no registered or built-in tool has (matching is case-sensitive).

```toml
[tools]
disabled = ["EnterPlanMode", "ExitPlanMode", "mcp__github__*"]
```

::: warning Note
Like the `tools` / `disallowedTools` fields of an agent file, this section shapes the tools shown to the model and is enforced again before execution. [Permission rules](#permission) remain a separate control for operations that require approval.
:::

## `image`

`image` controls how images are compressed before being sent to the model, across every ingestion point (pasted images, `ReadMediaFile` reads, images in MCP tool results, and so on).

| Field              | Type      | Default           | Description                                                                                                                                                                                                                                                                                                                                                           |
| ------------------ | --------- | ----------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `max_edge_px`      | `integer` | `2000`            | Longest-edge ceiling in pixels. Larger images are scaled down proportionally to fit; raising it preserves more detail at the cost of larger request bodies                                                                                                                                                                                                            |
| `read_byte_budget` | `integer` | `262144` (256 KB) | Per-image byte budget for images the model reads for itself (`ReadMediaFile` default reads). It bounds the accumulated request-body size when the model keeps screenshotting and reading images; fine detail stays reachable through the `region` parameter, which reads a crop back at full fidelity (`region` and `full_resolution` are not subject to this budget) |

`max_edge_px` can be overridden by the `KIMI_IMAGE_MAX_EDGE_PX` environment variable and `read_byte_budget` by `KIMI_IMAGE_READ_BYTE_BUDGET`; both take higher priority than `config.toml`.

<!--
## `experimental`

`experimental` stores persistent overrides for experimental-feature flags. Currently, `micro_compaction` is the only user-facing entry and defaults to `false`; set it to `true` to enable automatic trimming of older large tool results.

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `micro_compaction` | `boolean` | `false` | Trim older large tool results from context while preserving recent conversation |
-->

## `services`

`services` configures two built-in services: web search (`moonshot_search`) and web fetch (`moonshot_fetch`). Only these two fixed keys are recognized; other keys are ignored. Both entries share the same fields:

| Field            | Type                    | Required | Description                                  |
| ---------------- | ----------------------- | -------- | -------------------------------------------- |
| `base_url`       | `string`                | No       | Service API URL                              |
| `api_key`        | `string`                | No       | API key                                      |
| `custom_headers` | `table<string, string>` | No       | Custom HTTP headers attached to each request |

`base_url` and `api_key` can also come from environment variables, which take priority over the config file: `KIMI_WEB_SEARCH_BASE_URL` / `KIMI_WEB_SEARCH_API_KEY` for `moonshot_search`, and `KIMI_WEB_FETCH_BASE_URL` / `KIMI_WEB_FETCH_API_KEY` for `moonshot_fetch`. An env base URL defines a separate service endpoint, so the persisted API key and custom headers are not forwarded to it; set the matching env API key when that endpoint requires authentication. An env API key without an env base URL keeps the configured endpoint and custom headers but replaces the configured key. Setting the base URL and API key through env without any config section also enables the service.

```toml
[services.moonshot_search]
base_url = "https://api.moonshot.cn/v1/search"
api_key = "YOUR_API_KEY"

[services.moonshot_fetch]
base_url = "https://api.moonshot.cn/v1/fetch"
api_key = "YOUR_API_KEY"
```

## `permission`

`permission` sets permission rules that are automatically loaded when a session starts, controlling whether the Agent needs user confirmation before calling a tool. Rules are written as a `[[permission.rules]]` array of tables, matched in order — the first matching rule takes effect.

| Field      | Type     | Required | Description                                                                                          |
| ---------- | -------- | -------- | ---------------------------------------------------------------------------------------------------- |
| `decision` | `string` | Yes      | Action on match: `allow` (permit immediately), `deny` (reject immediately), `ask` (prompt each time) |
| `scope`    | `string` | No       | Rule scope: `turn-override`, `session-runtime`, `project`, `user`; defaults to `user`                |
| `pattern`  | `string` | Yes      | Match pattern in the form `ToolName` or `ToolName(arg-pattern)`, e.g. `Read` or `Bash(rm -rf*)`      |
| `reason`   | `string` | No       | Rule description for debugging and auditing                                                          |

Built-in tool names are listed in [Built-in tools](../reference/tools.md). Most built-in tools that accept rule arguments define their own matching subject, such as `Bash(command-pattern)` or `Read(path-pattern)`. `AgentSwarm`, MCP tools, and custom tools can only be matched by tool name — argument patterns are not supported for them.

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
MCP server declarations are configured in `~/.kimi-code/mcp.json` or the project-local `.kimi-code/mcp.json`, not in `config.toml`. The interactive configuration entry point is `/mcp-config`; see [Model Context Protocol](../customization/mcp.md).
:::

## `tui.toml`

Alongside `config.toml`, the CLI keeps terminal-UI and client preferences in a companion `tui.toml` in the same directory (`~/.kimi-code/tui.toml`, or `$KIMI_CODE_HOME/tui.toml` when overridden). It is created with defaults on first run, and the interactive commands `/config`, `/theme`, and `/editor` write to it for you — so you rarely need to edit it by hand. If the file is malformed, the CLI falls back to defaults and shows a notice instead of failing to start.

| Field                                    | Type                 | Default     | Description                                                                                                                                                                                                                                                                                                           |
| ---------------------------------------- | -------------------- | ----------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `theme`                                  | `string`             | `auto`      | Color theme: `auto` (follow the terminal), `dark`, `light`, or the name of a [custom theme](../customization/themes.md)                                                                                                                                                                                               |
| `disable_paste_burst`                    | `boolean`            | `false`     | Disable the non-bracketed paste-burst fallback that keeps rapid multi-line pastes from submitting line by line                                                                                                                                                                                                        |
| `busy_input_mode`                        | `"queue" \| "steer"` | `steer`     | What Enter does while the agent is mid-turn: `steer` injects into the current turn on Enter; `queue` waits until the current task ends (Ctrl-S still steers immediately). Changeable in `/settings` → Busy input                                                                                                      |
| `[editor].command`                       | `string`             | `""`        | External editor command for composing long input; empty falls back to `$VISUAL` / `$EDITOR`                                                                                                                                                                                                                           |
| `[notifications].enabled`                | `boolean`            | `true`      | Whether desktop notifications are sent                                                                                                                                                                                                                                                                                |
| `[notifications].notification_condition` | `string`             | `unfocused` | When to notify: `unfocused` (only when the terminal is not focused) or `always`                                                                                                                                                                                                                                       |
| `[upgrade].auto_install`                 | `boolean`            | `true`      | Whether new versions are installed automatically                                                                                                                                                                                                                                                                      |
| `[status_line].items`                    | `string[]`           | `[]`        | Built-in slots to show on the first footer line and their order: `mode`, `goal`, `model`, `tasks`, `cwd`, `git`, `tips`. Unset keeps the default layout; unknown ids are skipped with a warning                                                                                                                       |
| `[status_line].command`                  | `string`             | `""`        | Custom status line command. Its first stdout line replaces the first footer line, with a JSON snapshot (model, cwd, git branch, permission mode, plan mode, context usage, session id, version) passed on stdin. Runs are capped at 300ms and throttled to once per second; failures fall back to the built-in layout |

```toml
# ~/.kimi-code/tui.toml
theme = "auto" # "auto" | "dark" | "light" | custom theme name
disable_paste_burst = false # true disables non-bracketed paste-burst fallback
busy_input_mode = "steer" # "steer" (Enter steers mid-turn) | "queue" (Enter queues; Ctrl-S steers)

[editor]
command = "" # empty uses $VISUAL / $EDITOR

[notifications]
enabled = true
notification_condition = "unfocused" # "unfocused" | "always"

[upgrade]
auto_install = true

# [status_line]
# items = ["mode", "goal", "model", "tasks", "cwd", "git", "tips"]
# command = "~/.kimi-code/statusline.sh"
```

Changes apply on the next start, or immediately with `/reload-tui` (which reloads only `tui.toml`); `/reload` reloads both `config.toml` and `tui.toml`.

## Project-local configuration

In addition to the user-level files under `~/.kimi-code`, Kimi Code reads a project-local configuration file at `<project-root>/.kimi-code/local.toml`. It holds settings that are specific to one project checkout and typically should not be shared with teammates.

The file is created automatically when you add an extra workspace directory with [`/add-dir`](../reference/slash-commands.md) and choose to remember it for the project. You rarely need to edit it by hand.

### `[workspace]`

The `[workspace]` table groups project-level workspace settings:

| Field            | Type            | Required | Description                                                                                                                                                                                                                        |
| ---------------- | --------------- | -------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `additional_dir` | `array<string>` | No       | Additional workspace directories, stored as absolute paths. Written automatically when you confirm "remember this directory" in `/add-dir`; read back on startup so the directories are available in every session of this project |

```toml
[workspace]
additional_dir = ["/absolute/path/to/shared"]
```

Because directories are stored as absolute paths, which are specific to your machine, we recommend adding `.kimi-code/local.toml` to your project's `.gitignore` so it is not committed.

## Next steps

- [Providers and models](./providers.md) — built-in providers, authentication, and dynamic model catalogs
- [Config overrides](./overrides.md) — priority rules for CLI options, config file, and environment variables
- [Environment variables](./env-vars.md) — complete list of runtime variables like `KIMI_CODE_HOME`
