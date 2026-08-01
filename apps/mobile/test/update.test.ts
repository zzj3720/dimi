import { describe, expect, it, vi } from "vitest";

import { UpdateController, type UpdateManifest, type UpdatePlatform } from "../src/update";

const manifest: UpdateManifest = {
  schema: 1,
  channel: "internal",
  platform: "android",
  version: "0.2.0",
  versionCode: 2,
  minVersionCode: 1,
  publishedAt: "2026-07-31T00:00:00.000Z",
  commit: "0123456789abcdef",
  apk: {
    url: "https://install.k.test.3720.org/android/builds/2/k-3720-internal.apk",
    sha256: "a".repeat(64),
    size: 100,
  },
};

describe("Android update controller", () => {
  it("downloads a newer compatible build and hands it to the installer", async () => {
    const platform = fakePlatform();
    const controller = new UpdateController(platform);

    await controller.check();
    expect(controller.state).toEqual({ phase: "ready", manifest, artifact: "file:///update.apk" });
    await controller.install();
    expect(platform.install).toHaveBeenCalledWith("file:///update.apk");
  });

  it("stays hidden when the installed build is current", async () => {
    const controller = new UpdateController(fakePlatform({ currentVersionCode: () => 2 }));
    await controller.check();
    expect(controller.state).toEqual({ phase: "idle" });
  });

  it("surfaces verification failures and retries through the same path", async () => {
    const platform = fakePlatform({
      downloadAndVerify: vi
        .fn()
        .mockRejectedValueOnce(new Error("Downloaded update failed verification."))
        .mockResolvedValueOnce("file:///update.apk"),
    });
    const controller = new UpdateController(platform);

    await controller.check();
    expect(controller.state).toEqual({
      phase: "error",
      message: "Downloaded update failed verification.",
    });
    await controller.check();
    expect(controller.state.phase).toBe("ready");
  });
});

function fakePlatform(overrides: Partial<UpdatePlatform> = {}): UpdatePlatform {
  return {
    currentVersionCode: () => 1,
    fetchManifest: vi.fn().mockResolvedValue(manifest),
    downloadAndVerify: vi.fn().mockResolvedValue("file:///update.apk"),
    install: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}
