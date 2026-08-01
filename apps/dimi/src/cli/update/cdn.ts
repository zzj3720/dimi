import { valid } from 'semver';
import { z } from 'zod';

import { DIMI_CODE_UPDATE_CHANNEL_URL } from '#/constant/app';

import type { UpdateManifest } from './types';

const CDN_FETCH_TIMEOUT_MS = 3_000;

const RolloutBatchSchema = z.object({
  percent: z.number().int().min(0).max(100),
  delaySeconds: z.number().int().min(0),
});

/**
 * CDN `latest.json` wire format. Deliberately NOT `.strict()` — unknown
 * fields are ignored so future manifest additions never break shipped
 * clients (the plain-text `/latest` taught us that hard-failing on
 * unexpected content bricks the update path forever).
 */
export const UpdateManifestSchema = z.object({
  version: z.string().refine((value) => valid(value) !== null, { error: 'invalid semver' }),
  publishedAt: z
    .string()
    .refine((value) => Number.isFinite(Date.parse(value)), { error: 'invalid timestamp' }),
  rollout: z.array(RolloutBatchSchema).readonly().default([]),
});

export interface FetchLatestResult {
  /** Raw newest version — what `dimi upgrade` installs, never rollout-gated. */
  readonly latest: string;
  /** Null when the JSON manifest was unavailable and we fell back to plain text. */
  readonly manifest: UpdateManifest | null;
}

async function fetchWithTimeout(fetchImpl: typeof fetch, input: string): Promise<Response> {
  const controller = new AbortController();
  const timeout = setTimeout(() => {
    controller.abort();
  }, CDN_FETCH_TIMEOUT_MS);
  try {
    return await fetchImpl(input, { signal: controller.signal });
  } finally {
    clearTimeout(timeout);
  }
}

/**
 * The Dimi update authority is the `latest.json` manifest attached to the
 * newest GitHub Release (see `DIMI_CODE_UPDATE_CHANNEL_URL`). The guard below
 * keeps the HTTP boundary honest when no channel is configured: direct
 * callers cannot accidentally fall back to the old product's update channel.
 */
export function hasUpdateChannel(): boolean {
  return DIMI_CODE_UPDATE_CHANNEL_URL !== undefined;
}

function updateUrl(path: string, channelUrl: string | undefined): string {
  if (channelUrl === undefined) {
    throw new Error('Automatic updates are not configured for this build');
  }
  return new URL(path, channelUrl).toString();
}

/**
 * Fetch the latest published Dimi version from the CDN.
 *
 * **Throws** on any failure (network error, non-2xx, empty body, non-semver
 * text). Callers must catch — `refreshUpdateCache` deliberately lets the
 * error propagate so the existing cache stays intact instead of being
 * overwritten with a null `latest` on a transient blip.
 *
 * `fetchImpl` is injectable for tests; defaults to the global `fetch`.
 */
export async function fetchLatestVersionFromCdn(
  fetchImpl: typeof fetch = fetch,
  channelUrl: string | undefined = DIMI_CODE_UPDATE_CHANNEL_URL,
): Promise<string> {
  const response = await fetchWithTimeout(fetchImpl, updateUrl('latest', channelUrl));
  if (!response.ok) {
    throw new Error(`CDN /latest returned HTTP ${response.status}`);
  }
  const raw = (await response.text()).trim();
  if (valid(raw) === null) {
    throw new Error(`CDN /latest returned invalid semver: ${JSON.stringify(raw)}`);
  }
  return raw;
}

async function fetchUpdateManifestFromCdn(
  fetchImpl: typeof fetch,
  channelUrl: string | undefined,
): Promise<UpdateManifest> {
  const response = await fetchWithTimeout(fetchImpl, updateUrl('latest.json', channelUrl));
  if (!response.ok) {
    throw new Error(`CDN /latest.json returned HTTP ${response.status}`);
  }
  return UpdateManifestSchema.parse(JSON.parse(await response.text()));
}

/**
 * Fetch the rollout manifest, falling back to the plain-text `/latest` when
 * `latest.json` is unavailable or malformed. The fallback removes any
 * deployment-order coupling between client releases and the CDN file, and a
 * null manifest means "fully rolled out" — exactly the pre-rollout behavior.
 *
 * **Throws** only when both sources fail; callers must catch (see above).
 */
export async function fetchLatestFromCdn(
  fetchImpl: typeof fetch = fetch,
  channelUrl: string | undefined = DIMI_CODE_UPDATE_CHANNEL_URL,
): Promise<FetchLatestResult> {
  const manifest = await fetchUpdateManifestFromCdn(fetchImpl, channelUrl).catch(() => null);
  if (manifest !== null) {
    return { latest: manifest.version, manifest };
  }
  const latest = await fetchLatestVersionFromCdn(fetchImpl, channelUrl);
  return { latest, manifest: null };
}
