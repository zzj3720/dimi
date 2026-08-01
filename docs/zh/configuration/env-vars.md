# 环境变量

Kimi Code CLI 通过环境变量提供供应商 API 密钥、选择模型、控制运行时开关，以及迁移数据目录。供应商凭证不再写入 `config.toml`：可在 checkout 中用 `vp run dev:cli -- login <provider>` 安全保存密钥，也可为当前进程导出供应商的标准 API 密钥变量。

## 核心路径

### `DIMI_CODE_HOME`

覆盖数据根目录，默认 `~/.dimi`。设置后，配置文件、会话、日志、OAuth 凭据等全部数据都落到新路径下：

```sh
export DIMI_CODE_HOME="/path/to/custom/kimi-code"
```

> 确保目录可写。多个 `kimi` 实例共用同一个 `DIMI_CODE_HOME` 会共享配置和凭证。

数据目录的完整结构见[数据路径](./data-locations.md)。

### `DIMI_DISABLE_TELEMETRY`

设为 `1` 关闭匿名遥测上报（也接受 `true`/`yes`/`y`，不区分大小写）：

```sh
export DIMI_DISABLE_TELEMETRY=1
```

### `DIMI_MODEL_*` 系列

不修改 `config.toml`，直接选择一个内置或 `models.json` 供应商及其模型。详见[用环境变量选择模型](#用环境变量选择模型)。

## 供应商凭据与云身份

供应商运行时会读取下列标准 shell 环境变量。通过 `vp run dev:cli -- login <provider> --method api-key` 保存的 API 密钥优先于对应的环境变量。自定义 `models.json` 供应商请在 `apiKey` 中使用 `$VARIABLE` 或 `${VARIABLE}` 模板；它可以引用你控制的任何环境变量。

| 环境变量 | 供应商 |
| --- | --- |
| `ANT_LING_API_KEY` | `ant-ling` |
| `ANTHROPIC_API_KEY` | `anthropic` |
| `AZURE_OPENAI_API_KEY` | `azure-openai-responses` |
| `CEREBRAS_API_KEY` | `cerebras` |
| `CLOUDFLARE_API_KEY` | `cloudflare-workers-ai`、`cloudflare-ai-gateway` |
| `CLOUDFLARE_ACCOUNT_ID` | 两个 Cloudflare 供应商的 account 范围 |
| `CLOUDFLARE_GATEWAY_ID` | `cloudflare-ai-gateway` 的 gateway 范围 |
| `COPILOT_GITHUB_TOKEN` | `github-copilot` 的 token 替代 OAuth |
| `DEEPSEEK_API_KEY` | `deepseek` |
| `FIREWORKS_API_KEY` | `fireworks` |
| `GEMINI_API_KEY` | `google` |
| `GOOGLE_CLOUD_API_KEY` | `google-vertex` |
| `GROQ_API_KEY` | `groq` |
| `HF_TOKEN` | `huggingface` |
| `DIMI_API_KEY` | `kimi-coding`；`moonshotai` 的备用变量 |
| `MINIMAX_API_KEY` | `minimax` |
| `MINIMAX_CN_API_KEY` | `minimax-cn` |
| `MISTRAL_API_KEY` | `mistral` |
| `MOONSHOT_API_KEY` | `moonshotai`、`moonshotai-cn` |
| `NVIDIA_API_KEY` | `nvidia` |
| `OPENAI_API_KEY` | `openai` |
| `OPENCODE_API_KEY` | `opencode`、`opencode-go` |
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

`openai-codex` 只支持 OAuth。`kimi-coding`、`xai`、`anthropic`、`openrouter`、`github-copilot` 和 `radius` 在登录流程提供时可使用 OAuth。Anthropic 还识别 `ANTHROPIC_AUTH_TOKEN`（bearer gateway token）和 `ANTHROPIC_OAUTH_TOKEN`（OAuth token）；托管 OAuth 凭据优先使用 `vp run dev:cli -- login`。

Amazon Bedrock 从 `AWS_PROFILE`、`AWS_ACCESS_KEY_ID` 与 `AWS_SECRET_ACCESS_KEY`、容器凭据、web identity 或普通 AWS 配置文件发现 AWS 凭据。`AWS_BEARER_TOKEN_BEDROCK` 是 bearer token 替代方式。Vertex 使用 `GOOGLE_CLOUD_PROJECT`（或 `GCLOUD_PROJECT`）、`GOOGLE_CLOUD_LOCATION` 和可选的 `GOOGLE_APPLICATION_CREDENTIALS`；未设置凭据文件路径时使用 Google Application Default Credentials。这些变量属于身份链，不是模型元数据。

内置供应商完整列表和登录方式见[供应商与模型](./providers.md#内置供应商)。

## OAuth 与托管端点

这组变量用于将 OAuth 认证和托管服务端点指向自建或测试环境，日常使用不需要设置。

| 环境变量               | 用途                                         | 默认值                             |
| ---------------------- | -------------------------------------------- | ---------------------------------- |
| `DIMI_CODE_OAUTH_HOST` | OAuth 认证 host，优先级最高                  | 未设时回退到 `DIMI_OAUTH_HOST`     |
| `DIMI_OAUTH_HOST`      | OAuth 认证 host，作为上一个的 fallback       | 未设时使用 `https://auth.kimi.com` |
| `DIMI_CODE_BASE_URL`   | Kimi 托管服务与供应商感知 plugin 的 base URL | `https://api.kimi.com/coding/v1`   |

## 用环境变量选择模型

同时设置供应商和模型 ID，即可从运行时目录中选择模型而不修改 `config.toml`。它们优先于 `default_provider` 和 `default_model`，但启动参数 `-m <provider>/<model>` 的优先级仍然最高。

```sh
export DIMI_MODEL_PROVIDER="anthropic"
export DIMI_MODEL_NAME="claude-sonnet-4-6"
export ANTHROPIC_API_KEY="YOUR_API_KEY"
kimi
```

| 环境变量              | 用途                            |
| --------------------- | ------------------------------- |
| `DIMI_MODEL_PROVIDER` | 内置、SDK 或 `models.json` 目录中的供应商 ID，例如 `anthropic` |
| `DIMI_MODEL_NAME`     | 该供应商下的模型 ID |

任一值缺失或该组合不在目录中时，启动会报告所选模型无法解析。

## 运行时开关

控制遥测、后台任务、plugin marketplace 等子系统行为的开关变量：

| 环境变量                                 | 用途                                                                                                                                                                                                                                                    | 合法值                                                                                                 |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `DIMI_DISABLE_TELEMETRY`                 | 关闭匿名遥测上报                                                                                                                                                                                                                                        | `1`、`true`、`yes`、`y`（不区分大小写）                                                                |
| `DIMI_CODE_BACKGROUND_MAX_RUNNING_TASKS` | 同时运行的后台任务数上限，优先级高于 `config.toml` 的 `[task] max_running_tasks`（不设置表示无上限）                                                                                                                                                    | 正整数；非法值被忽略                                                                                   |
| `DIMI_IMAGE_MAX_EDGE_PX`                 | 图片压缩的最长边上限（像素），优先级高于 `config.toml` 的 `[image] max_edge_px`（默认 `2000`）                                                                                                                                                          | 正整数；非法值被忽略                                                                                   |
| `DIMI_IMAGE_READ_BYTE_BUDGET`            | 模型自行读图（`ReadMediaFile` 默认读取）的单图字节预算，优先级高于 `config.toml` 的 `[image] read_byte_budget`（默认 `262144`，即 256 KB）                                                                                                              | 正整数；非法值被忽略                                                                                   |
| `DIMI_CODE_PLUGIN_MARKETPLACE_URL`       | 覆盖 `/plugins` 加载的 plugin marketplace JSON，适合 dev loopback server、测试 CDN 文件或替换 marketplace 目录                                                                                                                                          | `https://code.kimi.com/kimi-code/plugins/marketplace.json`；也接受 `http://`、`file://` URL 和本地路径 |
| `DIMI_CODE_AGENT_SWARM_MAX_CONCURRENCY`  | 限制 AgentSwarm 初始提升并发阶段可同时运行的子 Agent 数量；不设置表示不限制                                                                                                                                                                             | 正整数；非法值会立即失败                                                                               |
| `DIMI_SUBAGENT_TIMEOUT_MS`               | 单个子 Agent（`Agent` / `AgentSwarm`）可运行的最长时间（毫秒）；优先级高于 `config.toml` 的 `[subagent] timeout_ms`（默认 `7200000`，即 2 小时）                                                                                                        | 正整数；非法值回退到配置或默认值                                                                       |
| `DIMI_CODE_EXPERIMENTAL_SECONDARY_MODEL` | 在包括交互式 TUI 在内的所有启动方式下启用实验性的次主力模型功能；master `DIMI_CODE_EXPERIMENTAL_FLAG=1` 也会启用本功能                                                                                                                                  | 真值：`1`/`true`/`yes`/`on`；假值：`0`/`false`/`no`/`off`                                              |
| `DIMI_SECONDARY_PROVIDER`                | 次主力模型的供应商 ID；优先级高于 `[secondary_model] provider`                                                                                                                                                                                          | 内置供应商 ID，例如 `anthropic`；空白值被忽略                                                          |
| `DIMI_SECONDARY_MODEL`                   | `DIMI_SECONDARY_PROVIDER` 下的模型 ID；优先级高于 `[secondary_model] model`。次主力模型实验功能启用后，新派生的子 Agent 使用该供应商/模型组合，而不再继承主 Agent 的模型                                                                                | 模型 ID，例如 `claude-sonnet-4-6`；空白值被忽略                                                        |
| `DIMI_SECONDARY_EFFORT`                  | 次主力模型的 thinking effort；优先级高于 `config.toml` 的 `[secondary_model] default_effort`，仅在次主力模型及其实验功能均启用时生效                                                                                                                    | effort 取值，如 `low`；空白值被忽略                                                                    |
| `DIMI_MCP_STARTUP_TIMEOUT_MS`            | 所有 MCP server 的全局默认连接超时（毫秒）；优先级高于 `config.toml` 的 `[mcp] startup_timeout_ms`，但低于 `mcp.json` 中单个 server 的 `startupTimeoutMs`（默认 `30000`）                                                                               | `1` 到 `2147483647` 的整数；非法值被忽略                                                               |
| `DIMI_MCP_TOOL_TIMEOUT_MS`               | 所有 MCP server 的全局默认单次工具调用超时（毫秒）；优先级高于 `config.toml` 的 `[mcp] tool_timeout_ms`，但低于 `mcp.json` 中单个 server 的 `toolTimeoutMs`（默认 `60000`）                                                                             | `1` 到 `2147483647` 的整数；非法值被忽略                                                               |
| `DIMI_LOOP_MAX_STEPS_PER_TURN`           | Agent 单轮最大步数；优先级高于 `config.toml` 的 `[loop_control] max_steps_per_turn`（不设或 `0` 表示无上限）                                                                                                                                            | 非负整数；非法值被忽略                                                                                 |
| `DIMI_LOOP_MAX_RETRIES_PER_STEP`         | 单步失败后的最大重试次数；优先级高于 `config.toml` 的 `[loop_control] max_retries_per_step`（默认 `10`）                                                                                                                                                | 非负整数；非法值被忽略                                                                                 |
| `DIMI_WEB_SEARCH_BASE_URL`               | 网页搜索（`WebSearch`）服务的 API URL；优先级高于 `config.toml` 的 `[services.moonshot_search] base_url`，未写配置段时也可启用服务。文件中持久化的凭据和自定义 header 不会发送到环境变量指定的端点                                                      | 非空字符串；空白值被忽略                                                                               |
| `DIMI_WEB_SEARCH_API_KEY`                | 网页搜索（`WebSearch`）服务的 API 密钥；设置后同时替换配置中的 API 密钥和 OAuth 凭据                                                                                                                                                                    | 非空字符串；空白值被忽略                                                                               |
| `DIMI_WEB_FETCH_BASE_URL`                | 网页抓取（`FetchURL`）服务的 API URL；优先级高于 `[services.moonshot_fetch] base_url`。文件中持久化的凭据和自定义 header 不会发送到环境变量指定的端点。环境变量和配置都没有指定端点时，已登录用户会先尝试 Kimi OAuth 托管抓取服务，再回退到本地直接请求 | 非空字符串；空白值被忽略                                                                               |
| `DIMI_WEB_FETCH_API_KEY`                 | 网页抓取（`FetchURL`）服务的 API 密钥；设置后同时替换配置中的 API 密钥和 OAuth 凭据                                                                                                                                                                     | 非空字符串；空白值被忽略                                                                               |
| `DIMI_CODE_EXPERIMENTAL_FLAG`            | 在当前进程启用所有已注册的实验功能                                                                                                                                                                                                                      | `1`、`true`、`yes`、`on`                                                                               |
| `DIMI_SHELL_PATH`                        | Windows 上覆盖 Git Bash 路径（自动探测失败时使用）                                                                                                                                                                                                      | 绝对路径                                                                                               |
| `DIMI_MODEL_MAX_COMPLETION_TOKENS`       | 单步 LLM 生成 token 的全局硬上限；覆盖 `[model_overrides] max_completion_tokens`                                                                                                                                                                        | 整数                                                                                                   |
| `DIMI_MODEL_TEMPERATURE`                 | 全局采样温度；覆盖 `[model_overrides] temperature`                                                                                                                                                                                                      | 数字，如 `0.3`                                                                                         |
| `DIMI_MODEL_TOP_P`                       | 全局核采样值；覆盖 `[model_overrides] top_p`                                                                                                                                                                                                            | 数字，如 `0.95`                                                                                        |
| `DIMI_MODEL_THINKING_EFFORT`             | 强制所选模型使用指定 thinking effort；覆盖 `[thinking] forced_effort`                                                                                                                                                                                   | 所选模型支持的 effort                                                                                  |
| `DIMI_MODEL_THINKING_KEEP`               | 全局保留 thinking 设置；覆盖 `[model_overrides] thinking_keep`                                                                                                                                                                                          | 供应商支持的值，例如 `all`                                                                             |
| `DIMI_CODE_NO_AUTO_UPDATE`               | 在配置了更新通道的发行版中禁用更新预检。此源码构建默认没有更新通道。同时兼容旧名 `DIMI_CLI_NO_AUTO_UPDATE`                                                                                                                                                   | 真值：`1`/`true`/`yes`/`on`                                                                            |
| `DIMI_DISABLE_CRON`                      | 禁用定时任务工具（`CronCreate` 拒绝新计划，已有任务不触发）                                                                                                                                                                                             | `1` 表示禁用                                                                                           |

## 诊断日志

这组变量控制日志级别和文件滚动，进程启动时读取一次：

| 环境变量                     | 用途                                              | 默认值            |
| ---------------------------- | ------------------------------------------------- | ----------------- |
| `DIMI_LOG_LEVEL`             | 日志级别：`off`、`error`、`warn`、`info`、`debug` | `info`            |
| `DIMI_LOG_GLOBAL_MAX_BYTES`  | 全局日志文件单个最大字节数                        | `6291456`（6 MB） |
| `DIMI_LOG_GLOBAL_FILES`      | 全局日志文件保留份数                              | `5`               |
| `DIMI_LOG_SESSION_MAX_BYTES` | 会话级日志文件单个最大字节数                      | `5242880`（5 MB） |
| `DIMI_LOG_SESSION_FILES`     | 会话级日志文件保留份数                            | `3`               |

## 系统环境变量

CLI 还会读取一些标准系统变量来检测运行环境，不会修改它们：

- `HOME`：解析默认数据路径
- `VISUAL`、`EDITOR`：外部编辑器命令（`VISUAL` 优先）
- `PATH`：定位 `rg`、`fd`、`fdfind`、`git` 等依赖；在 Windows 上，Git Bash 探测会检查 `PATH` 中找到的每个 `git.exe`，包括 Scoop 等包管理器提供的 shim
- `NO_COLOR`、`FORCE_COLOR`：控制颜色输出（遵循 [no-color.org](https://no-color.org) 约定）
- `CI`：非空且非 `"0"` 时关闭主题检测，回退深色主题
- `TERM_PROGRAM`、`TERM`、`TMUX`：检测终端特性和通知支持
- `DISPLAY`、`WAYLAND_DISPLAY`、`XDG_SESSION_TYPE`：检测 Linux 图形会话（用于剪贴板和图片功能）
- `WSL_DISTRO_NAME`、`WSLENV`：检测 WSL，用于剪贴板 PowerShell 桥接
- `LOCALAPPDATA`：Windows 上探测 Git Bash 安装路径时作为 fallback 使用

## HTTP 代理

Kimi Code 会遵循标准代理环境变量，让所有出网流量——模型 API 调用、MCP 服务、网络工具、遥测、登录、更新检查——都走代理：

- `HTTP_PROXY` / `http_proxy`：用于 `http://` 请求的代理
- `HTTPS_PROXY` / `https_proxy`：用于 `https://` 请求的代理
- `ALL_PROXY` / `all_proxy`：当对应 scheme 的变量未设置时使用的兜底代理；SOCKS 代理通常设在这里
- `NO_PROXY` / `no_proxy`：以逗号分隔的、绕过代理的主机列表

同时支持 HTTP(S) 代理和 SOCKS 代理。SOCKS 代理通过 scheme 识别——`socks5://`、`socks5h://`、`socks4://` 或 `socks://`（`socks5://` 的别名）——通常设在 `ALL_PROXY`（Clash、V2RayN 等工具使用的形式）。对 HTTP/HTTPS 流量，HTTP(S) 代理优先于 `ALL_PROXY`。

仅当设置了其中任一变量时才启用代理，否则直连。回环地址（`localhost`、`127.0.0.1`、`::1`）始终绕过代理，因此配置了代理后，本地服务（例如 localhost 上的 MCP 服务）仍能正常工作——你也可以把自己的内网主机加入 `NO_PROXY` 一并放行。

以 Node 子进程运行的 stdio MCP 服务，在其 Node 版本支持 `NODE_USE_ENV_PROXY` 时（Node ≥ 22.21 或 ≥ 24.5）会自动遵循 `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY`；SOCKS 代理仅作用于 Kimi Code 自身的流量。

## 下一步

- [配置覆盖](./overrides.md) — 环境变量、CLI 选项、配置文件的优先级关系
- [数据路径](./data-locations.md) — `DIMI_CODE_HOME` 影响的完整目录结构
- [供应商与模型](./providers.md) — 内置供应商、登录方式与动态目录
