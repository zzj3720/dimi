import { writeUpdateCache } from './cache';
import { fetchLatestFromCdn, hasUpdateChannel, type FetchLatestResult } from './cdn';
import { emptyUpdateCache, type UpdateCache } from './types';

export interface RefreshUpdateCacheDeps {
  /** Resolves with the latest version + rollout manifest. **Throws** on any
   * failure — callers (including the default background invocation in
   * preflight) must catch. Errors intentionally skip `writeCache` so a
   * transient CDN blip does not overwrite a previously known `latest` with
   * `null`. */
  readonly fetchLatest: () => Promise<FetchLatestResult>;
  readonly writeCache: (cache: UpdateCache) => Promise<void>;
  readonly now: () => Date;
}

export async function refreshUpdateCache(
  overrides: Partial<RefreshUpdateCacheDeps> = {},
): Promise<UpdateCache> {
  // Test and embedding callers may supply their own authoritative fetcher.
  // The product default must never look up a version on the former project's
  // channel while this repository has no authority of its own.
  if (overrides.fetchLatest === undefined && !hasUpdateChannel()) {
    return emptyUpdateCache();
  }
  const resolved: RefreshUpdateCacheDeps = {
    fetchLatest: overrides.fetchLatest ?? (() => fetchLatestFromCdn()),
    writeCache: overrides.writeCache ?? writeUpdateCache,
    now: overrides.now ?? (() => new Date()),
  };

  const { latest, manifest } = await resolved.fetchLatest();
  const cache: UpdateCache = {
    source: 'cdn',
    checkedAt: resolved.now().toISOString(),
    latest,
    manifest,
  };
  await resolved.writeCache(cache);
  return cache;
}
