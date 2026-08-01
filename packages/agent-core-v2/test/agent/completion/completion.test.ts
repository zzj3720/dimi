import { afterEach, describe, expect, it, vi } from "vitest";

import type { IAgentScopeHandle } from "#/_base/di/scope";
import { LifecycleScope } from "#/_base/di/scope";
import SYSTEM_PROMPT_TEMPLATE from "../../../src/app/agentProfileCatalog/system.md?raw";
import { ALL_DONE_TOOL_NAME, COMPLETION_REVIEW_REMINDER } from "#/agent/completion/completion";
import { IAgentProfileService } from "#/agent/profile/profile";
import type { AgentTaskInfo } from "#/agent/task/task";
import { AllDoneTool } from "#/agent/tools/all-done/allDoneTool";
import { IAgentToolActivationService } from "#/agent/toolActivation/toolActivation";
import { IAgentToolRegistryService } from "#/agent/toolRegistry/toolRegistry";
import { IAgentWaitService } from "#/agent/wait/wait";
import { runAgentTurn } from "#/session/subagent/runAgentTurn";
import { ToolAccesses, type ExecutableTool, type ToolExecution } from "#/tool/toolContract";
import type { ToolCall } from "#/llmProtocol/message";

import { createTestAgent, type TestAgentContext } from "../../harness";

function call(id: string, name: string, args: Record<string, unknown> = {}): ToolCall {
  return { type: "function", id, name, arguments: JSON.stringify(args) };
}

async function bindAgent(ctx: TestAgentContext, tools: readonly string[] = []): Promise<void> {
  ctx.get(IAgentProfileService).update({ profileName: "agent", activeToolNames: tools });
  await ctx.get(IAgentToolActivationService).activate();
}

function currentAgentHandle(ctx: TestAgentContext): IAgentScopeHandle {
  return {
    id: "main",
    kind: LifecycleScope.Agent,
    accessor: {
      get: ((serviceId: unknown) => ctx.get(serviceId as never)) as IAgentScopeHandle["accessor"]["get"],
    },
    dispose: () => {},
  };
}

describe("intentional completion", () => {
  let ctx: TestAgentContext;

  afterEach(async () => {
    await ctx?.dispose();
  });

  it("ends a short turn on a text-only reply without the completion reminder", async () => {
    ctx = createTestAgent();
    await bindAgent(ctx);

    ctx.mockNextResponse({ type: "text", text: "Done — here is the answer." });

    const run = await runAgentTurn(
      currentAgentHandle(ctx),
      { kind: "prompt", prompt: "What is 2+2?" },
      { signal: new AbortController().signal },
    );
    await expect(run.completion).resolves.toMatchObject({ summary: "Done — here is the answer." });

    // A quick answer ends the turn directly: no continuation, no AllDone.
    expect(ctx.llmCalls).toHaveLength(1);
    const reminders = ctx.contextData().history.filter(
      (message) =>
        message.origin?.kind === "system_trigger" && message.origin.name === "completion_review",
    );
    expect(reminders).toHaveLength(0);
  });

  it("keeps the completion review protocol once a turn exceeds the short-turn threshold", async () => {
    ctx = createTestAgent();
    await ctx.rpc.setPermission({ mode: "yolo" });
    await bindAgent(ctx, ["Probe"]);
    const execute = vi.fn(async () => ({ output: "probe complete" }));
    const probe: ExecutableTool = {
      name: "Probe",
      description: "Run a test probe.",
      parameters: { type: "object", properties: {}, additionalProperties: false },
      resolveExecution: (): ToolExecution => ({
        accesses: ToolAccesses.none(),
        taskMode: "control",
        approvalRule: "Probe",
        execute,
      }),
    };
    const registration = ctx.get(IAgentToolRegistryService).register(probe);

    try {
      // 10 tool-call steps (steps 0-9) keep the turn running; the text-only
      // reply at step 10 crosses the threshold and must be reviewed.
      for (let i = 0; i < 10; i++) {
        ctx.mockNextResponse(call(`call_probe_${i}`, "Probe"));
      }
      ctx.mockNextResponse({ type: "text", text: "Everything is verified." });
      ctx.mockNextResponse(call("call_done", ALL_DONE_TOOL_NAME));

      const run = await runAgentTurn(
        currentAgentHandle(ctx),
        { kind: "prompt", prompt: "Complete the task" },
        { signal: new AbortController().signal },
      );
      await expect(run.completion).resolves.toMatchObject({ summary: "Everything is verified." });

      expect(ctx.llmCalls).toHaveLength(12);
      // The completion protocol is announced up front in the base system prompt
      // template, so the model knows about AllDone from the first round instead
      // of learning it from a post-hoc reminder. (The test harness substitutes
      // its own prompt, so assert on the template directly.)
      expect(SYSTEM_PROMPT_TEMPLATE).toContain('AllDone');
      const reminders = ctx.contextData().history.filter(
        (message) =>
          message.origin?.kind === "system_trigger" && message.origin.name === "completion_review",
      );
      expect(reminders).toHaveLength(1);
      expect(JSON.stringify(reminders)).toContain(COMPLETION_REVIEW_REMINDER.trim());
    } finally {
      registration.dispose();
    }
  });

  it("rejects AllDone mixed with another tool without skipping its sibling", async () => {
    ctx = createTestAgent();
    await ctx.rpc.setPermission({ mode: "yolo" });
    await bindAgent(ctx, ["Probe"]);
    const execute = vi.fn(async () => ({ output: "probe complete" }));
    const probe: ExecutableTool = {
      name: "Probe",
      description: "Run a test probe.",
      parameters: { type: "object", properties: {}, additionalProperties: false },
      resolveExecution: (): ToolExecution => ({
        accesses: ToolAccesses.none(),
        taskMode: "control",
        approvalRule: "Probe",
        execute,
      }),
    };
    const registration = ctx.get(IAgentToolRegistryService).register(probe);

    try {
      ctx.mockNextResponse(
        call("call_done_mixed", ALL_DONE_TOOL_NAME),
        call("call_probe", "Probe"),
      );
      ctx.mockNextResponse(call("call_done_only", ALL_DONE_TOOL_NAME));
      await ctx.rpc.prompt({ input: [{ type: "text", text: "Finish after probing" }] });
      await ctx.untilTurnEnd();

      expect(execute).toHaveBeenCalledOnce();
      expect(JSON.stringify(ctx.contextData().history)).toContain(
        "AllDone must be the only tool call in its round.",
      );
      expect(ctx.llmCalls).toHaveLength(2);
    } finally {
      registration.dispose();
    }
  });

  it("rejects AllDone while background work is active", () => {
    const activeTask: AgentTaskInfo = {
      taskId: "tool-active",
      kind: "tool",
      description: "active test task",
      status: "running",
      detached: true,
      startedAt: 1,
      endedAt: null,
      turnId: 0,
      toolCallId: "call_active",
      toolName: "Probe",
      autoWaitTimeoutSeconds: 20,
    };
    const tool = new AllDoneTool({ list: () => [activeTask] });

    expect(
      tool.resolveExecution({}, { toolCalls: [call("call_done", ALL_DONE_TOOL_NAME)] }),
    ).toMatchObject({
      isError: true,
      output: expect.stringContaining("tool-active (running)"),
    });
  });

  it("preserves WaitFor as a deliberate turn stop", async () => {
    ctx = createTestAgent();
    await ctx.rpc.setPermission({ mode: "yolo" });
    await bindAgent(ctx, ["WaitFor"]);
    ctx.mockNextResponse(
      call("call_wait", "WaitFor", { reason: "background work is still running", timeout_seconds: 60 }),
    );

    await ctx.rpc.prompt({ input: [{ type: "text", text: "Monitor the background work" }] });
    await ctx.untilTurnEnd();

    expect(ctx.llmCalls).toHaveLength(1);
  });
});
