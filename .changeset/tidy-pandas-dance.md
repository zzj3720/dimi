---
"@dimi-agent/cli": patch
---

Ship the Rust runtime with npm installs: the platform-specific native binding is now installed for every supported platform, and native (SEA) release binaries embed it instead of falling back to the legacy TypeScript backend.
