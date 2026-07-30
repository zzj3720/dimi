# Tool call folding handtest

Run this flow through the local interactive TUI. Component rendering and replay tests do not replace it.

## Preconditions

- Start the TUI from the current worktree in a controlled `tmux` session.
- Use a real model with tool use enabled. An isolated `KIMI_CODE_HOME` and CLIProxyAPI are suitable; do not copy credentials into the repository.
- Use a prompt that produces at least two consecutive tool calls before the next Assistant message.

## Flow

1. Ask the model to read two files, search the repository, then return a unique final marker without intermediate Assistant text.
2. Wait for the final marker. Verify the default transcript contains the User message, one tool summary such as `Used 3 tools · read 2 files · searched 1 time`, and the final Assistant message. Thinking and individual tool cards must not be visible.
3. Press `Ctrl-O`. Verify the original collapsed tool cards and collapsed thinking are visible and retain their results.
4. Press `Ctrl-O` again. Verify long tool and thinking output expands.
5. Press `Ctrl-O` a third time. Verify the transcript returns to the one-line tool summary.
6. Resume the session in a new TUI process and repeat steps 2-5 to verify replay uses the same three levels.

## Evidence

Save the model, session ID, prompt, terminal captures for all three levels, and verification summary under the local runtime evidence directory. Do not store API keys or provider credential files.
