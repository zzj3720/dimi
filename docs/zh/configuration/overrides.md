# 配置覆盖

Kimi Code CLI 把长期偏好、供应商凭证和一次性运行参数放在不同通道：

- **`config.toml`** 保存默认供应商/模型、thinking 设置、循环限制等偏好，不保存供应商凭证。
- **`auth.json`** 保存通过 `kimi login` 写入的凭证。
- **环境变量**可提供供应商 API 密钥、选择供应商/模型组合、迁移数据目录，或覆盖文档明确列出的运行参数。
- **命令行选项**仅影响本次启动。

## 供应商与模型选择

新会话按以下顺序解析供应商/模型：

1. `-m, --model <provider>/<model>`
2. 同时设置的 `KIMI_MODEL_PROVIDER` 与 `KIMI_MODEL_NAME`
3. `config.toml` 中的 `default_provider` 与 `default_model`

所选组合必须存在于运行时模型目录。供应商支持远程发现时，`kimi provider refresh` 会更新目录，缓存结果写入 `models-store.json`。

CLI 当前只读取一个用户级配置文件，没有项目级配置机制。需要隔离配置时，将 `KIMI_CODE_HOME` 指向不同的数据目录。

## 供应商凭证

供应商认证独立于模型选择，按以下顺序解析：

1. 通过 `kimi login <provider>` 保存的凭证
2. 供应商支持 API 密钥时，对应的标准环境变量

用 `kimi logout <provider>` 删除已保存凭证。shell 中导出的环境变量会继续生效，直到取消设置或退出 shell。

例如，下列命令使用一次性的 Anthropic 密钥，不修改任何本机文件：

```sh
ANTHROPIC_API_KEY="YOUR_API_KEY" kimi -m anthropic/claude-sonnet-4-6
```

OpenAI Codex 使用 OAuth；Kimi Code 和 xAI 同时支持 OAuth 与 API 密钥。内置供应商及登录方式见[平台与模型](./providers.md)。

## 其他运行参数

对于 Plan 模式、权限模式、Skills 目录等普通参数，命令行选项在本次启动中覆盖 `config.toml`。文档明确列出的环境变量可覆盖对应配置字段；例如 `KIMI_CODE_BACKGROUND_MAX_RUNNING_TASKS` 覆盖 `[task].max_running_tasks`。

| 选项                             | 作用                                        |
| -------------------------------- | ------------------------------------------- |
| `-S, --session [id]`             | 恢复指定会话；不带 ID 时进入交互选择        |
| `-c, --continue`                 | 续上当前目录最近一次会话                    |
| `-y, --yolo`                     | 自动批准普通工具调用                        |
| `--auto`                         | 以 auto 权限模式启动                        |
| `--plan`                         | 以 Plan 模式启动                            |
| `-m, --model <provider>/<model>` | 选择运行时目录中的模型                      |
| `-p, --prompt <prompt>`          | 非交互执行一条提示词                        |
| `--output-format <format>`       | 配合 `-p` 使用 `text` 或 `stream-json` 输出 |
| `--skills-dir <dir>`             | 本次启动替换自动发现的 Skills 目录          |

## 典型场景

隔离所有本机状态：

```sh
KIMI_CODE_HOME="$PWD/.kimi-sandbox" kimi
```

不修改 `config.toml`，临时选择 OpenAI：

```sh
OPENAI_API_KEY="YOUR_API_KEY" \
  KIMI_MODEL_PROVIDER="openai" \
  KIMI_MODEL_NAME="gpt-5.4" \
  kimi
```

本次启动进入 Plan 模式：

```sh
kimi --plan
```

## 下一步

- [配置文件](./config-files.md) — 长期偏好字段
- [环境变量](./env-vars.md) — 供应商密钥与运行时覆盖
- [数据路径](./data-locations.md) — 凭证与模型缓存文件
