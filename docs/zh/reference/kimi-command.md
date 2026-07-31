# kimi 命令

`kimi` 是 Kimi Code CLI 的主命令，用于在终端中启动一次交互式会话。不带任何参数运行时，它会在当前工作目录下开启一个新会话；配合不同的 flag，可以续上历史会话、跳过审批、从 Plan 模式开始，或者指定自定义的 Skills 目录。

```sh
kimi [options]
kimi <subcommand> [options]
```

## 主命令选项

所有 flag 都是可选的，直接运行 `kimi` 即可进入交互式会话：

| 选项                       | 简写 | 说明                                                                                                                      |
| -------------------------- | ---- | ------------------------------------------------------------------------------------------------------------------------- |
| `--version`                | `-V` | 打印版本号并退出                                                                                                          |
| `--help`                   | `-h` | 显示帮助信息并退出                                                                                                        |
| `--session [id]`           | `-S` | 恢复一个会话。带 ID 时直接打开指定会话；不带 ID 时进入交互式选择器                                                        |
| `--continue`               | `-c` | 继续当前工作目录下最近一次的会话，无需手动指定 ID                                                                         |
| `--model <model>`          | `-m` | 为本次启动选择 `<provider>/<model>`。省略时新会话使用 `default_provider` 与 `default_model`                               |
| `--prompt <prompt>`        | `-p` | 非交互执行单次 prompt，并把 Assistant 输出流式写到 stdout。该模式不会打开 TUI                                             |
| `--output-format <format>` |      | 设置非交互输出格式，支持 `text` 与 `stream-json`。仅可与 `--prompt` 一起使用，默认 `text`                                 |
| `--yolo`                   | `-y` | 自动批准普通工具调用，跳过审批请求                                                                                        |
| `--auto`                   |      | 以 auto 权限模式启动；工具审批自动处理，Agent 不会向用户提问                                                              |
| `--plan`                   |      | 以 Plan 模式启动新会话，AI 会优先使用只读工具进行探索和规划                                                               |
| `--skills-dir <dir>`       |      | 从指定目录加载 Skills，替换自动发现的用户和项目目录。可重复传入                                                           |
| `--agent <name>`           |      | 以指定 Agent 作为主 Agent 启动新会话。不能与 `--session`/`--continue` 同时使用                                            |
| `--agent-file <path>`      |      | 从 Markdown 文件加载自定义 Agent 并为新会话选中它。不可重复传入，也不能与 `--agent`、`--session` 或 `--continue` 同时使用 |
| `--add-dir <dir>`          |      | 为本次会话添加额外的工作目录。相对路径按当前工作目录解析。可重复传入                                                      |

`-r` / `--resume` 是 `--session` 的隐藏别名；`--yes` 和 `--auto-approve` 是 `--yolo` 的隐藏别名，在帮助信息中不显示。

::: warning 注意
`--yolo` 会跳过普通工具调用的人工确认，包括文件写入和 Shell 命令执行，请只在受信任的工作目录下使用。Plan 模式的退出审批不会被 `--yolo` 跳过；Plan 模式下的 `Bash` 按普通放行规则处理。
:::

### flag 冲突规则

以下组合会在启动时被拒绝：

- `--continue` 与 `--session` 互斥——两者都表示"恢复历史会话"
- `--yolo` 和 `--auto` 互斥——两种权限模式互斥
- `--prompt` 不能与 `--yolo`、`--auto` 或 `--plan` 同时使用——非交互模式固定使用 `auto` 权限
- `--output-format` 只能与 `--prompt` 一起使用

恢复会话时，可以通过 `--auto`、`--yolo` 或 `--plan` 覆盖原会话保存的权限或计划模式。例如，`kimi --continue --auto` 会恢复最近会话并切换到 auto 权限模式。

## 典型用法

直接运行开启新会话：

```sh
kimi
```

从上次中断的地方继续（自动找到当前目录最近的会话）：

```sh
kimi --continue
```

从历史会话列表中挑选，或直接指定已知 ID：

```sh
kimi --session
kimi --session 01HZ...XYZ
```

跳过审批确认，适合已知安全的批处理任务：

```sh
kimi --yolo
```

让 Agent 自行处理一切，不再向用户提问：

```sh
kimi --auto
```

先阅读代码、产出实现计划，而不是立刻动手修改文件：

```sh
kimi --plan
```

### 自定义 Skills 目录

有两种方式指定 Skills 目录，语义不同：

- **`--skills-dir <dir>`**（CLI flag）：**替换**自动发现的用户和项目目录，仅对本次启动生效。可重复传入以叠加多个目录：

  ```sh
  kimi --skills-dir /path/to/team-skills --skills-dir ./local-skills
  ```

- **`extra_skill_dirs`**（`config.toml`）：**叠加**到自动发现的目录之上，长期生效，适合配置团队共享 Skills。详见 [Agent Skills](../customization/skills.md)。

### 自定义 Agent

`--agent` 和 `--agent-file` 用于选择驱动新会话的 Agent，在 print 模式（`kimi -p`）和交互式 TUI 中均可使用：

```sh
kimi --agent reviewer
kimi -p --agent reviewer "审查这个分支上的改动"
```

`--agent-file` 以最高优先级注册单个 Agent 文件（仅本次启动）并选中它；该 flag 不可重复传入，`--agent` 与 `--agent-file` 互斥。两个 flag 都仅在新建会话时有效——都不能与 `--session`/`--continue` 组合，因为 Agent 在会话创建时绑定，恢复会话时会自动还原已绑定的 Agent。选择在会话首次绑定后即固定，之后不可切换；在 TUI 中，这些 flag 只绑定启动时的会话，之后在同一进程内新建的会话（例如通过 `/new`）使用默认 Agent。Agent 文件格式与发现目录详见 [Agent 与子 Agent](../customization/agents.md#自定义-agent)。

## 非交互执行

在脚本或 CI 中运行单次 prompt 时，使用 `-p`：

```sh
kimi -p "Summarize the current repository status"
```

输出采用 transcript 样式：thinking 内容和 Assistant 正文都以 `• ` 开头，换行后两个空格缩进。Assistant 正文输出到 stdout；thinking、工具进度和"恢复会话"提示输出到 stderr。`-p` 模式不会请求人工审批，普通工具调用按 `auto` 权限策略处理，静态 deny 规则仍然生效。

临时切换模型：

```sh
kimi -m kimi-coding/kimi-for-coding -p "Explain the latest diff"
```

需要结构化读取输出时，使用 `stream-json` 格式——stdout 每行都是一个 JSON 对象：

```sh
kimi -p "List changed files" --output-format stream-json
```

`stream-json` 模式下，普通回复输出 Assistant 消息；模型调用工具时，先输出带 `tool_calls` 的 Assistant 消息，再输出对应的 Tool 消息，最后继续输出后续 Assistant 消息。thinking 内容不会写入 JSONL；工具进度和恢复会话提示仍写到 stderr。

## 子命令

`kimi` 提供以下子命令：`login` 与 `logout`（供应商凭证）、`provider`（供应商与模型目录）、`acp`（ACP IDE 模式）、`web`（本地 REST/WebSocket/web 服务）、`remote`（通过加密 relay 连接原生客户端）、`doctor`（校验配置文件）、`export`（导出会话）、`upgrade`（检查更新）。

### `kimi login`

通过 OAuth 或 API 密钥连接一个内置或 `models.json` LLM 供应商。省略供应商时交互选择；供应商同时支持两种方式时，省略 `--method` 可交互选择登录方式。云供应商可能继续询问凭据链、account、project 或 location。

```sh
kimi login
kimi login openai-codex --method oauth
kimi login anthropic --method api-key
```

OAuth 登录会打印供应商的授权 URL 或设备码并等待完成。API 密钥登录通过终端隐藏输入。保存的凭证写入 `auth.json`，下次启动时自动加载。

| 选项                | 说明                                         |
| ------------------- | -------------------------------------------- |
| `[provider]`        | 内置或 `models.json` 目录中的供应商 ID；省略时交互选择 |
| `--method <method>` | `oauth` 或 `api-key`；需要跳过交互选择时指定 |

### `kimi logout`

删除一个供应商已保存的凭证：

```sh
kimi logout anthropic
```

通过 shell 环境变量提供的 API 密钥会继续生效，直到取消设置。

### `kimi acp`

把 Kimi Code CLI 切换到 ACP（Agent Client Protocol）模式，在标准输入/输出上以 JSON-RPC 形式与 IDE 对话，让编辑器直接驱动 kimi 的会话和工具调用。通常不需要手动运行——IDE 会把它作为子进程入口启动。配置方式见[在 IDE 中使用](../guides/ides.md)，技术细节见 [kimi acp 参考](./kimi-acp.md)。

```sh
kimi acp
```

### `kimi web`

在当前终端前台运行本地 Kimi 服务 —— 同一个进程同时挂载 REST + WebSocket API 与 web UI —— 并在服务就绪后用默认浏览器打开 web UI。命令会一直挂在终端，直到收到 `SIGINT` / `SIGTERM`（如 `Ctrl-C`）时干净退出。

服务运行时，`GET /openapi.json` 会返回 REST OpenAPI 文档，`GET /asyncapi.json` 会返回本地 WebSocket 协议的 AsyncAPI 文档。

```sh
kimi web                 # 前台运行服务并打开浏览器
kimi web --no-open       # 不打开浏览器
kimi web --port 58628    # 指定绑定端口
```

同一 home 目录下可以同时运行多个实例：每个实例注册到 `~/.kimi-code/server/instances/`，端口被占用时自动 +1 重试（58628、58629……）。

| 选项                       | 说明                                                                                                                 |
| -------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `--port <port>`            | 绑定端口；默认 `58627`；被占用时自动 +1 重试                                                                         |
| `--host [host]`            | 绑定地址；缺省 `127.0.0.1`（仅本机），裸 `--host` 绑 `0.0.0.0`（所有网卡）                                           |
| `--allowed-host <host...>` | DNS 重绑定检查额外允许的 Host 头，可重复或逗号分隔                                                                   |
| `--log-level <level>`      | 按所选级别开启服务日志；默认不输出                                                                                   |
| `--debug-endpoints`        | 挂载 `/api/v1/debug/*` 调试路由（默认关闭）                                                                          |
| `--dangerous-bypass-auth`  | 关闭所有 REST 与 WebSocket 路由的 bearer token 鉴权，使 web UI 无需 token 即可连接；仅用于可信网络或自有鉴权代理之后 |
| `--no-open`                | 就绪后不自动打开浏览器                                                                                               |

`kimi web` 默认只绑定本机 loopback 地址，并在启动横幅中打印 bearer token；web UI 通过 URL 的 `#token=` 片段自动完成鉴权。

::: info 提示
`kimi server` 命令树已废弃：任何 `kimi server …` 调用（含全部旧子命令）只会打印弃用提示并以退出码 1 结束，请改用 `kimi web`。唯一的例外是 `kimi server kill`，它仍然可用，仅用于停止 0.28.0 之前版本启动的服务。该提示将在 Kimi Code 下个大版本移除。
:::

::: danger 警告
`--dangerous-bypass-auth` 会彻底关闭鉴权。任何能访问该端口的人都能完全控制你的会话、文件系统和 shell。请仅在可信网络或自有鉴权反向代理之后使用，用完后按 `Ctrl+C` 停止服务。
:::

#### `kimi server kill`

已废弃——仅用于停止 0.28.0 之前的 Kimi Code 版本启动的服务。那些版本可能在后台遗留服务进程，记录在 legacy 单实例锁文件 `~/.kimi-code/server/lock` 中；该命令先请求 `POST /api/v1/shutdown` 优雅退出，再对锁中记录的 pid 发 SIGTERM、必要时升级为 SIGKILL，并在确认进程退出后删除锁文件。`kimi web` 启动的服务在前台运行，直接用 `Ctrl+C` 停止即可。

#### `kimi web rotate-token`

生成新的持久化 bearer token（写入 `~/.kimi-code/server.token`），旧 token 立即失效。token 是整个 home 目录共享的，所有运行中的实例会在下一次鉴权校验时自动换用新 token，无需重启。

### `kimi remote`

通过端到端加密的 relay 将当前 Runtime 连接到原生客户端。relay 只转发密文，无法读取提示词或回复。命令会优先复用正在运行的本地 server；没有可用实例时会启动一个。客户端连接期间需保持命令运行；按 `Ctrl-C` 停止远程访问。

```sh
kimi remote start
kimi remote pair --relay wss://relay.example.test --name "Workstation"
```

`start` 是默认动作，只会让已配对设备重新连接，不会创建或显示配对凭据。添加新设备时运行一次 `pair`；它会启动 bridge，并打印二维码及一条 10 分钟有效、只能使用一次的配对 URI。

| 选项 | 说明 |
| --- | --- |
| `--relay <url>` | Relay WebSocket 地址；省略时使用内置 relay |
| `--server <url>` | 已有本地 server 地址；省略时复用或启动本地 server |
| `--name <name>` | 配对设备上显示的 Runtime 名称；默认使用机器主机名 |

配对后，Android 客户端与 Runtime 都会保存设备身份。之后运行 `remote start` 时会自动重连，无需再次扫码。停止 remote 不会撤销已配对设备。

### `kimi doctor`

校验 `config.toml` 和 `tui.toml`，不会启动 TUI，也不会修改任一文件。默认检查 `KIMI_CODE_HOME` 下的文件；未设置该环境变量时检查 `~/.kimi-code`。默认路径缺失时会显示为跳过，因为内置默认值仍可生效。

```sh
kimi doctor
```

| 命令                        | 说明                                                         |
| --------------------------- | ------------------------------------------------------------ |
| `kimi doctor`               | 校验默认 `config.toml` 和 `tui.toml`                         |
| `kimi doctor config [path]` | 只校验 `config.toml`；传入 `path` 时使用该文件而不是默认文件 |
| `kimi doctor tui [path]`    | 只校验 `tui.toml`；传入 `path` 时使用该文件而不是默认文件    |

显式传入路径时，文件必须存在。所有被检查的文件都有效或被跳过时，退出码为 `0`；任何指定文件缺失或配置无效时，退出码为 `1`。

```sh
# 检查默认配置文件
kimi doctor

# 只检查默认运行时配置
kimi doctor config

# 替换正式 TUI 配置前，先检查候选文件
kimi doctor tui ./tui.toml
```

### `kimi export`

把一个会话打包成 ZIP 文件，便于分享、归档或提交问题反馈。

```sh
kimi export [sessionId] [options]
```

| 参数 / 选项               | 简写 | 说明                                                                    |
| ------------------------- | ---- | ----------------------------------------------------------------------- |
| `sessionId`               |      | 要导出的会话 ID。省略时自动选择当前工作目录下最近一次的会话，并要求确认 |
| `--output <path>`         | `-o` | 输出 ZIP 文件路径。省略时写入当前目录下的默认文件名                     |
| `--yes`                   | `-y` | 跳过默认会话的确认提示，直接导出                                        |
| `--no-include-global-log` |      | 不打包全局诊断日志。默认包含                                            |

导出包含目标会话目录内的所有文件。全局诊断日志（`~/.kimi-code/logs/kimi-code.log`）默认包含，因为它可能含有其他会话或项目的事件；不想分享时加 `--no-include-global-log`。

```sh
# 导出当前工作目录最近一次会话，跳过确认
kimi export -y

# 导出指定会话到自定义路径
kimi export 01HZ...XYZ -o ./bug-report.zip

# 排除全局诊断日志
kimi export 01HZ...XYZ -o ./bug-report.zip --no-include-global-log
```

### `kimi upgrade`

本源码构建未配置发布通道。`kimi upgrade` 会提示自动升级不可用后退出；`kimi update` 是它的别名。

```sh
kimi upgrade
```

它绝不会回退到上游 Kimi Code 安装源。请用 `git pull --ff-only` 更新此 checkout，再运行 `vp install` 和 `vp run dev:cli`。

### `kimi vis`

在浏览器中启动会话可视化工具，直观查看一次会话的全过程。命令会启动一个指向本地会话的进程内服务器，打印访问地址并打开浏览器，持续运行直到你按下 `Ctrl-C`。

```sh
kimi vis [sessionId] [options]
```

| 参数 / 选项       | 说明                                                       |
| ----------------- | ---------------------------------------------------------- |
| `sessionId`       | 直接打开指定会话的可视化页面。省略时打开列出所有会话的首页 |
| `--port <number>` | 绑定的端口。默认自动挑选一个空闲端口                       |
| `--host <host>`   | 绑定的主机。默认 `127.0.0.1`                               |
| `--no-open`       | 不自动打开浏览器，仅打印访问地址                           |

```sh
# 启动可视化工具并在浏览器中打开首页
kimi vis

# 直接打开指定会话
kimi vis 01HZ...XYZ

# 绑定固定主机和端口且不打开浏览器（例如在远程主机上）
kimi vis --host 0.0.0.0 --port 8123 --no-open
```

### `kimi provider`

查看供应商和模型、刷新动态目录，并管理用户拥有的 `models.json` 供应商层。内置和 SDK 供应商定义仍归运行时所有；同 ID 的自定义定义是覆盖层，不是替换目录。

```sh
kimi provider list [--json]
kimi provider models [providerId]
kimi provider refresh
kimi provider add <id> [options]
kimi provider update <id> [options]
kimi provider remove <id>
kimi provider model add <providerId> <modelId> [options]
kimi provider model update <providerId> <modelId> [options]
kimi provider model remove <providerId> <modelId>
```

#### `kimi provider list`

每行打印一个内置和已配置供应商、连接状态与当前可用模型数。加 `--json` 输出供应商认证状态与模型元数据。

```sh
kimi provider list
kimi provider list --json
```

#### `kimi provider models [providerId]`

列出已认证供应商当前可用的模型。可传供应商 ID 缩小范围。每行包含规范的 `<provider>/<model>` 引用、上下文窗口与相关能力。

```sh
kimi provider models
kimi provider models openai-codex
```

#### `kimi provider refresh`

刷新所有已认证供应商的远程模型端点。失败会按供应商报告，其他成功目录仍会持久化。

```sh
kimi provider refresh
```

#### `kimi provider add` 与 `update`

创建或更新自定义供应商。`--from <path>` 导入单个供应商对象或 `{ "providers": { … } }` JSONC 文档，并选择请求的 ID。行内新建供应商时必须提供 `--base-url`、`--model`、`--context-window` 和 `--max-tokens`。`--api` 默认是 `openai-completions`；`--api-key-env` 会保存环境变量模板，而不是密钥。

```sh
kimi provider add example-gateway --from ./models.json
kimi provider add example-gateway \
  --base-url https://api.example.test/v1 --model example-chat \
  --context-window 128000 --max-tokens 8192 \
  --api-key-env EXAMPLE_GATEWAY_API_KEY --thinking --image
kimi provider update example-gateway --model example-chat --max-tokens 16384
```

除了必填字段，行内选项还有 `--name`、`--model-name`、`--api`、`--api-key-env`、`--thinking` 和 `--image`。供应商 header、`compat`、OAuth 设置、模型覆盖、单模型 base URL 或多个模型请使用文件；完整 JSONC 形状见[供应商配置参考](../configuration/providers.md#使用-modelsjson-添加或覆盖供应商)。

#### `kimi provider remove`

移除用户拥有的供应商定义。若它是内置供应商的自定义覆盖层，底层内置供应商会重新出现。移除独立自定义供应商还会移除其保存的凭据。

```sh
kimi provider remove example-gateway
```

#### `kimi provider model`

添加、更新或移除自定义供应商声明的模型。添加新模型必须提供 `--context-window` 与 `--max-tokens`；`--name`、`--thinking` 和 `--image` 可选。

```sh
kimi provider model add example-gateway example-reasoner \
  --context-window 128000 --max-tokens 8192 --thinking
kimi provider model update example-gateway example-reasoner --image
kimi provider model remove example-gateway example-reasoner
```

## 下一步

- [斜杠命令](./slash-commands.md) — 交互式 TUI 内的控制命令速查
- [配置文件](../configuration/config-files.md) — `default_model`、权限模式等启动参数的持久化配置
- [Agent Skills](../customization/skills.md) — `--skills-dir` 加载的 Skill 文件格式
- [Agent 与子 Agent](../customization/agents.md) — 内置子 Agent、自定义 Agent 文件与通过 `--agent` 选择主 Agent
