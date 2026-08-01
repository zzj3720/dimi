/**
 * Scenario: release metadata is fetched only from an explicit authority.
 * Responsibilities: parse/validate manifests, fall back to plain latest,
 * preserve timeout behavior, and fail closed when this build has no channel.
 * Wiring: real fetch helpers; fetch is the single external boundary.
 */
import { describe, expect, it, vi } from 'vitest';

import { fetchLatestFromCdn, fetchLatestVersionFromCdn, hasUpdateChannel } from '#/cli/update/cdn';

const UPDATE_CHANNEL = 'https://updates.example.test/dimi/';
const LATEST_URL = new URL('latest', UPDATE_CHANNEL).toString();
const LATEST_JSON_URL = new URL('latest.json', UPDATE_CHANNEL).toString();

function mockFetchOk(body: string): typeof fetch {
  return vi.fn(async () => ({
    ok: true,
    status: 200,
    text: async () => body,
  })) as unknown as typeof fetch;
}

function mockFetchStatus(status: number): typeof fetch {
  return vi.fn(async () => ({
    ok: status >= 200 && status < 300,
    status,
    text: async () => '',
  })) as unknown as typeof fetch;
}

type Route = { readonly status?: number; readonly body?: string } | Error;

function mockRoutedFetch(routes: Record<string, Route>): typeof fetch {
  return vi.fn(async (input: string | URL) => {
    const route = routes[String(input)];
    if (route === undefined) return { ok: false, status: 404, text: async () => '' };
    if (route instanceof Error) throw route;
    const status = route.status ?? 200;
    return { ok: status >= 200 && status < 300, status, text: async () => route.body ?? '' };
  }) as unknown as typeof fetch;
}

const MANIFEST_BODY = JSON.stringify({
  schemaVersion: 1,
  version: '2.0.0',
  publishedAt: '2026-06-12T00:00:00.000Z',
  rollout: [
    { percent: 30, delaySeconds: 0 },
    { percent: 30, delaySeconds: 43_200 },
    { percent: 40, delaySeconds: 86_400 },
  ],
});

describe('update channel', () => {
  it('points at the Dimi GitHub release manifest', () => {
    expect(hasUpdateChannel()).toBe(true);
  });

  it('fetches the Dimi channel when no explicit authority is passed', async () => {
    const fetchImpl = mockFetchOk(
      JSON.stringify({
        version: '0.5.0',
        publishedAt: '2026-08-01T00:00:00.000Z',
        rollout: [],
      }),
    );

    await expect(fetchLatestFromCdn(fetchImpl)).resolves.toEqual({
      latest: '0.5.0',
      manifest: {
        version: '0.5.0',
        publishedAt: '2026-08-01T00:00:00.000Z',
        rollout: [],
      },
    });
    expect(fetchImpl).toHaveBeenCalledWith(
      'https://github.com/zzj3720/dimi/releases/latest/download/latest.json',
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
  });
});

describe('fetchLatestVersionFromCdn', () => {
  it('returns the trimmed semver from an explicit authority', async () => {
    const fetchImpl = mockFetchOk('  0.5.0\n');
    await expect(fetchLatestVersionFromCdn(fetchImpl, UPDATE_CHANNEL)).resolves.toBe('0.5.0');
    expect(fetchImpl).toHaveBeenCalledWith(LATEST_URL, expect.objectContaining({ signal: expect.any(AbortSignal) }));
  });

  it('throws when an authority response is non-2xx', async () => {
    await expect(fetchLatestVersionFromCdn(mockFetchStatus(404), UPDATE_CHANNEL)).rejects.toThrow(/HTTP 404/);
  });

  it('throws when an authority returns invalid semver', async () => {
    await expect(fetchLatestVersionFromCdn(mockFetchOk('not-a-version'), UPDATE_CHANNEL)).rejects.toThrow(/invalid semver/);
  });

  it('throws when an authority returns an empty body', async () => {
    await expect(fetchLatestVersionFromCdn(mockFetchOk('   '), UPDATE_CHANNEL)).rejects.toThrow(/invalid semver/);
  });

  it('propagates an authority network error', async () => {
    const fetchImpl = vi.fn(async () => { throw new Error('network down'); }) as unknown as typeof fetch;
    await expect(fetchLatestVersionFromCdn(fetchImpl, UPDATE_CHANNEL)).rejects.toThrow(/network down/);
  });
});

describe('fetchLatestFromCdn', () => {
  it('parses a manifest from an explicit authority', async () => {
    const fetchImpl = mockRoutedFetch({ [LATEST_JSON_URL]: { body: MANIFEST_BODY } });
    await expect(fetchLatestFromCdn(fetchImpl, UPDATE_CHANNEL)).resolves.toEqual({
      latest: '2.0.0',
      manifest: {
        version: '2.0.0',
        publishedAt: '2026-06-12T00:00:00.000Z',
        rollout: [
          { percent: 30, delaySeconds: 0 },
          { percent: 30, delaySeconds: 43_200 },
          { percent: 40, delaySeconds: 86_400 },
        ],
      },
    });
    expect(fetchImpl).toHaveBeenCalledWith(LATEST_JSON_URL, expect.objectContaining({ signal: expect.any(AbortSignal) }));
    expect(fetchImpl).toHaveBeenCalledTimes(1);
  });

  it('ignores unknown manifest fields', async () => {
    const body = JSON.stringify({ schemaVersion: 99, version: '2.0.0', publishedAt: '2026-06-12T00:00:00.000Z', rollout: [], futureField: { nested: true } });
    const result = await fetchLatestFromCdn(mockRoutedFetch({ [LATEST_JSON_URL]: { body } }), UPDATE_CHANNEL);
    expect(result.manifest).toEqual({ version: '2.0.0', publishedAt: '2026-06-12T00:00:00.000Z', rollout: [] });
  });

  it('defaults a missing rollout to a fully rolled out manifest', async () => {
    const body = JSON.stringify({ version: '2.0.0', publishedAt: '2026-06-12T00:00:00.000Z' });
    const result = await fetchLatestFromCdn(mockRoutedFetch({ [LATEST_JSON_URL]: { body } }), UPDATE_CHANNEL);
    expect(result.manifest?.rollout).toEqual([]);
  });

  const fallbackCases: ReadonlyArray<readonly [string, Route]> = [
    ['latest.json is missing (HTTP 404)', { status: 404 }],
    ['latest.json fetch throws', new Error('network down')],
    ['body is not valid JSON', { body: 'not json {' }],
    ['version is not semver', { body: JSON.stringify({ version: 'nope', publishedAt: '2026-06-12T00:00:00.000Z' }) }],
    ['publishedAt is unparseable', { body: JSON.stringify({ version: '2.0.0', publishedAt: 'garbage' }) }],
    ['a batch percent is out of range', { body: JSON.stringify({ version: '2.0.0', publishedAt: '2026-06-12T00:00:00.000Z', rollout: [{ percent: 150, delaySeconds: 0 }] }) }],
    ['a batch delay is negative', { body: JSON.stringify({ version: '2.0.0', publishedAt: '2026-06-12T00:00:00.000Z', rollout: [{ percent: 100, delaySeconds: -1 }] }) }],
  ];

  for (const [name, route] of fallbackCases) {
    it(`falls back to plain latest when ${name}`, async () => {
      const fetchImpl = mockRoutedFetch({ [LATEST_JSON_URL]: route, [LATEST_URL]: { body: '1.9.0\n' } });
      await expect(fetchLatestFromCdn(fetchImpl, UPDATE_CHANNEL)).resolves.toEqual({ latest: '1.9.0', manifest: null });
    });
  }

  it('throws when both manifest and plain latest fail', async () => {
    const fetchImpl = mockRoutedFetch({ [LATEST_JSON_URL]: { status: 500 }, [LATEST_URL]: { status: 500 } });
    await expect(fetchLatestFromCdn(fetchImpl, UPDATE_CHANNEL)).rejects.toThrow(/HTTP 500/);
  });

  it('propagates the plain latest error when fallback fails', async () => {
    const fetchImpl = mockRoutedFetch({ [LATEST_JSON_URL]: new Error('json down'), [LATEST_URL]: { body: 'not-a-version' } });
    await expect(fetchLatestFromCdn(fetchImpl, UPDATE_CHANNEL)).rejects.toThrow(/invalid semver/);
  });

  it('falls back when the manifest request times out', async () => {
    vi.useFakeTimers();
    try {
      const fetchImpl = vi.fn(async (input: string | URL, init?: RequestInit) => {
        if (String(input) === LATEST_JSON_URL) {
          return new Promise<Response>((_resolve, reject) => init?.signal?.addEventListener('abort', () => reject(new Error('aborted')), { once: true }));
        }
        if (String(input) === LATEST_URL) return { ok: true, status: 200, text: async () => '1.9.0\n' };
        return { ok: false, status: 404, text: async () => '' };
      }) as unknown as typeof fetch;

      const result = fetchLatestFromCdn(fetchImpl, UPDATE_CHANNEL);
      await vi.advanceTimersByTimeAsync(3_000);
      await expect(result).resolves.toEqual({ latest: '1.9.0', manifest: null });
    } finally {
      vi.useRealTimers();
    }
  });

  it('rejects when plain latest also times out', async () => {
    vi.useFakeTimers();
    try {
      const fetchImpl = vi.fn(async (_input: string | URL, init?: RequestInit) =>
        new Promise<Response>((_resolve, reject) => init?.signal?.addEventListener('abort', () => reject(new Error('aborted')), { once: true })),
      ) as unknown as typeof fetch;

      const result = fetchLatestFromCdn(fetchImpl, UPDATE_CHANNEL);
      const expectation = expect(result).rejects.toThrow(/aborted/);
      await vi.advanceTimersByTimeAsync(6_000);
      await expectation;
    } finally {
      vi.useRealTimers();
    }
  });
});
