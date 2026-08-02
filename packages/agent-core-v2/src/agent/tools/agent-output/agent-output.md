Read the recent rendered output of a subagent — the same transcript-style view a human sees in the TUI: its latest assistant text, thinking, tool calls, and task progress, in time order.

Subagents run fully asynchronously. Use this tool to check on one:

- While you still have other work, do that work instead — the subagent's result (and completion notification) arrives on its own.
- When you have nothing else to do and want to see what the subagent is doing, call this tool with the `agent_id` returned by the Agent tool.
- If it is still working and you want to park until it progresses, call `WaitFor` with a reasonable `timeout_seconds` instead of polling this tool in a loop; the wait wakes you on the completion notification or the timeout, then check again with this tool.

Pass `tail_lines` to control how much of the recent activity to show (default 60). The `agent_id` is the Agent tool's `agent_id` parameter value (e.g. "agent-6"), NOT the `task_id` from its output.
