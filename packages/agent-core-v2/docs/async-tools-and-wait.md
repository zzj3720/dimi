# Async tools and WaitFor

This document defines the runtime contract for durable generic tool execution and explicit waiting. The implementation extends Kimi Code's existing wire, task, loop, and TUI projections; it does not add a second agent loop or a second task registry.

## Required behavior

For every runnable provider tool call, the runtime records a task before the tool may produce side effects. Calls in one model response share one three-second foreground budget. Calls that finish inside the budget return their real results; calls still running or queued at the deadline return a placeholder containing their task ID and continue under the existing resource scheduler. Model-facing results stay in provider order.

The resource scheduler retains a call's declared locks until the real execution settles. Returning a placeholder never releases a lock and never cancels the tool.

When a detached call settles, the final normalized result is persisted as task output and delivered through the existing task notification request with `activeOrNewTurn` admission. A foreground completion is recorded but does not create a duplicate notification. `TaskList`, `TaskOutput`, `TaskStop`, transcript projection, and the TUI continue to read the existing task model.

When one or more calls cross the foreground deadline, the runtime records one 20-second auto wait for the batch and ends the current turn. A detached task completion or any later user-facing notification ends that wait. If the deadline wins, the runtime sends an explicit timeout notification without cancelling any task.

`WaitFor` accepts only a human-readable `reason` and an optional `timeout_seconds` (default 60, range 1–1800). It replaces the current active wait for the Agent and ends the current turn immediately. Any later user-facing notification or the deadline ends that wait. Task completion uses the existing task notification as the wake message; an ordinary User message is itself the wake message; only a timeout creates a wait notification.

## Durable state

`wire.jsonl` is the replay authority:

- `task.started` is flushed before a runnable tool starts.
- `task.terminated` records its real terminal state and bounded output tail.
- `wait.started` records `waitId`, `reason`, `source`, `timeoutSeconds`, `startedAt`, and `deadlineAt`. Auto waits may additionally record the detached batch's task IDs for diagnostics; task IDs are never an input to `WaitFor`.
- `wait.terminated` records the matching `waitId` and wake reason.

Per-task JSON and output files remain the full-fidelity task read path. Promises, scheduler queues, timers, abort controllers, and waiter callbacks are live resources and never enter wire Models or scope state snapshots.

On restore, every non-terminal task becomes `lost`; the runtime does not replay a tool with possible side effects. An active wait is then reconciled against those terminal task facts. A restored deadline uses its persisted `waitId`; a timer whose ID no longer matches the active wait is ignored.

## Placement

```text
domain: toolExecutor (Agent, L3)
├─ owns: preflight, resource scheduling, shared foreground budget, result ordering
├─ exposes: one lifecycle-controller registration point
└─ depends: no task or wait domain

domain: task (Agent, L5)
├─ owns: durable tool-task identity, output, stop, detach, terminal notification
├─ registers: toolExecutor lifecycle controller and starts one batch auto wait
└─ depends: toolExecutor hook, wire, storage, loop notification

domain: wait (Agent, L4)
├─ owns: one durable active wait and its stale-safe deadline
├─ observes: task.terminated and context.spliced facts
└─ depends: wire, event bus, loop timeout notification

domain: WaitFor tool (Agent edge)
└─ starts wait through the wait Service and returns stopTurn immediately
```

## Acceptance gates

Automated behavior coverage must prove:

1. the whole runnable batch is durably recorded before the first execution starts;
2. the three-second budget is shared, results preserve provider order, and queued calls continue after placeholder return;
3. detached completion persists the final output and wakes the loop once;
4. `TaskStop` cancels one running generic tool without cancelling its siblings;
5. one detached batch creates one auto wait, only one wait is active, a new wait replaces it, stale deadlines are ignored, and timeout does not cancel tasks;
6. task completion, ordinary User input, timeout, and restart reconciliation each terminate waits correctly;
7. a positive CLI/TUI E2E observes the placeholder, `/tasks` projection, automatic completion turn, and a `WaitFor` wake through the real server/client entry.

The final handtest must run the local TUI against CLIProxyAPI with a Grok model, execute a tool longer than three seconds, observe the task in the TUI, invoke `WaitFor`, and verify automatic continuation with the real tool result. The evidence must record the isolated Kimi home, model identifier, session path, terminal transcript, and exact verification commands without recording credentials.
