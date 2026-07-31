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

import { FileCredentialStore, FileModelsStore } from "#/app/providerRuntime/storage";
import type { Model, ModelsStoreEntry } from "#/app/providerRuntime/types";

const directories: string[] = [];

afterEach(async () => {
  vi.restoreAllMocks();
  await Promise.all(
    directories.splice(0).map((path) => rm(path, { recursive: true, force: true })),
  );
});

async function paths(): Promise<{ auth: string; models: string }> {
  const directory = await mkdtemp(join(tmpdir(), "provider-runtime-storage-"));
  directories.push(directory);
  return {
    auth: join(directory, "auth.json"),
    models: join(directory, "models-store.json"),
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
