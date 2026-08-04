---
"@dimi-agent/cli": patch
---

Fix subagent tasks (Agent tool) completing instantly without doing any work: the tool definitions advertised to the nested subagent LLM were snapshotted at session construction, when the tool registry is still empty (TS hands an empty `tools` array and the bridge re-syncs the defs before every run/resume). The snapshot stayed empty forever, so subagent requests carried no tools and the model fell back to fabricated call formats (DSML), "completing" with 0 tool executions. Subagent tool defs are now read from a shared cell that every run/resume re-sync writes.
