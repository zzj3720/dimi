# Remote slash command handtest

Run this flow through the local interactive TUI. Unit tests and the standalone `remote` CLI do not replace it.

## Preconditions

- Start the TUI from the current worktree in a controlled `tmux` session.
- `wss://relay.k.3720.org` is reachable.
- Use an isolated `KIMI_CODE_HOME` if the default home is in use by another process.

## Flow

1. Enter `/remote pair` in the slash menu.
2. Verify the transcript shows a pairing QR code and URI, then pair the Android app.
3. Verify the footer progresses through `remote connecting` and displays `remote online` while the bridge is connected.
4. Enter `/remote stop` and verify the footer no longer contains a remote item while the Android app retains the runtime.
5. Enter `/remote start` and verify it does not show a QR code. Confirm the paired Android app reconnects automatically.
6. Enter `/remote start` again and verify it reports that remote access is already running without starting a second bridge or creating a pairing code.
7. Enter `/remote stop` twice and verify the second call reports that remote access is already stopped.
8. Start remote access once more, then exit the TUI. Verify the bridge and any local server owned by the TUI are closed.

## Evidence

Save terminal captures for the online and stopped states plus the pairing URI with secrets removed. Record the worktree commit and relay URL under the local runtime evidence directory.
