import { describe, expect, it } from "vitest";

import type { KimiHarness, ProviderModel } from "@moonshot-ai/kimi-code-sdk";

import {
  deriveAlwaysThinking,
  deriveDefaultThinkingEffort,
  deriveSupportEfforts,
  deriveThinkingSupported,
  listModelsFromHarness,
} from "../src/model-catalog";

function model(overrides: Partial<ProviderModel> = {}): ProviderModel {
  return {
    id: "model-a",
    name: "Model A",
    api: "openai-completions",
    provider: "example",
    baseUrl: "https://api.example.test/v1",
    reasoning: false,
    input: ["text"],
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    contextWindow: 128_000,
    maxTokens: 32_000,
    ...overrides,
  };
}

describe("ACP runtime model projection", () => {
  it("derives thinking support and the declared effort levels", () => {
    const runtimeModel = model({
      reasoning: true,
      thinkingLevelMap: {
        off: null,
        low: "low",
        high: "high",
        max: "max",
      },
    });

    expect(deriveThinkingSupported(runtimeModel)).toBe(true);
    expect(deriveAlwaysThinking(runtimeModel)).toBe(true);
    expect(deriveSupportEfforts(runtimeModel)).toEqual(["low", "high", "max"]);
    expect(deriveDefaultThinkingEffort(runtimeModel)).toBe("high");
  });

  it("uses the standard efforts for a reasoning model without an explicit map", () => {
    const runtimeModel = model({ reasoning: true });

    expect(deriveSupportEfforts(runtimeModel)).toEqual(["low", "medium", "high"]);
    expect(deriveDefaultThinkingEffort(runtimeModel)).toBe("medium");
  });

  it("does not infer thinking from a model name", () => {
    const runtimeModel = model({ id: "custom-thinking-model", name: "Claude-like model" });

    expect(deriveThinkingSupported(runtimeModel)).toBe(false);
    expect(deriveAlwaysThinking(runtimeModel)).toBe(false);
    expect(deriveSupportEfforts(runtimeModel)).toEqual([]);
    expect(deriveDefaultThinkingEffort(runtimeModel)).toBe("on");
  });

  it("lists the live authenticated runtime catalog", async () => {
    const harness = {
      auth: {
        models: async () => [
          model({
            id: "model-b",
            name: "Model B",
            provider: "provider-b",
            reasoning: true,
            thinkingLevelMap: { low: "low", high: "high" },
          }),
        ],
      },
    } as unknown as KimiHarness;

    await expect(listModelsFromHarness(harness)).resolves.toEqual([
      {
        id: "provider-b/model-b",
        name: "Model B",
        thinkingSupported: true,
        alwaysThinking: false,
        supportEfforts: ["low", "high"],
        defaultThinkingEffort: "high",
      },
    ]);
  });
});
