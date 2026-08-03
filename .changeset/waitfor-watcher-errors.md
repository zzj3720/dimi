---
"@dimi-agent/cli": patch
---

Make the WaitFor guidance surface watcher errors: a background poll script must print failures and exit non-zero so the wait wakes with the real reason instead of silently timing out.
