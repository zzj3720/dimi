/**
 * OpenAI Responses stream contracts — terminal SSE state, reasoning, tools,
 * failures and cancellation. Uses local Response streams through
 * `streamProvider`; run with `vp test -- openaiResponsesStream.test.ts`.
 */
import { afterEach, describe, expect, it, vi } from "vitest";

import { streamProvider } from "#/app/providerRuntime/stream";
import type {
  AssistantMessage,
  AssistantMessageEvent,
  AuthResult,
  Context,
  Model,
} from "#/app/providerRuntime/types";

function requestJson(init: RequestInit | undefined): Record<string, unknown> {
  if (typeof init?.body !== "string") throw new TypeError("Expected a JSON request body");
  return JSON.parse(init.body) as Record<string, unknown>;
}

const model: Model = {
  id: "responses-model",
  name: "Responses model",
  api: "openai-responses",
  provider: "openai",
  baseUrl: "https://responses.example.test/v1",
  reasoning: true,
  input: ["text"],
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  contextWindow: 128_000,
  maxTokens: 32_000,
};

const auth: AuthResult = { auth: { apiKey: "test-key" } };

function sse(events: readonly Record<string, unknown>[]): Response {
  return new Response(
    events.map((event) => `data: ${JSON.stringify(event)}`).join("\n\n") + "\n\n",
    {
      headers: { "content-type": "text/event-stream" },
    },
  );
}

async function collect(
  stream: AsyncIterable<AssistantMessageEvent>,
  onEvent?: (event: AssistantMessageEvent) => void,
): Promise<{ events: AssistantMessageEvent[]; message: AssistantMessage }> {
  const events: AssistantMessageEvent[] = [];
  for await (const event of stream) {
    events.push(event);
    onEvent?.(event);
  }
  const final = events.at(-1);
  if (final?.type === "done") return { events, message: final.message };
  if (final?.type === "error") return { events, message: final.error };
  throw new Error("Stream did not finish");
}

function context(messages: Context["messages"] = []): Context {
  return { messages };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("OpenAI Responses streaming", () => {
  it("finalizes completed streams with reasoning and multiple finalized tool calls", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        sse([
          { type: "response.output_text.delta", delta: "Answer" },
          { type: "response.reasoning_summary_text.delta", delta: "Reasoning" },
          {
            type: "response.output_item.added",
            item: {
              type: "function_call",
              id: "item-a",
              call_id: "call-a",
              name: "alpha",
              arguments: "",
            },
          },
          {
            type: "response.output_item.added",
            item: {
              type: "function_call",
              id: "item-b",
              call_id: "call-b",
              name: "beta",
              arguments: "",
            },
          },
          { type: "response.function_call_arguments.delta", item_id: "item-a", delta: '{"left":' },
          { type: "response.function_call_arguments.delta", item_id: "item-b", delta: '{"right":' },
          {
            type: "response.function_call_arguments.done",
            item_id: "item-a",
            arguments: '{"left":1}',
          },
          {
            type: "response.output_item.done",
            item: {
              type: "function_call",
              id: "item-a",
              call_id: "call-a",
              name: "alpha",
              arguments: '{"left":1}',
            },
          },
          {
            type: "response.output_item.done",
            item: {
              type: "function_call",
              id: "item-b",
              call_id: "call-b",
              name: "beta",
              arguments: '{"right":2}',
            },
          },
          {
            type: "response.completed",
            response: {
              id: "response-completed",
              model: "server-responses-model",
              status: "completed",
              usage: {
                input_tokens: 10,
                output_tokens: 6,
                total_tokens: 18,
                input_tokens_details: { cached_tokens: 2 },
              },
            },
          },
        ]),
      ),
    );

    const result = await collect(streamProvider(model, context(), auth));
    const tools = result.message.content.filter((part) => part.type === "toolCall");

    expect(result.message).toMatchObject({
      responseId: "response-completed",
      responseModel: "server-responses-model",
      stopReason: "toolUse",
      finishReason: "tool_calls",
      rawStopReason: "completed",
      usage: { input: 10, output: 6, cacheRead: 2, totalTokens: 18 },
      content: expect.arrayContaining([
        { type: "text", text: "Answer" },
        { type: "thinking", thinking: "Reasoning" },
      ]),
    });
    expect(tools).toEqual([
      {
        type: "toolCall",
        id: "call-a",
        name: "alpha",
        arguments: { left: 1 },
        argumentsRaw: '{"left":1}',
      },
      {
        type: "toolCall",
        id: "call-b",
        name: "beta",
        arguments: { right: 2 },
        argumentsRaw: '{"right":2}',
      },
    ]);
    expect(tools.every((tool) => !("partialJson" in tool))).toBe(true);
    expect(result.events.filter((event) => event.type === "toolcall_end")).toHaveLength(2);
  });

  it("maps an incomplete terminal event to a length result with terminal response facts", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        sse([
          {
            type: "response.incomplete",
            response: {
              id: "response-incomplete",
              model: "server-responses-model",
              status: "incomplete",
              usage: {
                input_tokens: 7,
                output_tokens: 3,
                total_tokens: 11,
                input_tokens_details: { cached_tokens: 1 },
              },
            },
          },
        ]),
      ),
    );

    const result = await collect(streamProvider(model, context(), auth));

    expect(result.message).toMatchObject({
      responseId: "response-incomplete",
      responseModel: "server-responses-model",
      stopReason: "length",
      finishReason: "truncated",
      rawStopReason: "incomplete",
      usage: { input: 7, output: 3, cacheRead: 1, totalTokens: 11 },
    });
  });

  it("turns a failed terminal event into a structured provider error", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        sse([
          {
            type: "response.failed",
            response: {
              id: "response-failed",
              model: "server-responses-model",
              status: "failed",
              error: { code: "server_error", message: "upstream unavailable" },
            },
          },
        ]),
      ),
    );

    const result = await collect(streamProvider(model, context(), auth));

    expect(result.message).toMatchObject({
      responseId: "response-failed",
      responseModel: "server-responses-model",
      rawStopReason: "failed",
      stopReason: "error",
      errorMessage: "server_error: upstream unavailable",
    });
  });

  it("reports an error when the response stream ends before a terminal event", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => sse([{ type: "response.output_text.delta", delta: "partial" }])),
    );

    const result = await collect(streamProvider(model, context(), auth));

    expect(result.message).toMatchObject({
      stopReason: "error",
      errorMessage: "OpenAI Responses stream ended before a terminal response event",
      content: [{ type: "text", text: "partial" }],
    });
  });

  it("returns an aborted message when the signal is already aborted before requesting Responses", async () => {
    const controller = new AbortController();
    controller.abort();
    const fetch = vi.fn(async (_input: string | URL, init?: RequestInit) => {
      if (init?.signal?.aborted) throw new DOMException("Request was aborted", "AbortError");
      return sse([]);
    });
    vi.stubGlobal("fetch", fetch);

    const result = await collect(
      streamProvider(model, context(), auth, { signal: controller.signal }),
    );

    expect(fetch).toHaveBeenCalledOnce();
    expect(result.message).toMatchObject({
      stopReason: "aborted",
      errorMessage: "Request was aborted",
    });
  });

  it("preserves partial output after abort and supports a following Responses request", async () => {
    const controller = new AbortController();
    const encoder = new TextEncoder();
    let request = 0;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_input: string | URL, init?: RequestInit) => {
        request += 1;
        if (request === 2) {
          return sse([
            { type: "response.output_text.delta", delta: "continued" },
            { type: "response.completed", response: { status: "completed" } },
          ]);
        }
        return new Response(
          new ReadableStream<Uint8Array>({
            start(stream) {
              stream.enqueue(
                encoder.encode(
                  `data: ${JSON.stringify({ type: "response.output_text.delta", delta: "first part" })}\n\n`,
                ),
              );
              init?.signal?.addEventListener(
                "abort",
                () => {
                  stream.error(new DOMException("Request was aborted", "AbortError"));
                },
                { once: true },
              );
            },
          }),
          { headers: { "content-type": "text/event-stream" } },
        );
      }),
    );
    const conversation = context([{ role: "user", content: "start", timestamp: 1 }]);
    const first = await collect(
      streamProvider(model, conversation, auth, { signal: controller.signal }),
      (event) => {
        if (event.type === "text_delta") controller.abort();
      },
    );

    expect(first.message).toMatchObject({
      stopReason: "aborted",
      content: [{ type: "text", text: "first part" }],
    });
    conversation.messages.push(first.message);
    conversation.messages.push({ role: "user", content: "continue", timestamp: 2 });

    const second = await collect(streamProvider(model, conversation, auth));

    expect(second.message).toMatchObject({
      stopReason: "stop",
      content: [{ type: "text", text: "continued" }],
    });
  });

  it("replays encrypted reasoning and sends the full Responses request intent", async () => {
    let body: Record<string, unknown> | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        body = requestJson(init);
        return sse([{ type: "response.completed", response: { status: "completed" } }]);
      }),
    );
    await collect(
      streamProvider(
        model,
        context([
          {
            role: "assistant",
            content: [{ type: "thinking", thinking: "summary", itemId: "rs_1", thinkingSignature: "encrypted" }],
            api: model.api,
            provider: model.provider,
            model: model.id,
            usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
            stopReason: "stop",
            timestamp: 1,
          },
        ]),
        auth,
        {
          topP: 0.3,
          cacheKey: "session-cache",
          responseFormat: { type: "json_object" },
        },
      ),
    );
    expect(body).toMatchObject({
      top_p: 0.3,
      prompt_cache_key: "session-cache",
      include: ["reasoning.encrypted_content"],
      text: { format: { type: "json_object" } },
      input: [
        {
          type: "reasoning",
          id: "rs_1",
          encrypted_content: "encrypted",
          summary: [{ type: "summary_text", text: "summary" }],
        },
      ],
    });
  });

  it("retains and replays the completed Responses reasoning item without reconstructing it", async () => {
    let request = 0;
    let replay: Record<string, unknown> | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        request += 1;
        if (request === 2) replay = requestJson(init);
        return request === 1
          ? sse([
              { type: "response.output_item.added", item: { type: "reasoning", id: "rs_full" } },
              { type: "response.reasoning_summary_text.delta", item_id: "rs_full", delta: "summary" },
              {
                type: "response.output_item.done",
                item: {
                  type: "reasoning",
                  id: "rs_full",
                  encrypted_content: "encrypted-full",
                  summary: [{ type: "summary_text", text: "summary" }],
                  provider_extension: { preserved: true },
                },
              },
              { type: "response.completed", response: { status: "completed" } },
            ])
          : sse([{ type: "response.completed", response: { status: "completed" } }]);
      }),
    );
    const first = await collect(streamProvider(model, context(), auth));
    const terminal = first.events.at(-1);
    if (terminal?.type !== "done") throw new Error("first Responses stream did not finish");

    await collect(streamProvider(model, context([terminal.message]), auth));

    const thinking = terminal.message.content.find((part) => part.type === "thinking");
    expect(thinking).toMatchObject({
      itemId: "rs_full",
      thinkingSignature: "encrypted-full",
      reasoningItem: { provider_extension: { preserved: true } },
    });
    expect(replay?.["input"]).toEqual([
      {
        type: "reasoning",
        id: "rs_full",
        encrypted_content: "encrypted-full",
        summary: [{ type: "summary_text", text: "summary" }],
        provider_extension: { preserved: true },
      },
    ]);
  });

  it("normalizes Azure v1 endpoints, preserves proxy query parameters, and uses only api-key authentication", async () => {
    const urls: string[] = [];
    let headers: Headers | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string, init?: RequestInit) => {
        urls.push(url);
        headers = new Headers(init?.headers);
        return sse([{ type: "response.completed", response: { status: "completed" } }]);
      }),
    );
    await collect(
      streamProvider(
        { ...model, api: "azure-openai-responses", baseUrl: "https://resource.openai.azure.com" },
        context(),
        auth,
      ),
    );
    await collect(
      streamProvider(
        { ...model, api: "azure-openai-responses", baseUrl: "https://gateway.example.test/v1?custom=true" },
        context(),
        auth,
      ),
    );
    expect(urls).toEqual([
      "https://resource.openai.azure.com/openai/v1/responses",
      "https://gateway.example.test/v1/responses?custom=true",
    ]);
    expect(headers?.get("api-key")).toBe("test-key");
    expect(headers?.get("authorization")).toBeNull();
  });

  it("keeps an always-thinking Grok Responses model on and maps max to its supported effort", async () => {
    const bodies: Record<string, unknown>[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        bodies.push(requestJson(init));
        return sse([{ type: "response.completed", response: { status: "completed" } }]);
      }),
    );
    const grok = {
      ...model,
      provider: "xai",
      thinkingLevelMap: { off: null, minimal: null, low: "low", medium: "medium", high: "high" },
      defaultThinkingLevel: "medium",
    };

    await collect(streamProvider(grok, context(), auth, { reasoning: "max" }));
    await collect(streamProvider(grok, context(), auth, { reasoning: "off" }));

    expect(bodies).toEqual([
      expect.objectContaining({ reasoning: { effort: "high", summary: "auto" } }),
      expect.objectContaining({ reasoning: { effort: "medium", summary: "auto" } }),
    ]);
  });

  it("sends the custom model's explicit off wire value instead of omitting reasoning", async () => {
    let body: Record<string, unknown> | undefined;
    vi.stubGlobal("fetch", vi.fn(async (_url: string, init?: RequestInit) => {
      body = requestJson(init);
      return sse([{ type: "response.completed", response: { status: "completed" } }]);
    }));

    await collect(streamProvider({
      ...model,
      provider: "custom-responses",
      thinkingLevelMap: { off: "none", high: "high" },
      defaultThinkingLevel: "high",
    }, context(), auth, { reasoning: "off" }));

    expect(body).toMatchObject({ reasoning: { effort: "none", summary: "auto" } });
  });

  it("uses only the configured Pi Responses session-affinity wire format", async () => {
    let headers: Headers | undefined;
    vi.stubGlobal("fetch", vi.fn(async (_url: string | URL, init?: RequestInit) => {
      headers = new Headers(init?.headers);
      return sse([{ type: "response.completed", response: { status: "completed" } }]);
    }));

    await collect(streamProvider({
      ...model,
      compat: { sessionAffinityFormat: "openai" },
    }, context(), auth, { sessionId: "session-example" }));

    expect(headers?.get("session_id")).toBe("session-example");
    expect(headers?.get("x-client-request-id")).toBe("session-example");
    expect(headers?.get("x-session-affinity")).toBeNull();
    expect(headers?.get("session-id")).toBeNull();
  });
});
