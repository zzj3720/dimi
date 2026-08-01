/**
 * OpenAI Codex stream contracts — Codex endpoint identity, bounded session
 * headers, terminal completion and cancellation. Uses local SSE Responses
 * fixtures through `streamProvider`; run with `vp test -- openaiCodexStream.test.ts`.
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
  id: "codex-model",
  name: "Codex model",
  api: "openai-codex-responses",
  provider: "openai-codex",
  baseUrl: "https://chatgpt.example.test/backend-api",
  reasoning: true,
  input: ["text"],
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  contextWindow: 128_000,
  maxTokens: 32_000,
};

function token(accountId = "account-test"): string {
  const payload = Buffer.from(
    JSON.stringify({ "https://api.openai.com/auth": { chatgpt_account_id: accountId } }),
  ).toString("base64url");
  return `header.${payload}.signature`;
}

function auth(): AuthResult {
  return { auth: { apiKey: token() } };
}

function sse(events: readonly Record<string, unknown>[]): string {
  return events.map((event) => `data: ${JSON.stringify(event)}`).join("\n\n") + "\n\n";
}

function response(events: readonly Record<string, unknown>[]): Response {
  return new Response(sse(events), { headers: { "content-type": "text/event-stream" } });
}

function headers(init: RequestInit | undefined): Headers {
  return new Headers(init?.headers);
}

function jsonBody(init: RequestInit | undefined): Record<string, unknown> {
  if (typeof init?.body !== "string") throw new TypeError("Expected a JSON request body");
  return JSON.parse(init.body) as Record<string, unknown>;
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
  return { systemPrompt: "Be concise.", messages };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("OpenAI Codex Responses streaming", () => {
  it("sends Codex request identity and completes on a terminal event without waiting for the open connection", async () => {
    const encoder = new TextEncoder();
    let capturedHeaders: Headers | undefined;
    let capturedBody: Record<string, unknown> | undefined;
    const fetch = vi.fn(async (input: string | URL, init?: RequestInit) => {
      expect(String(input)).toBe("https://chatgpt.example.test/backend-api/codex/responses");
      capturedHeaders = headers(init);
      capturedBody = jsonBody(init);
      return new Response(
        new ReadableStream<Uint8Array>({
          start(stream) {
            stream.enqueue(
              encoder.encode(
                sse([
                  { type: "response.output_text.delta", delta: "Answer" },
                  { type: "response.reasoning_text.delta", delta: "Reasoning" },
                  {
                    type: "response.output_item.added",
                    item: {
                      type: "function_call",
                      id: "item-1",
                      call_id: "call-1",
                      name: "edit",
                      arguments: "",
                    },
                  },
                  {
                    type: "response.function_call_arguments.delta",
                    item_id: "item-1",
                    delta: '{"path":',
                  },
                  {
                    type: "response.function_call_arguments.done",
                    item_id: "item-1",
                    arguments: '{"path":"README.md"}',
                  },
                  {
                    type: "response.completed",
                    response: {
                      id: "response-codex",
                      model: "server-codex-model",
                      status: "completed",
                      usage: {
                        input_tokens: 8,
                        output_tokens: 5,
                        total_tokens: 14,
                        input_tokens_details: { cached_tokens: 1 },
                      },
                    },
                  },
                ]),
              ),
            );
          },
        }),
        { headers: { "content-type": "text/event-stream" } },
      );
    });
    vi.stubGlobal("fetch", fetch);
    const sessionId = "x".repeat(80);

    const result = await collect(
      streamProvider(
        model,
        context([{ role: "user", content: "edit the file", timestamp: 1 }]),
        auth(),
        { sessionId },
      ),
    );

    expect(fetch).toHaveBeenCalledOnce();
    expect(capturedHeaders?.get("authorization")).toBe(`Bearer ${token()}`);
    expect(capturedHeaders?.get("chatgpt-account-id")).toBe("account-test");
    expect(capturedHeaders?.get("originator")).toBe("dimi");
    expect(capturedHeaders?.get("openai-beta")).toBe("responses=experimental");
    expect(capturedHeaders?.get("session-id")).toBe("x".repeat(64));
    expect(capturedHeaders?.get("x-client-request-id")).toBe("x".repeat(64));
    expect(capturedBody).toMatchObject({
      model: "codex-model",
      stream: true,
      store: false,
      instructions: "Be concise.",
      input: [{ role: "user", content: [{ type: "input_text", text: "edit the file" }] }],
    });
    expect(result.message).toMatchObject({
      responseId: "response-codex",
      responseModel: "server-codex-model",
      stopReason: "toolUse",
      finishReason: "tool_calls",
      usage: { input: 8, output: 5, cacheRead: 1, totalTokens: 14 },
      content: expect.arrayContaining([
        { type: "text", text: "Answer" },
        { type: "thinking", thinking: "Reasoning" },
        {
          type: "toolCall",
          id: "call-1",
          name: "edit",
          arguments: { path: "README.md" },
          argumentsRaw: '{"path":"README.md"}',
        },
      ]),
    });
  });

  it("surfaces terminal provider errors from Codex Responses", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        response([
          {
            type: "response.failed",
            response: {
              status: "failed",
              error: { code: "server_error", message: "Codex unavailable" },
            },
          },
        ]),
      ),
    );

    const result = await collect(streamProvider(model, context(), auth()));

    expect(result.message).toMatchObject({
      stopReason: "error",
      rawStopReason: "failed",
      errorMessage: "server_error: Codex unavailable",
    });
  });

  it("surfaces HTTP status and error body before Codex SSE starts", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(JSON.stringify({ error: { message: "subscription required" } }), {
            status: 403,
          }),
      ),
    );

    const result = await collect(streamProvider(model, context(), auth()));

    expect(result.message).toMatchObject({
      stopReason: "error",
      errorMessage:
        'Provider request failed (HTTP 403): {"error":{"message":"subscription required"}}',
    });
  });

  it("returns an aborted message when the signal is already aborted before Codex fetch", async () => {
    const controller = new AbortController();
    controller.abort();
    const fetch = vi.fn(async (_input: string | URL, init?: RequestInit) => {
      if (init?.signal?.aborted) throw new DOMException("Request was aborted", "AbortError");
      return response([]);
    });
    vi.stubGlobal("fetch", fetch);

    const result = await collect(
      streamProvider(model, context(), auth(), { signal: controller.signal }),
    );

    expect(fetch).toHaveBeenCalledOnce();
    expect(result.message).toMatchObject({
      stopReason: "aborted",
      errorMessage: "Request was aborted",
    });
  });

  it("preserves Codex partial text when an active stream is aborted", async () => {
    const controller = new AbortController();
    const encoder = new TextEncoder();
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async (_input: string | URL, init?: RequestInit) =>
          new Response(
            new ReadableStream<Uint8Array>({
              start(stream) {
                stream.enqueue(
                  encoder.encode(sse([{ type: "response.output_text.delta", delta: "partial" }])),
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
          ),
      ),
    );

    const result = await collect(
      streamProvider(model, context(), auth(), { signal: controller.signal }),
      (event) => {
        if (event.type === "text_delta") controller.abort();
      },
    );

    expect(result.message).toMatchObject({
      stopReason: "aborted",
      content: [{ type: "text", text: "partial" }],
    });
  });
});
