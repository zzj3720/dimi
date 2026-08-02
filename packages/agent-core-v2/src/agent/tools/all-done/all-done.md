Declare that every user requirement is complete, all necessary results are verified, the todo list is up to date (every tracked task marked done or cleared), and no work remains.

Call this tool **proactively** as soon as your work is done — end with this tool call, not with a text-only reply, when everything is complete. Do not wait for a reminder before calling it. Call `AllDone` by itself. The call is rejected when it appears with another tool call or while a background task is still active.

When you are explicitly waiting — the user asked you to pause, your next step needs their input, or you are blocked on external state — do NOT keep replying with status updates. Call `WaitFor` directly and wait instead; repeated "waiting" replies are noise, and `AllDone` is only for when the work is actually complete.
