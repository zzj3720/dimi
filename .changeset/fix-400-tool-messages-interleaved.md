---
"@dimi-agent/cli": patch
---

Fix HTTP 400 "insufficient tool messages following tool_calls message" from strict providers (DeepSeek/OpenAI): an async notification that landed between an assistant `tool_calls` and its tool result was sent in that order, and the engine's exchange-closing pass only checked global result existence. Tool results are now reordered back right after their assistant (TS contextProjector slot semantics), unresolved results keep the synthesized interrupted message, and orphan/duplicate results are dropped. Partial assistant messages left by an interrupted step are also filtered out on the TS→engine boundary (strict providers reject an empty assistant message).
