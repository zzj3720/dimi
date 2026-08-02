---
"@dimi-agent/cli": patch
---

Ship the Rust runtime binary in the npm package, and fall back to the TypeScript backends with a warning when it is unavailable instead of crashing at startup.
