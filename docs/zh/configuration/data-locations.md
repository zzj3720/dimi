# 数据路径

Dimi CLI 把所有运行时数据——配置文件、会话历史、登录凭据、诊断日志——集中存放在 `~/.dimi/` 下。本页帮你搞清楚每类数据在哪里、用来做什么，以及需要时怎么清理或搬迁。

## 数据根目录

默认数据根是 `~/.dimi/`，在不同平台的实际路径：

- macOS：`/Users/<name>/.dimi`
- Linux：`/home/<name>/.dimi`
- Windows：`C:\Users\<name>\.dimi`

如果你需要把数据目录挪到别处（比如用多个独立环境隔离不同项目的配置），设置 `DIMI_CODE_HOME` 即可：

```sh
export DIMI_CODE_HOME="$HOME/.config/dimi"
```

设置后，配置、会话、日志、OAuth 凭据、Dimi 专属用户级 Skills、全局 `AGENTS.md` 等 **Dimi 数据**都会落到新路径下。`DIMI_CODE_HOME` 的完整说明见[环境变量](./env-vars.md)。

::: tip 提示

**通用 `.agents` 资源**仍放在真实 OS home 下，以便跨工具共享。例如，用户级通用 Skills 仍位于 `~/.agents/skills/`，而 Dimi 专属用户级 Skills 会随 `DIMI_CODE_HOME` 移动到 `$DIMI_CODE_HOME/skills/`。
:::

## 目录结构

```
$DIMI_CODE_HOME  （默认 ~/.dimi）
├── config.toml             # 用户配置
├── tui.toml                # 终端界面偏好（含自动更新开关）
├── auth.json               # 已保存的供应商 OAuth/API 密钥凭证（文件 0600）
├── models.json             # 用户拥有的 JSONC 供应商定义与覆盖层（文件 0600）
├── models-store.json       # 动态供应商模型目录缓存
├── AGENTS.md               # 全局 Dimi 专属 Agent 指令（可选）
├── mcp.json                # 用户级 MCP server 声明（可选）
├── skills/                 # Dimi 专属用户级 Skills（可选）
├── plugins/
│   ├── installed.json      # 已安装 plugin 记录与启用状态
│   └── managed/            # zip/本地路径安装的 plugin 副本
├── workspaces.json         # 工作区目录
├── credentials/
│   └── mcp/                # MCP server OAuth 凭据
│       └── <key>-<suffix>.json
├── sessions/               # 会话数据（详见下文）
│   └── <workspaceId>/<sessionId>/
├── cron/                   # 按工作区分组的定时任务
│   └── <workspaceId>/<taskId>.json
├── blobs/                  # 运行时托管的二进制与文档 blob
├── store/                  # 运行时托管的 store
├── cache/                  # 运行时缓存
├── bin/
│   ├── rg                  # Grep 使用的托管 ripgrep 二进制（Windows 为 rg.exe）
│   └── fd                  # 文件引用使用的托管 fd 二进制（Windows 为 fd.exe）
├── logs/
│   └── dimi.log       # 全局诊断日志
├── updates/
│   ├── latest.json
│   ├── install.json
│   ├── install.lock
│   └── rollout.log
└── user-history/
    └── <md5(workDir)>.jsonl
```

## 各类文件说明

数据根下的顶层文件各有用途，大部分由 CLI 自动管理：

- **`config.toml`**：主运行时配置，存放默认供应商/模型、循环控制等偏好，不保存供应商凭证。详见[配置文件](./config-files.md)。
- **`tui.toml`**：终端界面客户端偏好，包括保存的 `[upgrade].auto_install` 偏好。此源码构建没有配置更新通道；除非发行方提供通道，该设置不生效。
- **`auth.json`**：通过 `vp run dev:cli -- login` 保存的供应商 OAuth token 与 API 密钥。运行时以 `0o600` 权限原子写入；请只通过 `vp run dev:cli -- login` 和 `vp run dev:cli -- logout` 修改。
- **`models.json`**：用户拥有的 JSONC 供应商层（`{ "providers": { … } }`）。它可添加完整供应商，或覆盖内置/SDK 供应商，并支持注释和尾随逗号。运行时会在模型选择和读取目录前重新加载它。`apiKey` 请优先使用 `$ENVIRONMENT_VARIABLE` 或 `!command`，不要把生产密钥写成字面量。详见[供应商与模型](./providers.md#使用-modelsjson-添加或覆盖供应商)。
- **`models-store.json`**：已认证供应商返回的动态模型目录缓存，包含新鲜度与 ETag 元数据。`vp run dev:cli -- provider refresh` 会更新它；离线使用要求缓存元数据不早于随包目录。
- **`AGENTS.md`**：全局 Dimi 专属 Agent 指令。该文件会随 `DIMI_CODE_HOME` 移动；跨工具通用指令仍可放在 `~/.agents/AGENTS.md`。
- **`mcp.json`**：用户级 MCP server 声明，启动时与项目内的 `.dimi/mcp.json` 合并加载。详见 [MCP](../customization/mcp.md)。
- **`skills/`**：Dimi 专属用户级 Skills。该目录会随 `DIMI_CODE_HOME` 移动；跨工具通用 Skills 仍可放在 `~/.agents/skills/`。详见 [Agent Skills](../customization/skills.md)。
- **`plugins/installed.json`**：记录已安装的 plugin、每个 plugin 的启用状态，以及通过 `/plugins` 或 `/plugins mcp disable|enable` 修改的 MCP server 能力状态。本地路径和 zip URL 安装的文件会复制到 `plugins/managed/<id>/`。详见 [Plugins](../customization/plugins.md)。
- **`workspaces.json`**：把稳定的 workspace ID 映射到工作区根目录和显示名称。会话目录使用该 ID，不再使用路径派生的桶名。
- **`credentials/mcp/`**：MCP server OAuth 凭据。供应商凭证不再使用该目录。

## 会话数据

每个会话存放在 `sessions/<workspaceId>/<sessionId>/` 下。运行时直接枚举这些目录并读取 `state.json`，不再维护独立的会话索引。

会话目录内部包含：

- **`state.json`**：会话标题、`lastPrompt`、创建/更新时间、`forkedFrom` 等元数据。
- **`agents/main/wire.jsonl`**：主 Agent 的完整通信记录，用于会话恢复和回放。
- **`agents/<agentId>/wire.jsonl`**：每个子 Agent 都有独立的事件日志。
- **`agents/<agentId>/plans/<id>.md`**：Plan 模式下可编辑的工作文件。
- **`agents/<agentId>/plan/<id>/v<N>.md`**：提交后的不可变计划版本，由 wire 日志引用。
- **`agents/<agentId>/tasks/<taskId>.json`**：后台任务和异步工具任务的持久状态。
- **`agents/<agentId>/tasks/<taskId>/output.log`**：持久化的任务输出。
- **`logs/dimi.log`**：该会话的诊断日志，只有发生诊断事件时才存在。

定时任务单独存放在 `cron/<workspaceId>/<taskId>.json`，不依赖某个会话处于运行状态。详见[定时任务](../reference/tools.md#定时任务)。

## 内置工具缓存

`Grep` 工具第一次需要 ripgrep 时，CLI 可自动下载 `rg` 并缓存到 `bin/rg`（Windows 为 `bin/rg.exe`）。终端界面的文件引用补全使用 `fd`；需要时 CLI 会在后台自动下载并缓存到 `bin/fd`（Windows 为 `bin/fd.exe`）。之后的运行会直接复用缓存的二进制。`rg` 优先使用系统 `PATH`，再使用缓存；`fd` 优先检查托管缓存，再回退到系统 `fd` / `fdfind`。删除 `bin/` 目录会在下次需要时触发重新下载。

## 日志与更新状态

- **`logs/dimi.log`**（全局）：记录启动、登录、导出等跨会话事件。
- **`<sessionDir>/logs/dimi.log`**（会话级）：记录单个会话内的诊断事件。

报 bug 时，优先用 `dimi export` 导出相关会话（详见 [dimi 命令](../reference/dimi-command.md)）；会话日志默认包含在导出包里。不想分享全局日志时加 `--no-include-global-log`。

`updates/` 目录可能遗留其他发行版写入的数据。此源码构建不会读取它，也不会联系更新服务；请使用 `git pull --ff-only` 更新 checkout。

## 输入历史

终端输入历史按工作目录分开保存，路径为 `user-history/<md5(workDir)>.jsonl`。用于在终端界面里用方向键浏览历史提示词。

## 清理数据

删除数据根目录（`~/.dimi/` 或 `DIMI_CODE_HOME` 指定路径）可清除所有运行时数据。只需清理部分内容时：

| 需求                          | 操作                                                                |
| ----------------------------- | ------------------------------------------------------------------- |
| 重置配置                      | 删除 `~/.dimi/config.toml`                                     |
| 重置终端界面偏好              | 删除 `~/.dimi/tui.toml`                                        |
| 清理所有会话                  | 删除 `~/.dimi/sessions/`                                       |
| 清理诊断日志                  | 删除 `~/.dimi/logs/`                                           |
| 清理输入历史                  | 删除 `~/.dimi/user-history/`                                   |
| 强制重新下载托管 `rg` 和 `fd` | 删除 `~/.dimi/bin/`                                            |
| 清除单个供应商登录态          | 运行 `vp run dev:cli -- logout <provider>` 或 TUI `/logout`                      |
| 清除全部已保存的供应商凭证    | 删除 `$DIMI_CODE_HOME/auth.json`                                    |
| 移除自定义供应商和本地覆盖层  | 删除 `$DIMI_CODE_HOME/models.json`                                  |
| 清除动态模型缓存              | 删除 `$DIMI_CODE_HOME/models-store.json`；刷新或启动后会重建        |
| 清除 MCP server OAuth 登录态  | 删除 `credentials/mcp/`（供应商 logout 不会清理 MCP 凭据）          |
| 移除用户级 MCP 声明           | 删除 `$DIMI_CODE_HOME/mcp.json`（默认为 `~/.dimi/mcp.json`）   |
| 清理全局 Dimi 专属 Agent 指令 | 删除 `$DIMI_CODE_HOME/AGENTS.md`（默认为 `~/.dimi/AGENTS.md`） |
| 清理 plugin 安装记录          | 删除 `$DIMI_CODE_HOME/plugins/`（本地 plugin 源码不受影响）         |
| 清空 Dimi 专属用户级 Skills   | 删除 `$DIMI_CODE_HOME/skills/`（默认为 `~/.dimi/skills/`）     |

## 下一步

- [配置文件](./config-files.md) — `config.toml` 各字段的完整说明
- [环境变量](./env-vars.md) — `DIMI_CODE_HOME` 等路径变量的详细用法
