/**
 * Shared conformance suite — the guarantee that the ipc and memory
 * transports are interchangeable. Every transport test file runs the exact
 * same assertions against a real in-process engine; only the `before` setup
 * differs per file.
 */

import { afterAll, beforeAll, describe, expect, it } from "vitest";

import type { Klient } from "../../src/index.js";

export interface KlientConformanceTarget {
  readonly klient: Klient;
  cleanup(): Promise<void>;
}

export function defineKlientConformance(
  transport: string,
  makeTarget: () => Promise<KlientConformanceTarget>,
): void {
  describe(`klient conformance: ${transport}`, () => {
    let target: KlientConformanceTarget;

    beforeAll(async () => {
      target = await makeTarget();
    });

    afterAll(async () => {
      await target.cleanup();
    });

    it("env() aggregates the host snapshot", async () => {
      const env = await target.klient.global.env();
      expect(env.platform).toBe(process.platform);
      expect(env.homeDir.length).toBeGreaterThan(0);
      expect(env.clientVersion.length).toBeGreaterThan(0);
    });

    it("workspaces round-trip through create/get/update/list/delete", async () => {
      const workspaces = target.klient.global.workspaces;
      const created = await workspaces.createOrTouch({ root: process.cwd(), name: "conformance" });
      expect(created.id.length).toBeGreaterThan(0);

      const fetched = await workspaces.get(created.id);
      expect(fetched?.name).toBe("conformance");

      const updated = await workspaces.update({ id: created.id, patch: { name: "conformance-2" } });
      expect(updated?.name).toBe("conformance-2");

      const list = await workspaces.list();
      expect(list.some((w) => w.id === created.id)).toBe(true);

      await workspaces.delete(created.id);
      expect(await workspaces.get(created.id)).toBeUndefined();
    });

    it("sessions index responds with a page shape", async () => {
      const page = await target.klient.global.sessions.list({});
      expect(Array.isArray(page.items)).toBe(true);
      const count = await target.klient.global.sessions.countActive(["no-such-workspace"]);
      expect(typeof count).toBe("number");
    });

    it("config reads respond", async () => {
      const all = await target.klient.global.config.getAll();
      expect(typeof all).toBe("object");
      expect(Array.isArray(await target.klient.global.config.diagnostics())).toBe(true);
    });

    it("hostFs.home() returns the host home and recent roots", async () => {
      const home = await target.klient.global.hostFs.home();
      expect(home.home.length).toBeGreaterThan(0);
      expect(Array.isArray(home.recent_roots)).toBe(true);

      const browse = await target.klient.global.hostFs.browse(home.home);
      expect(browse.path).toBe(home.home);
      expect(Array.isArray(browse.entries)).toBe(true);
    });

    it("catalog / flags / plugins read models respond", async () => {
      expect(Array.isArray(await target.klient.global.catalog.listModels())).toBe(true);
      expect(Array.isArray(await target.klient.global.catalog.listProviders())).toBe(true);
      expect(Array.isArray(await target.klient.global.flags.list())).toBe(true);
      expect(Array.isArray(await target.klient.global.flags.enabledIds())).toBe(true);
      expect(typeof (await target.klient.global.flags.snapshot())).toBe("object");
      expect(Array.isArray(await target.klient.global.plugins.list())).toBe(true);
    });
  });
}
