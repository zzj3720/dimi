---
"@dimi-agent/cli": patch
---

Surface silent turn-start failures: a RustTurnSession constructor error (e.g. a missing native binding after a rebuild) was swallowed by the turn runner's `catch(() => undefined)`, leaving the UI waiting forever with the user message recorded but no turn running. The failure is now logged through the runner's error log instead of vanishing.
