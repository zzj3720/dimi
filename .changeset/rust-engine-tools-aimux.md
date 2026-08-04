---
"@dimi-agent/cli": patch
---

Fix tool definitions being dropped from LLM requests in the Rust engine, which made models write tool calls as literal XML text instead of calling tools; tools are now sent in the format the request path parses.
