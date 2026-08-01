---
"@dimi-agent/cli": minor
---

Add experimental stdin support for background Bash tasks. Enable `background-bash-stdin`, start Bash with `stdin_mode="pipe"`, and let the agent use `TaskInput` to write input or send EOF.
