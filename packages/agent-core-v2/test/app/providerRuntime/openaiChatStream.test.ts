/**
 * OpenAI Chat stream contracts — SSE deltas, tools, errors and cancellation.
 * Drives `streamProvider` with local Response streams; run with
 * `vp test -- openaiChatStream.test.ts`.
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

const model: Model = {
  id: "chat-model",
  name: "Chat model",
  api: "openai-completions",
  provider: "chat-provider",
  baseUrl: "https://chat.example.test/v1",
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

describe("OpenAI Chat streaming", () => {
  it("projects text, reasoning, usage and parallel tool-call argument deltas into one final message", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        sse([
          { id: "response-1", model: "server-model", choices: [{ delta: { content: "Hello " } }] },
          { choices: [{ delta: { reasoning_content: "considering" } }] },
          {
            choices: [
              {
                delta: {
                  tool_calls: [
                    { index: 0, id: "call-a", function: { name: "alpha", arguments: '{"left":' } },
                    { index: 1, id: "call-b", function: { name: "beta", arguments: '{"right":' } },
                  ],
                },
              },
            ],
          },
          {
            choices: [
              {
                delta: {
                  tool_calls: [
                    { index: 0, function: { arguments: "1}" } },
                    { index: 1, function: { arguments: "2}" } },
                  ],
                },
              },
            ],
            usage: {
              prompt_tokens: 5,
              completion_tokens: 7,
              total_tokens: 12,
              prompt_tokens_details: { cached_tokens: 2 },
            },
          },
          { choices: [{ delta: {}, finish_reason: "tool_calls" }] },
        ]),
      ),
    );

    const result = await collect(streamProvider(model, context(), auth));

    expect(result.events.map((event) => event.type)).toEqual([
      "start",
      "text_delta",
      "thinking_delta",
      "toolcall_start",
      "toolcall_delta",
      "toolcall_start",
      "toolcall_delta",
      "toolcall_delta",
      "toolcall_delta",
      "toolcall_end",
      "toolcall_end",
      "done",
    ]);
    expect(result.message).toMatchObject({
      responseId: "response-1",
      responseModel: "server-model",
      stopReason: "toolUse",
      finishReason: "tool_calls",
      rawStopReason: "tool_calls",
      usage: { input: 5, output: 7, cacheRead: 2, totalTokens: 12 },
      content: [
        { type: "text", text: "Hello " },
        { type: "thinking", thinking: "considering" },
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
      ],
    });
  });

  it("keeps malformed tool arguments as raw text while exposing empty structured arguments", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        sse([
          {
            choices: [
              {
                delta: {
                  tool_calls: [
                    { index: 0, id: "call-invalid", function: { name: "edit", arguments: "{bad" } },
                  ],
                },
              },
            ],
          },
          { choices: [{ delta: {}, finish_reason: "tool_calls" }] },
        ]),
      ),
    );

    const result = await collect(streamProvider(model, context(), auth));
    const tool = result.message.content.find((part) => part.type === "toolCall");

    expect(tool).toMatchObject({
      type: "toolCall",
      id: "call-invalid",
      name: "edit",
      arguments: {},
      argumentsRaw: "{bad",
    });
    expect(result.message.stopReason).toBe("toolUse");
  });

  it("maps a length finish to the final and raw stop reasons", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        sse([{ choices: [{ delta: { content: "partial" }, finish_reason: "length" }] }]),
      ),
    );

    const result = await collect(streamProvider(model, context(), auth));

    expect(result.message).toMatchObject({
      stopReason: "length",
      finishReason: "truncated",
      rawStopReason: "length",
    });
  });

  it("returns the HTTP status and structured error body in the terminal error", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(JSON.stringify({ error: { message: "credential rejected" } }), {
            status: 401,
          }),
      ),
    );

    const result = await collect(streamProvider(model, context(), auth));

    expect(result.events.map((event) => event.type)).toEqual(["start", "error"]);
    expect(result.message).toMatchObject({
      stopReason: "error",
      errorMessage:
        'Provider request failed (HTTP 401): {"error":{"message":"credential rejected"}}',
    });
  });

  it("returns an aborted message when the signal is already aborted before the request", async () => {
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
    expect(result.message.content).toEqual([]);
  });

  it("preserves streamed text after abort and can continue with that message in the next request", async () => {
    const controller = new AbortController();
    const encoder = new TextEncoder();
    let request = 0;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_input: string | URL, init?: RequestInit) => {
        request += 1;
        if (request === 2)
          return sse([{ choices: [{ delta: { content: "continued" }, finish_reason: "stop" }] }]);
        return new Response(
          new ReadableStream<Uint8Array>({
            start(stream) {
              stream.enqueue(
                encoder.encode(
                  `data: ${JSON.stringify({ choices: [{ delta: { content: "first part" } }] })}\n\n`,
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
});
