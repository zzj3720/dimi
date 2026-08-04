---
"@dimi-agent/cli": patch
---

Fix the Rust engine sending the qualified "provider/model" alias as the request model id; strict providers (e.g. OpenCode) rejected it with HTTP 401, so the bare catalog model id is sent instead.
