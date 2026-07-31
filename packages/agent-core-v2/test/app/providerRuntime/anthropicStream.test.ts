/**
 * Anthropic Messages stream contracts — request shape, streamed blocks and
 * provider terminal conditions. Fixtures are local SSE payloads, exercised via
 * the public `streamProvider` entry point.
 */
import { afterEach, describe, expect, it, vi } from "vitest";

import { streamProvider } from "#/app/providerRuntime/stream";
import { composeProvider } from "#/app/providerRuntime/customProviders";
import { createModels } from "#/app/providerRuntime/models";
import { builtinProviders } from "#/app/providerRuntime/providers";
import type {
  AssistantMessage,
  AssistantMessageEvent,
  AuthResult,
  Context,
  Model,
} from "#/app/providerRuntime/types";

const model: Model = {
  id: "claude-example",
  name: "Claude Example",
  api: "anthropic-messages",
  provider: "anthropic",
  baseUrl: "https://api.example.test/v1",
  reasoning: true,
  input: ["text"],
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  contextWindow: 200_000,
  maxTokens: 8_192,
};

function auth(): AuthResult {
  return { auth: { apiKey: "anthropic-test-key" } };
}

function sse(events: readonly Record<string, unknown>[]): Response {
  return new Response(
    events.map((event) => `data: ${JSON.stringify(event)}`).join("\n\n") + "\n\n",
    {
      headers: { "content-type": "text/event-stream" },
    },
  );
}

function context(tools?: Context["tools"]): Context {
  return { systemPrompt: "Be concise.", messages: [], tools };
}

function jsonBody(init: RequestInit | undefined): Record<string, unknown> {
  if (typeof init?.body !== "string") throw new TypeError("Expected a JSON request body");
  return JSON.parse(init.body) as Record<string, unknown>;
}

async function collect(
  stream: AsyncIterable<AssistantMessageEvent>,
): Promise<AssistantMessageEvent[]> {
  const events: AssistantMessageEvent[] = [];
  for await (const event of stream) events.push(event);
  return events;
}

function terminal(events: AssistantMessageEvent[]): AssistantMessage {
  const event = events.at(-1);
  if (event?.type === "done") return event.message;
  if (event?.type === "error") return event.error;
  throw new Error("Stream did not terminate");
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("Anthropic Messages streaming", () => {
  it("keeps Anthropic's provider-owned API-key wire header when models.json supplies the key", async () => {
    let capturedHeaders: Headers | undefined;
    vi.stubGlobal("fetch", vi.fn(async (_input: string | URL, init?: RequestInit) => {
      capturedHeaders = new Headers(init?.headers);
      return sse([{ type: "message_delta", delta: { stop_reason: "end_turn" } }]);
    }));
    const base = builtinProviders().find((provider) => provider.id === "anthropic");
    if (base === undefined) throw new Error("missing Anthropic provider");
    const provider = composeProvider({ id: "anthropic", apiKey: "$OVERLAY_ANTHROPIC_KEY" }, base);
    const models = createModels({
      providers: [provider],
      authContext: {
        env: async (name) => name === "OVERLAY_ANTHROPIC_KEY" ? "overlay-key" : undefined,
        fileExists: async () => false,
      },
    });
    const configuredModel = models.getModels("anthropic")[0];
    if (configuredModel === undefined) throw new Error("missing Anthropic model");
    const configuredAuth = await models.getAuth(configuredModel);
    if (configuredAuth === undefined) throw new Error("missing Anthropic auth");

    await collect(streamProvider(configuredModel, context(), configuredAuth));

    expect(capturedHeaders?.get("x-api-key")).toBe("overlay-key");
    expect(capturedHeaders?.get("authorization")).toBeNull();
  });

  it("sends the baseline Anthropic version header and omits unsupported tool extensions without tools", async () => {
    let capturedHeaders: Headers | undefined;
    let capturedBody: Record<string, unknown> | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_input: string | URL, init?: RequestInit) => {
        capturedHeaders = new Headers(init?.headers);
        capturedBody = jsonBody(init);
        return sse([{ type: "message_delta", delta: { stop_reason: "end_turn" } }]);
      }),
    );

    await collect(streamProvider(model, context(), auth()));

    expect(capturedHeaders?.get("anthropic-version")).toBe("2023-06-01");
    expect(capturedHeaders?.get("anthropic-beta")).toBeNull();
    expect(capturedHeaders?.get("x-api-key")).toBe("anthropic-test-key");
    expect(capturedBody).toMatchObject({
      model: "claude-example",
      stream: true,
      system: "Be concise.",
      messages: [],
      max_tokens: 8_192,
    });
    expect(capturedBody).not.toHaveProperty("tools");
    expect(capturedBody).not.toHaveProperty("thinking");
  });

  it("omits temperature when a configured Anthropic-compatible endpoint rejects it", async () => {
    let capturedBody: Record<string, unknown> | undefined;
    vi.stubGlobal("fetch", vi.fn(async (_input: string | URL, init?: RequestInit) => {
      capturedBody = jsonBody(init);
      return sse([{ type: "message_delta", delta: { stop_reason: "end_turn" } }]);
    }));

    await collect(streamProvider({ ...model, compat: { supportsTemperature: false } }, context(), auth(), {
      temperature: 0.7,
    }));

    expect(capturedBody).not.toHaveProperty("temperature");
  });

  it("serializes tools with the current basic schema contract, without beta or strict extensions", async () => {
    let capturedHeaders: Headers | undefined;
    let capturedBody: Record<string, unknown> | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_input: string | URL, init?: RequestInit) => {
        capturedHeaders = new Headers(init?.headers);
        capturedBody = jsonBody(init);
        return sse([{ type: "message_delta", delta: { stop_reason: "end_turn" } }]);
      }),
    );

    await collect(
      streamProvider(
        model,
        context([
          {
            name: "read_file",
            description: "Read one file.",
            parameters: {
              type: "object",
              properties: { path: { type: "string" } },
              required: ["path"],
            },
          },
        ]),
        auth(),
      ),
    );

    expect(capturedHeaders?.get("anthropic-version")).toBe("2023-06-01");
    expect(capturedHeaders?.get("anthropic-beta")).toBeNull();
    expect(capturedBody?.["tools"]).toEqual([
      {
        name: "read_file",
        description: "Read one file.",
        input_schema: {
          type: "object",
          properties: { path: { type: "string" } },
          required: ["path"],
        },
      },
    ]);
  });

  it("assembles text, signed thinking and a streamed tool input with usage and raw stop reason", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        sse([
          {
            type: "message_start",
            message: {
              id: "msg-example",
              usage: {
                input_tokens: 12,
                cache_read_input_tokens: 3,
                cache_creation_input_tokens: 2,
              },
            },
          },
          { type: "content_block_start", index: 0, content_block: { type: "thinking" } },
          {
            type: "content_block_delta",
            index: 0,
            delta: { type: "thinking_delta", thinking: "Check " },
          },
          {
            type: "content_block_delta",
            index: 0,
            delta: { type: "thinking_delta", thinking: "the file." },
          },
          {
            type: "content_block_delta",
            index: 0,
            delta: { type: "signature_delta", signature: "sig-" },
          },
          {
            type: "content_block_delta",
            index: 0,
            delta: { type: "signature_delta", signature: "example" },
          },
          { type: "content_block_start", index: 1, content_block: { type: "text" } },
          {
            type: "content_block_delta",
            index: 1,
            delta: { type: "text_delta", text: "I will read it." },
          },
          {
            type: "content_block_start",
            index: 2,
            content_block: { type: "tool_use", id: "tool-1", name: "read_file", input: {} },
          },
          {
            type: "content_block_delta",
            index: 2,
            delta: { type: "input_json_delta", partial_json: '{"path":' },
          },
          {
            type: "content_block_delta",
            index: 2,
            delta: { type: "input_json_delta", partial_json: '"README.md"}' },
          },
          {
            type: "message_delta",
            delta: { stop_reason: "tool_use" },
            usage: { output_tokens: 7 },
          },
          { type: "message_stop" },
        ]),
      ),
    );

    const events = await collect(streamProvider(model, context(), auth()));

    expect(events.filter((event) => event.type === "thinking_delta")).toHaveLength(2);
    expect(events.filter((event) => event.type === "toolcall_delta")).toHaveLength(2);
    expect(terminal(events)).toMatchObject({
      responseId: "msg-example",
      stopReason: "toolUse",
      finishReason: "tool_calls",
      rawStopReason: "tool_use",
      usage: { input: 12, output: 7, cacheRead: 3, cacheWrite: 2, totalTokens: 24 },
      content: [
        { type: "thinking", thinking: "Check the file.", thinkingSignature: "sig-example" },
        { type: "text", text: "I will read it." },
        {
          type: "toolCall",
          id: "tool-1",
          name: "read_file",
          arguments: { path: "README.md" },
          argumentsRaw: '{"path":"README.md"}',
        },
      ],
    });
  });

  it("repairs only a trailing comma in final tool JSON and retains unrecoverable raw input", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        sse([
          {
            type: "content_block_start",
            index: 0,
            content_block: { type: "tool_use", id: "repair", name: "edit", input: {} },
          },
          {
            type: "content_block_delta",
            index: 0,
            delta: { type: "input_json_delta", partial_json: '{"path":"a.ts",}' },
          },
          {
            type: "content_block_start",
            index: 1,
            content_block: { type: "tool_use", id: "invalid", name: "edit", input: {} },
          },
          {
            type: "content_block_delta",
            index: 1,
            delta: { type: "input_json_delta", partial_json: '{path:"b.ts"}' },
          },
          { type: "message_delta", delta: { stop_reason: "tool_use" } },
        ]),
      ),
    );

    const message = terminal(await collect(streamProvider(model, context(), auth())));

    expect(message.content).toEqual([
      {
        type: "toolCall",
        id: "repair",
        name: "edit",
        arguments: { path: "a.ts" },
        argumentsRaw: '{"path":"a.ts",}',
      },
      {
        type: "toolCall",
        id: "invalid",
        name: "edit",
        arguments: {},
        argumentsRaw: '{path:"b.ts"}',
      },
    ]);
  });

  it.each([
    ["refusal", { explanation: "Policy denied this request." }, "Policy denied this request."],
    ["sensitive", undefined, "Provider stopped with: sensitive"],
    ["future_stop", undefined, "Unhandled Anthropic stop reason: future_stop"],
  ] as const)(
    "reports %s as a terminal error rather than silently completing",
    async (stopReason, stopDetails, message) => {
      vi.stubGlobal(
        "fetch",
        vi.fn(async () =>
          sse([
            {
              type: "message_delta",
              delta: { stop_reason: stopReason, stop_details: stopDetails },
            },
          ]),
        ),
      );

      const result = terminal(await collect(streamProvider(model, context(), auth())));

      expect(result).toMatchObject({
        stopReason: "error",
        rawStopReason: stopReason,
        errorMessage: message,
      });
    },
  );

  it("surfaces structured HTTP and SSE error frames", async () => {
    const fetch = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({ error: { type: "authentication_error", message: "Bad key" } }),
          { status: 401 },
        ),
      )
      .mockResolvedValueOnce(
        sse([{ type: "error", error: { type: "overloaded_error", message: "Try later" } }]),
      );
    vi.stubGlobal("fetch", fetch);

    const http = terminal(await collect(streamProvider(model, context(), auth())));
    const stream = terminal(await collect(streamProvider(model, context(), auth())));

    expect(http).toMatchObject({
      stopReason: "error",
      errorMessage: expect.stringContaining("HTTP 401"),
    });
    expect(stream).toMatchObject({
      stopReason: "error",
      errorMessage: "overloaded_error: Try later",
    });
  });

  it("reports a pre-aborted request without issuing a successful completion", async () => {
    const controller = new AbortController();
    controller.abort();
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_input: string | URL, init?: RequestInit) => {
        expect(init?.signal?.aborted).toBe(true);
        throw new DOMException("Aborted", "AbortError");
      }),
    );

    const result = terminal(
      await collect(streamProvider(model, context(), auth(), { signal: controller.signal })),
    );

    expect(result).toMatchObject({ stopReason: "aborted", errorMessage: "Aborted" });
  });

  it("preserves a partial response on mid-stream abort and permits a later continuation", async () => {
    const controller = new AbortController();
    const encoder = new TextEncoder();
    let continuationBody: Record<string, unknown> | undefined;
    const fetch = vi
      .fn()
      .mockImplementationOnce(
        async () =>
          new Response(
            new ReadableStream<Uint8Array>({
              start(stream) {
                stream.enqueue(
                  encoder.encode(
                    'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Partial"}}\n\n',
                  ),
                );
                controller.signal.addEventListener("abort", () => {
                  stream.error(new DOMException("Aborted", "AbortError"));
                });
              },
            }),
            { headers: { "content-type": "text/event-stream" } },
          ),
      )
      .mockImplementationOnce(async (_input: string | URL, init?: RequestInit) => {
        continuationBody = jsonBody(init);
        return sse([
          {
            type: "content_block_delta",
            index: 0,
            delta: { type: "text_delta", text: "Continued" },
          },
          { type: "message_delta", delta: { stop_reason: "end_turn" } },
        ]);
      });
    vi.stubGlobal("fetch", fetch);

    const events: AssistantMessageEvent[] = [];
    for await (const event of streamProvider(model, context(), auth(), {
      signal: controller.signal,
    })) {
      events.push(event);
      if (event.type === "text_delta") controller.abort();
    }
    const partial = terminal(events);
    const next = terminal(
      await collect(
        streamProvider(
          model,
          {
            systemPrompt: "Be concise.",
            messages: [{ role: "user", content: "Continue.", timestamp: 1 }, partial],
          },
          auth(),
        ),
      ),
    );

    expect(partial).toMatchObject({
      stopReason: "aborted",
      content: [{ type: "text", text: "Partial" }],
    });
    expect(continuationBody?.["messages"]).toEqual([
      { role: "user", content: "Continue." },
      { role: "assistant", content: [{ type: "text", text: "Partial" }] },
    ]);
    expect(next).toMatchObject({
      stopReason: "stop",
      content: [{ type: "text", text: "Continued" }],
    });
  });
});
