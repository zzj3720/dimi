---
"@dimi-agent/cli": patch
---

Fix the footer CH badge and /usage panel under-reporting the prompt cache hit rate for OpenAI-compatible providers, where cached input tokens were counted twice.
