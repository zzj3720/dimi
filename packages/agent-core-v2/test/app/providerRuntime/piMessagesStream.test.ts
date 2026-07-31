/**
 * Pi Messages adapter contracts — native context/options forwarding and the
 * serialized event stream's thought, tool-call and terminal projection.
 */
import { afterEach, describe, expect, it, vi } from "vitest";

import { streamProvider } from "#/app/providerRuntime/stream";
import type { AssistantMessageEvent, AuthResult, Model } from "#/app/providerRuntime/types";

function requestJson(init: RequestInit | undefined): Record<string, unknown> {
  if (typeof init?.body !== "string") throw new TypeError("Expected a JSON request body");
  return JSON.parse(init.body) as Record<string, unknown>;
}

const model: Model = {
  id: "radius-model",
  name: "Radius model",
  api: "pi-messages",
  provider: "radius",
  baseUrl: "https://radius.example.test",
  reasoning: true,
  input: ["text"],
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  contextWindow: 128_000,
  maxTokens: 16_000,
};

function sse(events: readonly Record<string, unknown>[]): Response {
  return new Response(events.map((event) => `data: ${JSON.stringify(event)}`).join("\n\n"), {
    headers: { "content-type": "text/event-stream" },
  });
}

async function collect(stream: AsyncIterable<AssistantMessageEvent>): Promise<AssistantMessageEvent[]> {
  const events: AssistantMessageEvent[] = [];
  for await (const event of stream) events.push(event);
  return events;
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("Pi Messages streaming", () => {
  it("forwards the native context and projects serialized thought and tool events", async () => {
    let body: Record<string, unknown> | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        body = requestJson(init);
        return sse([
          { type: "thinking_delta", contentIndex: 0, delta: "plan" },
          { type: "thinking_end", contentIndex: 0, content: "plan", contentSignature: "sig" },
          { type: "text_delta", contentIndex: 1, delta: "answer" },
          { type: "toolcall_start", contentIndex: 2, id: "call", toolName: "read" },
          { type: "toolcall_end", contentIndex: 2, toolCall: { id: "call", name: "read", arguments: { path: "a.ts" } } },
          { type: "done", reason: "toolUse", responseId: "response", usage: { input: 3, output: 2, cacheRead: 0, cacheWrite: 0, totalTokens: 5, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } } },
        ]);
      }),
    );
    const events = await collect(
      streamProvider(
        model,
        { systemPrompt: "system", messages: [{ role: "user", content: "hello", timestamp: 1 }] },
        { auth: { apiKey: "key" } } satisfies AuthResult,
        { cacheKey: "session", topP: 0.2, maxTokens: 9 },
      ),
    );
    expect(body).toMatchObject({
      model: "radius-model",
      context: { systemPrompt: "system", messages: [{ role: "user", content: "hello", timestamp: 1 }] },
      options: { sessionId: "session", topP: 0.2, maxTokens: 9 },
    });
    expect(events.at(-1)).toMatchObject({
      type: "done",
      message: {
        responseId: "response",
        stopReason: "toolUse",
        usage: { input: 3, output: 2, totalTokens: 5 },
        content: [
          { type: "thinking", thinking: "plan", thinkingSignature: "sig" },
          { type: "text", text: "answer" },
          { type: "toolCall", id: "call", name: "read", arguments: { path: "a.ts" } },
        ],
      },
    });
  });

  it("projects Pi Messages protocol errors and pre-aborted requests as terminal errors", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => sse([{ type: "error", errorMessage: "gateway rejected", usage: { input: 1, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 1 } }])),
    );
    const failed = await collect(streamProvider(model, { messages: [] }, { auth: { apiKey: "key" } }));
    expect(failed.at(-1)).toMatchObject({
      type: "error",
      reason: "error",
      error: { stopReason: "error", errorMessage: "gateway rejected", usage: { input: 1, totalTokens: 1 } },
    });

    const controller = new AbortController();
    controller.abort();
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        expect(init?.signal?.aborted).toBe(true);
        throw new DOMException("Pi request aborted", "AbortError");
      }),
    );
    const aborted = await collect(
      streamProvider(model, { messages: [] }, { auth: { apiKey: "key" } }, { signal: controller.signal }),
    );
    expect(aborted.at(-1)).toMatchObject({
      type: "error",
      reason: "aborted",
      error: { stopReason: "aborted", errorMessage: "Pi request aborted" },
    });
  });
});
