Launch a subagent to handle a task. The subagent runs as a same-process loop instance with its own context and wire file. Delegating also keeps the bulk of intermediate file contents out of your own context — you get a conclusion back instead of a pile of dumps.

The subagent is **fully asynchronous**: this tool returns immediately with the subagent's `agent_id` and a `task_id` — it never blocks your turn. Do the rest of your work while it runs; its result arrives on its own as a completion notification with the final summary. You do not need to poll, sleep, or check on it — and never fabricate or predict what the result will say.

When you genuinely have nothing else to do and want to see how it is going:

- Call `AgentOutput(agent_id="...")` to read its recent rendered output (assistant text, thinking, tool calls, progress) — the same view a human sees in the TUI.
- If it is still working, call `WaitFor` with a reasonable `timeout_seconds` instead of polling `AgentOutput` in a loop; the wait wakes you on the completion notification or the timeout, then check again with `AgentOutput`.

Writing the prompt:
- The subagent starts with zero context — it has not seen this conversation. Brief it like a colleague who just walked into the room: state the goal, list what you already know, hand over the specifics.
- Lookups (read this file, run that test): put the exact path or command in the prompt. The subagent should not have to search for things you already know.
- Investigations (figure out X, find why Y): give the question, not prescribed steps — fixed steps become dead weight when the premise is wrong.
- Do not delegate understanding. If the task hinges on a file path or line number, find it yourself first and write it into the prompt.

Usage notes:
- When the task continues earlier work a subagent already did, prefer resuming that agent (pass its `resume` id) over spawning a fresh instance — the resumed agent keeps its prior context. `resume` works exactly like a human steering the agent: while the subagent is still running, your prompt is injected into its current turn immediately; when it is idle, it starts a normal turn. Use it to redirect, follow up, or send it new information.
- A subagent's result is only visible to you, not to the user. When the user needs to see what a subagent produced, summarize the relevant parts yourself in your own reply.
- Subagents use a fixed 2-hour timeout. If one times out, resume the same agent instead of starting over.

When NOT to use Agent: skip delegation for trivial work you can do directly — reading a file whose path you already know, searching a small known set of files, or any task that takes only a step or two. Delegation has a context-handoff cost; it pays off only when the task is substantial enough to outweigh it.

Once a subagent is running, leave that scope to it: do not redo its searches or reads in parallel, and do not abandon it midway and finish the job manually. Both undo the context savings the delegation was meant to buy.
