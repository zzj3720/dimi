/**
 * Provider-runtime service transactions — persisted model layers and SDK
 * process providers must converge without leaving stale/ghost catalogs.
 */
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { envApiKeyAuth } from "#/app/providerRuntime/auth";
import { ProviderRuntimeService } from "#/app/providerRuntime/providerRuntimeService";
import type { Api, Provider } from "#/app/providerRuntime/types";

const homes: string[] = [];

afterEach(async () => {
  for (const home of homes.splice(0)) await rm(home, { recursive: true, force: true });
});

function processProvider(id: string, api: Api = "openai-completions"): Provider {
  return {
    id,
    name: id,
    baseUrl: "https://api.example.test/v1",
    auth: { apiKey: envApiKeyAuth(`${id} API key`, []) },
    getModels: () => [{
      id: `${id}-model`, name: `${id} model`, api, provider: id,
      baseUrl: "https://api.example.test/v1", reasoning: false, input: ["text"],
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      contextWindow: 128_000, maxTokens: 8_192,
    }],
    stream: async function* () {},
  };
}

async function service(source: string): Promise<{ home: string; runtime: ProviderRuntimeService }> {
  const home = await mkdtemp(join(tmpdir(), "provider-runtime-service-"));
  homes.push(home);
  await writeFile(join(home, "models.json"), source, "utf8");
  return {
    home,
    runtime: new ProviderRuntimeService({ homeDir: home } as never, { headers: {} } as never),
  };
}

const localDefinition = {
  id: "local",
  api: "openai-completions",
  baseUrl: "https://api.example.test/v1",
  models: [{ id: "local-model", contextWindow: 128_000, maxTokens: 8_192 }],
} as const;

describe("ProviderRuntimeService provider-layer transactions", () => {
  it("aggregates every invalid provider and leaves the last valid live layer in place", async () => {
    const { runtime } = await service(JSON.stringify({
      providers: {
        "bad-api": { api: "unimplemented", baseUrl: "https://api.example.test/v1", models: [{ id: "a", contextWindow: 1, maxTokens: 1 }] },
        "bad-limits": { api: "openai-completions", baseUrl: "https://api.example.test/v1", models: [{ id: "b" }] },
      },
    }));

    await runtime.ready;

    expect(runtime.getProviderDefinitionDiagnostic()).toContain("bad-api");
    expect(runtime.getProviderDefinitionDiagnostic()).toContain("bad-limits");
    expect(runtime.getProvider("xai")).toBeDefined();
  });

  it("validates the full locked candidate before a provider mutation writes models.json", async () => {
    const source = JSON.stringify({
      providers: {
        broken: { api: "unimplemented", baseUrl: "https://api.example.test/v1", models: [{ id: "broken", contextWindow: 1, maxTokens: 1 }] },
      },
    });
    const { home, runtime } = await service(source);
    await runtime.ready;

    await expect(runtime.upsertCustomProvider(localDefinition)).rejects.toThrow("unimplemented");
    await expect(readFile(join(home, "models.json"), "utf8")).resolves.toBe(source);

    await expect(runtime.upsertCustomProvider({ ...localDefinition, id: "broken", models: [{ id: "fixed", contextWindow: 1, maxTokens: 1 }] })).resolves.toBeUndefined();
    expect(runtime.getProviderDefinitionDiagnostic()).toBeUndefined();
    expect(runtime.getProvider("broken")?.getModels()).toEqual([
      expect.objectContaining({ id: "fixed", api: "openai-completions" }),
    ]);
  });

  it("rolls back an invalid SDK process-provider replacement without replacing live models", async () => {
    const { runtime } = await service(JSON.stringify({
      providers: { sdk: { compat: { supportsTemperature: false } } },
    }));
    const anthropic = processProvider("sdk", "anthropic-messages");
    runtime.setProvider(anthropic);
    await runtime.ready;
    expect(runtime.getModel("sdk", "sdk-model")?.api).toBe("anthropic-messages");

    expect(() => runtime.setProvider(processProvider("sdk", "openai-completions"))).toThrow("unsupported for openai-completions");
    expect(runtime.getModel("sdk", "sdk-model")?.api).toBe("anthropic-messages");
  });

  it("removes sparse SDK overlays instead of leaving ghost models after delete and clear", async () => {
    const first = await service(JSON.stringify({ providers: { sdk: { headers: { "x-sdk": "present" } } } }));
    first.runtime.setProvider(processProvider("sdk"));
    await first.runtime.ready;
    first.runtime.deleteProvider("sdk");
    expect(first.runtime.getProvider("sdk")).toBeUndefined();
    expect(first.runtime.getProviderDefinitionDiagnostic()).toContain("requires models");

    const second = await service(JSON.stringify({ providers: { sdk: { headers: { "x-sdk": "present" } } } }));
    second.runtime.setProvider(processProvider("sdk"));
    await second.runtime.ready;
    second.runtime.clearProviders();
    expect(second.runtime.getProvider("sdk")).toBeUndefined();
    expect(second.runtime.getProviderDefinitionDiagnostic()).toContain("requires models");
  });
});
