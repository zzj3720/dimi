# Environment variables

Kimi Code CLI uses environment variables for provider API keys, model selection, runtime switches, and relocating its data directory. Provider credentials are never written to `config.toml`: use `kimi login <provider>` to store a key securely, or export the provider's standard API-key variable for the current process.

## Core paths

### `KIMI_CODE_HOME`

Overrides the data root directory; the default is `~/.kimi-code`. Once set, the config file, sessions, logs, OAuth credentials, and all other data land under the new path:

```sh
export KIMI_CODE_HOME="/path/to/custom/kimi-code"
```

> Make sure the directory is writable. Multiple `kimi` instances sharing the same `KIMI_CODE_HOME` will share config and credential files.

For the complete data directory structure, see [Data locations](./data-locations.md).

### `KIMI_DISABLE_TELEMETRY`

Set to `1` to turn off anonymous telemetry reporting (also accepts `true`, `yes`, `y`, case-insensitive):

```sh
export KIMI_DISABLE_TELEMETRY=1
```

### `KIMI_MODEL_*` family

Select a built-in provider and one of its models without modifying `config.toml`. See [Select a model from environment variables](#select-a-model-from-environment-variables).

## Provider API keys

The provider runtime reads these standard shell environment variables. A key saved with `kimi login <provider> --method api-key` has priority over an environment variable.

| Variable             | Provider                                   |
| -------------------- | ------------------------------------------ |
| `KIMI_API_KEY`       | Kimi Code; also a fallback for Moonshot AI |
| `MOONSHOT_API_KEY`   | Moonshot AI                                |
| `OPENAI_API_KEY`     | OpenAI                                     |
| `XAI_API_KEY`        | xAI                                        |
| `ANTHROPIC_API_KEY`  | Anthropic                                  |
| `OPENROUTER_API_KEY` | OpenRouter                                 |
| `DEEPSEEK_API_KEY`   | DeepSeek                                   |
| `GROQ_API_KEY`       | Groq                                       |
| `MISTRAL_API_KEY`    | Mistral                                    |
| `TOGETHER_API_KEY`   | Together AI                                |
| `CEREBRAS_API_KEY`   | Cerebras                                   |
| `FIREWORKS_API_KEY`  | Fireworks AI                               |
| `ZAI_API_KEY`        | Z.AI                                       |
| `DASHSCOPE_API_KEY`  | Alibaba Cloud Model Studio                 |

OpenAI Codex uses OAuth and does not read `OPENAI_API_KEY`. Kimi Code and xAI accept either OAuth or an API key; use `kimi login <provider>` to choose explicitly.

For the full built-in provider list, see [Providers and models](./providers.md).

## OAuth and managed services

This group of variables redirects OAuth authentication and managed service endpoints to a self-hosted or test environment. They are not needed for everyday use.

| Variable               | Purpose                                                       | Default                                          |
| ---------------------- | ------------------------------------------------------------- | ------------------------------------------------ |
| `KIMI_CODE_OAUTH_HOST` | OAuth auth host; highest priority                             | Falls back to `KIMI_OAUTH_HOST` when unset       |
| `KIMI_OAUTH_HOST`      | OAuth auth host; fallback for `KIMI_CODE_OAUTH_HOST`          | Falls back to `https://auth.kimi.com` when unset |
| `KIMI_CODE_BASE_URL`   | Base URL for managed Kimi services and provider-aware plugins | `https://api.kimi.com/coding/v1`                 |

## Select a model from environment variables

Set the provider and model IDs together to select an entry from the runtime catalog without changing `config.toml`. They take priority over `default_provider` and `default_model`; the `-m <provider>/<model>` startup option still has the highest priority.

```sh
export KIMI_MODEL_PROVIDER="anthropic"
export KIMI_MODEL_NAME="claude-sonnet-4-6"
export ANTHROPIC_API_KEY="YOUR_API_KEY"
kimi
```

| Variable              | Purpose                                       |
| --------------------- | --------------------------------------------- |
| `KIMI_MODEL_PROVIDER` | Built-in provider ID, for example `anthropic` |
| `KIMI_MODEL_NAME`     | Model ID within that provider                 |

If either value is missing or the pair is not in the catalog, startup reports that the selected model cannot be resolved.

## Runtime switches

Switches that control the behavior of subsystems such as telemetry, background tasks, and the plugin marketplace:

| Variable                                 | Purpose                                                                                                                                                                                                                                                                                                                          | Valid values                                                                                                        |
| ---------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `KIMI_DISABLE_TELEMETRY`                 | Disable anonymous telemetry reporting                                                                                                                                                                                                                                                                                            | `1`, `true`, `yes`, `y` (case-insensitive)                                                                          |
| `KIMI_CODE_BACKGROUND_MAX_RUNNING_TASKS` | Cap on concurrently running background tasks; takes higher priority than `[task] max_running_tasks` in `config.toml` (unset means no cap)                                                                                                                                                                                        | Positive integer; invalid values are ignored                                                                        |
| `KIMI_IMAGE_MAX_EDGE_PX`                 | Longest-edge ceiling (px) for image compression; takes higher priority than `[image] max_edge_px` in `config.toml` (default `2000`)                                                                                                                                                                                              | Positive integer; invalid values are ignored                                                                        |
| `KIMI_IMAGE_READ_BYTE_BUDGET`            | Per-image byte budget for model-initiated image reads (`ReadMediaFile` default reads); takes higher priority than `[image] read_byte_budget` in `config.toml` (default `262144`, i.e. 256 KB)                                                                                                                                    | Positive integer; invalid values are ignored                                                                        |
| `KIMI_CODE_PLUGIN_MARKETPLACE_URL`       | Override the plugin marketplace JSON loaded by `/plugins`; useful for dev loopback servers, staging CDN files, or alternate marketplace directories                                                                                                                                                                              | `https://code.kimi.com/kimi-code/plugins/marketplace.json`; also accepts `http://`, `file://` URLs, and local paths |
| `KIMI_CODE_AGENT_SWARM_MAX_CONCURRENCY`  | Cap how many AgentSwarm subagents run concurrently during the initial ramp; leave unset for no cap                                                                                                                                                                                                                               | Positive integer; invalid values fail fast                                                                          |
| `KIMI_SUBAGENT_TIMEOUT_MS`               | Maximum wall-clock time (ms) a single subagent (`Agent` / `AgentSwarm`) may run; takes higher priority than `[subagent] timeout_ms` in `config.toml` (default `7200000`, i.e. 2 hours)                                                                                                                                           | Positive integer; invalid values fall back to the config or default                                                 |
| `KIMI_CODE_EXPERIMENTAL_SECONDARY_MODEL` | Enable the experimental secondary-model feature in every launch mode, including the interactive TUI; the master `KIMI_CODE_EXPERIMENTAL_FLAG=1` also enables it                                                                                                                                                                  | Truthy: `1`/`true`/`yes`/`on`; falsy: `0`/`false`/`no`/`off`                                                        |
| `KIMI_SECONDARY_PROVIDER`                | Provider ID for the secondary model; takes higher priority than `[secondary_model] provider`                                                                                                                                                                                                                                     | A built-in provider ID, e.g. `anthropic`; blank values are ignored                                                  |
| `KIMI_SECONDARY_MODEL`                   | Model ID within `KIMI_SECONDARY_PROVIDER`; takes higher priority than `[secondary_model] model`. When the secondary-model experiment is enabled, newly spawned subagents (`Agent` / `AgentSwarm`) use this provider/model pair instead of inheriting the main agent's model                                                      | A model ID, e.g. `claude-sonnet-4-6`; blank values are ignored                                                      |
| `KIMI_SECONDARY_EFFORT`                  | Thinking effort for the secondary model; takes higher priority than `[secondary_model] default_effort` in `config.toml` and applies only when both the model and its experiment are enabled                                                                                                                                      | An effort value, e.g. `low`; blank values are ignored                                                               |
| `KIMI_MCP_STARTUP_TIMEOUT_MS`            | Global default connection timeout (ms) for all MCP servers; takes higher priority than `[mcp] startup_timeout_ms` in `config.toml`, but a per-server `startupTimeoutMs` in `mcp.json` still wins (default `30000`)                                                                                                               | Integer from `1` to `2147483647`; invalid values are ignored                                                        |
| `KIMI_MCP_TOOL_TIMEOUT_MS`               | Global default single tool-call timeout (ms) for all MCP servers; takes higher priority than `[mcp] tool_timeout_ms` in `config.toml`, but a per-server `toolTimeoutMs` in `mcp.json` still wins (default `60000`)                                                                                                               | Integer from `1` to `2147483647`; invalid values are ignored                                                        |
| `KIMI_LOOP_MAX_STEPS_PER_TURN`           | Maximum Agent steps per turn; takes higher priority than `[loop_control] max_steps_per_turn` in `config.toml` (unset or `0` means unlimited)                                                                                                                                                                                     | Non-negative integer; invalid values are ignored                                                                    |
| `KIMI_LOOP_MAX_RETRIES_PER_STEP`         | Maximum retries after a step failure; takes higher priority than `[loop_control] max_retries_per_step` in `config.toml` (default `10`)                                                                                                                                                                                           | Non-negative integer; invalid values are ignored                                                                    |
| `KIMI_WEB_SEARCH_BASE_URL`               | API URL of the web search (`WebSearch`) service; takes higher priority than `[services.moonshot_search] base_url` in `config.toml`, and enables the service without that config section. Persisted credentials and custom headers are not forwarded to an env-selected endpoint                                                  | Non-blank string; blank values are ignored                                                                          |
| `KIMI_WEB_SEARCH_API_KEY`                | API key of the web search (`WebSearch`) service; replaces both the configured API key and OAuth credential when set                                                                                                                                                                                                              | Non-blank string; blank values are ignored                                                                          |
| `KIMI_WEB_FETCH_BASE_URL`                | API URL of the web fetch (`FetchURL`) service; takes higher priority than `[services.moonshot_fetch] base_url`. Persisted credentials and custom headers are not forwarded to an env-selected endpoint. Without an env or config endpoint, signed-in users try the managed Kimi OAuth fetch service before direct local requests | Non-blank string; blank values are ignored                                                                          |
| `KIMI_WEB_FETCH_API_KEY`                 | API key of the web fetch (`FetchURL`) service; replaces both the configured API key and OAuth credential when set                                                                                                                                                                                                                | Non-blank string; blank values are ignored                                                                          |
| `KIMI_CODE_EXPERIMENTAL_FLAG`            | Enable all registered experimental features for this process                                                                                                                                                                                                                                                                     | `1`, `true`, `yes`, `on`                                                                                            |
| `KIMI_SHELL_PATH`                        | Override the Git Bash path on Windows (used when auto-detection fails)                                                                                                                                                                                                                                                           | Absolute path                                                                                                       |
| `KIMI_MODEL_MAX_COMPLETION_TOKENS`       | Global hard cap on generated tokens per LLM step; overrides `[model_overrides] max_completion_tokens`                                                                                                                                                                                                                            | Integer                                                                                                             |
| `KIMI_MODEL_TEMPERATURE`                 | Global sampling temperature; overrides `[model_overrides] temperature`                                                                                                                                                                                                                                                           | Number, e.g. `0.3`                                                                                                  |
| `KIMI_MODEL_TOP_P`                       | Global nucleus-sampling value; overrides `[model_overrides] top_p`                                                                                                                                                                                                                                                               | Number, e.g. `0.95`                                                                                                 |
| `KIMI_MODEL_THINKING_EFFORT`             | Force a thinking effort for the selected model; overrides `[thinking] forced_effort`                                                                                                                                                                                                                                             | An effort value supported by the selected model                                                                     |
| `KIMI_MODEL_THINKING_KEEP`               | Global preserved-thinking setting; overrides `[model_overrides] thinking_keep`                                                                                                                                                                                                                                                   | A provider-supported value, e.g. `all`                                                                              |
| `KIMI_CODE_NO_AUTO_UPDATE`               | Fully disable the update preflight — no check, background install, or prompt. Legacy alias `KIMI_CLI_NO_AUTO_UPDATE` is also honored                                                                                                                                                                                             | Truthy: `1`/`true`/`yes`/`on`                                                                                       |
| `KIMI_DISABLE_CRON`                      | Disable the scheduled-task tool (`CronCreate` rejects new schedules; existing tasks do not fire)                                                                                                                                                                                                                                 | `1` to disable                                                                                                      |

## Diagnostic logs

These variables control log level and file rotation, read once at process startup:

| Variable                     | Purpose                                            | Default          |
| ---------------------------- | -------------------------------------------------- | ---------------- |
| `KIMI_LOG_LEVEL`             | Log level: `off`, `error`, `warn`, `info`, `debug` | `info`           |
| `KIMI_LOG_GLOBAL_MAX_BYTES`  | Maximum bytes per global log file                  | `6291456` (6 MB) |
| `KIMI_LOG_GLOBAL_FILES`      | Number of global log files to retain               | `5`              |
| `KIMI_LOG_SESSION_MAX_BYTES` | Maximum bytes per session log file                 | `5242880` (5 MB) |
| `KIMI_LOG_SESSION_FILES`     | Number of session log files to retain              | `3`              |

## System environment variables

The CLI also reads several standard system variables to detect the runtime environment; it does not modify them:

- `HOME`: used to resolve the default data path
- `VISUAL`, `EDITOR`: external editor command (`VISUAL` takes precedence)
- `PATH`: used to locate dependencies such as `rg`, `fd`, `fdfind`, and `git`; on Windows, Git Bash detection checks each `git.exe` found on `PATH`, including package-manager shims such as Scoop
- `NO_COLOR`, `FORCE_COLOR`: control color output (following the [no-color.org](https://no-color.org) convention)
- `CI`: when non-empty and not `"0"`, disables theme detection and falls back to the dark theme
- `TERM_PROGRAM`, `TERM`, `TMUX`: detect terminal features and notification support
- `DISPLAY`, `WAYLAND_DISPLAY`, `XDG_SESSION_TYPE`: detect Linux graphical sessions (for clipboard and image features)
- `WSL_DISTRO_NAME`, `WSLENV`: detect WSL for the clipboard PowerShell bridge
- `LOCALAPPDATA`: used on Windows as a fallback when probing for the Git Bash installation path

## HTTP proxy

Kimi Code honors the standard proxy environment variables for all outbound traffic — model API calls, MCP servers, web tools, telemetry, sign-in, and update checks:

- `HTTP_PROXY` / `http_proxy`: proxy for `http://` requests
- `HTTPS_PROXY` / `https_proxy`: proxy for `https://` requests
- `ALL_PROXY` / `all_proxy`: fallback proxy used when the scheme-specific variable is unset; this is where a SOCKS proxy is usually set
- `NO_PROXY` / `no_proxy`: comma-separated hosts that bypass the proxy

Both HTTP(S) and SOCKS proxies are supported. A SOCKS proxy is recognized by its scheme — `socks5://`, `socks5h://`, `socks4://`, or `socks://` (an alias for `socks5://`) — and is typically set via `ALL_PROXY` (the form used by tools like Clash and V2RayN). An HTTP(S) proxy takes precedence over `ALL_PROXY` for HTTP/HTTPS traffic.

The proxy is applied only when one of these variables is set; otherwise connections are made directly. Loopback hosts (`localhost`, `127.0.0.1`, `::1`) always bypass the proxy, so a local server such as a localhost MCP server keeps working when a proxy is configured — add your own internal hosts to `NO_PROXY` to exempt them too.

Stdio MCP servers that run as Node child processes honor `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY` automatically when the child's Node version supports `NODE_USE_ENV_PROXY` (Node ≥ 22.21 or ≥ 24.5); SOCKS proxying applies to Kimi Code's own traffic only.

## Next steps

- [Config overrides](./overrides.md) — how environment variables, CLI options, and the config file interact by priority
- [Data locations](./data-locations.md) — directory structure affected by `KIMI_CODE_HOME`
- [Providers and models](./providers.md) — built-in providers, login methods, and dynamic catalogs
