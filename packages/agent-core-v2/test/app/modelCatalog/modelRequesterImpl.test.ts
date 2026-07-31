/**
 * `modelCatalog` requester boundary — per-turn transport intent and typed
 * provider failures. Uses a runtime boundary stub and the public requester API.
 */
import { describe, expect, it } from "vitest";

import { unwrapErrorCause } from "#/_base/errors/errors";
import { ModelRequesterImpl } from "#/app/modelCatalog/modelRequesterImpl";
import { APIRequestTooLargeError } from "#/llmProtocol/errors";
import type { IProviderRuntime } from "#/app/providerRuntime/providerRuntime";
import type { Model } from "#/app/providerRuntime/types";

const model: Model = {
  id: "example",
  name: "Example",
  api: "openai-completions",
  provider: "example",
  baseUrl: "https://example.test/v1",
  reasoning: false,
  input: ["text"],
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  contextWindow: 100,
  maxTokens: 80,
};

async function drain<T>(stream: AsyncIterable<T>): Promise<T[]> {
  const values: T[] = [];
  for await (const value of stream) values.push(value);
  return values;
}

describe("ModelRequesterImpl request boundary", () => {
  it("clamps the output budget and preserves all per-turn wire intent", async () => {
    let options: Parameters<IProviderRuntime["streamSimple"]>[2] | undefined;
    const runtime = {
      streamSimple: async function* (_model, _context, received) {
        options = received;
        yield {
          type: "done" as const,
          reason: "stop" as const,
          message: {
            role: "assistant" as const,
            content: [],
            api: model.api,
            provider: model.provider,
            model: model.id,
            usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
            stopReason: "stop" as const,
            timestamp: 1,
          },
        };
      },
    } satisfies Pick<IProviderRuntime, "streamSimple">;
    const requester = new ModelRequesterImpl(model, runtime);
    await drain(
      requester.request(
        { systemPrompt: "system", tools: [], messages: [], responseFormat: { type: "json_object" } },
        undefined,
        {
          cacheKey: "cache",
          sampling: { temperature: 0.1, topP: 0.2 },
          thinkingEffort: "high",
          thinkingKeep: "all",
          maxCompletionTokens: 50,
          usedContextTokens: 90,
          maxContextTokens: 100,
        },
      ),
    );
    expect(options).toMatchObject({
      temperature: 0.1,
      topP: 0.2,
      cacheKey: "cache",
      thinkingKeep: "all",
      maxTokens: 10,
      responseFormat: { type: "json_object" },
    });
  });

  it("keeps a 413 provider cause recognizable by requester recovery", async () => {
    const raw = new APIRequestTooLargeError(413, "too large");
    const runtime = {
      streamSimple: async function* () {
        yield {
          type: "error" as const,
          reason: "error" as const,
          error: {
            role: "assistant" as const,
            content: [],
            api: model.api,
            provider: model.provider,
            model: model.id,
            usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
            stopReason: "error" as const,
            errorMessage: raw.message,
            timestamp: 1,
          },
          cause: raw,
        };
      },
    } satisfies Pick<IProviderRuntime, "streamSimple">;
    const requester = new ModelRequesterImpl(model, runtime);
    await expect(drain(requester.request({ systemPrompt: "system", tools: [], messages: [] }))).rejects.toSatisfy(
      (error: unknown) => unwrapErrorCause(error) === raw,
    );
  });
});
