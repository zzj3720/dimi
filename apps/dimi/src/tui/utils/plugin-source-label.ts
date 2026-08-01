import type { PluginSummary } from '@dimi-agent/dimi-sdk';

export const OFFICIAL_BADGE = 'official';
export const CURATED_BADGE = 'curated';
export const THIRD_PARTY_BADGE = 'third-party';

export type PluginTrustLabel = 'official' | 'curated' | 'third-party';

/**
 * Human-readable provenance label for a plugin, suitable for inline display
 * in `/plugins` overviews and lists.
 *
 * - github source → `github <owner>/<repo>@<ref>`
 * - zip-url with parseable URL → `via <host[:port]>`
 * - everything else → raw source kind (`local-path`, `zip-url`)
 */
export function formatPluginSourceLabel(plugin: PluginSummary): string {
  if (plugin.source === 'github' && plugin.github !== undefined) {
    return `github ${plugin.github.owner}/${plugin.github.repo}@${plugin.github.ref.value}`;
  }
  if (plugin.source === 'zip-url' && plugin.originalSource !== undefined) {
    const host = hostFromUrl(plugin.originalSource);
    if (host !== undefined) return `via ${host}`;
  }
  return plugin.source;
}

/**
 * Returns one of three trust labels for a plugin. Only Dimi-hosted plugin zip
 * assets receive official or curated badges. Everything else is third-party.
 *
 * Official plugin zips are published to GitHub Releases under the `dimi-`
 * prefix (e.g. `dimi-datasource.zip`); curated zips use `curated-`.
 */
const DIMI_RELEASE_PREFIX = '/zzj3720/dimi/releases/latest/download/';

function releaseAssetName(url: URL): string | undefined {
  if (url.protocol !== 'https:' || url.hostname !== 'github.com') return undefined;
  if (!url.pathname.startsWith(DIMI_RELEASE_PREFIX)) return undefined;
  return url.pathname.slice(DIMI_RELEASE_PREFIX.length);
}

export function pluginTrustLabel(plugin: PluginSummary): PluginTrustLabel {
  if (plugin.source !== 'zip-url' || plugin.originalSource === undefined) {
    return 'third-party';
  }
  try {
    const url = new URL(plugin.originalSource);
    const asset = releaseAssetName(url);
    if (asset === undefined) return 'third-party';
    if (asset.startsWith('dimi-') && asset.endsWith('.zip')) {
      return 'official';
    }
    if (asset.startsWith('curated-') && asset.endsWith('.zip')) {
      return 'curated';
    }
    return 'third-party';
  } catch {
    return 'third-party';
  }
}

/**
 * Returns true only for install sources that are unambiguously Dimi-built
 * official plugins — an https URL under the official Dimi release asset path.
 * Everything else (local paths, GitHub repos, curated or third-party URLs)
 * is treated as unofficial and should be confirmed before install.
 */
export function isOfficialPluginSource(source: string): boolean {
  const trimmed = source.trim();
  if (!trimmed.startsWith('https://')) return false;
  try {
    const url = new URL(trimmed);
    const asset = releaseAssetName(url);
    return asset !== undefined && asset.startsWith('dimi-') && asset.endsWith('.zip');
  } catch {
    return false;
  }
}

/**
 * Returns true when an installed plugin provably came from a trusted official
 * source — a zip download under the official CDN plugin path. Local paths,
 * GitHub repos, and third-party URLs do not qualify, even when their manifest
 * id matches an official plugin.
 */
export function isOfficialPluginInstall(plugin: PluginSummary): boolean {
  return (
    plugin.source === 'zip-url' &&
    plugin.originalSource !== undefined &&
    isOfficialPluginSource(plugin.originalSource)
  );
}

function hostFromUrl(raw: string): string | undefined {
  try {
    const url = new URL(raw);
    if (url.port.length > 0) return `${url.hostname}:${url.port}`;
    return url.hostname;
  } catch {
    return undefined;
  }
}
