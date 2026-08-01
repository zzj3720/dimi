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

function requestJson(init: RequestInit | undefined): Record<string, unknown> {
  if (typeof init?.body !== "string") throw new TypeError("Expected a JSON request body");
  return JSON.parse(init.body) as Record<string, unknown>;
}

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

function deepSeekFlash(): Model {
  return {
    ...model,
    id: "deepseek-v4-flash",
    name: "DeepSeek V4 Flash",
    provider: "deepseek",
    baseUrl: "https://api.deepseek.com/v1",
    compat: {
      requiresReasoningContentOnAssistantMessages: true,
      supportsReasoningEffort: true,
      thinkingFormat: "deepseek",
    },
    thinkingLevelMap: { low: "low", high: "high", max: "max" },
    defaultThinkingLevel: "high",
  };
}

function deepSeekPro(): Model {
  return {
    ...deepSeekFlash(),
    id: "deepseek-v4-pro",
    name: "DeepSeek V4 Pro",
    thinkingLevelMap: { high: "high", max: "max" },
  };
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

  it.each(["content_filter", "network_error", "unknown_finish"]) (
    "returns OpenAI Chat finish_reason %s as a terminal error",
    async (finishReason) => {
      vi.stubGlobal(
        "fetch",
        vi.fn(async () => sse([{ choices: [{ delta: {}, finish_reason: finishReason }] }])),
      );

      const result = await collect(streamProvider(model, context(), auth));

      expect(result.message).toMatchObject({
        stopReason: "error",
        rawStopReason: finishReason,
        errorMessage: `Provider finish_reason: ${finishReason}`,
      });
    },
  );

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

  it("sends sampling, prompt cache, structured output, reasoning and tool-result images", async () => {
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
        context([
          {
            role: "assistant",
            content: [{ type: "thinking", thinking: "prior", thinkingSignature: "reasoning_content" }],
            api: model.api,
            provider: model.provider,
            model: model.id,
            usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
            stopReason: "stop",
            timestamp: 1,
          },
          {
            role: "toolResult",
            toolCallId: "call-1",
            toolName: "image",
            content: [{ type: "image", mimeType: "image/png", data: "AA==" }],
            isError: false,
            timestamp: 2,
          },
        ]),
        auth,
        {
          topP: 0.2,
          cacheKey: "cache-key",
          responseFormat: { type: "json_schema", jsonSchema: { name: "answer", schema: { type: "object" }, strict: true } },
        },
      ),
    );

    expect(body).toMatchObject({
      top_p: 0.2,
      prompt_cache_key: "cache-key",
      response_format: { type: "json_schema", json_schema: { name: "answer", schema: { type: "object" }, strict: true } },
    });
    expect(body?.["messages"]).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ role: "assistant", reasoning_content: "prior" }),
        expect.objectContaining({ role: "tool", content: [{ type: "image_url", image_url: { url: "data:image/png;base64,AA==" } }] }),
      ]),
    );
  });

  it.each(["low", "high", "max"] as const)(
    "sends DeepSeek V4 Flash effort %s when thinking is enabled",
    async (effort) => {
      let body: Record<string, unknown> | undefined;
      vi.stubGlobal(
        "fetch",
        vi.fn(async (_url: string, init?: RequestInit) => {
          body = requestJson(init);
          return sse([{ choices: [{ delta: {}, finish_reason: "stop" }] }]);
        }),
      );

      await collect(streamProvider(deepSeekFlash(), context(), auth, { reasoning: effort }));

      expect(body).toMatchObject({
        thinking: { type: "enabled" },
        reasoning_effort: effort,
      });
    },
  );

  it.each([
    ["low", "high"],
    ["high", "high"],
    ["xhigh", "max"],
    ["max", "max"],
  ] as const)("maps DeepSeek V4 Pro effort %s to %s", async (requested, expected) => {
    let body: Record<string, unknown> | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        body = requestJson(init);
        return sse([{ choices: [{ delta: {}, finish_reason: "stop" }] }]);
      }),
    );

    await collect(streamProvider(deepSeekPro(), context(), auth, { reasoning: requested }));

    expect(body).toMatchObject({
      thinking: { type: "enabled" },
      reasoning_effort: expected,
    });
  });

  it("disables DeepSeek V4 Flash thinking without sending an effort", async () => {
    let body: Record<string, unknown> | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        body = requestJson(init);
        return sse([{ choices: [{ delta: {}, finish_reason: "stop" }] }]);
      }),
    );

    await collect(streamProvider(deepSeekFlash(), context(), auth, { reasoning: "off" }));

    expect(body).toMatchObject({ thinking: { type: "disabled" } });
    expect(body).not.toHaveProperty("reasoning_effort");
  });

  it("replays DeepSeek reasoning content without leaking its opaque signature", async () => {
    let body: Record<string, unknown> | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        body = requestJson(init);
        return sse([{ choices: [{ delta: {}, finish_reason: "stop" }] }]);
      }),
    );
    const history: Context = {
      messages: [
        {
          role: "assistant",
          content: [{ type: "thinking", thinking: "prior", thinkingSignature: "encrypted-responses-value" }],
          api: "openai-responses",
          provider: "other",
          model: "other",
          usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
          stopReason: "stop",
          timestamp: 1,
        },
      ],
    };

    await collect(streamProvider(deepSeekFlash(), history, auth, { reasoning: "high" }));

    const assistant = (body?.["messages"] as Record<string, unknown>[]).find(
      (message) => message["role"] === "assistant",
    );
    expect(assistant).toMatchObject({ reasoning_content: "prior" });
    expect(assistant).not.toHaveProperty("encrypted-responses-value");
  });

  it("enables boolean OpenAI thinking with the protocol's default effort", async () => {
    let body: Record<string, unknown> | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        body = requestJson(init);
        return sse([{ choices: [{ delta: {}, finish_reason: "stop" }] }]);
      }),
    );

    await collect(streamProvider({ ...model, thinkingLevelMap: undefined }, context(), auth, { reasoning: "medium" }));

    expect(body).toMatchObject({ reasoning_effort: "medium" });
  });

  it("projects Pi-compatible OpenAI request and replay compatibility fields on the wire", async () => {
    let body: Record<string, any> | undefined;
    vi.stubGlobal("fetch", vi.fn(async (_url: string, init?: RequestInit) => {
      body = requestJson(init);
      return sse([{ choices: [{ delta: {}, finish_reason: "stop" }] }]);
    }));
    const compatible = {
      ...model,
      compat: {
        supportsStore: true,
        supportsUsageInStreaming: false,
        maxTokensField: "max_completion_tokens",
        supportsDeveloperRole: true,
        requiresToolResultName: true,
        requiresAssistantAfterToolResult: true,
        requiresThinkingAsText: true,
        supportsStrictMode: false,
        zaiToolStream: true,
        vercelGatewayRouting: { only: ["example-upstream"] },
      },
    };
    await collect(streamProvider(compatible, {
      systemPrompt: "system",
      tools: [{ name: "tool", description: "tool", parameters: { type: "object" } }],
      messages: [
        {
          role: "assistant",
          content: [{ type: "thinking", thinking: "private" }],
          api: compatible.api, provider: compatible.provider, model: compatible.id,
          usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
          stopReason: "stop", timestamp: 1,
        },
        { role: "toolResult", toolCallId: "call-1", toolName: "tool", content: [{ type: "text", text: "result" }], isError: false, timestamp: 2 },
        { role: "user", content: "continue", timestamp: 3 },
      ],
    }, auth, { maxTokens: 123 }));
    expect(body).toMatchObject({
      store: false,
      max_completion_tokens: 123,
      tool_stream: true,
      providerOptions: { gateway: { only: ["example-upstream"] } },
    });
    expect(body).not.toHaveProperty("stream_options");
    expect(body).not.toHaveProperty("max_tokens");
    expect(body?.["tools"][0].function).not.toHaveProperty("strict");
    expect(body?.["messages"]).toEqual(expect.arrayContaining([
      expect.objectContaining({ role: "developer", content: "system" }),
      expect.objectContaining({ role: "assistant", content: "private" }),
      expect.objectContaining({ role: "tool", tool_call_id: "call-1", name: "tool" }),
      expect.objectContaining({ role: "assistant", content: "I have processed the tool results." }),
    ]));
  });

  it.each([
    ["qwen", { enable_thinking: true }],
    ["qwen-chat-template", { chat_template_kwargs: { enable_thinking: true, preserve_thinking: true } }],
    ["together", { reasoning: { enabled: true } }],
    ["zai", { thinking: { type: "enabled", clear_thinking: false } }],
    ["string-thinking", { thinking: "high" }],
    ["ant-ling", { reasoning: { effort: "high" } }],
    ["chat-template", { chat_template_kwargs: { enabled: true, effort: "high" } }],
  ] as const)("serializes Pi thinking format %s", async (thinkingFormat, expected) => {
    let body: Record<string, unknown> | undefined;
    vi.stubGlobal("fetch", vi.fn(async (_url: string, init?: RequestInit) => {
      body = requestJson(init);
      return sse([{ choices: [{ delta: {}, finish_reason: "stop" }] }]);
    }));
    await collect(streamProvider({
      ...model,
      compat: {
        thinkingFormat,
        supportsReasoningEffort: true,
        ...(thinkingFormat === "chat-template" ? { chatTemplateKwargs: { enabled: { $var: "thinking.enabled" }, effort: { $var: "thinking.effort" } } } : undefined),
      },
      thinkingLevelMap: { high: "high" },
    }, context(), auth, { reasoning: "high" }));
    expect(body).toMatchObject(expected);
  });

  it("honors Pi chat-template omitWhenOff variables", async () => {
    let body: Record<string, unknown> | undefined;
    vi.stubGlobal("fetch", vi.fn(async (_url: string, init?: RequestInit) => {
      body = requestJson(init);
      return sse([{ choices: [{ delta: {}, finish_reason: "stop" }] }]);
    }));
    await collect(streamProvider({
      ...model,
      compat: {
        thinkingFormat: "chat-template",
        chatTemplateKwargs: {
          enabled: { $var: "thinking.enabled" },
          effort: { $var: "thinking.effort", omitWhenOff: true },
        },
      },
      thinkingLevelMap: undefined,
    }, context(), auth, { reasoning: "off" }));
    expect(body).toMatchObject({ chat_template_kwargs: { enabled: false } });
    expect((body?.["chat_template_kwargs"] as Record<string, unknown>)?.["effort"]).toBeUndefined();
  });

  it("sends OpenAI-completions affinity only when Pi compat enables it", async () => {
    let headers: Headers | undefined;
    vi.stubGlobal("fetch", vi.fn(async (_url: string, init?: RequestInit) => {
      headers = new Headers(init?.headers);
      return sse([{ choices: [{ delta: {}, finish_reason: "stop" }] }]);
    }));
    await collect(streamProvider({
      ...model,
      compat: { sendSessionAffinityHeaders: true, sessionAffinityFormat: "openai-nosession" },
    }, context(), auth, { sessionId: "session-example" }));
    expect(headers?.get("x-client-request-id")).toBe("session-example");
    expect(headers?.get("x-session-affinity")).toBe("session-example");
    expect(headers?.get("session_id")).toBeNull();
    expect(headers?.get("session-id")).toBeNull();
  });
});
