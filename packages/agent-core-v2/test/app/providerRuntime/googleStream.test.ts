/**
 * Google Generative AI adapter contracts — request authentication, multimodal
 * replay, thought signatures, parallel function responses and SSE projection.
 */
import { afterEach, describe, expect, it, vi } from "vitest";

import { streamProvider } from "#/app/providerRuntime/stream";
import type { AssistantMessageEvent, AuthResult, Context, Model } from "#/app/providerRuntime/types";

function requestJson(init: RequestInit | undefined): Record<string, unknown> {
  if (typeof init?.body !== "string") throw new TypeError("Expected a JSON request body");
  return JSON.parse(init.body) as Record<string, unknown>;
}

const model: Model = {
  id: "gemini-example",
  name: "Gemini example",
  api: "google-generative-ai",
  provider: "google",
  baseUrl: "https://google.example.test/v1beta",
  reasoning: true,
  input: ["text", "image"],
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  contextWindow: 128_000,
  maxTokens: 8_192,
};

const auth: AuthResult = { auth: { apiKey: "google-key" } };

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

describe("Google Generative AI streaming", () => {
  it("sends only Google API-key authentication and preserves URL image input", async () => {
    let request: RequestInit | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        request = init;
        return sse([{ candidates: [{ finishReason: "STOP", content: { parts: [{ text: "ok" }] } }] }]);
      }),
    );

    await collect(
      streamProvider(
        model,
        { messages: [{ role: "user", content: [{ type: "image", mimeType: "image/png", url: "https://image.example.test/a.png" }], timestamp: 1 }] },
        auth,
        { topP: 0.4, maxTokens: 99, responseFormat: { type: "json_object" } },
      ),
    );

    const headers = new Headers(request?.headers);
    const body = requestJson(request);
    expect(headers.get("x-goog-api-key")).toBe("google-key");
    expect(headers.get("authorization")).toBeNull();
    expect(body).toMatchObject({
      generationConfig: { topP: 0.4, maxOutputTokens: 99, responseMimeType: "application/json" },
      contents: [{ role: "user", parts: [{ fileData: { mimeType: "image/png", fileUri: "https://image.example.test/a.png" } }] }],
    });
  });

  it("retains thought signatures and combines parallel function results into one user turn", async () => {
    let body: Record<string, unknown> | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        body = requestJson(init);
        return sse([
          {
            usageMetadata: {
              promptTokenCount: 9,
              cachedContentTokenCount: 2,
              candidatesTokenCount: 3,
              thoughtsTokenCount: 1,
              totalTokenCount: 10,
            },
            candidates: [
              {
                finishReason: "STOP",
                content: {
                  parts: [
                    { text: "thought", thought: true, thoughtSignature: "sig" },
                    { functionCall: { id: "call-a", name: "alpha", args: { value: 1 } }, thoughtSignature: "tool-sig" },
                  ],
                },
              },
            ],
          },
        ]);
      }),
    );
    const events = await collect(
      streamProvider(
        model,
        {
          messages: [
            { role: "toolResult", toolCallId: "call-a", toolName: "alpha", content: [{ type: "text", text: "one" }], isError: false, timestamp: 1 },
            { role: "toolResult", toolCallId: "call-b", toolName: "beta", content: [{ type: "text", text: "two" }], isError: false, timestamp: 2 },
          ],
        },
        auth,
      ),
    );
    expect(body?.["contents"]).toEqual([
      {
        role: "user",
        parts: [
          { functionResponse: { name: "alpha", id: "call-a", response: { content: "one" }, parts: [] } },
          { functionResponse: { name: "beta", id: "call-b", response: { content: "two" }, parts: [] } },
        ],
      },
    ]);
    const terminal = events.at(-1);
    expect(terminal).toMatchObject({
      type: "done",
      message: {
        usage: { input: 7, output: 4, reasoning: 1, cacheRead: 2, totalTokens: 10 },
        content: [
          { type: "thinking", thinking: "thought", thinkingSignature: "sig" },
          { type: "toolCall", id: "call-a", name: "alpha", thoughtSignature: "tool-sig" },
        ],
      },
    });
  });

  it("replays Google thought and function signatures on the next request", async () => {
    let body: Record<string, unknown> | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        body = requestJson(init);
        return sse([{ candidates: [{ finishReason: "STOP", content: { parts: [] } }] }]);
      }),
    );

    await collect(
      streamProvider(
        model,
        {
          messages: [
            {
              role: "assistant",
              api: "google-generative-ai",
              provider: "google",
              model: "gemini-example",
              content: [
                { type: "thinking", thinking: "prior plan", thinkingSignature: "thought-signature" },
                {
                  type: "toolCall",
                  id: "call-1",
                  name: "read",
                  arguments: { path: "a.ts" },
                  thoughtSignature: "call-signature",
                },
              ],
              usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
              stopReason: "toolUse",
              timestamp: 1,
            },
          ],
        },
        auth,
      ),
    );

    expect(body?.["contents"]).toEqual([
      {
        role: "model",
        parts: [
          { text: "prior plan", thought: true, thoughtSignature: "thought-signature" },
          { functionCall: { name: "read", args: { path: "a.ts" }, id: "call-1" }, thoughtSignature: "call-signature" },
        ],
      },
    ]);
  });

  it("uses Google API-key authentication and the Vertex publisher path", async () => {
    let url = "";
    let headers: Headers | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string, init?: RequestInit) => {
        url = input;
        headers = new Headers(init?.headers);
        return sse([{ candidates: [{ finishReason: "STOP", content: { parts: [] } }] }]);
      }),
    );
    await collect(
      streamProvider(
        { ...model, api: "google-vertex", baseUrl: "https://vertex.example.test/v1/projects/p/locations/l" },
        { messages: [] },
        { auth: { apiKey: "access-token" } },
      ),
    );
    expect(url).toBe("https://vertex.example.test/v1/projects/p/locations/l/publishers/google/models/gemini-example:streamGenerateContent?alt=sse");
    expect(headers?.get("authorization")).toBeNull();
    expect(headers?.get("x-goog-api-key")).toBe("access-token");
  });

  it("maps the normalized thinking effort through the Google uppercase wire value", async () => {
    let body: Record<string, unknown> | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        body = requestJson(init);
        return sse([{ candidates: [{ finishReason: "STOP", content: { parts: [] } }] }]);
      }),
    );
    await collect(
      streamProvider(
        { ...model, thinkingLevelMap: { high: "HIGH" }, defaultThinkingLevel: "high" },
        { messages: [] },
        auth,
        { reasoning: "max" },
      ),
    );

    expect(body).toMatchObject({ generationConfig: { thinkingConfig: { includeThoughts: true, thinkingLevel: "HIGH" } } });
  });

  it("keeps Google HTTP failures and aborts distinguishable", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("quota", { status: 429 })));
    const failed = await collect(streamProvider(model, { messages: [] }, auth));
    expect(failed.at(-1)).toMatchObject({
      type: "error",
      reason: "error",
      error: { errorMessage: "Provider request failed (HTTP 429): quota" },
    });

    const controller = new AbortController();
    controller.abort();
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        expect(init?.signal?.aborted).toBe(true);
        throw new DOMException("Google request aborted", "AbortError");
      }),
    );
    const aborted = await collect(streamProvider(model, { messages: [] }, auth, { signal: controller.signal }));
    expect(aborted.at(-1)).toMatchObject({
      type: "error",
      reason: "aborted",
      error: { stopReason: "aborted", errorMessage: "Google request aborted" },
    });
  });

  it("does not turn a Google safety finish reason into a successful response", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => sse([{ candidates: [{ finishReason: "SAFETY", content: { parts: [] } }] }])),
    );

    const events = await collect(streamProvider(model, { messages: [] }, auth));

    expect(events.at(-1)).toMatchObject({
      type: "error",
      reason: "error",
      error: { stopReason: "error", rawStopReason: "SAFETY", errorMessage: "Provider stopped with: SAFETY" },
    });
  });
});
