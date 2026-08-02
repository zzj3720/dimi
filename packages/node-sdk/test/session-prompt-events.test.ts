/**
 * Scenario: prompt-driven session behavior, including historical-turn forks.
 * Responsibilities: public SDK events, persisted replay, metadata, and input errors.
 * Wiring: real in-process core/storage with only the remote model provider stubbed.
 * Run: pnpm exec vitest run packages/node-sdk/test/session-prompt-events.test.ts
 */
import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

import { createDimiDefaultHeaders } from "@dimi-agent/dimi-oauth";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  createDimiHarness as createBaseHarness,
  type Event,
  type DimiHarness,
  type DimiHarnessOptions,
} from "#/index";

import {
  createFakeProviderHarness,
  type FakeProviderHarness,
} from "../../../test/fixtures/fake-provider-harness";
import { createTestProviderRuntime } from "./session-runtime-helpers";
import { TEST_IDENTITY } from "./test-identity";

const tempDirs: string[] = [];
let provider: FakeProviderHarness;
let responseText: string;

beforeEach(async () => {
  responseText = "hello from fake provider";
  provider = await createFakeProviderHarness();
  provider.route("POST", "/v1/chat/completions", async (_request, reply) => {
    await reply.sseJson(200, [
      completionChunk({ content: responseText }),
      completionChunk({}, "stop"),
    ]);
  });
});

afterEach(async () => {
  await provider.close();
  for (const dir of tempDirs.splice(0)) {
    await removeTempDir(dir);
  }
});

async function makeTempDir(): Promise<string> {
  const dir = await mkdtemp(join(tmpdir(), "dimi-sdk-prompt-"));
  tempDirs.push(dir);
  return dir;
}

function createDimiHarness(options: DimiHarnessOptions): DimiHarness {
  const homeDir = options.homeDir;
  if (homeDir === undefined) throw new Error("prompt integration tests require homeDir");
  return createBaseHarness({
    ...options,
    providerRuntime: createTestProviderRuntime({
      providerId: "kimi-coding",
      modelId: "fake-model",
      baseUrl: `${provider.baseUrl}/v1`,
      model: {
        headers: createDimiDefaultHeaders({ homeDir, ...TEST_IDENTITY }),
      },
    }),
  });
}

async function removeTempDir(dir: string): Promise<void> {
  for (let attempt = 0; attempt < 10; attempt += 1) {
    try {
      await rm(dir, { recursive: true, force: true });
      return;
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      if (code !== "ENOTEMPTY" && code !== "EBUSY" && code !== "EPERM") {
        throw error;
      }
      await delay(10);
    }
  }

  await rm(dir, { recursive: true, force: true });
}

describe("Session.prompt events", () => {

  it("persists sanitized prompt metadata without marking the title custom", async () => {
    const homeDir = await makeTempDir();
    const workDir = await makeTempDir();
    const harness = createDimiHarness({
      identity: TEST_IDENTITY,
      homeDir,
    });

    try {
      await configureFakeProvider(harness);
      const session = await harness.createSession({ id: "ses_prompt_meta", workDir });
      const events: Event[] = [];
      const unsubscribe = session.onEvent((event) => {
        events.push(event);
      });

      let done = waitForEvent(session, (event) => event.type === "turn.ended");
      await session.prompt("use api_key=secret-value for the request");
      await done;

      const statePath = join(session.summary!.sessionDir, "state.json");
      const firstState = JSON.parse(await readFile(statePath, "utf-8")) as Record<string, unknown>;
      expect(firstState["title"]).toBe("use api_key=[redacted] for the request");
      expect(firstState["isCustomTitle"]).toBe(false);
      expect(firstState["lastPrompt"]).toBe("use api_key=[redacted] for the request");
      expect(events).toContainEqual(
        expect.objectContaining({
          type: "session.meta.updated",
          title: "use api_key=[redacted] for the request",
          patch: expect.objectContaining({
            isCustomTitle: false,
            lastPrompt: "use api_key=[redacted] for the request",
          }),
        }),
      );

      events.length = 0;
      done = waitForEvent(session, (event) => event.type === "turn.ended");
      await session.prompt("second prompt");
      await done;

      const secondState = JSON.parse(await readFile(statePath, "utf-8")) as Record<string, unknown>;
      expect(secondState["title"]).toBe("use api_key=[redacted] for the request");
      expect(secondState["isCustomTitle"]).toBe(false);
      expect(secondState["lastPrompt"]).toBe("second prompt");
      expect(events).toContainEqual(
        expect.objectContaining({
          type: "session.meta.updated",
          patch: expect.objectContaining({
            lastPrompt: "second prompt",
          }),
        }),
      );

      events.length = 0;
      done = waitForEvent(session, (event) => event.type === "turn.ended");
      await session.prompt([{ type: "image_url", imageUrl: { url: "https://example.com/a.png" } }]);
      await done;
      unsubscribe();

      const mediaState = JSON.parse(await readFile(statePath, "utf-8")) as Record<string, unknown>;
      expect(mediaState["title"]).toBe("use api_key=[redacted] for the request");
      expect(mediaState["lastPrompt"]).toBe("[image]");
      expect(events).toContainEqual(
        expect.objectContaining({
          type: "session.meta.updated",
          patch: expect.objectContaining({
            lastPrompt: "[image]",
          }),
        }),
      );
    } finally {
      await harness.close();
    }
  });

  it("emits mapped turn events through Session.onEvent", async () => {
    const homeDir = await makeTempDir();
    const workDir = await makeTempDir();
    const harness = createDimiHarness({
      identity: TEST_IDENTITY,
      homeDir,
    });

    try {
      await configureFakeProvider(harness);
      const session = await harness.createSession({ id: "ses_prompt_events", workDir });
      const events: Event[] = [];
      const done = waitForEvent(session, (event) => event.type === "turn.ended");
      const unsubscribe = session.onEvent((event) => {
        events.push(event);
      });

      await session.prompt("hello");
      await done;
      unsubscribe();

      expect(events.some((event) => event.type === "turn.started")).toBe(true);
      expect(events).toContainEqual(
        expect.objectContaining({
          type: "assistant.delta",
          sessionId: session.id,
          turnId: 0,
          delta: "hello from fake provider",
        }),
      );
      expect(events).toContainEqual(
        expect.objectContaining({
          type: "turn.ended",
          sessionId: session.id,
          turnId: 0,
          reason: "completed",
        }),
      );
      const request = provider.requests[0];
      const systemMessage = requestMessages(request?.bodyJson)[0];
      expect(systemMessage).toMatchObject({
        role: "system",
        content: expect.stringContaining("# Tool Use"),
      });
      expect(JSON.stringify(systemMessage)).not.toContain("Dimi CLI");
      expect(JSON.stringify(systemMessage)).toContain("Available skills");
      expect(request?.headers["user-agent"]).toBe("dimi-cli/0.0.0-test");
      expect(request?.headers["x-msh-platform"]).toBe("kimi_code_cli");
      expect(existsSync(join(homeDir, "device_id"))).toBe(true);
    } finally {
      await harness.close();
    }
  });

  it("lets the agent write to a real background process and send EOF", async () => {
    const homeDir = await makeTempDir();
    const workDir = await makeTempDir();
    const script =
      'process.stdin.setEncoding("utf8");let data="";process.stdin.on("data",chunk=>data+=chunk);process.stdin.on("end",()=>process.stdout.write("received:"+data))';
    provider.route("POST", "/v1/chat/completions", async (request, reply) => {
      if (request.index === 0) {
        await reply.sseJson(200, [
          toolCallChunk("call-bash-stdin", "Bash", {
            command: `${JSON.stringify(process.execPath)} -e ${JSON.stringify(script)}`,
            description: "stdin e2e reader",
            run_in_background: true,
            stdin_mode: "pipe",
          }),
          completionChunk({}, "tool_calls"),
        ]);
        return;
      }
      if (request.index === 1) {
        const taskId = JSON.stringify(requestMessages(request.bodyJson)).match(
          /task_id: (bash-[0-9a-z]{8})/,
        )?.[1];
        if (taskId === undefined) throw new Error("Bash result did not contain a task id");
        await reply.sseJson(200, [
          toolCallChunk("call-task-input", "TaskInput", {
            task_id: taskId,
            input: "hello world\n",
            close_stdin: true,
          }),
          completionChunk({}, "tool_calls"),
        ]);
        return;
      }
      await reply.sseJson(200, [
        completionChunk({ content: "stdin complete" }),
        completionChunk({}, "stop"),
      ]);
    });

    const harness = createDimiHarness({ identity: TEST_IDENTITY, homeDir });
    try {
      await harness.setConfig({ experimental: { "background-bash-stdin": true } });
      expect(await harness.getExperimentalFeatures()).toContainEqual(
        expect.objectContaining({ id: "background-bash-stdin", enabled: true }),
      );
      await configureFakeProvider(harness);
      const session = await harness.createSession({
        id: "ses_background_bash_stdin",
        workDir,
        permission: "auto",
      });
      const terminated = waitForEvent(
        session,
        (event) => event.type === "task.terminated" && event.info.kind === "process",
        10_000,
      );
      const turnEnded = waitForEvent(session, (event) => event.type === "turn.ended", 10_000);

      await session.prompt("Send hello world to a background stdin reader and close stdin.");
      await Promise.all([terminated, turnEnded]);

      const task = (await session.listBackgroundTasks()).find((entry) => entry.kind === "process");
      expect(task).toMatchObject({ status: "completed", exitCode: 0 });
      expect(await session.getBackgroundTaskOutput(task!.taskId)).toBe("received:hello world\n");
      expect(JSON.stringify(provider.requests[0]?.bodyJson)).toContain('"name":"TaskInput"');
    } finally {
      await harness.close();
    }
  }, 15_000);

  it("supports onEvent unsubscribe without touching runtime wire directly", async () => {
    const homeDir = await makeTempDir();
    const workDir = await makeTempDir();
    const harness = createDimiHarness({
      identity: TEST_IDENTITY,
      homeDir,
    });

    try {
      await configureFakeProvider(harness);
      const session = await harness.createSession({ id: "ses_prompt_unsubscribe", workDir });
      const unsubscribedEvents: Event[] = [];
      const unsubscribe = session.onEvent((event) => {
        unsubscribedEvents.push(event);
      });
      unsubscribe();
      const done = waitForEvent(session, (event) => event.type === "turn.ended");

      await session.prompt([{ type: "text", text: "hello" }]);
      await done;

      expect(unsubscribedEvents).toEqual([]);
    } finally {
      await harness.close();
    }
  });

  it("runs init through generateAgentsMd RPC as a subagent system trigger without prompt metadata updates", async () => {
    const homeDir = await makeTempDir();
    const workDir = await makeTempDir();
    const harness = createDimiHarness({
      identity: TEST_IDENTITY,
      homeDir,
    });

    try {
      await configureFakeProvider(harness);
      const session = await harness.createSession({ id: "ses_init_rpc", workDir });
      const events: Event[] = [];
      const unsubscribe = session.onEvent((event) => {
        events.push(event);
      });

      await session.init();
      unsubscribe();

      const spawned = events.find((event) => event.type === "subagent.spawned");
      expect(spawned).toMatchObject({
        type: "subagent.spawned",
        sessionId: session.id,
        agentId: "main",
        subagentName: "coder",
        parentToolCallId: "generate-agents-md",
      });
      expect(events).toContainEqual(
        expect.objectContaining({
          type: "turn.started",
          sessionId: session.id,
          agentId: spawned?.type === "subagent.spawned" ? spawned.subagentId : undefined,
          origin: { kind: "system_trigger", name: "subagent" },
        }),
      );
      expect(events).not.toContainEqual(
        expect.objectContaining({
          type: "session.meta.updated",
        }),
      );
      expect(requestMessages(provider.requests[0]?.bodyJson).slice(1)).toMatchObject([
        {
          role: "user",
          content: expect.stringContaining("Task requirements:"),
        },
      ]);

      const statePath = join(session.summary!.sessionDir, "state.json");
      const state = JSON.parse(await readFile(statePath, "utf-8")) as Record<string, unknown>;
      expect(state["lastPrompt"]).toBeUndefined();
    } finally {
      await harness.close();
    }
  });

  it("includes persisted subagent replay only when resume explicitly requests it", async () => {
    const homeDir = await makeTempDir();
    const workDir = await makeTempDir();
    const harness = createDimiHarness({ identity: TEST_IDENTITY, homeDir });

    try {
      await configureFakeProvider(harness);
      const session = await harness.createSession({ id: "ses_subagent_replay", workDir });
      const events: Event[] = [];
      const unsubscribe = session.onEvent((event) => events.push(event));
      await session.init();
      unsubscribe();
      const spawned = events.find((event) => event.type === "subagent.spawned");
      if (spawned?.type !== "subagent.spawned") throw new Error("Expected persisted subagent");
      await session.close();

      const defaultResume = await harness.resumeSession({ id: session.id });
      expect(defaultResume.getResumeState()?.agents).not.toHaveProperty(spawned.subagentId);
      await defaultResume.close();

      const fullResume = await harness.resumeSession({
        id: session.id,
        includeSubagents: true,
      });
      expect(fullResume.getResumeState()?.agents[spawned.subagentId]?.replay).toContainEqual(
        expect.objectContaining({
          type: "message",
          message: expect.objectContaining({ role: "assistant" }),
        }),
      );
    } finally {
      await harness.close();
    }
  });

  it("starts btw through RPC as a forked subagent without prompt metadata updates", async () => {
    const homeDir = await makeTempDir();
    const workDir = await makeTempDir();
    const harness = createDimiHarness({
      identity: TEST_IDENTITY,
      homeDir,
    });

    try {
      await configureFakeProvider(harness);
      const session = await harness.createSession({ id: "ses_btw_rpc", workDir });
      const events: Event[] = [];
      const unsubscribe = session.onEvent((event) => {
        events.push(event);
      });

      let done = waitForEvent(session, (event) => event.type === "turn.ended");
      await session.prompt("main task context");
      await done;

      responseText = "The main agent is working from the existing context.";
      events.length = 0;
      done = waitForEvent(
        session,
        (event) => event.type === "turn.ended" && event.agentId !== "main",
      );

      const agentId = await session.startBtw();
      await harness.withInteractiveAgent(agentId, () =>
        session.prompt("What are you working on right now?"),
      );
      await done;
      unsubscribe();
      expect(harness.interactiveAgentId).toBe("main");

      const started = events.find(
        (event) =>
          event.type === "turn.started" &&
          event.agentId === agentId &&
          event.origin.kind === "user",
      );
      expect(events).toContainEqual(
        expect.objectContaining({
          type: "turn.started",
          sessionId: session.id,
          agentId,
          origin: { kind: "user" },
        }),
      );
      expect(started?.agentId).not.toBe("main");
      expect(events).not.toContainEqual(expect.objectContaining({ type: "subagent.spawned" }));
      expect(events).not.toContainEqual(expect.objectContaining({ type: "subagent.completed" }));
      expect(events).not.toContainEqual(expect.objectContaining({ type: "subagent.failed" }));
      expect(events).not.toContainEqual(
        expect.objectContaining({
          type: "session.meta.updated",
        }),
      );
      const firstMessages = requestMessages(provider.requests[0]?.bodyJson);
      const secondMessages = requestMessages(provider.requests[1]?.bodyJson);
      expect(secondMessages[0]).toEqual(firstMessages[0]);
      const btwHistoryText = JSON.stringify(secondMessages.slice(1));
      expect(btwHistoryText).toContain("main task context");
      expect(btwHistoryText).toContain("What are you working on right now?");

      const statePath = join(session.summary!.sessionDir, "state.json");
      const state = JSON.parse(await readFile(statePath, "utf-8")) as Record<string, unknown>;
      expect(state["lastPrompt"]).toBe("main task context");
      expect(state["agents"]).toMatchObject({ main: expect.any(Object) });
      expect(state["agents"]).toHaveProperty(agentId);

      await harness.closeSession(session.id);
      const resumed = await harness.resumeSession({ id: session.id });
      const resumeState = resumed.getResumeState();
      expect(resumeState?.agents).toMatchObject({ main: expect.any(Object) });
      expect(resumeState?.agents).not.toHaveProperty(agentId);
      expect(resumeState?.sessionMetadata.agents).toHaveProperty(agentId);
    } finally {
      await harness.close();
    }
  });

  it("persists only conversation through the selected turn across resume", async () => {
    const homeDir = await makeTempDir();
    const workDir = await makeTempDir();
    const harness = createDimiHarness({ identity: TEST_IDENTITY, homeDir });

    try {
      await configureFakeProvider(harness);
      const source = await harness.createSession({ id: "ses_turn_fork_source", workDir });
      await runPrompt(source, "first question", "first answer");
      await runPrompt(source, "second question", "second answer");
      await runPrompt(source, "third question", "third answer");

      const fork = await harness.forkSession({
        id: source.id,
        forkId: "ses_turn_fork_child",
        turnIndex: 1,
      });
      await fork.close();
      const resumed = await harness.resumeSession({ id: fork.id });
      const replayText = visibleReplayText(resumed.getResumeState()?.agents["main"]?.replay ?? []);

      expect(replayText).toEqual([
        "user:first question",
        "assistant:first answer",
        "user:second question",
        "assistant:second answer",
      ]);
    } finally {
      await harness.close();
    }
  });

  it("returns the requested identity for a historical fork", async () => {
    const homeDir = await makeTempDir();
    const workDir = await makeTempDir();
    const harness = createDimiHarness({ identity: TEST_IDENTITY, homeDir });

    try {
      await configureFakeProvider(harness);
      const source = await harness.createSession({
        id: "ses_turn_fork_metadata_source",
        workDir,
        metadata: { source: "vscode" },
      });
      await runPrompt(source, "branch here", "kept answer");
      await runPrompt(source, "future prompt", "discarded answer");

      const fork = await harness.forkSession({
        id: source.id,
        forkId: "ses_turn_fork_metadata_child",
        title: "Historical branch",
        metadata: { branch: "historical" },
        turnIndex: 0,
      });
      const state = fork.getResumeState();

      expect(fork.id).toBe("ses_turn_fork_metadata_child");
      expect(fork.workDir).toBe(source.workDir);
      expect(state?.sessionMetadata.forkedFrom).toBe(source.id);
    } finally {
      await harness.close();
    }
  });

  it("derives historical fork metadata from the selected turn", async () => {
    const homeDir = await makeTempDir();
    const workDir = await makeTempDir();
    const harness = createDimiHarness({ identity: TEST_IDENTITY, homeDir });

    try {
      await configureFakeProvider(harness);
      const source = await harness.createSession({
        id: "ses_turn_fork_state_source",
        workDir,
        metadata: { source: "vscode" },
      });
      await runPrompt(source, "branch here", "kept answer");
      await runPrompt(source, "future prompt", "discarded answer");

      const fork = await harness.forkSession({
        id: source.id,
        forkId: "ses_turn_fork_state_child",
        title: "Historical branch",
        metadata: { branch: "historical" },
        turnIndex: 0,
      });

      expect(fork.summary).toMatchObject({
        title: "Historical branch",
        lastPrompt: "branch here",
        metadata: { source: "vscode", branch: "historical" },
      });
      expect(fork.getResumeState()?.sessionMetadata).toMatchObject({
        title: "Historical branch",
        lastPrompt: "branch here",
        custom: { source: "vscode", branch: "historical" },
      });
    } finally {
      await harness.close();
    }
  });

  it("continues with the next turn id after a historical fork", async () => {
    const homeDir = await makeTempDir();
    const workDir = await makeTempDir();
    const harness = createDimiHarness({ identity: TEST_IDENTITY, homeDir });

    try {
      await configureFakeProvider(harness);
      const source = await harness.createSession({ id: "ses_turn_fork_id_source", workDir });
      await runPrompt(source, "kept prompt", "kept answer");
      await runPrompt(source, "future prompt", "future answer");
      const fork = await harness.forkSession({ id: source.id, turnIndex: 0 });
      const started = waitForEvent(fork, (event) => event.type === "turn.started");
      const ended = waitForEvent(fork, (event) => event.type === "turn.ended");

      await fork.prompt("branch continuation");

      await expect(started).resolves.toMatchObject({ type: "turn.started", turnId: 1 });
      await ended;
    } finally {
      await harness.close();
    }
  });

  it("omits subagents created after the selected historical turn", async () => {
    const homeDir = await makeTempDir();
    const workDir = await makeTempDir();
    const harness = createDimiHarness({ identity: TEST_IDENTITY, homeDir });

    try {
      await configureFakeProvider(harness);
      const source = await harness.createSession({ id: "ses_turn_fork_agents_source", workDir });
      await runPrompt(source, "kept prompt", "kept answer");
      await runPrompt(source, "future prompt", "future answer");
      await source.init();

      const fork = await harness.forkSession({ id: source.id, turnIndex: 0 });

      expect(Object.keys(fork.getResumeState()?.sessionMetadata.agents ?? {})).toEqual(["main"]);
    } finally {
      await harness.close();
    }
  });

  it("rejects a negative historical turn index with request.invalid", async () => {
    const homeDir = await makeTempDir();
    const workDir = await makeTempDir();
    const harness = createDimiHarness({ identity: TEST_IDENTITY, homeDir });

    try {
      const source = await harness.createSession({ id: "ses_turn_fork_negative", workDir });

      await expect(harness.forkSession({ id: source.id, turnIndex: -1 })).rejects.toMatchObject({
        name: "DimiError",
        code: "request.invalid",
      });
    } finally {
      await harness.close();
    }
  });

  it("rejects an out-of-range historical turn without creating the fork", async () => {
    const homeDir = await makeTempDir();
    const workDir = await makeTempDir();
    const harness = createDimiHarness({ identity: TEST_IDENTITY, homeDir });

    try {
      await configureFakeProvider(harness);
      const source = await harness.createSession({ id: "ses_turn_fork_range_source", workDir });
      await runPrompt(source, "only question", "only answer");

      await expect(
        harness.forkSession({
          id: source.id,
          forkId: "ses_turn_fork_range_child",
          turnIndex: 1,
        }),
      ).rejects.toMatchObject({
        name: "DimiError",
        code: "request.invalid",
        details: { turnIndex: 1, availableTurns: 1 },
      });
      await expect(
        harness.listSessions({ sessionId: "ses_turn_fork_range_child" }),
      ).resolves.toEqual([]);
    } finally {
      await harness.close();
    }
  });

  it("rejects empty prompt input", async () => {
    const homeDir = await makeTempDir();
    const workDir = await makeTempDir();
    const harness = createDimiHarness({
      identity: TEST_IDENTITY,
      homeDir,
    });

    try {
      const session = await harness.createSession({ id: "ses_empty_prompt", workDir });
      await expect(session.prompt("   ")).rejects.toMatchObject({
        name: "DimiError",
        code: "request.prompt_input_empty",
      });
    } finally {
      await harness.close();
    }
  });
});

async function runPrompt(
  session: Parameters<typeof waitForEvent>[0] & { prompt(input: string): Promise<void> },
  input: string,
  response: string,
): Promise<void> {
  responseText = response;
  const done = waitForEvent(session, (event) => event.type === "turn.ended");
  await session.prompt(input);
  await done;
}

function visibleReplayText(
  records: readonly {
    readonly type: string;
    readonly message?: {
      readonly role: string;
      readonly content: ReadonlyArray<{ readonly type: string; readonly text?: string }>;
      readonly origin?: { readonly kind: string };
    };
  }[],
): readonly string[] {
  const entries: string[] = [];
  for (const record of records) {
    if (record.type !== "message" || record.message === undefined) continue;
    const { message } = record;
    if (message.role === "user" && message.origin?.kind !== "user") continue;
    if (message.role !== "user" && message.role !== "assistant") continue;
    const text = message.content
      .filter((part) => part.type === "text")
      .map((part) => part.text ?? "")
      .join("");
    entries.push(`${message.role}:${text}`);
  }
  return entries;
}

async function configureFakeProvider(harness: DimiHarness): Promise<void> {
  await harness.setConfig({
    defaultProvider: "kimi-coding",
    defaultModel: "fake-model",
  });
}

function completionChunk(
  delta: Record<string, unknown>,
  finishReason: string | null = null,
): Record<string, unknown> {
  return {
    id: "chatcmpl-node-sdk-test",
    object: "chat.completion.chunk",
    created: 1,
    model: "fake-model",
    choices: [{ index: 0, delta, finish_reason: finishReason }],
  };
}

function toolCallChunk(
  id: string,
  name: string,
  args: Record<string, unknown>,
): Record<string, unknown> {
  return completionChunk({
    tool_calls: [
      { index: 0, id, type: "function", function: { name, arguments: JSON.stringify(args) } },
    ],
  });
}

function requestMessages(body: unknown): readonly Record<string, unknown>[] {
  if (typeof body !== "object" || body === null || !("messages" in body)) return [];
  const messages = (body as { readonly messages?: unknown }).messages;
  return Array.isArray(messages)
    ? messages.filter(
        (message): message is Record<string, unknown> =>
          typeof message === "object" && message !== null,
      )
    : [];
}

function waitForEvent(
  session: {
    onEvent(listener: (event: Event) => void): () => void;
  },
  predicate: (event: Event) => boolean,
  timeoutMs = 1_000,
): Promise<Event> {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      unsubscribe();
      reject(new Error("Timed out waiting for session event"));
    }, timeoutMs);
    const unsubscribe = session.onEvent((event) => {
      if (!predicate(event)) return;
      clearTimeout(timeout);
      unsubscribe();
      resolve(event);
    });
  });
}
