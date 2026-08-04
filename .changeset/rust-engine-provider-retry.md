---
"@dimi-agent/cli": patch
---

Fix the Rust engine not retrying some transient provider errors (rate limits, overloads, connection failures) that should be retried per step.
