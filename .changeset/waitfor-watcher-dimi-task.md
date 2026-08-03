---
"@dimi-agent/cli": patch
---

Require external-state watchers to run as dimi background tasks so their completion wakes the wait; detached processes (nohup, cron) are invisible to dimi and no longer suggested in the WaitFor guidance.
