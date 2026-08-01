# Config overrides

Dimi CLI keeps persistent preferences, provider credentials, and one-off runtime changes in separate channels:

- **`config.toml`** stores preferences such as the default provider/model, thinking settings, and loop limits. It does not store provider credentials.
- **`auth.json`** stores credentials saved through `vp run dev:cli -- login`.
- **`models.json`** is the user-owned JSONC provider layer. It can add a custom provider or overlay a built-in/SDK provider's model metadata and request settings.
- **Environment variables** can supply provider API keys, select a provider/model pair, relocate the data directory, or override a documented runtime setting.
- **Command-line options** apply only to the current launch.

## Provider and model selection

For a new session, provider/model selection is resolved in this order:

1. `-m, --model <provider>/<model>`
2. `DIMI_MODEL_PROVIDER` together with `DIMI_MODEL_NAME`
3. `default_provider` together with `default_model` in `config.toml`

The selected pair must exist in the runtime model catalog. That catalog composes built-in or SDK providers with the `models.json` layer. When the provider supports remote discovery, `vp run dev:cli -- provider refresh` updates that catalog; the cached result is stored in `models-store.json` and only complete remote metadata changes a model's context, output, or Thinking capability.

The CLI currently reads one user-level config file and has no project-level config mechanism. To isolate configurations, point `DIMI_CODE_HOME` at a different data directory.

## Provider credentials

Provider authentication is resolved separately from model selection:

1. A credential saved by `vp run dev:cli -- login <provider>`
2. A `models.json` API-key template or command, when configured for that provider
3. The provider's standard API-key environment variable, when API-key authentication is supported

Use `vp run dev:cli -- logout <provider>` to remove a saved credential. An exported environment variable remains available until you unset it or exit the shell.

For example, this uses a one-off Anthropic key without modifying local files:

```sh
ANTHROPIC_API_KEY="YOUR_API_KEY" vp run dev:cli -- -m anthropic/claude-sonnet-4-6
```

OpenAI Codex uses OAuth. Dimi, xAI, Anthropic, OpenRouter, GitHub Copilot, and Radius can offer OAuth; cloud providers can use their credential chains. See [Providers and models](./providers.md) for the built-in provider list and login methods.

## Other runtime parameters

For ordinary parameters such as Plan mode, permission mode, and Skills directories, command-line options override `config.toml` for the current launch. A documented environment variable may override its matching config field; for example, `DIMI_CODE_BACKGROUND_MAX_RUNNING_TASKS` overrides `[task].max_running_tasks`.

| Option                           | Effect                                                                     |
| -------------------------------- | -------------------------------------------------------------------------- |
| `-S, --session [id]`             | Resume a specific session; enter interactive selection when no ID is given |
| `-c, --continue`                 | Resume the last session for the current working directory                  |
| `-y, --yolo`                     | Auto-approve regular tool calls                                            |
| `--auto`                         | Start in auto permission mode                                              |
| `--plan`                         | Start in Plan mode                                                         |
| `-m, --model <provider>/<model>` | Select a runtime catalog model                                             |
| `-p, --prompt <prompt>`          | Run one prompt non-interactively                                           |
| `--output-format <format>`       | Use `text` or `stream-json` output with `-p`                               |
| `--skills-dir <dir>`             | Replace auto-discovered Skills directories for this launch                 |

## Common scenarios

Isolate all local state:

```sh
DIMI_CODE_HOME="$PWD/.dimi-sandbox" dimi
```

Temporarily select OpenAI without changing `config.toml`:

```sh
OPENAI_API_KEY="YOUR_API_KEY" \
  DIMI_MODEL_PROVIDER="openai" \
  DIMI_MODEL_NAME="gpt-5.4" \
  dimi
```

Enter Plan mode for one launch:

```sh
dimi --plan
```

## Next steps

- [Configuration files](./config-files.md) — persistent preference fields
- [Environment variables](./env-vars.md) — provider keys and runtime overrides
- [Data locations](./data-locations.md) — credential, provider-definition, and model-cache files
