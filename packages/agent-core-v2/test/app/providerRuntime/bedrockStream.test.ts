import { afterEach, describe, expect, it, vi } from "vitest";

import { streamProvider } from "#/app/providerRuntime/stream";
import type { AssistantMessageEvent, Model } from "#/app/providerRuntime/types";

function requestJson(init: RequestInit | undefined): Record<string, unknown> {
  if (typeof init?.body !== "string") throw new TypeError("Expected a JSON request body");
  return JSON.parse(init.body) as Record<string, unknown>;
}

const model: Model = {
  id: "anthropic.example",
  name: "Bedrock example",
  api: "bedrock-converse-stream",
  provider: "bedrock",
  baseUrl: "https://bedrock.example.test",
  reasoning: true,
  input: ["text", "image"],
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  contextWindow: 128_000,
  maxTokens: 8_192,
  thinkingLevelMap: { high: "high" },
};

function header(name: string, value: string): Uint8Array {
  const encoder = new TextEncoder();
  const key = encoder.encode(name);
  const text = encoder.encode(value);
  const result = new Uint8Array(1 + key.length + 1 + 2 + text.length);
  result[0] = key.length;
  result.set(key, 1);
  result[1 + key.length] = 7;
  new DataView(result.buffer).setUint16(2 + key.length, text.length);
  result.set(text, 4 + key.length);
  return result;
}

function eventFrame(type: string, payload: Record<string, unknown>): Uint8Array {
  const headers = [header(":message-type", "event"), header(":event-type", type), header(":content-type", "application/json")];
  const headerBytes = concat(...headers);
  const body = new TextEncoder().encode(JSON.stringify(payload));
  const frame = new Uint8Array(12 + headerBytes.length + body.length + 4);
  const view = new DataView(frame.buffer);
  view.setUint32(0, frame.length);
  view.setUint32(4, headerBytes.length);
  frame.set(headerBytes, 12);
  frame.set(body, 12 + headerBytes.length);
  return frame;
}

function concat(...parts: Uint8Array[]): Uint8Array {
  const result = new Uint8Array(parts.reduce((total, part) => total + part.length, 0));
  let offset = 0;
  for (const part of parts) {
    result.set(part, offset);
    offset += part.length;
  }
  return result;
}

function eventStream(events: readonly Uint8Array[]): Response {
  return new Response(concat(...events), {
    headers: { "content-type": "application/vnd.amazon.eventstream", "x-amzn-requestid": "request-1" },
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

describe("Bedrock ConverseStream adapter", () => {
  it("posts ConverseStream input and projects EventStream text, thinking, tools and usage", async () => {
    let url = "";
    let headers: Headers | undefined;
    let body: Record<string, unknown> | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string, init?: RequestInit) => {
        url = input;
        headers = new Headers(init?.headers);
        body = requestJson(init);
        return eventStream([
          eventFrame("contentBlockDelta", {
            contentBlockIndex: 0,
            delta: { reasoningContent: { reasoningText: { text: "plan", signature: "sig" } } },
          }),
          eventFrame("contentBlockDelta", { contentBlockIndex: 1, delta: { text: "answer" } }),
          eventFrame("contentBlockStart", { contentBlockIndex: 2, start: { toolUse: { toolUseId: "call", name: "read" } } }),
          eventFrame("contentBlockDelta", { contentBlockIndex: 2, delta: { toolUse: { input: "{\"path\":\"a\"}" } } }),
          eventFrame("contentBlockStop", { contentBlockIndex: 2 }),
          eventFrame("metadata", { usage: { inputTokens: 5, outputTokens: 3, totalTokens: 8 } }),
          eventFrame("messageStop", { stopReason: "tool_use" }),
        ]);
      }),
    );

    const events = await collect(
      streamProvider(
        model,
        {
          systemPrompt: "system",
          tools: [{ name: "read", description: "Read", parameters: { type: "object" } }],
          messages: [
            {
              role: "toolResult",
              toolCallId: "previous",
              toolName: "read",
              content: [{ type: "image", mimeType: "image/png", data: "AA==" }],
              isError: false,
              timestamp: 1,
            },
          ],
        },
        { auth: { apiKey: "bedrock-bearer" } },
        { reasoning: "max", topP: 0.2, maxTokens: 55 },
      ),
    );

    expect(url).toBe("https://bedrock.example.test/model/anthropic.example/converse-stream");
    expect(headers?.get("authorization")).toBe("Bearer bedrock-bearer");
    expect(headers?.get("accept")).toBe("application/vnd.amazon.eventstream");
    expect(body).toMatchObject({
      system: [{ text: "system" }],
      inferenceConfig: { maxTokens: 55, topP: 0.2 },
      toolConfig: { tools: [{ toolSpec: { name: "read" } }] },
      additionalModelRequestFields: { thinking: { type: "enabled", budget_tokens: 8192 } },
      messages: [
        {
          role: "user",
          content: [{ toolResult: { toolUseId: "previous", status: "success", content: [{ image: { format: "png", source: { bytes: "AA==" } } }] } }],
        },
      ],
    });
    expect(events.at(-1)).toMatchObject({
      type: "done",
      message: {
        traceId: "request-1",
        stopReason: "toolUse",
        usage: { input: 5, output: 3, totalTokens: 8 },
        content: [
          { type: "thinking", thinking: "plan", thinkingSignature: "sig" },
          { type: "text", text: "answer" },
          { type: "toolCall", id: "call", name: "read", arguments: { path: "a" } },
        ],
      },
    });
  });

  it("projects Bedrock HTTP failures as structured terminal errors", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("denied", { status: 403 })));

    const events = await collect(streamProvider(model, { messages: [] }, { auth: { apiKey: "bad" } }));

    expect(events).toMatchObject([
      { type: "start" },
      { type: "error", reason: "error", error: { errorMessage: "Provider request failed (HTTP 403): denied" } },
    ]);
  });

  it("does not treat an unknown Bedrock message stop reason as a successful turn", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => eventStream([eventFrame("messageStop", { stopReason: "provider_fault" })])),
    );

    const events = await collect(streamProvider(model, { messages: [] }, { auth: { apiKey: "bedrock-bearer" } }));

    expect(events.at(-1)).toMatchObject({
      type: "error",
      reason: "error",
      error: { stopReason: "error", rawStopReason: "provider_fault", errorMessage: "Provider stopped with: provider_fault" },
    });
  });

  it("keeps an aborted Bedrock EventStream request distinct from an HTTP failure", async () => {
    const controller = new AbortController();
    controller.abort();
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        expect(init?.signal?.aborted).toBe(true);
        throw new DOMException("Bedrock request aborted", "AbortError");
      }),
    );

    const events = await collect(
      streamProvider(model, { messages: [] }, { auth: { apiKey: "bedrock-bearer" } }, { signal: controller.signal }),
    );

    expect(events.at(-1)).toMatchObject({
      type: "error",
      reason: "aborted",
      error: { stopReason: "aborted", errorMessage: "Bedrock request aborted" },
    });
  });
});
