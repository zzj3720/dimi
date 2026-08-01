import { beforeEach, describe, expect, it, vi } from "vitest";

import type { RemoteConnectionState, RemoteHttpResponse } from "@dimi-agent/remote";
import { MobileRuntime } from "../src/runtime";

const remote = vi.hoisted(() => {
  type StateListener = (state: RemoteConnectionState) => void;

  class Client {
    static current: Client | undefined;
    readonly requests: { method: "GET" | "POST"; path: string; body?: unknown }[] = [];
    readonly wsFrames: string[] = [];
    readonly #listeners = new Set<StateListener>();
    readonly #wsListeners = new Set<(data: string) => void>();
    state: RemoteConnectionState = "disconnected";

    constructor() {
      Client.current = this;
    }

    start(): void {}
    close(): void {}
    sendWs(data: string): void {
      this.wsFrames.push(data);
    }
    onWsMessage(listener: (data: string) => void): () => void {
      this.#wsListeners.add(listener);
      return () => this.#wsListeners.delete(listener);
    }

    onState(listener: StateListener): () => void {
      this.#listeners.add(listener);
      listener(this.state);
      return () => this.#listeners.delete(listener);
    }

    emit(state: RemoteConnectionState): void {
      this.state = state;
      for (const listener of this.#listeners) listener(state);
    }

    emitWs(frame: unknown): void {
      const data = JSON.stringify(frame);
      for (const listener of this.#wsListeners) listener(data);
    }

    async request(
      method: "GET" | "POST",
      path: string,
      body?: unknown,
    ): Promise<RemoteHttpResponse> {
      this.requests.push({ method, path, body });
      return responseFor(method, path, body);
    }
  }

  return {
    Client,
    transcript: {
      agent_id: "main",
      items: [],
      has_more: false,
      tasks: [],
      interactions: [],
      attachments: [],
      todos: [],
      prompts: [],
      meta: {},
      agents: [{ agentId: "main", type: "main" }],
      pending_interactions: [],
      seq: 1,
    },
    approvals: [
      {
        approval_id: "approval-1",
        session_id: "session-1",
        tool_call_id: "tool-1",
        tool_name: "Bash",
        action: "Running: sleep 20",
        tool_input_display: { kind: "command", command: "sleep 20" },
        created_at: "2026-07-31T00:00:00Z",
        expires_at: "2026-08-01T00:00:00Z",
      },
    ],
    questions: [
      {
        question_id: "question-1",
        session_id: "session-1",
        tool_call_id: "tool-2",
        questions: [
          {
            id: "q1",
            question: "Pick a mode",
            options: [
              { id: "safe", label: "Safe" },
              { id: "fast", label: "Fast" },
            ],
            allow_other: true,
          },
        ],
        created_at: "2026-07-31T00:00:00Z",
      },
    ],
    promptFailure: undefined as string | undefined,
  };
});

vi.mock("react-native", () => ({ Platform: { OS: "android" } }));
vi.mock("expo-crypto", () => ({
  getRandomBytes: (length: number) => new Uint8Array(length),
  randomUUID: () => "uuid",
}));
vi.mock("../src/storage", () => ({
  clearStoredRemote: vi.fn(),
  loadStoredRemote: vi.fn(async () => ({
    pairing: {
      relayUrl: "wss://relay.example",
      runtimeId: "runtime-1",
      runtimeName: "Local runtime",
      runtimePublicKey: "runtime-public-key",
      token: "pairing-token",
      expiresAt: Date.now() + 60_000,
    },
    identity: {
      deviceId: "device-1",
      deviceName: "Android",
      publicKey: "device-public-key",
      secretKey: "device-secret-key",
    },
    paired: true,
  })),
  saveStoredRemote: vi.fn(),
}));
vi.mock("@dimi-agent/remote", () => ({
  createDeviceIdentity: vi.fn(),
  parsePairingUri: vi.fn(),
  RemoteClient: remote.Client,
}));

const session = {
  id: "session-1",
  workspace_id: "wd_mobile_0123456789ab",
  title: "Android handtest",
  created_at: "2026-07-31T00:00:00Z",
  updated_at: "2026-07-31T00:00:00Z",
  busy: false,
  metadata: { cwd: "/workspace" },
  agent_config: { model: "provider/model" },
  usage: {
    input_tokens: 0,
    output_tokens: 0,
    cache_read_tokens: 0,
    cache_creation_tokens: 0,
    total_cost_usd: 0,
    context_tokens: 0,
    context_limit: 1000,
    turn_count: 0,
  },
  permission_rules: [],
  message_count: 0,
  last_seq: 0,
};

function responseFor(
  method: "GET" | "POST",
  path: string,
  body?: unknown,
): RemoteHttpResponse {
  if (method === "POST" && path.endsWith("/prompts")) {
    if (remote.promptFailure !== undefined) {
      return {
        status: 200,
        body: { code: 40113, msg: remote.promptFailure, data: null, request_id: "request-1" },
      };
    }
    return envelope({
      prompt_id: "prompt-1",
      user_message_id: "prompt-1",
      status: "running",
      content: (body as { content: unknown[] }).content,
      created_at: "2026-07-31T00:00:00Z",
    });
  }
  if (method === "POST" && path.endsWith("/prompts:steer")) {
    return envelope({ steered: true, prompt_ids: ["prompt-1"] });
  }
  if (method === "POST" && path.endsWith(":abort")) {
    return envelope({ aborted: true });
  }
  if (method === "POST" && path.includes("/approvals/")) {
    remote.approvals.splice(0);
    return envelope({ resolved: true, resolved_at: "2026-07-31T00:01:00Z" });
  }
  if (method === "POST" && path.endsWith(":dismiss")) {
    remote.questions.splice(0);
    return envelope({ dismissed: true, dismissed_at: "2026-07-31T00:01:00Z" });
  }
  if (method === "POST" && path.includes("/questions/")) {
    remote.questions.splice(0);
    return envelope({ resolved: true, resolved_at: "2026-07-31T00:01:00Z" });
  }
  if (path.startsWith("/api/v1/sessions?page_size=")) {
    return envelope({ items: [session], has_more: false });
  }
  if (path.endsWith("/approvals?status=pending")) {
    return envelope({ items: remote.approvals });
  }
  if (path.endsWith("/questions?status=pending")) {
    return envelope({ items: remote.questions });
  }
  if (path.includes("/transcript?")) {
    return envelope(remote.transcript);
  }
  throw new Error(`Unexpected request: ${path}`);
}

function envelope(data: unknown): RemoteHttpResponse {
  return {
    status: 200,
    body: { code: 0, msg: "success", data, request_id: "request-1" },
  };
}

function transcriptTurn(turnId: string, text: string) {
  return {
    kind: "turn" as const,
    turnId,
    ordinal: 0,
    state: "completed" as const,
    origin: { kind: "user" as const },
    prompt: `Prompt ${turnId}`,
    steps: [
      {
        kind: "step" as const,
        stepId: `${turnId}.1`,
        turnId,
        ordinal: 1,
        state: "completed" as const,
        frames: [
          {
            kind: "text" as const,
            frameId: `${turnId}.1.f1`,
            role: "assistant" as const,
            text,
          },
        ],
      },
    ],
  };
}

describe("MobileRuntime reconnect recovery", () => {
  beforeEach(() => {
    remote.Client.current = undefined;
    remote.transcript.items = [];
    remote.transcript.seq = 1;
    remote.promptFailure = undefined;
    remote.approvals.splice(0, remote.approvals.length, {
      approval_id: "approval-1",
      session_id: "session-1",
      tool_call_id: "tool-1",
      tool_name: "Bash",
      action: "Running: sleep 20",
      tool_input_display: { kind: "command", command: "sleep 20" },
      created_at: "2026-07-31T00:00:00Z",
      expires_at: "2026-08-01T00:00:00Z",
    });
    remote.questions.splice(0, remote.questions.length, {
      question_id: "question-1",
      session_id: "session-1",
      tool_call_id: "tool-2",
      questions: [
        {
          id: "q1",
          question: "Pick a mode",
          options: [
            { id: "safe", label: "Safe" },
            { id: "fast", label: "Fast" },
          ],
          allow_other: true,
        },
      ],
      created_at: "2026-07-31T00:00:00Z",
    });
  });

  it("reloads the transcript and pending interactions after reconnecting", async () => {
    const runtime = new MobileRuntime();
    await runtime.initialize();
    const client = remote.Client.current;
    expect(client).toBeDefined();
    client?.emit("online");
    await vi.waitFor(() => {
      expect(runtime.state.sessions).toHaveLength(1);
    });
    await runtime.selectSession("session-1");
    expect(runtime.state.approvals).toHaveLength(1);

    remote.transcript.items = [
      {
        kind: "turn",
        turnId: "t0",
        ordinal: 0,
        state: "completed",
        origin: { kind: "user" },
        prompt: "Run a command",
        steps: [
          {
            kind: "step",
            stepId: "t0.1",
            turnId: "t0",
            ordinal: 1,
            state: "completed",
            frames: [
              {
                kind: "text",
                frameId: "t0.1.f1",
                role: "assistant",
                text: "Recovered final answer",
              },
            ],
          },
        ],
      },
    ];
    remote.transcript.seq = 2;
    remote.approvals.splice(0);

    client?.emit("disconnected");
    client?.emit("online");

    await vi.waitFor(() => {
      expect(runtime.state.approvals).toEqual([]);
      expect(runtime.state.transcript).toEqual(remote.transcript.items);
    });
    expect(client?.requests.filter(({ path }) => path.includes("/transcript?"))).toHaveLength(2);
    runtime.dispose();
  });

  it("keeps REST history when reconnect emits an empty baseline reset", async () => {
    remote.transcript.items = [transcriptTurn("persisted", "Persisted answer")];
    remote.transcript.seq = 2;
    const runtime = new MobileRuntime();
    await runtime.initialize();
    const client = remote.Client.current!;
    client.emit("online");
    await vi.waitFor(() => {
      expect(runtime.state.sessions).toHaveLength(1);
    });
    await runtime.selectSession("session-1");

    client.emit("disconnected");
    client.emit("online");
    await vi.waitFor(() => {
      expect(runtime.state.transcript).toEqual(remote.transcript.items);
    });
    client.emitWs({
      session_id: "session-1",
      payload: {
        type: "transcript.reset",
        agent_id: "main",
        snapshot: {
          items: [],
          tasks: [],
          interactions: [],
          attachments: [],
          todos: [],
          prompts: [],
          meta: {},
          hasMoreOlder: false,
        },
        has_more_older: false,
        seq: 2,
      },
    });

    expect(runtime.state.transcript).toEqual(remote.transcript.items);
    runtime.dispose();
  });

  it("surfaces prompt submission failures", async () => {
    const runtime = new MobileRuntime();
    await runtime.initialize();
    const client = remote.Client.current!;
    client.emit("online");
    await vi.waitFor(() => {
      expect(runtime.state.sessions).toHaveLength(1);
    });
    await runtime.selectSession("session-1");
    remote.promptFailure = "no default model configured";

    await expect(runtime.sendPrompt("hello")).rejects.toThrow("no default model configured");
    expect(runtime.state.error).toBe("no default model configured");
    runtime.dispose();
  });

  it("subscribes only to main and never replaces it with a subagent transcript", async () => {
    const runtime = new MobileRuntime();
    await runtime.initialize();
    const client = remote.Client.current!;
    client.emit("online");
    await vi.waitFor(() => {
      expect(runtime.state.sessions).toHaveLength(1);
    });
    await runtime.selectSession("session-1");

    const subscribeV2 = client.wsFrames
      .map((frame) => JSON.parse(frame) as Record<string, unknown>)
      .find((frame) => frame["type"] === "subscribe_v2");
    expect(subscribeV2).toMatchObject({
      payload: { session_id: "session-1", transcript: { main: "block" } },
    });

    client.emitWs({
      session_id: "session-1",
      payload: {
        type: "transcript.reset",
        agent_id: "sub-reviewer",
        snapshot: {
          items: [transcriptTurn("sub", "Subagent answer")],
          tasks: [],
          interactions: [],
          attachments: [],
          todos: [],
          prompts: [],
          meta: {},
          hasMoreOlder: false,
        },
        has_more_older: false,
        seq: 2,
      },
    });
    expect(runtime.state.transcript).toEqual([]);

    client.emitWs({
      session_id: "session-1",
      payload: {
        type: "transcript.reset",
        agent_id: "main",
        snapshot: {
          items: [transcriptTurn("main", "Main answer")],
          tasks: [],
          interactions: [],
          attachments: [],
          todos: [],
          prompts: [],
          meta: {},
          hasMoreOlder: false,
        },
        has_more_older: false,
        seq: 2,
      },
    });
    await vi.waitFor(() => {
      expect(runtime.state.transcript).toEqual([transcriptTurn("main", "Main answer")]);
    });
    runtime.dispose();
  });

  it("reloads the selected transcript when the server requires a resync", async () => {
    const runtime = new MobileRuntime();
    await runtime.initialize();
    const client = remote.Client.current!;
    client.emit("online");
    await vi.waitFor(() => {
      expect(runtime.state.sessions).toHaveLength(1);
    });
    await runtime.selectSession("session-1");
    remote.transcript.items = [transcriptTurn("resync", "Recovered after resync")];

    client.emitWs({
      type: "resync_required",
      payload: { session_id: "session-1", reason: "buffer_overflow", current_seq: 9 },
    });

        await vi.waitFor(() => {
          expect(runtime.state.transcript).toEqual([
            transcriptTurn("resync", "Recovered after resync"),
          ]);
        });
    expect(client.requests.filter(({ path }) => path.includes("/transcript?"))).toHaveLength(2);
    runtime.dispose();
  });

  it("sends prompt, steer, cancel, approval, question, and dismiss through public routes", async () => {
    const runtime = new MobileRuntime();
    await runtime.initialize();
    const client = remote.Client.current!;
    client.emit("online");
    await vi.waitFor(() => {
      expect(runtime.state.sessions).toHaveLength(1);
    });
    await runtime.selectSession("session-1");

    await runtime.sendPrompt("hello");
    await runtime.steerPrompt("focus on tests");
    await runtime.cancel();
    await runtime.respondApproval("approval-1", {
      decision: "approved",
      scope: "session",
      feedback: "Only this workspace",
    });
    await runtime.respondQuestion("question-1", {
      q1: { kind: "other", text: "Balanced" },
    });

    remote.questions.push({
      question_id: "question-2",
      session_id: "session-1",
      questions: [
        {
          id: "q2",
          question: "Continue?",
          options: [
            { id: "yes", label: "Yes" },
            { id: "no", label: "No" },
          ],
        },
      ],
      created_at: "2026-07-31T00:02:00Z",
    });
    await runtime.dismissQuestion("question-2");

    expect(client.requests).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          method: "POST",
          path: "/api/v1/sessions/session-1/prompts",
          body: { content: [{ type: "text", text: "hello" }] },
        }),
        expect.objectContaining({
          method: "POST",
          path: "/api/v1/sessions/session-1/prompts:steer",
          body: { prompt_ids: ["prompt-1"] },
        }),
        expect.objectContaining({
          method: "POST",
          path: "/api/v1/sessions/session-1:abort",
        }),
        expect.objectContaining({
          method: "POST",
          path: "/api/v1/sessions/session-1/approvals/approval-1",
          body: {
            decision: "approved",
            scope: "session",
            feedback: "Only this workspace",
          },
        }),
        expect.objectContaining({
          method: "POST",
          path: "/api/v1/sessions/session-1/questions/question-1",
          body: {
            answers: { q1: { kind: "other", text: "Balanced" } },
            method: "click",
          },
        }),
        expect.objectContaining({
          method: "POST",
          path: "/api/v1/sessions/session-1/questions/question-2:dismiss",
        }),
      ]),
    );
    runtime.dispose();
  });
});
