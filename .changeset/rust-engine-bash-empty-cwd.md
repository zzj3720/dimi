---
"@dimi-agent/cli": patch
---

Fix every Bash tool call failing with "No such file or directory" when the session working directory resolved to an empty string; the shell now falls back to the process working directory.
