---
"@dimi-agent/cli": patch
---

Fix the WaitFor wait card keeping its elapsed timer running after a notification wakes the wait; the elapsed time is now frozen when the wait ends.
