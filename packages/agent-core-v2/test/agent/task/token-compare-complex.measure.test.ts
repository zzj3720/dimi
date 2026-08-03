/**
 * Complex multi-turn / multi-tool token measure (async worktree path).
 *
 * Scenario (scripted model, controllable tools):
 *  Wave 1 — parallel FastRead (200B, instant) + SlowSearch (8KB, blocks past 3s budget)
 *  Wave 2 — after SlowSearch settles, model does FastMeta (100B) + SlowLog (12KB) + SlowScan (4KB)
 *           only FastMeta finishes before budget; the two slow tools complete later, staggered.
 *
 * Metrics: per-LLM-call system/tools/history via estimateTokens, plus live context size.
 *
 *   MEASURE_LABEL=worktree pnpm --filter @dimi-agent/agent-core-v2 \
 *     exec vitest run test/agent/task/token-compare-complex.measure.test.ts
 */
/* eslint-disable jest/expect-expect -- measurement scenarios emit structured output */
// The Rust engine is the default runtime (`--legacy` sets DIMI_LEGACY=1);
// this suite drives the TS loop, so pin legacy mode for this file.
process.env["DIMI_LEGACY"] = "1";

import { afterEach, beforeEach, describe, it } from "vitest";

import { IAgentProfileService } from "#/agent/profile/profile";
import { IAgentContextMemoryService } from "#/agent/contextMemory/contextMemory";
import { IAgentTaskService } from "#/agent/task/task";
import { IAgentToolExecutorService } from "#/agent/toolExecutor/toolExecutor";
import { IAgentToolRegistryService } from "#/agent/toolRegistry/toolRegistry";
import { IAgentWaitService } from "#/agent/wait/wait";
import {
  estimateTokens,
  estimateTokensForMessages,
  estimateTokensForTools,
} from "#/llmProtocol/tokens";
import type { ExecutableTool, ExecutableToolResult, ToolExecution } from "#/tool/toolContract";

import { createTestAgent, type TestAgentContext } from "../../harness";

const LABEL = process.env["MEASURE_LABEL"] ?? "worktree";

class ControllableTool implements ExecutableTool<{ readonly query: string }> {
  readonly name: string;
  readonly description: string;
  readonly parameters = {
    type: "object",
    properties: { query: { type: "string" } },
    required: ["query"],
    additionalProperties: false,
  } as const;

  readonly started: Promise<void>;
  private resolveStarted!: () => void;
  private resolveResult!: (result: ExecutableToolResult) => void;
  private readonly result = new Promise<ExecutableToolResult>((resolve) => {
    this.resolveResult = resolve;
  });

  constructor(name: string, description: string) {
    this.name = name;
    this.description = description;
    this.started = new Promise((resolve) => {
      this.resolveStarted = resolve;
    });
  }

  resolveExecution(): ToolExecution {
    return {
      approvalRule: this.name,
      accesses: [],
      execute: async ({ signal }) => {
        this.resolveStarted();
        return Promise.race([
          this.result,
          new Promise<never>((_resolve, reject) => {
            signal.addEventListener("abort", () => reject(signal.reason), { once: true });
          }),
        ]);
      },
    };
  }

  complete(output: string): void {
    this.resolveResult({ output });
  }
}

function costOf(call: {
  systemPrompt?: string | undefined;
  tools: readonly { name: string; description: string; parameters: unknown }[];
  history: readonly unknown[];
}) {
  const system = estimateTokens(call.systemPrompt ?? "");
  const tools = estimateTokensForTools(call.tools as never);
  const history = estimateTokensForMessages(call.history as never);
  return {
    system,
    tools,
    history,
    total: system + tools + history,
    historyMessages: call.history.length,
  };
}

function snapshot(ctx: TestAgentContext, phase: string) {
  const calls = ctx.llmCalls.map(costOf);
  const contextTokens = estimateTokensForMessages(
    ctx.get(IAgentContextMemoryService).get() as never,
  );
  const tasks = ctx
    .get(IAgentTaskService)
    .list(false)
    .map((t) => ({
      kind: t.kind,
      status: t.status,
      detached: "detached" in t ? (t as { detached?: boolean }).detached : undefined,
      description: t.description,
    }));
  return {
    phase,
    llmCalls: calls.length,
    perCall: calls,
    sumTotal: calls.reduce((a, c) => a + c.total, 0),
    sumHistory: calls.reduce((a, c) => a + c.history, 0),
    lastHistory: calls.at(-1)?.history ?? 0,
    lastTotal: calls.at(-1)?.total ?? 0,
    contextTokens,
    peakHistory: Math.max(0, ...calls.map((c) => c.history)),
    tasks,
  };
}

function payload(char: string, bytes: number): string {
  return char.repeat(bytes);
}

describe("token compare complex (worktree async)", () => {
  let ctx: TestAgentContext;

  beforeEach(async () => {
    ctx = createTestAgent();
    ctx.get(IAgentToolExecutorService);
    ctx.get(IAgentTaskService);
    ctx.get(IAgentWaitService);
    await ctx.rpc.setPermission({ mode: "yolo" });
  });

  afterEach(async () => {
    await ctx.dispose();
  });

  it("mixed multi-wave: fast inline + slow detach + staggered completions", async () => {
    const fastRead = new ControllableTool("FastRead", "Read a small snippet quickly.");
    const slowSearch = new ControllableTool("SlowSearch", "Search a large corpus (may run long).");
    const fastMeta = new ControllableTool("FastMeta", "Tiny metadata probe.");
    const slowLog = new ControllableTool("SlowLog", "Stream a long build log.");
    const slowScan = new ControllableTool("SlowScan", "Scan many files slowly.");

    const tools = [fastRead, slowSearch, fastMeta, slowLog, slowScan];
    const regs = tools.map((t) => ctx.get(IAgentToolRegistryService).register(t));
    ctx.get(IAgentProfileService).update({
      activeToolNames: tools.map((t) => t.name),
    });

    const phases: ReturnType<typeof snapshot>[] = [];

    try {
      // ── Wave 1: model requests FastRead + SlowSearch in parallel ──
      ctx.mockNextResponse(
        {
          type: "function",
          id: "call_fast_read",
          name: fastRead.name,
          arguments: JSON.stringify({ query: "readme" }),
        },
        {
          type: "function",
          id: "call_slow_search",
          name: slowSearch.name,
          arguments: JSON.stringify({ query: "async token" }),
        },
      );

      await ctx.rpc.prompt({
        input: [{ type: "text", text: "Investigate async tool token impact" }],
      });
      await Promise.all([fastRead.started, slowSearch.started]);
      // Fast finishes under budget; slow stays open past budget → detach.
      fastRead.complete(payload("R", 200));
      await ctx.untilTurnEnd();
      phases.push(snapshot(ctx, "wave1_after_detach"));

      // SlowSearch finishes → auto-wait wakes a new turn with task notification.
      ctx.mockNextResponse({
        type: "text",
        text: "Wave1 search done; continuing investigation.",
      });
      const wake1 = ctx.untilTurnEnd();
      slowSearch.complete(payload("S", 8_000));
      await wake1;
      phases.push(snapshot(ctx, "wave1_after_slow_notification"));

      // ── Wave 2: three tools; only FastMeta finishes in budget ──
      ctx.mockNextResponse(
        {
          type: "function",
          id: "call_fast_meta",
          name: fastMeta.name,
          arguments: JSON.stringify({ query: "meta" }),
        },
        {
          type: "function",
          id: "call_slow_log",
          name: slowLog.name,
          arguments: JSON.stringify({ query: "build" }),
        },
        {
          type: "function",
          id: "call_slow_scan",
          name: slowScan.name,
          arguments: JSON.stringify({ query: "src" }),
        },
      );
      // Model needs another user/system push to take a new turn after idle.
      // The previous turn already ended with text; prompt to continue.
      await ctx.rpc.prompt({
        input: [{ type: "text", text: "Pull logs and scan sources too" }],
      });
      await Promise.all([fastMeta.started, slowLog.started, slowScan.started]);
      fastMeta.complete(payload("M", 100));
      await ctx.untilTurnEnd();
      phases.push(snapshot(ctx, "wave2_after_detach_two_slow"));

      // Stagger completions: SlowLog first, then SlowScan.
      ctx.mockNextResponse({
        type: "text",
        text: "Log arrived; still waiting on scan.",
      });
      const wakeLog = ctx.untilTurnEnd();
      slowLog.complete(payload("L", 12_000));
      await wakeLog;
      phases.push(snapshot(ctx, "wave2_after_log_notification"));

      ctx.mockNextResponse({
        type: "text",
        text: "Scan complete. Done.",
      });
      const wakeScan = ctx.untilTurnEnd();
      slowScan.complete(payload("C", 4_000));
      await wakeScan;
      phases.push(snapshot(ctx, "wave2_after_scan_notification"));

      const final = phases.at(-1)!;
      console.log(
        JSON.stringify(
          {
            measure: "complex-multi-wave",
            label: LABEL,
            path: "async-detach",
            payloads: {
              fastRead: 200,
              slowSearch: 8000,
              fastMeta: 100,
              slowLog: 12000,
              slowScan: 4000,
              totalToolOutputBytes: 200 + 8000 + 100 + 12000 + 4000,
            },
            phases,
            summary: {
              llmCalls: final.llmCalls,
              sumTotalAllCalls: final.sumTotal,
              sumHistoryAllCalls: final.sumHistory,
              finalContextTokens: final.contextTokens,
              peakHistoryInAnyCall: Math.max(...phases.map((p) => p.peakHistory)),
              peakContextAcrossPhases: Math.max(...phases.map((p) => p.contextTokens)),
            },
            note: "estimateTokens heuristic (ASCII≈4 chars/token), not provider billing",
          },
          null,
          2,
        ),
      );
    } finally {
      for (const r of regs) r.dispose();
    }
  }, 60_000);
});
