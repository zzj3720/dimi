/**
 * Provider adapter registry contracts — the public API list, supported lookup,
 * dispatch and unknown-protocol rejection share one registry source.
 */
import { describe, expect, it } from "vitest";

import { createAdapterRegistry } from "#/app/providerRuntime/adapterRegistry";
import { providerApis, supportsProviderApi } from "#/app/providerRuntime/stream";
import type { AssistantMessageEvent, KnownApi, Model } from "#/app/providerRuntime/types";

const APIs = [
  "openai-completions",
  "mistral-conversations",
  "openai-responses",
  "azure-openai-responses",
  "openai-codex-responses",
  "anthropic-messages",
  "bedrock-converse-stream",
  "google-generative-ai",
  "google-vertex",
  "pi-messages",
] as const satisfies readonly KnownApi[];

function model(api: KnownApi): Model {
  return {
    id: "example",
    name: "Example",
    api,
    provider: "example",
    baseUrl: "https://example.test",
    reasoning: false,
    input: ["text"],
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    contextWindow: 1,
    maxTokens: 1,
  };
}

async function collect(stream: AsyncIterable<AssistantMessageEvent>): Promise<AssistantMessageEvent[]> {
  const events: AssistantMessageEvent[] = [];
  for await (const event of stream) events.push(event);
  return events;
}

describe("provider protocol adapter registry", () => {
  it("enumerates exactly the public APIs and recognizes each entry", () => {
    expect(providerApis()).toEqual(APIs);
    for (const api of APIs) expect(supportsProviderApi(api)).toBe(true);
    expect(supportsProviderApi("unknown-api")).toBe(false);
  });

  it("dispatches every registered API and rejects an unknown API", async () => {
    const adapter = async function* (): AsyncIterable<AssistantMessageEvent> {
      yield {
        type: "done",
        reason: "stop",
        message: {
          role: "assistant",
          content: [],
          api: "openai-completions",
          provider: "example",
          model: "example",
          usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
          stopReason: "stop",
          timestamp: 1,
        },
      };
    };
    const registry = createAdapterRegistry(Object.fromEntries(APIs.map((api) => [api, adapter])) as Record<KnownApi, typeof adapter>);
    for (const api of APIs) {
      await expect(collect(registry.stream(model(api), { messages: [] }, { auth: {} })!)).resolves.toHaveLength(1);
    }
    expect(registry.stream({ ...model("openai-completions"), api: "unknown-api" }, { messages: [] }, { auth: {} })).toBeUndefined();
  });
});
