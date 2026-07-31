# Providers and models

Kimi Code CLI has a built-in provider catalog. You connect an account or API key, and the runtime exposes the models currently available through that provider. Providers, authentication, model discovery, request headers, and streaming now follow one shared lifecycle; `config.toml` no longer defines custom `[providers]` or `[models]` tables.

## Connect a provider

Use `kimi login` in a terminal, or `/login` in the TUI. Without an argument, the TUI first asks whether to use an account or an API key, then shows only compatible providers. Use `/login <provider>` to go directly to one provider; if it supports multiple methods, the TUI asks which one to use. After authentication, select a model from that provider's current catalog.

```sh
# Subscription login
kimi login kimi-coding --method oauth
kimi login openai-codex --method oauth
kimi login xai --method oauth

# Stored API key
kimi login anthropic --method api-key
kimi login openai --method api-key
```

API keys entered through `kimi login` are stored in `$KIMI_CODE_HOME/auth.json`. You can instead set a provider's environment variable; a stored credential takes priority. OAuth credentials refresh automatically when they have five minutes or less remaining.

In the TUI, `/provider` opens a provider list with connection state and credential source, then starts login for the selected provider. `/provider <provider>` connects directly, `/provider refresh` refreshes every dynamic model catalog, `/model` opens the models currently available from all providers, and `/logout` disconnects a provider.

## Built-in providers

The catalog includes the following providers:

| Provider ID    | Service                    | Authentication                       |
| -------------- | -------------------------- | ------------------------------------ |
| `kimi-coding`  | Kimi Code subscription     | OAuth or `KIMI_API_KEY`              |
| `openai-codex` | ChatGPT Codex              | OAuth                                |
| `xai`          | Grok / xAI                 | OAuth or `XAI_API_KEY`               |
| `openai`       | OpenAI API                 | `OPENAI_API_KEY`                     |
| `anthropic`    | Anthropic API              | `ANTHROPIC_API_KEY`                  |
| `openrouter`   | OpenRouter                 | `OPENROUTER_API_KEY`                 |
| `deepseek`     | DeepSeek                   | `DEEPSEEK_API_KEY`                   |
| `groq`         | Groq                       | `GROQ_API_KEY`                       |
| `mistral`      | Mistral                    | `MISTRAL_API_KEY`                    |
| `together`     | Together AI                | `TOGETHER_API_KEY`                   |
| `cerebras`     | Cerebras                   | `CEREBRAS_API_KEY`                   |
| `fireworks`    | Fireworks AI               | `FIREWORKS_API_KEY`                  |
| `zai`          | Z.AI                       | `ZAI_API_KEY`                        |
| `qwen`         | Alibaba Cloud Model Studio | `DASHSCOPE_API_KEY`                  |
| `moonshot`     | Moonshot AI Open Platform  | `MOONSHOT_API_KEY` or `KIMI_API_KEY` |

`kimi-coding` is the Kimi Code platform at `https://api.kimi.com/coding/v1`. The separate `moonshot` provider is for Moonshot AI Open Platform accounts.

## List and refresh models

Models are addressed as `provider/model`, for example `anthropic/claude-sonnet-4-6`. Inspect the current catalog with:

```sh
kimi provider list
kimi provider list --json
kimi provider models
kimi provider models anthropic
```

Providers with a model-list endpoint refresh their catalog dynamically. The runtime keeps the last successful catalog in `$KIMI_CODE_HOME/models-store.json`, sends conditional requests when the upstream supplies an ETag, and falls back to a built-in baseline model when no dynamic result is available.

Force a refresh after a provider adds or removes models:

```sh
kimi provider refresh
```

The long-running web server also refreshes connected dynamic providers in the background. Configure its cadence with `[model_catalog]` in [`config.toml`](./config-files.md#model-catalog).

## Select defaults

`config.toml` stores selection and request preferences, not provider definitions:

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

The `--model` flag and `/model` picker use the same `provider/model` references. The TUI writes `default_provider` and `default_model` when you persist a selection.

## Disconnect a provider

Remove a stored credential with:

```sh
kimi logout anthropic
```

An environment-variable credential remains available until you unset it. Logging out one provider does not affect sessions or credentials for other providers.

## Migration from provider tables

The previous `[providers]`, `[models]`, catalog-import, and custom-registry configuration is no longer read. Connect one of the built-in providers, then select a model from its runtime catalog:

```sh
kimi login <provider> --method api-key
kimi provider models <provider>
```

If you previously stored an API key directly in `config.toml`, move it to the provider's environment variable or enter it through `kimi login`.

## Next steps

- [Configuration files](./config-files.md) — default provider/model and runtime request settings
- [Environment variables](./env-vars.md) — credential and model-selection overrides
- [`kimi` command](../reference/kimi-command.md) — provider, login, and logout command reference
