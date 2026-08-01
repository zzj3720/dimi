# Intentional completion handtest

Run this flow through the local interactive TUI. Unit tests, a headless prompt, or direct service calls do not replace it.

## Preconditions

- CLIProxyAPI is healthy on its local OpenAI-compatible endpoint.
- An isolated `KIMI_CODE_HOME` configures a Grok model through CLIProxyAPI without copying credentials into this repository.
- Start the current worktree in a controlled `tmux` terminal.

## Flow

1. Ask the model to emit a unique marker without calling tools in its first round, then obey the runtime completion review.
2. Verify the marker is rendered as an Assistant message and the internal completion-review reminder is not rendered in the TUI.
3. Verify the same user turn performs a second model round without new user input.
4. Verify the second round calls `AllDone` as its only tool call, without an approval prompt.
5. Verify the TUI becomes idle and accepts `/exit`; no third model round may occur.
6. Repeat with an active background task and verify `AllDone` is rejected until the task settles or the model deliberately calls `WaitFor`.

## Evidence

Save the model, isolated home path, session path, terminal capture, round count, tool-call sequence, and verification summary under the local runtime evidence directory. Do not store credentials.
