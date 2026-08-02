---
"@dimi-agent/cli": minor
---

Add a Context size setting that caps the conversation context window as a percentage of the model's default, in 5% steps with a 200k token floor (models below 200k keep their window). Set it under Settings → Context size in the TUI, or via `loop_control.context_size_percent` in config.toml.
