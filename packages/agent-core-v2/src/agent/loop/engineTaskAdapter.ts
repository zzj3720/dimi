/**
 * `EngineTaskAdapter` — the `AgentTask` adapter the Rust-engine runner
 * registers with `IAgentTaskService` for every engine background task
 * (`task.started` transport event), so TaskList / TaskOutput / TaskStop can
 * see and control engine tasks exactly like TS-owned tasks.
 *
 * The engine is the wire-op owner: the runner dispatches `task.started` /
 * `task.terminated` itself and delivers the completion notification itself,
 * so the adapter registers `detached: false` (the task service then skips its
 * own start/terminated records and notification — `recorded: false`) and the
 * adapter's `toInfo` marks the task `detached: true` (it IS a background
 * task) for TaskList rendering.
 *
 * Lifecycle:
 * - `start(sink)` (called by the service at registration) captures the sink,
 *   bridges an abort (TaskStop / session close) into the engine's per-task
 *   cancel, and resolves when the runner settles the task (`complete`).
 * - `appendOutput(delta)` (called by the runner on the engine's `task.output`
 *   events) streams live output into the sink, so TaskOutput shows partial
 *   output while the task is still running (TS ProcessTask parity). Deltas
 *   arriving before `start` are buffered.
 * - `complete(output, settlement, exitCode?)` (called by the runner on the
 *   engine's `task.settled`) appends only the not-yet-streamed tail of the
 *   final output (the streamed deltas already landed in the sink) and settles
 *   the registry entry, so TaskList stops showing it as running.
 * - `forceStop()` (the service's kill path) re-invokes the engine cancel —
 *   the abort listener already fired it, this is the graceful-exit fallback.
 */

import type {
  AgentTask,
  AgentTaskInfo,
  AgentTaskInfoBase,
  AgentTaskSettlement,
  AgentTaskSink,
} from "#/agent/task/types";

export interface EngineTaskAdapterOptions {
  /** The engine's wire task id (`agent-<8>` / `bash-<8>`). */
  readonly taskId: string;
  /** The engine's subagent id (`agent-<n>`) — `kind === "agent"` only. */
  readonly agentId?: string;
  readonly kind: "agent" | "process";
  readonly description: string;
  /** Bash background tasks carry a pid from `task.started`. */
  readonly pid?: number;
  /** Subagent profile-ish label (the Agent-tool description today). */
  readonly subagentType?: string;
  /** Bridge the engine's per-task cancel (TaskStop parity). The optional
   *  reason is the TaskStop stop reason (the abort signal's reason, when it
   *  is a string / Error) — the engine settles "killed" with it on the wire,
   *  matching the task-service entry's stopReason. */
  readonly forceStop: (reason?: string) => void;
}

/** Extract a human-readable stop reason from an AbortSignal's abort reason:
 *  TaskStop aborts with the normalized reason string; `stopByUser` aborts
 *  with a `UserCancellationError` (its `.message` is the user-facing reason);
 *  anything else (undefined / session close) yields no reason — the engine
 *  falls back to "Stopped by TaskStop". */
function abortReasonString(reason: unknown): string | undefined {
  if (typeof reason === "string") return reason;
  if (reason instanceof Error) return reason.message;
  return undefined;
}

export class EngineTaskAdapter implements AgentTask {
  readonly idPrefix: string;
  readonly kind: "agent" | "process";
  readonly description: string;

  private sink: AgentTaskSink | undefined;
  private pendingOutput = "";
  private pendingSettlement: AgentTaskSettlement | undefined;
  private pendingExitCode: number | null | undefined;
  private exitCode: number | null | undefined;
  private settleResolve: (() => void) | undefined;
  private readonly settled: Promise<void>;
  private removeAbortListener: (() => void) | undefined;
  /** UTF-8 bytes already pushed to the sink (or buffered pre-start). The
   *  engine guarantees its `task.output` deltas concatenate byte-for-byte to
   *  the settle's full output — the bash poller seeds a backgrounded command
   *  with its pre-timeout foreground output as the FIRST delta, and the
   *  subagent worker streams the nested turn's assistant deltas verbatim —
   *  so `complete` appends only the not-yet-streamed tail. */
  private streamedBytes = 0;

  constructor(private readonly options: EngineTaskAdapterOptions) {
    this.idPrefix = options.kind === "agent" ? "agent" : "bash";
    this.kind = options.kind;
    this.description = options.description;
    this.settled = new Promise<void>((resolve) => {
      this.settleResolve = resolve;
    });
  }

  async start(sink: AgentTaskSink): Promise<void> {
    this.sink = sink;
    const requestStop = (): void => {
      // The abort event's reason is the TaskStop stop reason
      // (`taskService.terminateWithGrace` aborts the entry's controller with
      // the normalized reason). Thread it into the engine's per-task cancel
      // so the wire `task.terminated` stopReason matches the service entry.
      this.options.forceStop(abortReasonString(sink.signal.reason));
    };
    if (sink.signal.aborted) {
      requestStop();
    } else {
      sink.signal.addEventListener("abort", requestStop, { once: true });
      this.removeAbortListener = () => {
        sink.signal.removeEventListener("abort", requestStop);
      };
    }
    // The runner may have settled the task before the service invoked start
    // (engine `task.settled` raced registration): flush the buffered state.
    this.flush();
    await this.settled;
  }

  async forceStop(): Promise<void> {
    this.options.forceStop();
  }

  /** Runner-side live output (engine `task.output` events): stream the delta
   *  straight into the service sink so TaskOutput shows partial output while
   *  the task runs. Deltas racing `start()` (settle/stream before the
   *  service invoked start) are buffered and flushed with the settle. */
  appendOutput(delta: string): void {
    this.streamedBytes += Buffer.byteLength(delta, "utf-8");
    if (this.sink !== undefined) {
      this.sink.appendOutput(delta);
    } else {
      this.pendingOutput += delta;
    }
  }

  /** Runner-side settle: record the final output/exit code and settle the
   *  registry entry (the runner dispatches the wire `task.terminated` and
   *  delivers the notification itself — the service's `recorded: false`
   *  entry only drives TaskList/TaskStop). The settle output is the engine's
   *  full accumulated output; the deltas streamed during the run already
   *  landed in the sink, so only the not-yet-streamed tail is appended (the
   *  concatenated deltas equal the settle output byte-for-byte). */
  complete(
    output: string,
    settlement: AgentTaskSettlement,
    exitCode?: number | null,
  ): void {
    this.pendingSettlement = settlement;
    this.pendingExitCode = exitCode ?? null;
    const fullBytes = Buffer.byteLength(output, "utf-8");
    const streamed = this.streamedBytes;
    if (streamed < fullBytes) {
      const tail = Buffer.from(output, "utf-8").subarray(streamed).toString("utf-8");
      if (this.sink !== undefined) {
        this.sink.appendOutput(tail);
      } else {
        this.pendingOutput += tail;
      }
    }
    this.flush();
  }

  toInfo(base: AgentTaskInfoBase): AgentTaskInfo {
    if (this.kind === "agent") {
      return {
        ...base,
        kind: "agent",
        agentId: this.options.agentId ?? "",
        subagentType: this.options.subagentType ?? this.options.description,
        detached: true,
      } as AgentTaskInfo;
    }
    return {
      ...base,
      kind: "process",
      command: this.description,
      pid: this.options.pid ?? 0,
      exitCode: this.exitCode ?? null,
      detached: true,
    } as AgentTaskInfo;
  }

  private flush(): void {
    const sink = this.sink;
    const settlement = this.pendingSettlement;
    if (sink === undefined || settlement === undefined) return;
    this.exitCode = this.pendingExitCode ?? null;
    if (this.pendingOutput.length > 0) sink.appendOutput(this.pendingOutput);
    this.pendingOutput = "";
    this.pendingSettlement = undefined;
    this.removeAbortListener?.();
    this.removeAbortListener = undefined;
    void sink.settle(settlement);
    this.settleResolve?.();
  }
}
