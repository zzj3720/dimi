# Async tools and WaitFor handtest

Run this flow through the local interactive TUI. API calls and internal service invocations do not replace it.

## Preconditions

- CLIProxyAPI is healthy on its local OpenAI-compatible endpoint.
- An isolated `DIMI_CODE_HOME` configures a Grok model through CLIProxyAPI without copying credentials into this repository.
- The repository build used by the TUI contains the current worktree.
- A slow tool that does not implement its own background-task handoff is available. A local stdio MCP probe is suitable; `Bash` is not suitable for the timeout flow because it may return its own process task before the generic wait expires.

## Flow

1. Start the TUI in an 80x24 `tmux` session with the isolated home and Grok model.
2. Ask the model to start one tool call that takes longer than three seconds and to continue without polling.
3. Verify the tool result shown to the model contains a durable task ID while the real tool remains active.
4. Open `/tasks` and verify that task is running; return to the conversation without stopping it.
5. Verify the detached batch creates one auto wait and the current turn ends without the model polling.
6. Verify completion ends the auto wait, wakes a new turn automatically, and the model consumes the real final output.
7. Start a second tool that outlives the 20-second auto wait. After the explicit timeout notification, ask the model to call `WaitFor(reason, timeout_seconds)` with a shorter timeout and verify that timeout wakes the model without cancelling the task; verify its later completion arrives once.
8. Resume the session in a fresh TUI process and verify terminal task state and output remain readable.

## Background Bash stdin flow

Run this flow with the `background-bash-stdin` experiment enabled. A deterministic local model endpoint is suitable as long as the request still enters through `kimi -p` or the interactive TUI and emits real `Bash` and `TaskInput` tool calls.

1. Ask the model to start a background program that reads stdin until EOF and prints the exact received text. Verify the `Bash` call uses `run_in_background=true` and `stdin_mode="pipe"`.
2. Verify `Bash` returns a `bash-*` task ID without closing the process stdin.
3. Ask the model to call `TaskInput` with text containing a trailing newline and `close_stdin=true`.
4. Verify `TaskInput` reports the UTF-8 byte count and `stdin_closed: true`.
5. Open `/tasks` or inspect the completed print-mode session. Verify the process task reaches `completed` with exit code 0 and its output contains the exact input text.
6. Repeat without enabling the experiment and verify neither `TaskInput` nor Bash's `stdin_mode` parameter is exposed to the model.

## Evidence

Save the terminal captures, session path, task IDs, wire records, and verification summary under the local runtime evidence directory. Do not store API keys or provider credential files.
