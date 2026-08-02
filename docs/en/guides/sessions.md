# Sessions and context

Dimi CLI persists every conversation as a "session" — storing message history and metadata so you can close the terminal and pick up right where you left off. This page covers how to resume sessions, manage context, and export or fork sessions.

## Session storage

All sessions are saved under `$DIMI_CODE_HOME/sessions/` (default: `~/.dimi/sessions/`), grouped by working directory:

```text
~/.dimi/
├── config.toml
├── workspaces.json
└── sessions/
    └── <workspaceId>/
        └── <sessionId>/
            ├── state.json
            └── agents/
                ├── main/
                │   ├── wire.jsonl
                │   └── tasks/
                └── <subagentId>/
                    ├── wire.jsonl
                    └── tasks/
```

- `state.json`: session metadata such as title and creation time.
- `agents/*/wire.jsonl`: the agent event stream, used for session recovery and replay. It also carries a request trace — the tool schemas, request parameters, and MCP tool listings sent to the model — for debugging.

::: warning
Do not manually edit files inside the `sessions/` directory — doing so may prevent sessions from being restored correctly.
:::

## Starting and resuming sessions

Every time you run `dimi` directly it creates a new session. To resume a previous session, use one of the following:

**Resume the most recent session in the current directory:**

```sh
dimi --continue
```

**Resume a specific session by ID:**

```sh
dimi --session abc123
```

**Interactively browse session history and choose one:**

```sh
dimi --session
```

::: warning
`--continue` and `--session` are mutually exclusive.
:::

## Switching sessions inside the TUI

You can manage sessions without leaving the terminal. The following slash commands manage sessions (`/title` is always available; the rest are available only when the agent is idle):

- **`/new`** (alias `/clear`): switch to a new session, discarding the current context.
- **`/sessions`** (alias `/resume`): browse and resume a previous session.
- **`/fork`**: fork the current session (see below).
- **`/title <text>`** (alias `/rename`): set a session title for easier identification; without arguments, displays the current title.

## Context compression

As a conversation grows, Dimi CLI automatically compresses the message history when the context approaches the window limit, freeing up token space. You can also trigger compression manually at any time:

```
/compact
```

You can pass a hint to tell the model what to prioritize when compressing:

```
/compact Keep the discussion about database migrations
```

## Forking a session

To explore a new direction without disrupting the current conversation, use `/fork`:

```
/fork
```

The two resulting sessions are completely independent and do not affect each other. You can switch back to the original at any time using `/sessions`. Forked sessions are fully independent; plan mode and other session state are not carried over.

## Exporting a session

Use `dimi export` to package a session as a ZIP file — useful for sharing, archiving, or filing a bug report:

```sh
dimi export <sessionId>
```

Omitting `sessionId` exports the most recent session in the current directory (with an interactive confirmation prompt; add `-y` to skip). Use `-o` to specify an output path:

```sh
dimi export <sessionId> -o ~/Desktop/my-session.zip
```

The export includes all files in the session directory, including diagnostic logs. The global diagnostic log (`~/.dimi/logs/dimi.log`) is also bundled by default; add `--no-include-global-log` to exclude it.

You can also export from inside the TUI without leaving the interactive session:

- **`/export-debug-zip`**: produces the same debug ZIP as `dimi export`.
- **`/export-md`** (alias `/export`): exports the conversation as a human-readable Markdown file, suitable for sharing or archiving. Accepts an optional path argument; without one, it writes to `dimi-export-<short-id>-<timestamp>.md` in the current working directory.

In the web UI, `/export` downloads the current session as a diagnostic ZIP. It includes the persisted session data, diagnostic logs, and a bounded metadata-only `logs/dimi-web.jsonl` record of key browser events. Prompt text, WebSocket payloads, and console arguments are not copied into this browser log. This web command differs from the TUI `/export` alias above.

The browser buffers the ZIP before saving it, so web exports are limited to 64 MiB. For a larger session, use `dimi export <sessionId>` or the TUI `/export-debug-zip` command.

::: tip
Exported files may contain code, command output, and file paths that are sensitive. Review the content before sharing.
:::

## Next steps

- [Data locations](../configuration/data-locations.md) — full directory layout for session files
- [dimi command reference](../reference/dimi-command.md) — complete parameter reference for `--continue`, `--session`, `export`, and other commands
