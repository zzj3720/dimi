# AllDone / WaitFor waiting-state guidance handtest

Run this flow through the local interactive TUI with a real model. The tool description is consumed by the model, so scripted or stubbed LLM responses cannot verify it.

## Preconditions

- A real model endpoint that reads tool descriptions (any provider configured for the isolated home).
- An isolated `DIMI_CODE_HOME` so the session does not touch real credentials.
- The repository build used by the TUI contains the current worktree (the `all-done.md` guidance edit).
- A long-running tool is available (e.g. `Bash` with `sleep 60`, or a slow MCP probe) so the agent has something to wait on.

## Flow

1. Start the TUI in an 80x24 `tmux` session with the isolated home.
2. Ask the model to start a long-running tool and to report back when it finishes (e.g. "run `sleep 60 && echo done` and tell me when it completes").
3. While the task is still running, send an explicit pause message: "等会儿，我先看看别的" / "hold on, I need a moment".
4. Verify the agent ends the round by calling `WaitFor` (or stops without outputting repeated "waiting…" status text). It must NOT spam multiple text replies about waiting, and it must NOT call `AllDone` while the work is unfinished (if `AllDone` is attempted, the round shows the rejection error).
5. Send a follow-up message ("继续" / "continue"). Verify the agent wakes and keeps monitoring the background task.
6. After the task completes, verify the agent consumes the real result and ends the round with `AllDone` only once everything is genuinely done.
7. Repeat step 2 but this time send no pause message; verify the normal completion path is unchanged (task notification → new turn → `AllDone`).

## Evidence

Save the terminal captures, session path, task IDs, wire records, and verification summary under the local runtime evidence directory. Do not store API keys or provider credential files.
