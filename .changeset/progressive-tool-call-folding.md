---
"@dimi-agent/cli": patch
---

Fold tool calls progressively in the TUI: while a tool-call run is still growing, only the newest two calls stay expanded and older finished ones merge into the summary immediately, instead of everything staying expanded until the run ends. Running calls (streaming args, live subagents, pending reads) are never folded away.
