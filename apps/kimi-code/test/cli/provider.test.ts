import { describe, expect, it, vi } from "vitest";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  handleProviderList,
  handleProviderModels,
  handleProviderRefresh,
  handleProviderUpsert,
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
          providerDefinitionDiagnostic: vi.fn(async () => undefined),
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

  it("imports the complete custom-provider structure through --from", async () => {
    const directory = await mkdtemp(join(tmpdir(), "kimi-provider-cli-"));
    const definitionPath = join(directory, "models.json");
    const definition = {
      providers: {
        local: {
          name: "Local protocol stub",
          api: "openai-completions",
          baseUrl: "https://api.example.test/v1",
          apiKey: "$LOCAL_KEY",
          headers: { "x-client": "kimi" },
          modelOverrides: { dynamic: { contextWindow: 32_000, maxTokens: 4_096 } },
          models: [{
            id: "local-chat",
            reasoning: true,
            input: ["text", "image"],
            contextWindow: 128_000,
            maxTokens: 16_000,
            cost: { input: 1, output: 2, cacheRead: 0, cacheWrite: 0 },
          }],
        },
      },
    };
    await writeFile(
      definitionPath,
      `/* Pi-compatible JSONC import */\n${JSON.stringify(definition, null, 2).replace(
        '"baseUrl": "https://api.example.test/v1",',
        '"baseUrl": "https://api.example.test/v1", // endpoint',
      )}`,
      "utf8",
    );
    const stdout = output();
    const upsertCustomProvider = vi.fn(async () => {});
    try {
      await handleProviderUpsert(
        {
          ...deps(),
          stdout,
          getHarness: () => ({
            auth: {
              customProviders: vi.fn(async () => []),
              upsertCustomProvider,
            },
          }) as never,
        },
        "local",
        { from: definitionPath },
      );
      expect(upsertCustomProvider).toHaveBeenCalledWith({ ...definition.providers.local, id: "local" });
      expect(stdout.value()).toContain("Added provider local");
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });
});
