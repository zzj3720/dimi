Wait for a future notification when no independent work remains. Provide a concise reason and an optional timeout in seconds. The default is 60 seconds; use longer waits only for clearly long-running work, up to 1800 seconds.

Call WaitFor by itself. It waits on the current agent, not a specific task. Any later notification wakes the agent. A timeout wakes the agent with an explicit `wait_expired` message and never cancels background work.
