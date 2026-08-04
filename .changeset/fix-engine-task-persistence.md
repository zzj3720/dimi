---
"@dimi-agent/cli": patch
---

Persist Rust-engine task state so a restart can mark running tasks lost instead of dropping them. Engine tasks (subagents and backgrounded shell commands) were registered with the task service as non-detached because the engine runner owns the wire ops and notifications, which also skipped task persistence: no task files, no ghosts, no lost markers. After a restart the running tasks vanished without a trace and a `WaitFor` on them failed with "No subagent task found", leaving the agent stuck waiting on a subagent that can never complete. Engine tasks now persist their running and settled state (without duplicating wire events or notifications), so restarts restore them as ghosts and mark them lost, letting waits resolve and the agent re-dispatch.
