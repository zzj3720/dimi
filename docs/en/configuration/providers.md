# Providers and models

Dimi CLI has one provider runtime for built-in services, locally configured endpoints, and SDK-supplied providers. It resolves the selected model's protocol, credentials, request headers, Thinking mode, context limits, streaming, and recovery behavior from the same model definition. A model reference is always `provider/model`.

Built-in metadata is a generated snapshot. It provides model limits and capabilities before a provider is connected; a provider's own catalog may replace that snapshot when it returns complete metadata. A bare remote model ID never invents a context window, output limit, or Thinking capability.

## Connect a built-in provider

Use `vp run dev:cli -- login` in a terminal from the checkout, or `/login` in the TUI. Omit the provider ID to choose an available provider and authentication method. `/login <provider>` goes directly to one provider, and `/provider <provider>` is a shortcut to the same connection flow.

```sh
# Subscription or OAuth login
vp run dev:cli -- login kimi-coding --method oauth
vp run dev:cli -- login openai-codex --method oauth
vp run dev:cli -- login github-copilot --method oauth

# API key login; the terminal masks typed keys
vp run dev:cli -- login anthropic --method api-key
vp run dev:cli -- login openai --method api-key
```

Saved API keys and OAuth credentials are stored in `$DIMI_CODE_HOME/auth.json`, not `config.toml`. A saved key takes priority over the equivalent shell variable. OAuth tokens refresh automatically when needed. `/logout` removes the saved credential for one provider; it does not unset a shell environment variable.

Some cloud providers use their normal credential chain instead of a single API key:

- **Amazon Bedrock**: choose a bearer token, an AWS profile, or the AWS default credential chain (profile, IAM keys, workload identity, or container role).
- **Vertex**: choose a Google Cloud API key, Application Default Credentials (ADC), or a service-account credentials file, then provide the Google Cloud project and location.
- **Cloudflare Workers AI and AI Gateway**: the login flow also asks for the account ID; AI Gateway additionally needs its gateway ID.

## Built-in providers

The current generated catalog contains these provider IDs. The authentication column names the normal login path; the environment-variable reference is in [Environment variables](./env-vars.md#provider-credentials-and-cloud-identity).

| Provider ID | Service | Authentication |
| --- | --- | --- |
| `amazon-bedrock` | Amazon Bedrock | AWS credential chain or bearer token |
| `ant-ling` | Ant Ling | API key |
| `anthropic` | Anthropic | API key or OAuth |
| `azure-openai-responses` | Azure OpenAI | API key |
| `cerebras` | Cerebras | API key |
| `cloudflare-ai-gateway` | Cloudflare AI Gateway | API key + account/gateway IDs |
| `cloudflare-workers-ai` | Cloudflare Workers AI | API key + account ID |
| `deepseek` | DeepSeek | API key |
| `fireworks` | Fireworks AI | API key |
| `github-copilot` | GitHub Copilot | OAuth or token |
| `google` | Google Gemini | API key |
| `google-vertex` | Google Vertex AI | API key, ADC, or service account |
| `groq` | Groq | API key |
| `huggingface` | Hugging Face | API key |
| `kimi-coding` | Dimi subscription | OAuth or API key |
| `minimax`, `minimax-cn` | MiniMax | API key |
| `mistral` | Mistral | API key |
| `moonshotai`, `moonshotai-cn` | Moonshot AI | API key |
| `nvidia` | NVIDIA | API key |
| `openai` | OpenAI | API key |
| `openai-codex` | OpenAI Codex / ChatGPT | OAuth |
| `opencode`, `opencode-go` | OpenCode | API key |
| `openrouter` | OpenRouter | API key or OAuth |
| `qwen-token-plan`, `qwen-token-plan-cn` | Alibaba Coding Plan | API key |
| `radius` | Radius | API key or OAuth |
| `together` | Together AI | API key |
| `vercel-ai-gateway` | Vercel AI Gateway | API key |
| `xai` | xAI / Grok | API key or OAuth |
| `xiaomi`, `xiaomi-token-plan-ams`, `xiaomi-token-plan-cn`, `xiaomi-token-plan-sgp` | Xiaomi | API key |
| `zai`, `zai-coding-cn` | Z.AI | API key |

`kimi-coding` is the Dimi platform at `https://api.kimi.com/coding/…`. `moonshotai` and `moonshotai-cn` are separate Moonshot AI Open Platform endpoints. Do not substitute one platform URL or credential for the other.

## Browse and refresh models

Use the same runtime catalog in the CLI and TUI:

```sh
vp run dev:cli -- provider list
vp run dev:cli -- provider list --json
vp run dev:cli -- provider models
vp run dev:cli -- provider models anthropic
vp run dev:cli -- provider refresh
```

`provider list` shows built-in and configured providers with their connection state. `provider models` shows models currently available to authenticated providers, including the authoritative context window and capabilities. `provider refresh` fetches every authenticated provider that has a remote model endpoint; a failed provider is reported without discarding the other successful refreshes.

The cache is `$DIMI_CODE_HOME/models-store.json`. It records freshness and ETags, supports conditional requests, and is used while offline only when its metadata is at least as new as the bundled catalog. The TUI `/model` picker and `/provider refresh` use the same refresh path. When a model changes, its visible Thinking choices and context limit change with the model metadata rather than being inferred from its name.

## Add or overlay a provider with `models.json`

`$DIMI_CODE_HOME/models.json` is the user-owned provider layer. It is JSONC: comments and trailing commas are accepted. Its root must be `{ "providers": { … } }`; each key is the provider ID, so do not repeat an `id` field inside the entry. The runtime reloads this file before catalog reads, refreshes, and model selection, so an external edit takes effect without restarting Dimi.

A provider entry has optional fields. With the ID of a built-in provider (or an SDK-provided provider), it overlays that provider: its omitted models, authentication behavior, catalog, and stream adapter remain in place. With a new ID, it defines a complete provider and must give each new model an explicit `contextWindow` and `maxTokens`; Dimi never guesses those limits.

```jsonc
{
  // Use an ID not owned by the built-in catalog for a new endpoint.
  "providers": {
    "example-gateway": {
      "name": "Example gateway",
      "baseUrl": "https://api.example.test/v1",
      "api": "openai-completions",
      "apiKey": "$EXAMPLE_GATEWAY_API_KEY",
      "headers": {
        "X-Client": "dimi"
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
    // This is an overlay, not a second Anthropic provider.
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

Provider fields are `name`, `baseUrl`, `api`, `apiKey`, `oauth`, `headers`, `authHeader`, `compat`, `models`, and `modelOverrides`. `apiKey` accepts a literal, `$NAME` / `${NAME}` environment template, or `!command`; use an environment template or command for secrets rather than committing a literal. `oauth: "radius"` enables Radius OAuth (and requires `baseUrl`). `authHeader: true` turns the resolved API key into a bearer `Authorization` header. Headers and `compat` values layer from provider to model, with a model header taking precedence.

Each `models` item may set `id`, `name`, `api`, `baseUrl`, `reasoning`, `thinkingLevelMap`, `input`, `cost` (including `tiers`), `contextWindow`, `maxTokens`, `headers`, and `compat`. A `modelOverrides` item has the same optional fields except `id`; its key must name an existing built-in or SDK model. Available protocol adapters are shown by the CLI/TUI; they currently include `openai-completions`, `openai-responses`, `azure-openai-responses`, `openai-codex-responses`, `anthropic-messages`, `mistral-conversations`, `bedrock-converse-stream`, `google-generative-ai`, `google-vertex`, and `pi-messages`.

## Manage custom definitions from the CLI and TUI

The CLI writes the same `models.json` layer. Start with a complete file when a provider needs headers, OAuth, or more than one model:

```sh
vp run dev:cli -- provider add example-gateway --from ./models.json
vp run dev:cli -- provider update example-gateway --from ./models.json
vp run dev:cli -- provider remove example-gateway
```

For a simple API-key endpoint, create the first model on the command line. New models require both limits:

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

`--from` accepts either one provider object or a `{ "providers": { … } }` file and imports the requested ID. The inline `--api-key-env` form writes the equivalent `$EXAMPLE_GATEWAY_API_KEY` template. After adding a provider, run `vp run dev:cli -- login example-gateway --method api-key` or set the referenced variable.

In the TUI, `/provider add` opens a form for a complete one-model custom provider. `/provider import <path>` imports a JSONC provider file, `/provider remove <id>` removes a user-owned definition, and `/provider refresh` refreshes remote catalogs. `/provider` and `/provider <id>` remain connection commands. A user-owned overlay can be removed without removing the built-in provider beneath it.

## Thinking, context, and recovery

The model picker only offers Thinking when `reasoning` is true. When `thinkingLevelMap` declares levels, the picker sends only those mapped levels; when a reasoning model has no level map, Thinking is an on/off capability. Explicitly mapped `null` means that level is unsupported. Context accounting uses the selected model's `contextWindow` and `maxTokens`, including dynamic metadata and local overrides, so a custom model with unknown limits is rejected at configuration time instead of failing later with a misleading overflow.

Provider failures preserve their category. A 401 for OAuth may refresh once before retrying the same untouched request; API-key requests never trigger OAuth refresh, a second 401 is surfaced, and a request that already emitted content is not replayed. Context-size failures, including HTTP 413, are reported as context errors so the normal compaction/retry path can recover. Permanent authentication and definition errors tell you which provider or field needs attention rather than silently falling back to another model.

## Next steps

- [Configuration files](./config-files.md) — select defaults and Thinking behavior
- [Environment variables](./env-vars.md) — built-in credentials and cloud identity
- [Data locations](./data-locations.md) — the credentials, model cache, and `models.json` files
- [`dimi` command](../reference/dimi-command.md) — full provider and login command reference
