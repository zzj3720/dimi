import { describe, expect, it, vi } from 'vitest';

import { refreshUpdateCache } from '#/cli/update/refresh';
import type { UpdateManifest } from '#/cli/update/types';

const MANIFEST: UpdateManifest = {
  version: '0.5.0',
  publishedAt: '2026-05-20T12:00:00.000Z',
  rollout: [
    { percent: 30, delaySeconds: 0 },
    { percent: 30, delaySeconds: 43_200 },
    { percent: 40, delaySeconds: 86_400 },
  ],
};

describe('refreshUpdateCache', () => {
  it('fetches from the configured channel by default', async () => {
    const fetchImpl = vi.fn(async () => ({
      ok: true,
      status: 200,
      text: async () =>
        JSON.stringify({
          version: '0.5.0',
          publishedAt: '2026-08-01T00:00:00.000Z',
          rollout: [],
        }),
    }));
    vi.stubGlobal('fetch', fetchImpl);
    const writeCache = vi.fn(async () => {});

    await expect(
      refreshUpdateCache({ writeCache, now: () => new Date('2026-08-01T12:00:00.000Z') }),
    ).resolves.toEqual({
      source: 'cdn',
      checkedAt: '2026-08-01T12:00:00.000Z',
      latest: '0.5.0',
      manifest: {
        version: '0.5.0',
        publishedAt: '2026-08-01T00:00:00.000Z',
        rollout: [],
      },
    });
    expect(writeCache).toHaveBeenCalledTimes(1);
    expect(fetchImpl).toHaveBeenCalledWith(
      'https://github.com/zzj3720/dimi/releases/latest/download/latest.json',
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
    vi.unstubAllGlobals();
  });

  it('writes a fresh cache carrying the manifest on successful fetch', async () => {
    const writeCache = vi.fn(async () => {});
    const result = await refreshUpdateCache({
      fetchLatest: async () => ({ latest: '0.5.0', manifest: MANIFEST }),
      writeCache,
      now: () => new Date('2026-05-20T12:34:56.000Z'),
    });

    expect(result).toEqual({
      source: 'cdn',
      checkedAt: '2026-05-20T12:34:56.000Z',
      latest: '0.5.0',
      manifest: MANIFEST,
    });
    expect(writeCache).toHaveBeenCalledWith(result);
  });

  it('writes a null manifest when the fetch fell back to plain text', async () => {
    const writeCache = vi.fn(async () => {});
    const result = await refreshUpdateCache({
      fetchLatest: async () => ({ latest: '0.5.0', manifest: null }),
      writeCache,
      now: () => new Date('2026-05-20T12:34:56.000Z'),
    });

    expect(result).toEqual({
      source: 'cdn',
      checkedAt: '2026-05-20T12:34:56.000Z',
      latest: '0.5.0',
      manifest: null,
    });
    expect(writeCache).toHaveBeenCalledWith(result);
  });

  it('propagates fetch errors and skips writeCache so the cache is preserved', async () => {
    const writeCache = vi.fn(async () => {});
    await expect(
      refreshUpdateCache({
        fetchLatest: async () => {
          throw new Error('network down');
        },
        writeCache,
        now: () => new Date(),
      }),
    ).rejects.toThrow(/network down/);

    expect(writeCache).not.toHaveBeenCalled();
  });
});
