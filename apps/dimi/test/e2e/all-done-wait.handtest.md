# AllDone / WaitFor waiting-state guidance handtest

Run this flow through the local interactive TUI with a real model. The tool description is consumed by the model, so scripted or stubbed LLM responses cannot verify it.

## Preconditions

- A real model endpoint that reads tool descriptions (any provider configured for the isolated home).
- An isolated `DIMI_CODE_HOME` so the session does not touch real credentials.
- The repository build used by the TUI contains the current worktree (the `all-done.md` guidance edit).
- A long-running tool is available (e.g. `Bash` with `sleep 60`, or a slow MCP probe) so the agent has something to wait on.
- When driving the TUI from `tmux`, `set -g extended-keys on` first; otherwise the Enter key does not reach the editor. `tmux send-keys -t <session> "<text>"` followed by a separate `tmux send-keys -t <session> Enter` sends a message reliably.

## Flow

1. Start the TUI in an 80x24 `tmux` session with the isolated home.
2. Ask the model to start a long-running tool and to report back when it finishes (e.g. "run `sleep 60 && echo done` and tell me when it completes"). The tool is detached to the background after the foreground budget and the agent waits for it.
3. While the task is still running, send an explicit pause message: "等会儿，我先看看别的" / "hold on, I need a moment".
4. Verify the agent replies with at most one short acknowledgment and then ends the round by calling `WaitFor` (footer shows `Waiting Ns / …` with the agent's reason). It must NOT spam repeated "waiting…" text replies, and it must NOT call `AllDone` while work is unfinished (if `AllDone` is attempted, the round shows the rejection error).
5. Send a follow-up message ("继续" / "continue"). Verify the agent wakes from the wait and keeps working or reports the task result.
6. After the task completes, verify the agent consumes the real result and ends the round with `AllDone` only once everything is genuinely done.
7. Repeat step 2 but this time send no pause message; verify the normal completion path is unchanged (task notification → new turn → `AllDone`).

## Evidence

Save the terminal captures, session path, task IDs, wire records, and verification summary under the local runtime evidence directory. Do not store API keys or provider credential files.
