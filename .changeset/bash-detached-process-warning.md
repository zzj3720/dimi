---
"@dimi-agent/cli": patch
---

Warn when a Bash command leaves processes running outside dimi control (e.g. via `nohup` or `&`): the result lists them and explains they are not tracked, cannot be stopped with TaskStop, and WaitFor only wakes on timeout.
