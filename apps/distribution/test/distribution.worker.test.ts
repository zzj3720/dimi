import { env, SELF } from "cloudflare:test";
import { beforeEach, describe, expect, it } from "vitest";

const manifest = {
  schema: 1,
  channel: "internal",
  platform: "android",
  version: "0.1.0",
  versionCode: 2,
  minVersionCode: 1,
  publishedAt: "2026-07-31T00:00:00.000Z",
  commit: "0123456789abcdef",
  apk: {
    url: "https://install.k.test.3720.org/android/builds/2/k-3720-internal.apk",
    sha256: "a".repeat(64),
    size: 3,
  },
};

beforeEach(async () => {
  await env.BUILDS.put("android/internal/latest.json", JSON.stringify(manifest));
  await env.BUILDS.put("android/builds/2/k-3720-internal.apk", new Uint8Array([1, 2, 3]), {
    httpMetadata: { contentType: "application/vnd.android.package-archive" },
  });
});

describe("Android distribution", () => {
  it("serves a no-store update manifest", async () => {
    const response = await SELF.fetch("https://install.k.test.3720.org/android/internal/latest.json");
    expect(response.status).toBe(200);
    expect(response.headers.get("cache-control")).toBe("no-store");
    expect(response.headers.get("access-control-allow-origin")).toBe("*");
    await expect(response.json()).resolves.toEqual(manifest);
  });

  it("streams immutable APKs with Android metadata", async () => {
    const response = await SELF.fetch(manifest.apk.url);
    expect(response.status).toBe(200);
    expect(response.headers.get("cache-control")).toContain("immutable");
    expect(response.headers.get("content-type")).toBe(
      "application/vnd.android.package-archive",
    );
    expect(new Uint8Array(await response.arrayBuffer())).toEqual(new Uint8Array([1, 2, 3]));
  });

  it("serves the install page from the manifest host", async () => {
    const response = await SELF.fetch("https://install.k.test.3720.org/");
    expect(response.status).toBe(200);
    expect(response.headers.get("content-type")).toContain("text/html");
  });
});
