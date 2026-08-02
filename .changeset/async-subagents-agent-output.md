---
"@dimi-agent/cli": patch
---

Make subagents fully asynchronous, add the AgentOutput tool so the agent can inspect a subagent's recent transcript-style output (assistant text, thinking, tool calls, progress), and let it message a running subagent via resume — the prompt is steered into its current turn, like a human steering the agent.
