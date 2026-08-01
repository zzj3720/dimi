# 供应商与模型

Kimi Code CLI 使用同一套供应商运行时处理内置服务、本地配置端点和 SDK 注入的供应商。它从同一份模型定义解析所选模型的协议、凭据、请求头、Thinking 模式、上下文限制、流式输出和错误恢复。模型引用始终是 `provider/model`。

内置元数据来自生成的快照，供应商尚未连接时也能提供模型限制和能力；供应商自身的目录返回完整元数据时会覆盖该快照。远程目录只有模型 ID 时，不会臆测上下文窗口、输出限制或 Thinking 能力。

## 连接内置供应商

在 checkout 的终端中使用 `vp run dev:cli -- login`，或在 TUI 中使用 `/login`。省略供应商 ID 可以选择可用供应商和认证方式。`/login <provider>` 直接连接指定供应商，`/provider <provider>` 是同一连接流程的快捷方式。

```sh
# 订阅或 OAuth 登录
vp run dev:cli -- login kimi-coding --method oauth
vp run dev:cli -- login openai-codex --method oauth
vp run dev:cli -- login github-copilot --method oauth

# API 密钥登录；终端会隐藏输入内容
vp run dev:cli -- login anthropic --method api-key
vp run dev:cli -- login openai --method api-key
```

保存的 API 密钥和 OAuth 凭据写入 `$DIMI_CODE_HOME/auth.json`，不会写入 `config.toml`。保存的密钥优先于对应的 shell 环境变量。OAuth token 会在需要时自动刷新。`/logout` 只移除一个供应商保存的凭据，不会取消设置 shell 环境变量。

部分云供应商使用各自的标准凭据链，而不是单个 API 密钥：

- **Amazon Bedrock**：可选择 bearer token、AWS profile 或 AWS 默认凭据链（profile、IAM 密钥、工作负载身份或容器角色）。
- **Vertex**：可选择 Google Cloud API 密钥、Application Default Credentials（ADC）或 service account 凭据文件，然后填写 Google Cloud project 和 location。
- **Cloudflare Workers AI 和 AI Gateway**：登录流程还会询问 account ID；AI Gateway 还需要 gateway ID。

## 内置供应商

当前生成的目录包含以下供应商 ID。认证列是通常的登录路径；环境变量见[环境变量](./env-vars.md#供应商凭据与云身份)。

| 供应商 ID | 服务 | 认证方式 |
| --- | --- | --- |
| `amazon-bedrock` | Amazon Bedrock | AWS 凭据链或 bearer token |
| `ant-ling` | Ant Ling | API 密钥 |
| `anthropic` | Anthropic | API 密钥或 OAuth |
| `azure-openai-responses` | Azure OpenAI | API 密钥 |
| `cerebras` | Cerebras | API 密钥 |
| `cloudflare-ai-gateway` | Cloudflare AI Gateway | API 密钥 + account/gateway ID |
| `cloudflare-workers-ai` | Cloudflare Workers AI | API 密钥 + account ID |
| `deepseek` | DeepSeek | API 密钥 |
| `fireworks` | Fireworks AI | API 密钥 |
| `github-copilot` | GitHub Copilot | OAuth 或 token |
| `google` | Google Gemini | API 密钥 |
| `google-vertex` | Google Vertex AI | API 密钥、ADC 或 service account |
| `groq` | Groq | API 密钥 |
| `huggingface` | Hugging Face | API 密钥 |
| `kimi-coding` | Kimi Code 订阅 | OAuth 或 API 密钥 |
| `minimax`、`minimax-cn` | MiniMax | API 密钥 |
| `mistral` | Mistral | API 密钥 |
| `moonshotai`、`moonshotai-cn` | Moonshot AI | API 密钥 |
| `nvidia` | NVIDIA | API 密钥 |
| `openai` | OpenAI | API 密钥 |
| `openai-codex` | OpenAI Codex / ChatGPT | OAuth |
| `opencode`、`opencode-go` | OpenCode | API 密钥 |
| `openrouter` | OpenRouter | API 密钥或 OAuth |
| `qwen-token-plan`、`qwen-token-plan-cn` | 阿里云 Coding Plan | API 密钥 |
| `radius` | Radius | API 密钥或 OAuth |
| `together` | Together AI | API 密钥 |
| `vercel-ai-gateway` | Vercel AI Gateway | API 密钥 |
| `xai` | xAI / Grok | API 密钥或 OAuth |
| `xiaomi`、`xiaomi-token-plan-ams`、`xiaomi-token-plan-cn`、`xiaomi-token-plan-sgp` | Xiaomi | API 密钥 |
| `zai`、`zai-coding-cn` | Z.AI | API 密钥 |

`kimi-coding` 是 `https://api.kimi.com/coding/…` 上的 Kimi Code 平台。`moonshotai` 和 `moonshotai-cn` 是独立的 Moonshot AI Open Platform 端点。不要混用两个平台的 URL 或凭据。

## 浏览和刷新模型

CLI 与 TUI 使用同一个运行时目录：

```sh
vp run dev:cli -- provider list
vp run dev:cli -- provider list --json
vp run dev:cli -- provider models
vp run dev:cli -- provider models anthropic
vp run dev:cli -- provider refresh
```

`provider list` 显示内置和已配置供应商及其连接状态。`provider models` 显示已认证供应商当前可用的模型，包括权威的上下文窗口和能力。`provider refresh` 会获取每个拥有远程模型端点的已认证供应商；某个供应商刷新失败不会丢弃其他供应商的成功结果。

缓存位于 `$DIMI_CODE_HOME/models-store.json`。它记录新鲜度和 ETag，支持条件请求；离线时仅在元数据不早于随包目录时使用。TUI 的 `/model` 选择器和 `/provider refresh` 走同一刷新路径。模型发生变化时，显示的 Thinking 选项和上下文限制也会随模型元数据更新，不会根据模型名称猜测。

## 使用 `models.json` 添加或覆盖供应商

`$DIMI_CODE_HOME/models.json` 是用户拥有的供应商层，支持 JSONC：可以使用注释和尾随逗号。根对象必须是 `{ "providers": { … } }`；每个 key 就是供应商 ID，因此条目内部不要重复 `id`。运行时会在读取目录、刷新和选择模型前重新加载此文件，外部修改无需重启 Kimi Code 即可生效。

供应商条目的字段都是可选的。使用内置供应商（或 SDK 注入供应商）的 ID 时，它会覆盖该供应商：未写出的模型、认证行为、目录和流式适配器仍然保留。使用新 ID 时，它定义完整供应商，所有新模型都必须显式提供 `contextWindow` 和 `maxTokens`；Kimi Code 不会猜测这些限制。

```jsonc
{
  // 新端点请使用未被内置目录占用的 ID。
  "providers": {
    "example-gateway": {
      "name": "Example gateway",
      "baseUrl": "https://api.example.test/v1",
      "api": "openai-completions",
      "apiKey": "$EXAMPLE_GATEWAY_API_KEY",
      "headers": {
        "X-Client": "kimi-code"
      },
      "models": [
        {
          "id": "example-reasoner",
          "contextWindow": 128000,
          "maxTokens": 8192,
          "reasoning": true,
          "thinkingLevelMap": {
            "low": "low",
            "high": "high"
          },
          "input": ["text", "image"]
        }
      ]
    },
    // 这是覆盖层，不是第二个 Anthropic 供应商。
    "anthropic": {
      "headers": {
        "X-Example-Route": "team-a"
      },
      "modelOverrides": {
        "claude-sonnet-4-6": {
          "maxTokens": 16384
        }
      }
    }
  }
}
```

供应商字段为 `name`、`baseUrl`、`api`、`apiKey`、`oauth`、`headers`、`authHeader`、`compat`、`models` 和 `modelOverrides`。`apiKey` 可以是字面量、`$NAME` / `${NAME}` 环境变量模板或 `!command`；密钥请使用环境变量模板或命令，不要把字面量提交到仓库。`oauth: "radius"` 启用 Radius OAuth（并要求 `baseUrl`）。`authHeader: true` 会把解析出的 API 密钥写成 bearer `Authorization` header。`headers` 和 `compat` 会按供应商到模型逐层合并，模型 header 优先。

每个 `models` 项可设置 `id`、`name`、`api`、`baseUrl`、`reasoning`、`thinkingLevelMap`、`input`、`cost`（包括 `tiers`）、`contextWindow`、`maxTokens`、`headers` 和 `compat`。`modelOverrides` 的 key 必须是已有内置或 SDK 模型 ID，其值可使用同样的可选字段（没有 `id`）。可用协议适配器可在 CLI/TUI 中查看；当前包括 `openai-completions`、`openai-responses`、`azure-openai-responses`、`openai-codex-responses`、`anthropic-messages`、`mistral-conversations`、`bedrock-converse-stream`、`google-generative-ai`、`google-vertex` 和 `pi-messages`。

## 在 CLI 和 TUI 中管理自定义定义

CLI 写入的也是同一层 `models.json`。供应商需要 header、OAuth 或多个模型时，先准备完整文件：

```sh
vp run dev:cli -- provider add example-gateway --from ./models.json
vp run dev:cli -- provider update example-gateway --from ./models.json
vp run dev:cli -- provider remove example-gateway
```

简单 API 密钥端点可以直接在终端创建第一个模型。新模型必须提供两个限制：

```sh
vp run dev:cli -- provider add example-gateway \
  --base-url https://api.example.test/v1 \
  --api openai-completions \
  --model example-chat \
  --context-window 128000 \
  --max-tokens 8192 \
  --api-key-env EXAMPLE_GATEWAY_API_KEY \
  --thinking --image

vp run dev:cli -- provider model add example-gateway example-reasoner \
  --context-window 128000 --max-tokens 8192 --thinking
vp run dev:cli -- provider model update example-gateway example-reasoner --image
vp run dev:cli -- provider model remove example-gateway example-reasoner
```

`--from` 接受单个供应商对象或 `{ "providers": { … } }` 文件，并导入指定 ID。行内的 `--api-key-env` 会写入等价的 `$EXAMPLE_GATEWAY_API_KEY` 模板。添加后运行 `vp run dev:cli -- login example-gateway --method api-key`，或设置被引用的变量。

在 TUI 中，`/provider add` 会打开一个完整单模型自定义供应商表单。`/provider import <path>` 导入 JSONC 供应商定义，`/provider remove <id>` 移除用户拥有的定义，`/provider refresh` 刷新远程目录。`/provider` 和 `/provider <id>` 仍然用于连接。移除用户覆盖层不会移除底层内置供应商。

## Thinking、上下文和错误恢复

只有 `reasoning` 为 true 的模型才会在模型选择器中提供 Thinking。`thinkingLevelMap` 声明了档位时，选择器只发送已映射的档位；推理模型没有档位映射时，Thinking 只有开/关能力。显式映射到 `null` 代表该档位不受支持。上下文计算使用所选模型的 `contextWindow` 和 `maxTokens`，包括动态元数据与本地覆盖；上下文未知的自定义模型会在配置时被拒绝，而不是稍后以误导性的溢出错误失败。

供应商失败会保留错误类别。OAuth 的 401 可以先刷新一次，然后重试同一个未改动的请求；API 密钥请求绝不会触发 OAuth 刷新，第二次 401 会直接显示，已经产生内容的请求也不会重放。上下文大小失败（包括 HTTP 413）会作为上下文错误上报，使正常的压缩/重试路径能够恢复。永久认证或定义错误会指出要处理的供应商或字段，不会静默回退到其他模型。

## 后续阅读

- [配置文件](./config-files.md) —— 选择默认模型和 Thinking 行为
- [环境变量](./env-vars.md) —— 内置凭据与云身份
- [数据位置](./data-locations.md) —— 凭据、模型缓存和 `models.json` 文件
- [`kimi` 命令](../reference/kimi-command.md) —— provider 与 login 命令完整参考
