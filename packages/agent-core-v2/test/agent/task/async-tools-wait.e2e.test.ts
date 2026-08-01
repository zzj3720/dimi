/** Positive E2E: the real loop, wire, task, and wait services coordinate async tools. */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { IAgentProfileService } from "#/agent/profile/profile";
import { IAgentContextMemoryService } from "#/agent/contextMemory/contextMemory";
import { IAgentTaskService } from "#/agent/task/task";
import { IAgentToolActivationService } from "#/agent/toolActivation/toolActivation";
import { IAgentToolExecutorService } from "#/agent/toolExecutor/toolExecutor";
import { IAgentToolRegistryService } from "#/agent/toolRegistry/toolRegistry";
import { IAgentWaitService } from "#/agent/wait/wait";
import { ISessionInteractionService } from "#/session/interaction/interaction";
import { SessionInteractionService } from "#/session/interaction/interactionService";
import { ISessionQuestionService } from "#/session/question/question";
import { SessionQuestionService } from "#/session/question/questionService";
import type { ExecutableTool, ExecutableToolResult, ToolExecution } from "#/tool/toolContract";
import "#/agent/tools/ask-user-question/askUserQuestionTool";
import "#/agent/tools/wait-for/waitForTool";

import { createTestAgent, sessionServices, type TestAgentContext } from "../../harness";

class SlowLookupTool implements ExecutableTool<{ readonly query: string }> {
  readonly name: string;
  readonly description = "Resolve a lookup after an external completion.";
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

  constructor(name = "SlowLookup") {
    this.name = name;
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
            signal.addEventListener(
              "abort",
              () => {
                reject(signal.reason);
              },
              { once: true },
            );
          }),
        ]);
      },
    };
  }

  complete(output: string): void {
    this.resolveResult({ output });
  }
}

describe("async tools and wait end-to-end", () => {
  let ctx: TestAgentContext;

  beforeEach(async () => {
    ctx = createTestAgent(
      sessionServices((services) => {
        services.define(ISessionInteractionService, SessionInteractionService);
        services.define(ISessionQuestionService, SessionQuestionService);
      }),
    );
    ctx.get(IAgentToolExecutorService);
    ctx.get(IAgentTaskService);
    ctx.get(IAgentWaitService);
    await ctx.rpc.setPermission({ mode: "yolo" });
  });

  afterEach(async () => {
    await ctx.dispose();
  });

  it("records a slow tool, returns a placeholder, then wakes with its durable result", async () => {
    const tool = new SlowLookupTool();
    const registration = ctx.get(IAgentToolRegistryService).register(tool);
    ctx.get(IAgentProfileService).update({ activeToolNames: [tool.name] });
    ctx.mockNextResponse({
      type: "function",
      id: "call_slow_lookup",
      name: tool.name,
      arguments: JSON.stringify({ query: "moon" }),
    });
    ctx.mockNextResponse({ type: "text", text: "Observed the completed task notification." });

    try {
      await ctx.rpc.prompt({ input: [{ type: "text", text: "Look up moon asynchronously" }] });
      await tool.started;
      await ctx.untilTurnEnd();

      const [task] = ctx.get(IAgentTaskService).list(true);
      expect(task).toMatchObject({
        kind: "tool",
        status: "running",
        detached: true,
        toolCallId: "call_slow_lookup",
        toolName: tool.name,
      });
      expect(ctx.get(IAgentWaitService).active()).toMatchObject({
        source: "auto_wait",
        taskIds: [task!.taskId],
      });
      expect(JSON.stringify(ctx.llmCalls[0]?.history)).toContain("Look up moon asynchronously");
      const initialContext = JSON.stringify(ctx.get(IAgentContextMemoryService).get());
      expect(initialContext).toContain(task!.taskId);
      expect(initialContext).toContain("running");

      const completionTurn = ctx.untilTurnEnd();
      tool.complete("slow-final");
      await completionTurn;

      expect(ctx.get(IAgentWaitService).active()).toBeNull();
      expect(ctx.get(IAgentTaskService).getTask(task!.taskId)).toMatchObject({
        status: "completed",
        detached: true,
      });
      expect(await ctx.get(IAgentTaskService).readOutput(task!.taskId)).toBe("slow-final");
      const completionInput = JSON.stringify(ctx.llmCalls[1]?.history);
      expect(completionInput).toContain(`task:${task!.taskId}:completed`);
      expect(completionInput).toContain("output.log");
    } finally {
      registration.dispose();
    }
  }, 10_000);

  it("runs WaitFor through the model and an ordinary message wakes the agent early", async () => {
    ctx.get(IAgentProfileService).update({ activeToolNames: ["WaitFor"] });
    ctx.mockNextResponse({
      type: "function",
      id: "call_wait_for",
      name: "WaitFor",
      arguments: JSON.stringify({ reason: "waiting for user input", timeout_seconds: 60 }),
    });

    await ctx.rpc.prompt({ input: [{ type: "text", text: "Wait for my next message" }] });
    await ctx.untilTurnEnd();

    expect(ctx.llmCalls[0]?.tools.map((tool) => tool.name)).toContain("WaitFor");
    expect(ctx.get(IAgentWaitService).active()).toMatchObject({
      reason: "waiting for user input",
      source: "wait_for",
      timeoutSeconds: 60,
    });

    ctx.mockNextResponse({ type: "text", text: "The follow-up woke me." });
    await ctx.rpc.prompt({ input: [{ type: "text", text: "Continue now" }] });
    await ctx.untilTurnEnd();

    await vi.waitFor(() => {
      expect(ctx.get(IAgentWaitService).active()).toBeNull();
    });
    expect(JSON.stringify(ctx.llmCalls[1]?.history)).toContain("Continue now");
  });

  it("keeps a detached foreground question answerable after its originating turn ends", async () => {
    ctx.get(IAgentProfileService).update({ activeToolNames: ["AskUserQuestion"] });
    await ctx.get(IAgentToolActivationService).activate();
    expect(ctx.get(IAgentToolRegistryService).resolve("AskUserQuestion")).toBeDefined();
    ctx.mockNextResponse({
      type: "function",
      id: "call_question",
      name: "AskUserQuestion",
      arguments: JSON.stringify({
        questions: [
          {
            question: "Choose a color?",
            options: [{ label: "Red" }, { label: "Blue" }],
          },
        ],
      }),
    });
    ctx.mockNextResponse({ type: "text", text: "The user chose Blue." });

    const questions = ctx.get(ISessionQuestionService);
    await ctx.rpc.prompt({ input: [{ type: "text", text: "Ask me for a color" }] });
    await ctx.untilTurnEnd();

    ctx.get(ISessionInteractionService).cancelPendingForTurn(0);
    expect(questions.listPending()).toHaveLength(1);
    const completionTurn = ctx.untilTurnEnd();
    questions.answer("call_question", { answers: { "Choose a color?": "Blue" } });
    await completionTurn;

    expect(questions.listPending()).toEqual([]);
    expect(JSON.stringify(ctx.llmCalls[1]?.history)).toContain("Blue");
  }, 10_000);

  it("stops one detached generic tool without cancelling its sibling", async () => {
    const first = new SlowLookupTool("SlowFirst");
    const second = new SlowLookupTool("SlowSecond");
    const firstRegistration = ctx.get(IAgentToolRegistryService).register(first);
    const secondRegistration = ctx.get(IAgentToolRegistryService).register(second);
    ctx
      .get(IAgentProfileService)
      .update({ activeToolNames: [first.name, second.name, "TaskStop"] });
    ctx.mockNextResponse(
      {
        type: "function",
        id: "call_slow_first",
        name: first.name,
        arguments: JSON.stringify({ query: "first" }),
      },
      {
        type: "function",
        id: "call_slow_second",
        name: second.name,
        arguments: JSON.stringify({ query: "second" }),
      },
    );

    try {
      await ctx.rpc.prompt({ input: [{ type: "text", text: "Start both lookups" }] });
      await Promise.all([first.started, second.started]);
      await ctx.untilTurnEnd();

      const tasks = ctx.get(IAgentTaskService).list(true);
      const firstTask = tasks.find((task) => task.kind === "tool" && task.toolName === first.name);
      const secondTask = tasks.find(
        (task) => task.kind === "tool" && task.toolName === second.name,
      );
      expect(firstTask).toBeDefined();
      expect(secondTask).toBeDefined();

      ctx.mockNextResponse({
        type: "function",
        id: "call_task_stop",
        name: "TaskStop",
        arguments: JSON.stringify({ task_id: firstTask!.taskId, reason: "stop only first" }),
      });
      ctx.mockNextResponse({ type: "text", text: "Stopped only the first lookup." });
      await ctx.rpc.prompt({ input: [{ type: "text", text: "Stop only the first lookup" }] });
      await ctx.untilTurnEnd();

      expect(ctx.get(IAgentTaskService).getTask(firstTask!.taskId)).toMatchObject({
        status: "killed",
      });
      expect(ctx.get(IAgentTaskService).getTask(secondTask!.taskId)).toMatchObject({
        status: "running",
      });

      ctx.mockNextResponse({ type: "text", text: "The second lookup completed." });
      const completionTurn = ctx.untilTurnEnd();
      second.complete("second-final");
      await completionTurn;
      expect(ctx.get(IAgentTaskService).getTask(secondTask!.taskId)).toMatchObject({
        status: "completed",
      });
      expect(await ctx.get(IAgentTaskService).readOutput(secondTask!.taskId)).toBe("second-final");
    } finally {
      firstRegistration.dispose();
      secondRegistration.dispose();
    }
  }, 10_000);
});
