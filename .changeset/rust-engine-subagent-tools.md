---
"@dimi-agent/cli": patch
---

Fix subagents (Agent tool) not receiving the tool definitions in their LLM request, so they did not know the available tools existed (e.g. claimed there were no file-reading tools); subagent requests now advertise the parent's tool set.
