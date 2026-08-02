---
"@dimi-agent/cli": patch
---

Make subagents fully asynchronous and add the AgentOutput tool so the agent can inspect a subagent's recent transcript-style output (assistant text, thinking, tool calls, progress) while it runs, parking with WaitFor when there is nothing else to do.
