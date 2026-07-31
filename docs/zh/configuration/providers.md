# 供应商与模型

Kimi Code CLI 内置供应商目录。连接账号或 API 密钥后，运行时会提供该供应商当前可用的模型。供应商、认证、模型发现、请求头和流式响应现在共用同一套生命周期；`config.toml` 不再定义自定义 `[providers]` 或 `[models]` 表。

## 连接供应商

在终端中使用 `kimi login`，或在 TUI 中使用 `/login`。不带参数时，TUI 会先询问使用账号还是 API 密钥，再只显示支持该认证方式的供应商。使用 `/login <provider>` 可以直接进入指定供应商；如果它支持多种认证方式，TUI 会继续询问使用哪一种。认证完成后，从该供应商当前的模型目录中选择模型。

```sh
# 订阅账号登录
kimi login kimi-coding --method oauth
kimi login openai-codex --method oauth
kimi login xai --method oauth

# 保存 API 密钥
kimi login anthropic --method api-key
kimi login openai --method api-key
```

通过 `kimi login` 输入的 API 密钥保存在 `$KIMI_CODE_HOME/auth.json`。也可以设置供应商对应的环境变量；已保存凭据的优先级更高。OAuth 凭据剩余有效期不超过五分钟时会自动刷新。

在 TUI 中，`/provider` 打开带连接状态和凭据来源的供应商列表，并为选中的供应商启动登录。`/provider <provider>` 直接连接指定供应商，`/provider refresh` 刷新所有动态模型目录，`/model` 打开所有供应商当前可用的模型，`/logout` 断开供应商。

## 内置供应商

目录包含以下供应商：

| 供应商 ID      | 服务                 | 认证方式                             |
| -------------- | -------------------- | ------------------------------------ |
| `kimi-coding`  | Kimi Code 订阅       | OAuth 或 `KIMI_API_KEY`              |
| `openai-codex` | ChatGPT Codex        | OAuth                                |
| `xai`          | Grok / xAI           | OAuth 或 `XAI_API_KEY`               |
| `openai`       | OpenAI API           | `OPENAI_API_KEY`                     |
| `anthropic`    | Anthropic API        | `ANTHROPIC_API_KEY`                  |
| `openrouter`   | OpenRouter           | `OPENROUTER_API_KEY`                 |
| `deepseek`     | DeepSeek             | `DEEPSEEK_API_KEY`                   |
| `groq`         | Groq                 | `GROQ_API_KEY`                       |
| `mistral`      | Mistral              | `MISTRAL_API_KEY`                    |
| `together`     | Together AI          | `TOGETHER_API_KEY`                   |
| `cerebras`     | Cerebras             | `CEREBRAS_API_KEY`                   |
| `fireworks`    | Fireworks AI         | `FIREWORKS_API_KEY`                  |
| `zai`          | Z.AI                 | `ZAI_API_KEY`                        |
| `qwen`         | 阿里云百炼           | `DASHSCOPE_API_KEY`                  |
| `moonshot`     | Moonshot AI 开放平台 | `MOONSHOT_API_KEY` 或 `KIMI_API_KEY` |

`kimi-coding` 对应 `https://api.kimi.com/coding/v1` 上的 Kimi Code 平台。独立的 `moonshot` 供应商用于 Moonshot AI 开放平台账号。

## 列出并刷新模型

模型使用 `provider/model` 标识，例如 `anthropic/claude-sonnet-4-6`。可以查看当前目录：

```sh
kimi provider list
kimi provider list --json
kimi provider models
kimi provider models anthropic
```

拥有模型列表端点的供应商会动态刷新目录。运行时把最近一次成功的目录保存在 `$KIMI_CODE_HOME/models-store.json`；上游提供 ETag 时会发送条件请求；没有动态结果时，使用内置基线模型。

供应商新增或删除模型后，可以强制刷新：

```sh
kimi provider refresh
```

长期运行的 Web server 也会在后台刷新已连接的动态供应商。刷新周期通过 [`config.toml`](./config-files.md#model-catalog) 中的 `[model_catalog]` 配置。

## 选择默认模型

`config.toml` 保存选择和请求偏好，不保存供应商定义：

```toml
default_provider = "anthropic"
default_model = "claude-sonnet-4-6"

[thinking]
enabled = true
effort = "high"

[model_overrides]
temperature = 0.3
max_completion_tokens = 8192
```

`--model` 参数和 `/model` 选择器使用同样的 `provider/model` 标识。在 TUI 中持久化选择时，会写入 `default_provider` 和 `default_model`。

## 断开供应商

移除已保存凭据：

```sh
kimi logout anthropic
```

通过环境变量提供的凭据在取消设置前仍然可用。退出一个供应商不会影响其他供应商的会话或凭据。

## 从供应商表迁移

原有 `[providers]`、`[models]`、目录导入和自定义 registry 配置不再读取。连接一个内置供应商，再从运行时目录选择模型：

```sh
kimi login <provider> --method api-key
kimi provider models <provider>
```

如果此前把 API 密钥直接写在 `config.toml`，请迁移到对应供应商的环境变量，或通过 `kimi login` 输入。

## 后续阅读

- [配置文件](./config-files.md) —— 默认供应商、模型和运行时请求设置
- [环境变量](./env-vars.md) —— 凭据与模型选择覆盖
- [`kimi` 命令](../reference/kimi-command.md) —— 供应商、登录和退出命令参考
