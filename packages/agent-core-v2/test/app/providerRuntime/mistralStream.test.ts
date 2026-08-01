import { afterEach, describe, expect, it, vi } from "vitest";

import { streamProvider } from "#/app/providerRuntime/stream";
import type { AssistantMessageEvent, Model } from "#/app/providerRuntime/types";

function requestJson(init: RequestInit | undefined): Record<string, unknown> {
  if (typeof init?.body !== "string") throw new TypeError("Expected a JSON request body");
  return JSON.parse(init.body) as Record<string, unknown>;
}

const model: Model = {
  id: "mistral-example",
  name: "Mistral example",
  api: "mistral-conversations",
  provider: "mistral",
  baseUrl: "https://mistral.example.test/v1",
  reasoning: true,
  input: ["text", "image"],
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  contextWindow: 128_000,
  maxTokens: 8_192,
  thinkingLevelMap: { high: "high" },
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

describe("Mistral conversations adapter", () => {
  it("uses Mistral's chat wire shape, strict tools, affinity and SSE content", async () => {
    let url = "";
    let headers: Headers | undefined;
    let body: Record<string, unknown> | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string, init?: RequestInit) => {
        url = input;
        headers = new Headers(init?.headers);
        body = requestJson(init);
        return sse([
          {
            id: "mistral-response",
            choices: [{ delta: { content: [{ type: "thinking", thinking: [{ type: "text", text: "plan" }] }, { type: "text", text: "answer" }] } }],
          },
          {
            choices: [{ delta: { tool_calls: [{ index: 0, id: "tool-1", function: { name: "read", arguments: "{\"path\":\"a\"}" } }] }, finish_reason: "tool_calls" }],
            usage: { prompt_tokens: 5, completion_tokens: 3, total_tokens: 8 },
          },
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
              toolCallId: "tool/previous",
              toolName: "read",
              content: [{ type: "image", mimeType: "image/png", data: "AA==" }],
              isError: false,
              timestamp: 1,
            },
          ],
        },
        { auth: { apiKey: "mistral-key" } },
        { cacheKey: "affinity", reasoning: "max", topP: 0.3, maxTokens: 44 },
      ),
    );

    expect(url).toBe("https://mistral.example.test/v1/chat/completions");
    expect(headers?.get("authorization")).toBe("Bearer mistral-key");
    expect(headers?.get("x-affinity")).toBe("affinity");
    expect(body).toMatchObject({
      model: "mistral-example",
      stream: true,
      top_p: 0.3,
      max_tokens: 44,
      prompt_mode: "reasoning",
      reasoning_effort: "high",
      tools: [{ type: "function", function: { name: "read", strict: true } }],
    });
    expect(body?.["messages"]).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ role: "system", content: "system" }),
        expect.objectContaining({ role: "tool", tool_call_id: expect.stringMatching(/^[A-Za-z0-9]{9}$/u) }),
      ]),
    );
    expect(events.at(-1)).toMatchObject({
      type: "done",
      message: {
        responseId: "mistral-response",
        stopReason: "toolUse",
        usage: { input: 5, output: 3, totalTokens: 8 },
        content: [
          { type: "thinking", thinking: "plan" },
          { type: "text", text: "answer" },
          { type: "toolCall", id: "tool-1", name: "read", arguments: { path: "a" } },
        ],
      },
    });
  });

  it("projects an HTTP failure as the Mistral terminal error", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("invalid key", { status: 401 })));

    const events = await collect(streamProvider(model, { messages: [] }, { auth: { apiKey: "bad" } }));

    expect(events).toMatchObject([
      { type: "start" },
      { type: "error", reason: "error", error: { errorMessage: "Provider request failed (HTTP 401): invalid key" } },
    ]);
  });

  it.each(["error", "unexpected_finish_reason"]) (
    "treats Mistral finish_reason %s as a terminal provider error",
    async (finishReason) => {
      vi.stubGlobal(
        "fetch",
        vi.fn(async () => sse([{ choices: [{ delta: {}, finish_reason: finishReason }] }])),
      );

      const events = await collect(streamProvider(model, { messages: [] }, { auth: { apiKey: "key" } }));

      expect(events.at(-1)).toMatchObject({
        type: "error",
        reason: "error",
        error: { stopReason: "error", rawStopReason: finishReason, errorMessage: `Provider stopped with: ${finishReason}` },
      });
    },
  );

  it("normalizes empty, null-like and long Mistral tool ids when replaying tool results", async () => {
    let body: Record<string, unknown> | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        body = requestJson(init);
        return sse([{ choices: [{ delta: {}, finish_reason: "stop" }] }]);
      }),
    );
    await collect(
      streamProvider(
        model,
        {
          messages: [
            { role: "toolResult", toolCallId: "", toolName: "a", content: [{ type: "text", text: "a" }], isError: false, timestamp: 1 },
            { role: "toolResult", toolCallId: "null", toolName: "b", content: [{ type: "text", text: "b" }], isError: false, timestamp: 2 },
            { role: "toolResult", toolCallId: "very-long/tool-call-id", toolName: "c", content: [{ type: "text", text: "c" }], isError: false, timestamp: 3 },
          ],
        },
        { auth: { apiKey: "key" } },
      ),
    );

    const ids = (body?.["messages"] as Record<string, unknown>[])
      .filter((message) => message["role"] === "tool")
      .map((message) => message["tool_call_id"]);
    expect(ids).toHaveLength(3);
    expect(ids).toEqual(ids.map((id) => expect.stringMatching(/^[A-Za-z0-9]{9}$/u)));
    expect(new Set(ids).size).toBe(3);
  });

  it("classifies an aborted Mistral request without converting it to a provider failure", async () => {
    const controller = new AbortController();
    controller.abort();
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        expect(init?.signal?.aborted).toBe(true);
        throw new DOMException("Mistral request aborted", "AbortError");
      }),
    );

    const events = await collect(
      streamProvider(model, { messages: [] }, { auth: { apiKey: "key" } }, { signal: controller.signal }),
    );

    expect(events.at(-1)).toMatchObject({
      type: "error",
      reason: "aborted",
      error: { stopReason: "aborted", errorMessage: "Mistral request aborted" },
    });
  });
});
