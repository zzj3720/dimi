---
"@dimi-agent/cli": patch
---

Fix user input typed while a session's history is replaying being silently dropped: `handleUserInput` rejected it with an error while `isReplaying` was set and never queued it, so a prompt typed during a slow session resume (large history) vanished. Input is now queued and flushed once the replay finishes.
