import { Platform } from "react-native";

import * as Crypto from "expo-crypto";
import {
  approvalResolveResultSchema,
  envelopeSchema,
  listPendingApprovalsResponseSchema,
  listPendingQuestionsResponseSchema,
  pageResponseSchema,
  promptSubmitResultSchema,
  questionDismissResultSchema,
  questionResolveResultSchema,
  sessionSchema,
  type ApprovalResponse,
  type ApprovalRequest,
  type QuestionAnswer,
  type QuestionRequest,
  type Session,
} from "@dimi-agent/protocol";
import {
  createDeviceIdentity,
  parsePairingUri,
  RemoteClient,
  type RemoteConnectionState,
  type SocketLike,
  type StoredRemote,
} from "@dimi-agent/remote";
import {
  TranscriptStore,
  transcriptEventSchema,
  transcriptResponseSchema,
  type AgentTranscript,
  type TranscriptItem,
  type TranscriptOperation,
  type TranscriptResetEvent,
} from "@dimi-agent/transcript";
import { z } from "zod";

import { clearStoredRemote, loadStoredRemote, saveStoredRemote } from "./storage";

const sessionPageSchema = pageResponseSchema(sessionSchema);

export interface MobileRuntimeState {
  readonly connection: RemoteConnectionState;
  readonly remoteName?: string;
  readonly sessions: readonly Session[];
  readonly selectedSessionId?: string;
  readonly transcript: readonly TranscriptItem[];
  readonly activity?: string;
  readonly approvals: readonly ApprovalRequest[];
  readonly questions: readonly QuestionRequest[];
  readonly error?: string;
}

type RuntimeListener = (state: MobileRuntimeState) => void;

export class MobileRuntime {
  readonly #listeners = new Set<RuntimeListener>();
  readonly #stores = new Map<string, TranscriptStore>();
  readonly #transcriptSeq = new Map<string, number>();

  #client?: RemoteClient;
  #state: MobileRuntimeState = {
    connection: "disconnected",
    sessions: [],
    transcript: [],
    approvals: [],
    questions: [],
  };

  get state(): MobileRuntimeState {
    return this.#state;
  }

  subscribe(listener: RuntimeListener): () => void {
    this.#listeners.add(listener);
    listener(this.#state);
    return () => this.#listeners.delete(listener);
  }

  dispose(): void {
    this.#client?.close();
    this.#listeners.clear();
  }

  async initialize(): Promise<void> {
    const stored = await loadStoredRemote();
    if (stored === undefined) return;
    this.#connect(stored);
  }

  async pair(pairingUri: string): Promise<void> {
    const pairing = parsePairingUri(pairingUri);
    if (pairing.expiresAt < Date.now()) throw new Error("This pairing code has expired.");
    if (
      (typeof __DEV__ === "undefined" || !__DEV__) &&
      new URL(pairing.relayUrl).protocol !== "wss:"
    ) {
      throw new Error("Release builds require a secure relay.");
    }
    this.#client?.close();
    const identity = createDeviceIdentity(
      Platform.OS === "ios" ? "iPhone" : Platform.OS === "android" ? "Android" : "Mobile",
      Crypto.getRandomBytes,
    );
    const stored: StoredRemote = { pairing, identity, paired: false };
    await saveStoredRemote(stored);
    this.#connect(stored);
  }

  async forgetRemote(): Promise<void> {
    this.#client?.close();
    this.#client = undefined;
    this.#stores.clear();
    this.#transcriptSeq.clear();
    await clearStoredRemote();
    this.#replace({
      connection: "disconnected",
      sessions: [],
      transcript: [],
      approvals: [],
      questions: [],
    });
  }

  async refreshSessions(): Promise<void> {
    const response = await this.#request("GET", "/api/v1/sessions?page_size=100");
    const sessions = unwrap(response.body, sessionPageSchema).items;
    this.#patch({ sessions });
  }

  async selectSession(sessionId: string): Promise<void> {
    const previous = this.#state.selectedSessionId;
    this.#patch({
      selectedSessionId: sessionId,
      transcript: [],
      approvals: [],
      questions: [],
      error: undefined,
    });
    if (previous !== undefined && previous !== sessionId) {
      this.#sendWs({
        type: "unsubscribe",
        id: Crypto.randomUUID(),
        payload: { session_ids: [previous] },
      });
    }
    await this.#loadSelectedSession(sessionId);
  }

  deselectSession(): void {
    const sessionId = this.#state.selectedSessionId;
    if (sessionId !== undefined && this.#state.connection === "online") {
      this.#sendWs({
        type: "unsubscribe",
        id: Crypto.randomUUID(),
        payload: { session_ids: [sessionId] },
      });
    }
    this.#patch({
      selectedSessionId: undefined,
      transcript: [],
      activity: undefined,
      approvals: [],
      questions: [],
    });
  }

  async sendPrompt(text: string): Promise<void> {
    const sessionId = this.#requireSelected();
    try {
      await this.#post(
        `/api/v1/sessions/${encodeURIComponent(sessionId)}/prompts`,
        {
          content: [{ type: "text", text }],
        },
        promptSubmitResultSchema,
      );
      this.#patch({ error: undefined });
    } catch (error) {
      this.#setError(error);
      throw error;
    }
  }

  async steerPrompt(text: string): Promise<void> {
    const sessionId = this.#requireSelected();
    const prompt = await this.#post(
      `/api/v1/sessions/${encodeURIComponent(sessionId)}/prompts`,
      { content: [{ type: "text", text }] },
      promptSubmitResultSchema,
    );
    await this.#post(
      `/api/v1/sessions/${encodeURIComponent(sessionId)}/prompts:steer`,
      { prompt_ids: [prompt.prompt_id] },
      z.object({ steered: z.boolean(), prompt_ids: z.array(z.string()) }),
    );
  }

  async cancel(): Promise<void> {
    const sessionId = this.#requireSelected();
    await this.#post(
      `/api/v1/sessions/${encodeURIComponent(sessionId)}:abort`,
      {},
      z.object({ aborted: z.boolean() }),
    );
  }

  async respondApproval(approvalId: string, response: ApprovalResponse): Promise<void> {
    const sessionId = this.#requireSelected();
    await this.#post(
      `/api/v1/sessions/${encodeURIComponent(sessionId)}/approvals/${encodeURIComponent(approvalId)}`,
      response,
      approvalResolveResultSchema,
    );
    await this.#loadInteractions(sessionId);
  }

  async respondQuestion(questionId: string, answers: Record<string, QuestionAnswer>): Promise<void> {
    const sessionId = this.#requireSelected();
    await this.#post(
      `/api/v1/sessions/${encodeURIComponent(sessionId)}/questions/${encodeURIComponent(questionId)}`,
      { answers, method: "click" },
      questionResolveResultSchema,
    );
    await this.#loadInteractions(sessionId);
  }

  async dismissQuestion(questionId: string): Promise<void> {
    const sessionId = this.#requireSelected();
    await this.#post(
      `/api/v1/sessions/${encodeURIComponent(sessionId)}/questions/${encodeURIComponent(questionId)}:dismiss`,
      {},
      questionDismissResultSchema,
    );
    await this.#loadInteractions(sessionId);
  }

  #connect(stored: StoredRemote): void {
    const client = new RemoteClient({
      pairing: stored.pairing,
      identity: stored.identity,
      paired: stored.paired,
      randomBytes: Crypto.getRandomBytes,
      createSocket: (url) => new WebSocket(url) as unknown as SocketLike,
      onPaired: () => {
        const next = { ...stored, paired: true };
        void saveStoredRemote(next);
      },
    });
    this.#client = client;
    client.onState((connection) => {
      this.#patch({
        connection,
        remoteName: stored.pairing.runtimeName,
        error: connection === "online" ? undefined : this.#state.error,
      });
      if (connection === "online") {
        this.#sendWs({
          type: "client_hello",
          id: Crypto.randomUUID(),
          payload: { client_id: stored.identity.deviceId },
        });
        void this.refreshSessions().catch((error) => {
          this.#setError(error);
        });
        const sessionId = this.#state.selectedSessionId;
        if (sessionId !== undefined) {
          void this.#loadSelectedSession(sessionId).catch((error) => {
            this.#setError(error);
          });
        }
      }
    });
    client.onWsMessage((data) => {
      this.#onWsMessage(data);
    });
    client.start();
  }

  async #loadTranscript(sessionId: string): Promise<void> {
    const path = `/api/v1/sessions/${encodeURIComponent(sessionId)}/transcript?agent_id=main&page_size=100`;
    const response = await this.#request("GET", path);
    const transcript = unwrap(response.body, transcriptResponseSchema);
    const store = this.#stores.get(sessionId) ?? new TranscriptStore(sessionId);
    this.#stores.set(sessionId, store);
    for (const descriptor of transcript.agents) store.describeAgent(descriptor);
    const agent = store.ensureAgent(transcript.agent_id);
    agent.receive([
      toResetOperation({
        type: "transcript.reset",
        agent_id: transcript.agent_id,
        snapshot: {
          items: transcript.items,
          tasks: transcript.tasks,
          interactions: transcript.interactions,
          attachments: transcript.attachments,
          todos: transcript.todos,
          prompts: transcript.prompts,
          meta: transcript.meta,
          hasMoreOlder: transcript.has_more,
        },
        has_more_older: transcript.has_more,
        seq: transcript.seq,
      }),
    ]);
    if (transcript.seq !== undefined) {
      this.#transcriptSeq.set(`${sessionId}:${transcript.agent_id}`, transcript.seq);
    }
    this.#syncTranscriptState(sessionId, agent);
  }

  async #loadInteractions(sessionId: string): Promise<void> {
    const encoded = encodeURIComponent(sessionId);
    const [approvals, questions] = await Promise.all([
      this.#request("GET", `/api/v1/sessions/${encoded}/approvals?status=pending`),
      this.#request("GET", `/api/v1/sessions/${encoded}/questions?status=pending`),
    ]);
    if (this.#state.selectedSessionId !== sessionId) return;
    this.#patch({
      approvals: unwrap(approvals.body, listPendingApprovalsResponseSchema).items,
      questions: unwrap(questions.body, listPendingQuestionsResponseSchema).items,
    });
  }

  async #loadSelectedSession(sessionId: string): Promise<void> {
    await Promise.all([this.#loadTranscript(sessionId), this.#loadInteractions(sessionId)]);
    if (this.#state.connection === "online" && this.#state.selectedSessionId === sessionId) {
      this.#subscribeSession(sessionId);
    }
  }

  #subscribeSession(sessionId: string): void {
    if (this.#state.connection !== "online") return;
    this.#sendWs({
      type: "subscribe",
      id: Crypto.randomUUID(),
      payload: { session_ids: [sessionId] },
    });
    const cursor = this.#transcriptSeq.get(`${sessionId}:main`);
    this.#sendWs({
      type: "subscribe_v2",
      id: Crypto.randomUUID(),
      payload: {
        session_id: sessionId,
        transcript: { main: "block" },
        transcript_since: cursor === undefined ? undefined : { main: cursor },
      },
    });
  }

  #onWsMessage(data: string): void {
    let frame: Record<string, unknown>;
    try {
      const value = JSON.parse(data) as unknown;
      if (typeof value !== "object" || value === null) return;
      frame = value as Record<string, unknown>;
    } catch {
      return;
    }
    if (frame["type"] === "resync_required") {
      const sessionId = this.#state.selectedSessionId;
      if (sessionId !== undefined) {
        void this.#loadTranscript(sessionId).catch((error) => {
          this.#setError(error);
        });
      }
      return;
    }
    const sessionId = typeof frame["session_id"] === "string" ? frame["session_id"] : undefined;
    const parsed = transcriptEventSchema.safeParse(frame["payload"]);
    if (parsed.success && sessionId !== undefined) {
      const event = parsed.data;
      if (event.agent_id !== "main") return;
      if (event.type === "transcript.reset" && event.snapshot.items.length === 0) {
        if (event.seq !== undefined)
          this.#transcriptSeq.set(`${sessionId}:${event.agent_id}`, event.seq);
        return;
      }
      const store = this.#stores.get(sessionId) ?? new TranscriptStore(sessionId);
      this.#stores.set(sessionId, store);
      const agent = store.ensureAgent(event.agent_id);
      agent.apply(event.type === "transcript.reset" ? [toResetOperation(event)] : event.ops);
      if (event.seq !== undefined)
        this.#transcriptSeq.set(`${sessionId}:${event.agent_id}`, event.seq);
      this.#syncTranscriptState(sessionId, agent);
      if (agent.listPendingInteractions().length > 0) {
        void this.#loadInteractions(sessionId).catch((error) => {
          this.#setError(error);
        });
      }
      return;
    }
    const type = frame["type"];
    if (
      type === "session.meta.updated" ||
      (typeof type === "string" && type.startsWith("event.session."))
    ) {
      void this.refreshSessions().catch((error) => {
        this.#setError(error);
      });
    }
  }

  #syncTranscriptState(sessionId: string, agent: AgentTranscript): void {
    if (this.#state.selectedSessionId !== sessionId) return;
    const transcript = agent.getItems();
    this.#patch({ transcript, activity: deriveActivity(transcript) });
  }

  async #post<T>(path: string, body: unknown, schema: z.ZodType<T>): Promise<T> {
    const response = await this.#request("POST", path, body);
    return unwrap(response.body, schema);
  }

  #request(method: "GET" | "POST", path: string, body?: unknown) {
    const client = this.#client;
    if (client === undefined) throw new Error("No remote runtime is paired.");
    return client.request(method, path, body);
  }

  #sendWs(frame: object): void {
    try {
      this.#client?.sendWs(JSON.stringify(frame));
    } catch (error) {
      this.#setError(error);
    }
  }

  #requireSelected(): string {
    const sessionId = this.#state.selectedSessionId;
    if (sessionId === undefined) throw new Error("No session is selected.");
    return sessionId;
  }

  #setError(error: unknown): void {
    this.#patch({ error: error instanceof Error ? error.message : String(error) });
  }

  #patch(patch: Partial<MobileRuntimeState>): void {
    this.#replace({ ...this.#state, ...patch });
  }

  #replace(state: MobileRuntimeState): void {
    this.#state = state;
    for (const listener of this.#listeners) listener(state);
  }
}

function unwrap<T>(body: unknown, dataSchema: z.ZodType<T>): T {
  const envelope = envelopeSchema(z.unknown()).parse(body);
  if (envelope.code !== 0) throw new Error(envelope.msg);
  return dataSchema.parse(envelope.data);
}

function toResetOperation(event: TranscriptResetEvent): TranscriptOperation {
  return {
    op: "reset",
    agentId: event.agent_id,
    snapshot: event.snapshot,
  };
}

function deriveActivity(items: readonly TranscriptItem[]): string | undefined {
  for (let itemIndex = items.length - 1; itemIndex >= 0; itemIndex -= 1) {
    const item = items[itemIndex];
    if (item?.kind !== "turn" || (item.state !== "running" && item.state !== "queued")) continue;
    for (let stepIndex = item.steps.length - 1; stepIndex >= 0; stepIndex -= 1) {
      const step = item.steps[stepIndex];
      if (step?.state !== "running") continue;
      for (let frameIndex = step.frames.length - 1; frameIndex >= 0; frameIndex -= 1) {
        const frame = step.frames[frameIndex];
        if (frame?.kind === "tool" && frame.state === "running") return `Using ${frame.name}…`;
      }
      return "Thinking…";
    }
    return item.state === "queued" ? "Queued…" : "Working…";
  }
  return undefined;
}
