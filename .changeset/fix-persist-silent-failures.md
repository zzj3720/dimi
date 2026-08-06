---
"@dimi-agent/cli": patch
---

Log task and session metadata persistence failures instead of swallowing them. `AgentTaskService` and `SessionMetadata` both discarded storage errors with bare `catch(() => {})` callbacks, so a broken persistence path (e.g. after a failed write) silently stopped updating task files and `state.json` with no trace in any log. Failures are now reported through the log service while the write queues keep their continuation semantics.
