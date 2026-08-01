# Environment variables

Dimi CLI uses environment variables for provider API keys, model selection, runtime switches, and relocating its data directory. Provider credentials are never written to `config.toml`: use `vp run dev:cli -- login <provider>` from the checkout to store a key securely, or export the provider's standard API-key variable for the current process.

## Core paths

### `DIMI_CODE_HOME`

Overrides the data root directory; the default is `~/.dimi`. Once set, the config file, sessions, logs, OAuth credentials, and all other data land under the new path:

```sh
export DIMI_CODE_HOME="/path/to/custom/dimi"
```

> Make sure the directory is writable. Multiple `dimi` instances sharing the same `DIMI_CODE_HOME` will share config and credential files.

For the complete data directory structure, see [Data locations](./data-locations.md).

### `DIMI_DISABLE_TELEMETRY`

Set to `1` to turn off anonymous telemetry reporting (also accepts `true`, `yes`, `y`, case-insensitive):

```sh
export DIMI_DISABLE_TELEMETRY=1
```

### `DIMI_MODEL_*` family

Select a built-in or `models.json` provider and one of its models without modifying `config.toml`. See [Select a model from environment variables](#select-a-model-from-environment-variables).

## Provider credentials and cloud identity

The provider runtime reads the following standard shell variables. An API key saved with `vp run dev:cli -- login <provider> --method api-key` takes priority over a matching environment variable. For a custom `models.json` provider, use its `apiKey` value as a `$VARIABLE` or `${VARIABLE}` template; it can name any environment variable you control.

| Variable | Provider |
| --- | --- |
| `ANT_LING_API_KEY` | `ant-ling` |
| `ANTHROPIC_API_KEY` | `anthropic` |
| `AZURE_OPENAI_API_KEY` | `azure-openai-responses` |
| `CEREBRAS_API_KEY` | `cerebras` |
| `CLOUDFLARE_API_KEY` | `cloudflare-workers-ai`, `cloudflare-ai-gateway` |
| `CLOUDFLARE_ACCOUNT_ID` | Cloudflare account scope for both Cloudflare providers |
| `CLOUDFLARE_GATEWAY_ID` | Gateway scope for `cloudflare-ai-gateway` |
| `COPILOT_GITHUB_TOKEN` | `github-copilot` token alternative to OAuth |
| `DEEPSEEK_API_KEY` | `deepseek` |
| `FIREWORKS_API_KEY` | `fireworks` |
| `GEMINI_API_KEY` | `google` |
| `GOOGLE_CLOUD_API_KEY` | `google-vertex` |
| `GROQ_API_KEY` | `groq` |
| `HF_TOKEN` | `huggingface` |
| `DIMI_API_KEY` | `kimi-coding`; fallback for `moonshotai` |
| `MINIMAX_API_KEY` | `minimax` |
| `MINIMAX_CN_API_KEY` | `minimax-cn` |
| `MISTRAL_API_KEY` | `mistral` |
| `MOONSHOT_API_KEY` | `moonshotai`, `moonshotai-cn` |
| `NVIDIA_API_KEY` | `nvidia` |
| `OPENAI_API_KEY` | `openai` |
| `OPENCODE_API_KEY` | `opencode`, `opencode-go` |
| `OPENROUTER_API_KEY` | `openrouter` |
| `QWEN_TOKEN_PLAN_API_KEY` | `qwen-token-plan` |
| `QWEN_TOKEN_PLAN_CN_API_KEY` | `qwen-token-plan-cn` |
| `RADIUS_API_KEY` | `radius` |
| `TOGETHER_API_KEY` | `together` |
| `AI_GATEWAY_API_KEY` | `vercel-ai-gateway` |
| `XAI_API_KEY` | `xai` |
| `XIAOMI_API_KEY` | `xiaomi` |
| `XIAOMI_TOKEN_PLAN_AMS_API_KEY` | `xiaomi-token-plan-ams` |
| `XIAOMI_TOKEN_PLAN_CN_API_KEY` | `xiaomi-token-plan-cn` |
| `XIAOMI_TOKEN_PLAN_SGP_API_KEY` | `xiaomi-token-plan-sgp` |
| `ZAI_API_KEY` | `zai` |
| `ZAI_CODING_CN_API_KEY` | `zai-coding-cn` |

`openai-codex` is OAuth-only. `kimi-coding`, `xai`, `anthropic`, `openrouter`, `github-copilot`, and `radius` can use OAuth when their login flow offers it. Anthropic also recognizes `ANTHROPIC_AUTH_TOKEN` (bearer gateway token) and `ANTHROPIC_OAUTH_TOKEN` (OAuth token); prefer `vp run dev:cli -- login` for managed OAuth credentials.

Amazon Bedrock discovers AWS credentials from `AWS_PROFILE`, `AWS_ACCESS_KEY_ID` with `AWS_SECRET_ACCESS_KEY`, container credentials, web identity, or the normal AWS config files. `AWS_BEARER_TOKEN_BEDROCK` is a bearer-token alternative. Vertex uses `GOOGLE_CLOUD_PROJECT` (or `GCLOUD_PROJECT`), `GOOGLE_CLOUD_LOCATION`, and optionally `GOOGLE_APPLICATION_CREDENTIALS`; when no credentials file path is set it uses Google Application Default Credentials. These variables are part of the identity chain, not model metadata.

For the full built-in provider list and their login modes, see [Providers and models](./providers.md#built-in-providers).

## OAuth and managed services

This group of variables redirects OAuth authentication and managed service endpoints to a self-hosted or test environment. They are not needed for everyday use.

| Variable               | Purpose                                                       | Default                                          |
| ---------------------- | ------------------------------------------------------------- | ------------------------------------------------ |
| `DIMI_CODE_OAUTH_HOST` | OAuth auth host; highest priority                             | Falls back to `DIMI_OAUTH_HOST` when unset       |
| `DIMI_OAUTH_HOST`      | OAuth auth host; fallback for `DIMI_CODE_OAUTH_HOST`          | Falls back to `https://auth.kimi.com` when unset |
| `DIMI_CODE_BASE_URL`   | Base URL for managed Dimi services and provider-aware plugins | `https://api.kimi.com/coding/v1`                 |

## Select a model from environment variables

Set the provider and model IDs together to select an entry from the runtime catalog without changing `config.toml`. They take priority over `default_provider` and `default_model`; the `-m <provider>/<model>` startup option still has the highest priority.

```sh
export DIMI_MODEL_PROVIDER="anthropic"
export DIMI_MODEL_NAME="claude-sonnet-4-6"
export ANTHROPIC_API_KEY="YOUR_API_KEY"
dimi
```

| Variable              | Purpose                                       |
| --------------------- | --------------------------------------------- |
| `DIMI_MODEL_PROVIDER` | Provider ID from the built-in, SDK, or `models.json` catalog, for example `anthropic` |
| `DIMI_MODEL_NAME`     | Model ID within that provider |

If either value is missing or the pair is not in the catalog, startup reports that the selected model cannot be resolved.

## Runtime switches

Switches that control the behavior of subsystems such as telemetry, background tasks, and the plugin marketplace:

| Variable                                 | Purpose                                                                                                                                                                                                                                                                                                                          | Valid values                                                                                                        |
| ---------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `DIMI_DISABLE_TELEMETRY`                 | Disable anonymous telemetry reporting                                                                                                                                                                                                                                                                                            | `1`, `true`, `yes`, `y` (case-insensitive)                                                                          |
| `DIMI_CODE_BACKGROUND_MAX_RUNNING_TASKS` | Cap on concurrently running background tasks; takes higher priority than `[task] max_running_tasks` in `config.toml` (unset means no cap)                                                                                                                                                                                        | Positive integer; invalid values are ignored                                                                        |
| `DIMI_IMAGE_MAX_EDGE_PX`                 | Longest-edge ceiling (px) for image compression; takes higher priority than `[image] max_edge_px` in `config.toml` (default `2000`)                                                                                                                                                                                              | Positive integer; invalid values are ignored                                                                        |
| `DIMI_IMAGE_READ_BYTE_BUDGET`            | Per-image byte budget for model-initiated image reads (`ReadMediaFile` default reads); takes higher priority than `[image] read_byte_budget` in `config.toml` (default `262144`, i.e. 256 KB)                                                                                                                                    | Positive integer; invalid values are ignored                                                                        |
| `DIMI_CODE_PLUGIN_MARKETPLACE_URL`       | Override the plugin marketplace JSON loaded by `/plugins`; useful for dev loopback servers, staging CDN files, or alternate marketplace directories                                                                                                                                                                              | `https://github.com/zzj3720/dimi/releases/latest/download/plugins/marketplace.json`; also accepts `http://`, `file://` URLs, and local paths |
| `DIMI_CODE_AGENT_SWARM_MAX_CONCURRENCY`  | Cap how many AgentSwarm subagents run concurrently during the initial ramp; leave unset for no cap                                                                                                                                                                                                                               | Positive integer; invalid values fail fast                                                                          |
| `DIMI_SUBAGENT_TIMEOUT_MS`               | Maximum wall-clock time (ms) a single subagent (`Agent` / `AgentSwarm`) may run; takes higher priority than `[subagent] timeout_ms` in `config.toml` (default `7200000`, i.e. 2 hours)                                                                                                                                           | Positive integer; invalid values fall back to the config or default                                                 |
| `DIMI_CODE_EXPERIMENTAL_SECONDARY_MODEL` | Enable the experimental secondary-model feature in every launch mode, including the interactive TUI; the master `DIMI_CODE_EXPERIMENTAL_FLAG=1` also enables it                                                                                                                                                                  | Truthy: `1`/`true`/`yes`/`on`; falsy: `0`/`false`/`no`/`off`                                                        |
| `DIMI_SECONDARY_PROVIDER`                | Provider ID for the secondary model; takes higher priority than `[secondary_model] provider`                                                                                                                                                                                                                                     | A built-in provider ID, e.g. `anthropic`; blank values are ignored                                                  |
| `DIMI_SECONDARY_MODEL`                   | Model ID within `DIMI_SECONDARY_PROVIDER`; takes higher priority than `[secondary_model] model`. When the secondary-model experiment is enabled, newly spawned subagents (`Agent` / `AgentSwarm`) use this provider/model pair instead of inheriting the main agent's model                                                      | A model ID, e.g. `claude-sonnet-4-6`; blank values are ignored                                                      |
| `DIMI_SECONDARY_EFFORT`                  | Thinking effort for the secondary model; takes higher priority than `[secondary_model] default_effort` in `config.toml` and applies only when both the model and its experiment are enabled                                                                                                                                      | An effort value, e.g. `low`; blank values are ignored                                                               |
| `DIMI_MCP_STARTUP_TIMEOUT_MS`            | Global default connection timeout (ms) for all MCP servers; takes higher priority than `[mcp] startup_timeout_ms` in `config.toml`, but a per-server `startupTimeoutMs` in `mcp.json` still wins (default `30000`)                                                                                                               | Integer from `1` to `2147483647`; invalid values are ignored                                                        |
| `DIMI_MCP_TOOL_TIMEOUT_MS`               | Global default single tool-call timeout (ms) for all MCP servers; takes higher priority than `[mcp] tool_timeout_ms` in `config.toml`, but a per-server `toolTimeoutMs` in `mcp.json` still wins (default `60000`)                                                                                                               | Integer from `1` to `2147483647`; invalid values are ignored                                                        |
| `DIMI_LOOP_MAX_STEPS_PER_TURN`           | Maximum Agent steps per turn; takes higher priority than `[loop_control] max_steps_per_turn` in `config.toml` (unset or `0` means unlimited)                                                                                                                                                                                     | Non-negative integer; invalid values are ignored                                                                    |
| `DIMI_LOOP_MAX_RETRIES_PER_STEP`         | Maximum retries after a step failure; takes higher priority than `[loop_control] max_retries_per_step` in `config.toml` (default `10`)                                                                                                                                                                                           | Non-negative integer; invalid values are ignored                                                                    |
| `DIMI_WEB_SEARCH_BASE_URL`               | API URL of the web search (`WebSearch`) service; takes higher priority than `[services.moonshot_search] base_url` in `config.toml`, and enables the service without that config section. Persisted credentials and custom headers are not forwarded to an env-selected endpoint                                                  | Non-blank string; blank values are ignored                                                                          |
| `DIMI_WEB_SEARCH_API_KEY`                | API key of the web search (`WebSearch`) service; replaces both the configured API key and OAuth credential when set                                                                                                                                                                                                              | Non-blank string; blank values are ignored                                                                          |
| `DIMI_WEB_FETCH_BASE_URL`                | API URL of the web fetch (`FetchURL`) service; takes higher priority than `[services.moonshot_fetch] base_url`. Persisted credentials and custom headers are not forwarded to an env-selected endpoint. Without an env or config endpoint, signed-in users try the managed Dimi OAuth fetch service before direct local requests | Non-blank string; blank values are ignored                                                                          |
| `DIMI_WEB_FETCH_API_KEY`                 | API key of the web fetch (`FetchURL`) service; replaces both the configured API key and OAuth credential when set                                                                                                                                                                                                                | Non-blank string; blank values are ignored                                                                          |
| `DIMI_CODE_EXPERIMENTAL_FLAG`            | Enable all registered experimental features for this process                                                                                                                                                                                                                                                                     | `1`, `true`, `yes`, `on`                                                                                            |
| `DIMI_SHELL_PATH`                        | Override the Git Bash path on Windows (used when auto-detection fails)                                                                                                                                                                                                                                                           | Absolute path                                                                                                       |
| `DIMI_MODEL_MAX_COMPLETION_TOKENS`       | Global hard cap on generated tokens per LLM step; overrides `[model_overrides] max_completion_tokens`                                                                                                                                                                                                                            | Integer                                                                                                             |
| `DIMI_MODEL_TEMPERATURE`                 | Global sampling temperature; overrides `[model_overrides] temperature`                                                                                                                                                                                                                                                           | Number, e.g. `0.3`                                                                                                  |
| `DIMI_MODEL_TOP_P`                       | Global nucleus-sampling value; overrides `[model_overrides] top_p`                                                                                                                                                                                                                                                               | Number, e.g. `0.95`                                                                                                 |
| `DIMI_MODEL_THINKING_EFFORT`             | Force a thinking effort for the selected model; overrides `[thinking] forced_effort`                                                                                                                                                                                                                                             | An effort value supported by the selected model                                                                     |
| `DIMI_MODEL_THINKING_KEEP`               | Global preserved-thinking setting; overrides `[model_overrides] thinking_keep`                                                                                                                                                                                                                                                   | A provider-supported value, e.g. `all`                                                                              |
| `DIMI_CODE_NO_AUTO_UPDATE`               | Disable update preflight in a distribution that configures an update channel. This source build has no channel by default. Legacy alias `DIMI_CLI_NO_AUTO_UPDATE` is also honored.                                                                                                                                                     | Truthy: `1`/`true`/`yes`/`on`                                                                                       |
| `DIMI_DISABLE_CRON`                      | Disable the scheduled-task tool (`CronCreate` rejects new schedules; existing tasks do not fire)                                                                                                                                                                                                                                 | `1` to disable                                                                                                      |

## Diagnostic logs

These variables control log level and file rotation, read once at process startup:

| Variable                     | Purpose                                            | Default          |
| ---------------------------- | -------------------------------------------------- | ---------------- |
| `DIMI_LOG_LEVEL`             | Log level: `off`, `error`, `warn`, `info`, `debug` | `info`           |
| `DIMI_LOG_GLOBAL_MAX_BYTES`  | Maximum bytes per global log file                  | `6291456` (6 MB) |
| `DIMI_LOG_GLOBAL_FILES`      | Number of global log files to retain               | `5`              |
| `DIMI_LOG_SESSION_MAX_BYTES` | Maximum bytes per session log file                 | `5242880` (5 MB) |
| `DIMI_LOG_SESSION_FILES`     | Number of session log files to retain              | `3`              |

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

Dimi honors the standard proxy environment variables for all outbound traffic — model API calls, MCP servers, web tools, telemetry, sign-in, and update checks:

- `HTTP_PROXY` / `http_proxy`: proxy for `http://` requests
- `HTTPS_PROXY` / `https_proxy`: proxy for `https://` requests
- `ALL_PROXY` / `all_proxy`: fallback proxy used when the scheme-specific variable is unset; this is where a SOCKS proxy is usually set
- `NO_PROXY` / `no_proxy`: comma-separated hosts that bypass the proxy

Both HTTP(S) and SOCKS proxies are supported. A SOCKS proxy is recognized by its scheme — `socks5://`, `socks5h://`, `socks4://`, or `socks://` (an alias for `socks5://`) — and is typically set via `ALL_PROXY` (the form used by tools like Clash and V2RayN). An HTTP(S) proxy takes precedence over `ALL_PROXY` for HTTP/HTTPS traffic.

The proxy is applied only when one of these variables is set; otherwise connections are made directly. Loopback hosts (`localhost`, `127.0.0.1`, `::1`) always bypass the proxy, so a local server such as a localhost MCP server keeps working when a proxy is configured — add your own internal hosts to `NO_PROXY` to exempt them too.

Stdio MCP servers that run as Node child processes honor `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY` automatically when the child's Node version supports `NODE_USE_ENV_PROXY` (Node ≥ 22.21 or ≥ 24.5); SOCKS proxying applies to Dimi's own traffic only.

## Next steps

- [Config overrides](./overrides.md) — how environment variables, CLI options, and the config file interact by priority
- [Data locations](./data-locations.md) — directory structure affected by `DIMI_CODE_HOME`
- [Providers and models](./providers.md) — built-in providers, login methods, and dynamic catalogs
