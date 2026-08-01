# `dimi` Command

`dimi` is the main command for Dimi CLI, used to start an interactive session in the terminal. Running it without any arguments opens a new session in the current working directory; combined with different flags, you can resume a previous session, skip approvals, start in Plan mode, or load Skills from a custom directory.

```sh
dimi [options]
dimi <subcommand> [options]
```

## Main Command Options

All flags are optional — run `dimi` directly to enter an interactive session:

| Option                     | Short | Description                                                                                                                                             |
| -------------------------- | ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `--version`                | `-V`  | Print the version number and exit                                                                                                                       |
| `--help`                   | `-h`  | Show help information and exit                                                                                                                          |
| `--session [id]`           | `-S`  | Resume a session. With an ID, opens that session directly; without an ID, enters an interactive selector                                                |
| `--continue`               | `-c`  | Continue the most recent session in the current working directory, without specifying an ID manually                                                    |
| `--model <model>`          | `-m`  | Select `<provider>/<model>` for this launch. When omitted, new sessions use `default_provider` and `default_model`                                      |
| `--prompt <prompt>`        | `-p`  | Run a single prompt non-interactively and stream the Assistant output to stdout. This mode does not open the TUI                                        |
| `--output-format <format>` |       | Set the non-interactive output format; supports `text` and `stream-json`. Can only be used with `--prompt`; defaults to `text`                          |
| `--yolo`                   | `-y`  | Auto-approve regular tool calls, skipping approval requests                                                                                             |
| `--auto`                   |       | Start with auto permission mode; tool approvals are handled automatically and the Agent will not ask the user questions                                 |
| `--plan`                   |       | Start a new session in Plan mode — the AI will prioritize read-only tools for exploration and planning                                                  |
| `--skills-dir <dir>`       |       | Load Skills from the specified directory, replacing the automatically discovered user and project directories. Can be repeated                          |
| `--agent <name>`           |       | Start a new session with the specified agent as the main Agent. Cannot be combined with `--session`/`--continue`                                        |
| `--agent-file <path>`      |       | Load a custom agent from a Markdown file for the new session and select it. Cannot be repeated or combined with `--agent`, `--session`, or `--continue` |
| `--add-dir <dir>`          |       | Add an extra workspace directory for this session. Relative paths resolve against the current working directory. Can be repeated                        |

`-r` / `--resume` is a hidden alias for `--session`; `--yes` and `--auto-approve` are hidden aliases for `--yolo` and are not shown in help output.

::: warning
`--yolo` skips human approval for regular tool calls, including file writes and shell command execution. Use it only in trusted working directories. Plan mode exit approval is not bypassed by `--yolo`; `Bash` inside Plan mode is handled under the regular allow rules.
:::

### Flag Conflict Rules

The following combinations are rejected at startup:

- `--continue` and `--session` are mutually exclusive — both mean "resume a previous session"
- `--yolo` and `--auto` are mutually exclusive — the two permission modes cannot be combined
- `--prompt` cannot be used with `--yolo`, `--auto`, or `--plan` — non-interactive mode uses `auto` permission by default
- `--output-format` can only be used together with `--prompt`

When resuming a session, you can override its saved permission or plan mode by adding `--auto`, `--yolo`, or `--plan`. For example, `dimi --continue --auto` resumes the latest session and switches it to auto permission mode.

## Common Usage

Start a new session directly:

```sh
dimi
```

Pick up where you left off (automatically finds the most recent session in the current directory):

```sh
dimi --continue
```

Choose from the session history list, or specify a known ID directly:

```sh
dimi --session
dimi --session 01HZ...XYZ
```

Skip approval prompts — suitable for batch tasks that are known to be safe:

```sh
dimi --yolo
```

Let the Agent handle everything autonomously, without asking the user questions:

```sh
dimi --auto
```

Read the code and produce an implementation plan before making any file changes:

```sh
dimi --plan
```

### Custom Skills Directories

There are two ways to specify Skills directories, with different semantics:

- **`--skills-dir <dir>`** (CLI flag): **Replaces** the automatically discovered user and project directories for this launch only. Can be repeated to stack multiple directories:

  ```sh
  dimi --skills-dir /path/to/team-skills --skills-dir ./local-skills
  ```

- **`extra_skill_dirs`** (`config.toml`): **Adds** directories on top of the automatically discovered ones, taking effect permanently. Suitable for configuring team-shared Skills. See [Agent Skills](../customization/skills.md).

### Custom Agents

`--agent` and `--agent-file` select which agent drives a new session, in both print mode (`dimi -p`) and the interactive TUI:

```sh
dimi --agent reviewer
dimi -p --agent reviewer "Review the changes on this branch"
```

`--agent-file` registers a single agent file at the highest priority for this launch only and selects it; the flag cannot be repeated, and `--agent` and `--agent-file` are mutually exclusive. Both flags only apply when starting a new session — neither can be combined with `--session`/`--continue`, because the agent is bound at session creation and resuming restores the bound agent automatically. The selection is fixed at the session's first bind and cannot be switched later; in the TUI the flags bind only the startup session, and a session created later in the same process (for example via `/new`) starts with the default agent. See [Agents and Sub-Agents](../customization/agents.md#custom-agents) for the agent file format and discovery directories.

## Non-Interactive Execution

When running a single prompt in a script or CI environment, use `-p`:

```sh
dimi -p "Summarize the current repository status"
```

Output uses a transcript style: thinking content and Assistant text are both prefixed with `• `, and wrapped lines are indented by two spaces. Assistant text goes to stdout; thinking, tool progress, and "resuming session" notices go to stderr. In `-p` mode, no human approval is requested — regular tool calls are handled under the `auto` permission policy, while static deny rules remain in effect.

Temporarily switch the model:

```sh
dimi -m kimi-coding/kimi-for-coding -p "Explain the latest diff"
```

When you need to parse output programmatically, use the `stream-json` format — each line on stdout is a JSON object:

```sh
dimi -p "List changed files" --output-format stream-json
```

In `stream-json` mode, regular replies produce an Assistant message; when the model calls a tool, an Assistant message with `tool_calls` is emitted first, followed by the corresponding Tool message, then subsequent Assistant messages. Thinking content is not written to JSONL; tool progress and "resuming session" notices are still written to stderr.

## Subcommands

`dimi` provides the following subcommands: `login` and `logout` (provider credentials), `provider` (provider and model catalogs), `acp` (ACP IDE mode), `web` (run the local REST/WebSocket/web service), `doctor` (validate configuration files), `export` (export a session), and `upgrade` (check for updates).

### `dimi login`

Connect a built-in or `models.json` LLM provider with OAuth or an API key. Omit the provider to choose interactively. When a provider supports both methods, omit `--method` to choose one interactively. Cloud providers can ask follow-up questions for a credential chain, account, project, or location.

```sh
dimi login
dimi login openai-codex --method oauth
dimi login anthropic --method api-key
```

OAuth login prints the provider's authorization URL or device code and waits for completion. API-key login prompts through the terminal without echoing the key. Saved credentials are written to `auth.json` and loaded on the next startup.

| Option              | Description                                                            |
| ------------------- | ---------------------------------------------------------------------- |
| `[provider]`        | Provider ID from the built-in or `models.json` catalog; omit to choose |
| `--method <method>` | `oauth` or `api-key`; specify it to skip the interactive method choice |

### `dimi logout`

Remove the saved credential for one provider:

```sh
dimi logout anthropic
```

An API key exported through the shell remains active until it is unset.

### `dimi acp`

Switch Dimi CLI to ACP (Agent Client Protocol) mode, communicating with an IDE via JSON-RPC over stdin/stdout so the editor can directly drive dimi's sessions and tool calls. You typically do not need to run this manually — the IDE starts it as a subprocess entry point. For configuration, see [Using in IDEs](../guides/ides.md); for technical details, see the [dimi acp reference](./dimi-acp.md).

```sh
dimi acp
```

### `dimi web`

Run the local Dimi server in the foreground of the current terminal — a single process that exposes the REST + WebSocket API and serves the web UI from the same origin — and open the web UI in the default browser once it is ready. The command stays attached to the terminal and shuts down cleanly on `SIGINT` / `SIGTERM` (e.g. `Ctrl-C`).

When the server is running, `GET /openapi.json` returns the REST OpenAPI document and `GET /asyncapi.json` returns the local WebSocket AsyncAPI document.

```sh
dimi web                 # run the server in the foreground and open the browser
dimi web --no-open       # don't open the browser
dimi web --port 58628    # pick a specific bind port
```

Multiple instances can share one home directory: each registers itself under `~/.dimi/server/instances/`, and a busy port is retried with `port + 1` (58628, 58629, …).

| Option                     | Description                                                                                                                                                    |
| -------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `--port <port>`            | Bind port; defaults to `58627`; a busy port is retried with `+1`                                                                                               |
| `--host [host]`            | Bind host; omit for `127.0.0.1` (this machine only), pass a bare `--host` for `0.0.0.0` (all interfaces)                                                       |
| `--allowed-host <host...>` | Extra Host header values allowed through the DNS-rebinding check; repeatable or comma-separated                                                                |
| `--log-level <level>`      | Enable server logs at the selected level; omitted by default                                                                                                   |
| `--debug-endpoints`        | Mount `/api/v1/debug/*` routes (off by default)                                                                                                                |
| `--dangerous-bypass-auth`  | Disable bearer-token auth on all REST and WebSocket routes so the web UI connects without a token; only for trusted networks or behind an authenticating proxy |
| `--no-open`                | Do not open the browser once the server is ready                                                                                                               |

`dimi web` binds to local loopback only by default and prints the bearer token in the startup banner; the web UI authenticates automatically via the `#token=` URL fragment.

::: info
The `dimi server` command tree is deprecated: any `dimi server …` invocation (including all legacy subcommands) only prints a deprecation notice and exits with code 1 — use `dimi web` instead. The one exception is `dimi server kill`, which stays functional for stopping servers started by a version before 0.28.0. The notice will be removed in the next major version of Dimi.
:::

::: danger
`--dangerous-bypass-auth` disables authentication entirely. Anyone who can reach the port gets full access to your sessions, filesystem, and shell. Only use it on a trusted network or behind your own authenticating reverse proxy, and stop the server with `Ctrl+C` when you are done.
:::

#### `dimi server kill`

Deprecated — only stops a server started by a version before 0.28.0. Those versions could leave a background server behind, recorded in the legacy single-instance lock at `~/.dimi/server/lock`; the command first tries `POST /api/v1/shutdown` for a graceful exit, then signals the recorded pid with SIGTERM, escalating to SIGKILL when needed, and removes the lock file once the process is confirmed dead. Servers started by `dimi web` run in the foreground — stop them with `Ctrl+C` instead.

#### `dimi web rotate-token`

Generate a new persistent bearer token (written to `~/.dimi/server.token`); the previous token stops working immediately. The token is shared by the whole home directory, so every running instance picks the new one up on its next auth check — no restart needed.

### `dimi doctor`

Validate `config.toml` and `tui.toml` without starting the TUI or modifying either file. By default, the command checks the files under `DIMI_CODE_HOME` (or `~/.dimi` when the environment variable is unset). Missing default files are reported as skipped because built-in defaults can apply.

```sh
dimi doctor
```

| Command                     | Description                                                                         |
| --------------------------- | ----------------------------------------------------------------------------------- |
| `dimi doctor`               | Validate the default `config.toml` and `tui.toml`                                   |
| `dimi doctor config [path]` | Validate only `config.toml`, using `path` instead of the default file when provided |
| `dimi doctor tui [path]`    | Validate only `tui.toml`, using `path` instead of the default file when provided    |

When an explicit path is passed, the file must exist. The command exits with `0` when all checked files are valid or skipped, and `1` when any requested file is missing or invalid.

```sh
# Check the default config files
dimi doctor

# Check only the default runtime config
dimi doctor config

# Check a candidate TUI config before replacing the live config
dimi doctor tui ./tui.toml
```

### `dimi export`

Package a session into a ZIP file for sharing, archiving, or submitting bug reports.

```sh
dimi export [sessionId] [options]
```

| Parameter / Option        | Short | Description                                                                                                                                                 |
| ------------------------- | ----- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `sessionId`               |       | The ID of the session to export. When omitted, the most recent session in the current working directory is automatically selected and requires confirmation |
| `--output <path>`         | `-o`  | Output ZIP file path. When omitted, writes to a default filename in the current directory                                                                   |
| `--yes`                   | `-y`  | Skip the confirmation prompt for the default session and export directly                                                                                    |
| `--no-include-global-log` |       | Do not include the global diagnostic log. Included by default                                                                                               |

The export contains all files in the target session directory. The global diagnostic log (`~/.dimi/logs/dimi.log`) is included by default because it may contain events from other sessions or projects; add `--no-include-global-log` if you do not want to share it.

```sh
# Export the most recent session in the current directory, skipping confirmation
dimi export -y

# Export a specific session to a custom path
dimi export 01HZ...XYZ -o ./bug-report.zip

# Exclude the global diagnostic log
dimi export 01HZ...XYZ -o ./bug-report.zip --no-include-global-log
```

### `dimi upgrade`

This source build has no configured release channel. `dimi upgrade` exits after reporting that automatic upgrades are unavailable; `dimi update` is an alias.

```sh
dimi upgrade
```

It never falls back to an upstream Dimi install source. Update this checkout with `git pull --ff-only`, then run `vp install` and `vp run dev:cli`.

### `dimi vis`

Launch the session visualizer in your browser to inspect a session as it unfolds. The command starts an in-process server pointed at your local sessions, prints the URL, opens your browser, and keeps running until you press `Ctrl-C`.

```sh
dimi vis [sessionId] [options]
```

| Parameter / Option | Description                                                                                              |
| ------------------ | -------------------------------------------------------------------------------------------------------- |
| `sessionId`        | Open the visualizer directly to this session. When omitted, it opens the home view listing your sessions |
| `--port <number>`  | Port to bind. By default an available port is picked automatically                                       |
| `--host <host>`    | Host to bind. Default: `127.0.0.1`                                                                       |
| `--no-open`        | Do not open the browser automatically; just print the URL                                                |

```sh
# Start the visualizer and open the browser at the home view
dimi vis

# Open directly to a specific session
dimi vis 01HZ...XYZ

# Bind a fixed port and host without opening a browser (e.g. on a remote host)
dimi vis --host 0.0.0.0 --port 8123 --no-open
```

### `dimi provider`

Inspect providers and models, refresh dynamic catalogs, and manage the user-owned `models.json` provider layer. Built-in and SDK provider definitions remain runtime-owned; a custom definition with the same ID is an overlay, not a replacement catalog.

```sh
dimi provider list [--json]
dimi provider models [providerId]
dimi provider refresh
dimi provider add <id> [options]
dimi provider update <id> [options]
dimi provider remove <id>
dimi provider model add <providerId> <modelId> [options]
dimi provider model update <providerId> <modelId> [options]
dimi provider model remove <providerId> <modelId>
```

#### `dimi provider list`

Print every built-in and configured provider, its connection state, and the number of currently available models. Add `--json` to output provider auth states and model metadata.

```sh
dimi provider list
dimi provider list --json
```

#### `dimi provider models [providerId]`

List models currently available through authenticated providers. Pass a provider ID to narrow the list. Each line includes the canonical `<provider>/<model>` reference, context window, and relevant capabilities.

```sh
dimi provider models
dimi provider models openai-codex
```

#### `dimi provider refresh`

Refresh the remote model endpoints for every authenticated provider. Refresh failures are reported per provider while successful catalogs are still persisted.

```sh
dimi provider refresh
```

#### `dimi provider add` and `update`

Create or update a custom provider. `--from <path>` imports either one provider object or a `{ "providers": { … } }` JSONC document and selects the requested ID. For an inline new provider, `--base-url`, `--model`, `--context-window`, and `--max-tokens` are required. `--api` defaults to `openai-completions`; `--api-key-env` records an environment template instead of a secret.

```sh
dimi provider add example-gateway --from ./models.json
dimi provider add example-gateway \
  --base-url https://api.example.test/v1 --model example-chat \
  --context-window 128000 --max-tokens 8192 \
  --api-key-env EXAMPLE_GATEWAY_API_KEY --thinking --image
dimi provider update example-gateway --model example-chat --max-tokens 16384
```

The inline options are `--name`, `--model-name`, `--api`, `--api-key-env`, `--thinking`, and `--image` in addition to the required fields. Use a file for provider headers, `compat`, OAuth settings, model overrides, per-model base URLs, or several models; the [provider configuration reference](../configuration/providers.md#add-or-overlay-a-provider-with-modelsjson) documents the full JSONC shape.

#### `dimi provider remove`

Remove a user-owned provider definition. If it is a custom overlay of a built-in provider, the underlying built-in provider becomes visible again. Removing a standalone custom provider also removes its saved credential.

```sh
dimi provider remove example-gateway
```

#### `dimi provider model`

Add, update, or remove a model declared by a custom provider. Adding a new model requires `--context-window` and `--max-tokens`; `--name`, `--thinking`, and `--image` are optional.

```sh
dimi provider model add example-gateway example-reasoner \
  --context-window 128000 --max-tokens 8192 --thinking
dimi provider model update example-gateway example-reasoner --image
dimi provider model remove example-gateway example-reasoner
```

## Next steps

- [Slash Commands](./slash-commands.md) — Quick reference for control commands in the interactive TUI
- [Configuration Files](../configuration/config-files.md) — Persistent configuration for `default_model`, permission mode, and other startup parameters
- [Agent Skills](../customization/skills.md) — Skill file format for directories loaded via `--skills-dir`
- [Agents and Sub-Agents](../customization/agents.md) — Built-in sub-agents, custom agent files, and main Agent selection via `--agent`
