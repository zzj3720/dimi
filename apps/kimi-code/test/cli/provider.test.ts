import { describe, expect, it, vi } from "vitest";

import {
  handleProviderList,
  handleProviderModels,
  handleProviderRefresh,
  type ProviderDeps,
} from "#/cli/sub/provider";

function output() {
  let value = "";
  return {
    write: vi.fn((chunk: string) => {
      value += chunk;
      return true;
    }),
    value: () => value,
  };
}

function deps(): ProviderDeps {
  const stdout = output();
  const stderr = output();
  return {
    stdout,
    stderr,
    exit: vi.fn() as never,
    close: async () => {},
    getHarness: () =>
      ({
        auth: {
          providers: vi.fn(async () => [
            {
              id: "openai-codex",
              name: "OpenAI Codex",
              configured: true,
              credentialType: "oauth",
              methods: [],
            },
          ]),
          models: vi.fn(async () => [
            {
              provider: "openai-codex",
              id: "gpt-5",
              name: "GPT-5",
              api: "openai-codex-responses",
              baseUrl: "https://example.test",
              reasoning: true,
              input: ["text"],
              cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
              contextWindow: 128_000,
              maxTokens: 16_000,
            },
          ]),
          refreshModels: vi.fn(async () => ({ aborted: false, errors: new Map() })),
        },
      }) as never,
  };
}

describe("provider CLI", () => {
  it("lists provider connection state and its live model count", async () => {
    const target = deps();
    await handleProviderList(target, { json: false });
    expect((target.stdout as ReturnType<typeof output>).value()).toContain(
      "openai-codex\toauth\tmodels=1",
    );
  });

  it("lists canonical provider/model references", async () => {
    const target = deps();
    await handleProviderModels(target, "openai-codex");
    expect((target.stdout as ReturnType<typeof output>).value()).toContain("openai-codex/gpt-5");
  });

  it("refreshes dynamic catalogs through the auth facade", async () => {
    const target = deps();
    await handleProviderRefresh(target);
    expect((target.stdout as ReturnType<typeof output>).value()).toContain("catalogs refreshed");
  });
});
