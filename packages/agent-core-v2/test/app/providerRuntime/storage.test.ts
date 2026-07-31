/**
 * Provider runtime file-storage contracts — provider isolation, atomic updates,
 * cross-instance locking, malformed-document preservation, and lock recovery.
 * Uses temporary local files; run with `vp test -- storage.test.ts`.
 */
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import lockfile from "proper-lockfile";
import { afterEach, describe, expect, it, vi } from "vitest";

import { FileCredentialStore, FileCustomProvidersStore, FileModelsStore } from "#/app/providerRuntime/storage";
import { ProviderRuntimeErrors } from "#/app/providerRuntime/errors";
import type { CustomProviderDefinition, Model, ModelsStoreEntry } from "#/app/providerRuntime/types";

const directories: string[] = [];

afterEach(async () => {
  vi.restoreAllMocks();
  await Promise.all(
    directories.splice(0).map((path) => rm(path, { recursive: true, force: true })),
  );
});

async function paths(): Promise<{ auth: string; models: string; customProviders: string }> {
  const directory = await mkdtemp(join(tmpdir(), "provider-runtime-storage-"));
  directories.push(directory);
  return {
    auth: join(directory, "auth.json"),
    models: join(directory, "models-store.json"),
    customProviders: join(directory, "models.json"),
  };
}

function customProvider(id: string): CustomProviderDefinition {
  return {
    id,
    name: id,
    api: "openai-completions",
    baseUrl: "https://api.example.test/v1",
    headers: { "x-example": "one" },
    compat: { responses: false },
    models: [{
      id: `${id}-model`,
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 200_000,
      maxTokens: 10_000,
      cost: { input: 1, output: 2, cacheRead: 0.1, cacheWrite: 0.2 },
    }],
  };
}

function model(provider: string, id: string): Model {
  return {
    id,
    name: id,
    api: "openai-completions",
    provider,
    baseUrl: "https://api.example.test/v1",
    reasoning: false,
    input: ["text"],
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    contextWindow: 128_000,
    maxTokens: 32_000,
  };
}

function catalog(provider: string, id: string): ModelsStoreEntry {
  return { models: [model(provider, id)], checkedAt: 1 };
}

describe("provider runtime file storage", () => {
  it("keeps credential providers isolated when one is deleted", async () => {
    const { auth } = await paths();
    const store = new FileCredentialStore(auth);

    await store.modify("first", async () => ({ type: "api_key", key: "first-key" }));
    await store.modify("second", async () => ({
      type: "oauth",
      access: "second-access",
      refresh: "second-refresh",
      expires: 10,
    }));
    await store.delete("first");

    const reloaded = new FileCredentialStore(auth);
    await expect(reloaded.read("first")).resolves.toBeUndefined();
    await expect(reloaded.read("second")).resolves.toEqual({
      type: "oauth",
      access: "second-access",
      refresh: "second-refresh",
      expires: 10,
    });
    await expect(reloaded.list()).resolves.toEqual([{ providerId: "second", type: "oauth" }]);
  });

  it("keeps model catalogs isolated when one is deleted", async () => {
    const { models } = await paths();
    const store = new FileModelsStore(models);

    await store.write("first", catalog("first", "first-model"));
    await store.write("second", catalog("second", "second-model"));
    await store.delete("first");

    const reloaded = new FileModelsStore(models);
    await expect(reloaded.read("first")).resolves.toBeUndefined();
    await expect(reloaded.read("second")).resolves.toEqual(catalog("second", "second-model"));
  });

  it("persists full custom provider definitions in the Pi-compatible models.json root", async () => {
    const { customProviders } = await paths();
    const store = new FileCustomProvidersStore(customProviders);
    await store.set(customProvider("example"));

    const source = await readFile(customProviders, "utf8");
    expect(source).toContain('"providers"');
    expect(source).not.toContain('"version"');
    expect(source).not.toContain('"id": "example"');
    await expect(new FileCustomProvidersStore(customProviders).list()).resolves.toEqual([
      customProvider("example"),
    ]);
  });

  it("round-trips a sparse built-in overlay without adding a second id or schema wrapper", async () => {
    const { customProviders } = await paths();
    const store = new FileCustomProvidersStore(customProviders);
    await store.set({
      id: "xai",
      baseUrl: "https://gateway.example.test/v1",
      modelOverrides: { "grok-4.5": { maxTokens: 16_384 } },
    });

    await expect(store.list()).resolves.toEqual([{
      id: "xai",
      baseUrl: "https://gateway.example.test/v1",
      modelOverrides: { "grok-4.5": { maxTokens: 16_384 } },
    }]);
    await expect(readFile(customProviders, "utf8")).resolves.not.toContain('"id": "xai"');
  });

  it("accepts Pi-compatible JSONC provider files", async () => {
    const { customProviders } = await paths();
    await writeFile(customProviders, `{
      /* gateway override */
      "providers": {
        "xai": {
          "baseUrl": "https://gateway.example.test/v1//not-a-comment",
        },
      },
    }`, "utf8");

    await expect(new FileCustomProvidersStore(customProviders).list()).resolves.toEqual([{
      id: "xai",
      baseUrl: "https://gateway.example.test/v1//not-a-comment",
    }]);
  });

  it("returns a recoverable diagnostic without rewriting damaged models.json", async () => {
    const { customProviders } = await paths();
    const malformed = '{ "providers": ';
    await writeFile(customProviders, malformed, "utf8");

    const loaded = await new FileCustomProvidersStore(customProviders).load();
    expect(loaded.providers).toEqual([]);
    expect(loaded.error?.message).toMatch(/Unexpected end|Expected/u);
    await expect(readFile(customProviders, "utf8")).resolves.toBe(malformed);
  });

  it("rejects and preserves a damaged custom provider document", async () => {
    const { customProviders } = await paths();
    const malformed = '{"providers":[]}';
    await writeFile(customProviders, malformed, "utf8");

    await expect(new FileCustomProvidersStore(customProviders).list()).rejects.toMatchObject({
      code: ProviderRuntimeErrors.codes.PROVIDER_DEFINITION_STORE_INVALID,
    });
    await expect(readFile(customProviders, "utf8")).resolves.toBe(malformed);
  });

  it("commits a credential modification against the latest value without replacing other providers", async () => {
    const { auth } = await paths();
    const store = new FileCredentialStore(auth);
    await store.modify("first", async () => ({ type: "api_key", key: "first-key" }));
    await store.modify("second", async () => ({ type: "api_key", key: "second-key" }));

    await expect(
      store.modify("first", async (current) => {
        expect(current).toEqual({ type: "api_key", key: "first-key" });
        return undefined;
      }),
    ).resolves.toEqual({ type: "api_key", key: "first-key" });

    await expect(store.read("second")).resolves.toEqual({ type: "api_key", key: "second-key" });
  });

  it("serializes concurrent credential modifications from one store instance", async () => {
    const { auth } = await paths();
    const store = new FileCredentialStore(auth);
    let releaseFirst!: () => void;
    const firstMayFinish = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    let markFirstStarted!: () => void;
    const firstStarted = new Promise<void>((resolve) => {
      markFirstStarted = resolve;
    });

    const first = store.modify("provider", async () => {
      markFirstStarted();
      await firstMayFinish;
      return { type: "api_key", key: "first-key" };
    });
    await firstStarted;
    const second = store.modify("provider", async (current) => ({
      type: "api_key",
      key: `${current?.type === "api_key" ? current.key : "missing"}-second`,
    }));
    releaseFirst();

    await Promise.all([first, second]);
    await expect(store.read("provider")).resolves.toEqual({
      type: "api_key",
      key: "first-key-second",
    });
  });

  it("serializes concurrent credential modifications across store instances", async () => {
    const { auth } = await paths();
    const first = new FileCredentialStore(auth);
    const second = new FileCredentialStore(auth);
    let releaseFirst!: () => void;
    const firstMayFinish = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    let markFirstStarted!: () => void;
    const firstStarted = new Promise<void>((resolve) => {
      markFirstStarted = resolve;
    });

    const writeFirst = first.modify("provider", async () => {
      markFirstStarted();
      await firstMayFinish;
      return { type: "api_key", key: "first-key" };
    });
    await firstStarted;
    const writeSecond = second.modify("provider", async (current) => ({
      type: "api_key",
      key: `${current?.type === "api_key" ? current.key : "missing"}-second`,
    }));
    releaseFirst();
    await Promise.all([writeFirst, writeSecond]);

    await expect(new FileCredentialStore(auth).read("provider")).resolves.toEqual({
      type: "api_key",
      key: "first-key-second",
    });
  });

  it("serializes concurrent model-catalog writes across store instances", async () => {
    const { models } = await paths();
    const first = new FileModelsStore(models);
    const second = new FileModelsStore(models);

    await Promise.all([
      first.write("first", catalog("first", "first-model")),
      second.write("second", catalog("second", "second-model")),
    ]);

    const reloaded = new FileModelsStore(models);
    await expect(reloaded.read("first")).resolves.toEqual(catalog("first", "first-model"));
    await expect(reloaded.read("second")).resolves.toEqual(catalog("second", "second-model"));
  });

  it("does not overwrite a malformed credential document", async () => {
    const { auth } = await paths();
    const malformed = "{ this is not json";
    await writeFile(auth, malformed, "utf8");
    const store = new FileCredentialStore(auth);

    await expect(
      store.modify("provider", async () => ({ type: "api_key", key: "new-key" })),
    ).rejects.toThrow();

    await expect(readFile(auth, "utf8")).resolves.toBe(malformed);
  });

  it("does not overwrite a malformed model catalog document", async () => {
    const { models } = await paths();
    const malformed = "{ this is not json";
    await writeFile(models, malformed, "utf8");
    const store = new FileModelsStore(models);

    await expect(store.write("provider", catalog("provider", "new-model"))).rejects.toThrow();

    await expect(readFile(models, "utf8")).resolves.toBe(malformed);
  });

  it("recovers after one credential lock-acquisition failure", async () => {
    const { auth } = await paths();
    const store = new FileCredentialStore(auth);
    await store.read("bootstrap");
    const lock = vi.spyOn(lockfile, "lock").mockRejectedValueOnce(new Error("lock unavailable"));

    await expect(
      store.modify("provider", async () => ({ type: "api_key", key: "blocked-key" })),
    ).rejects.toThrow("lock unavailable");
    lock.mockRestore();

    await store.modify("provider", async () => ({ type: "api_key", key: "recovered-key" }));

    await expect(store.read("provider")).resolves.toEqual({
      type: "api_key",
      key: "recovered-key",
    });
  });
});
