/**
 * Produce the published marketplace catalog + official plugin zips for a
 * GitHub Release.
 *
 * GitHub Release assets are flat: there is no `plugins/` directory, so the
 * in-repo `plugins/marketplace.json` (whose official entry sources are
 * relative paths like `./official/dimi-datasource`, resolved against the
 * source checkout) cannot be uploaded as-is. This script rewrites every
 * relative entry source into a flat release-asset URL
 * (`https://github.com/zzj3720/dimi/releases/latest/download/<id>.zip`) and
 * packages each official plugin directory under `plugins/official/<id>` into
 * `<id>.zip` next to the marketplace catalog.
 *
 * Usage:
 *   node produce-marketplace.mjs <output-dir> <release-tag>
 *
 * Output (all in <output-dir>, uploaded to the Release):
 *   marketplace.json   ← rewritten catalog
 *   dimi-datasource.zip ← official plugin archives (one per official entry)
 */

import { readFile, readdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import { ZipFile } from 'yazl';

import { appRoot } from './paths.mjs';

const [, , outputDir, tag] = process.argv;
if (!outputDir || !tag) {
  console.error('Usage: produce-marketplace.mjs <output-dir> <release-tag>');
  process.exit(1);
}

// apps/dimi/scripts/native → repo root
const repoRoot = resolve(appRoot, '..', '..');
const RELEASE_BASE = 'https://github.com/zzj3720/dimi/releases/latest/download';
const marketplacePath = resolve(repoRoot, 'plugins/marketplace.json');
const officialRoot = resolve(repoRoot, 'plugins/official');

const raw = JSON.parse(await readFile(marketplacePath, 'utf8'));
if (!Array.isArray(raw.plugins)) {
  console.error('marketplace.json must contain a "plugins" array');
  process.exit(1);
}

// Version of the catalog itself: derive from the release tag (`@dimi-agent/cli@x.y.z`).
const version = tag.replace(/^@dimi-agent\/cli@/, '').replace(/^v/, '');

const plugins = [];
for (const entry of raw.plugins) {
  const source = entry.source;
  if (typeof source === 'string' && source.startsWith('./')) {
    // Rewrite `./official/<id>` → flat release asset URL.
    const id = String(entry.id);
    const rewritten = { ...entry, source: `${RELEASE_BASE}/${id}.zip` };
    plugins.push(rewritten);
    if (entry.tier === 'official') {
      await packOfficialPlugin(id, outputDir);
    }
  } else {
    plugins.push(entry);
  }
}

const catalog = { ...raw, version, plugins };
await writeFile(resolve(outputDir, 'marketplace.json'), `${JSON.stringify(catalog, null, 2)}\n`);
console.log(`Wrote ${resolve(outputDir, 'marketplace.json')} (${plugins.length} plugins)`);

async function packOfficialPlugin(id, dir) {
  const pluginDir = resolve(officialRoot, id);
  const zip = new ZipFile();
  await addDirToZip(zip, pluginDir, '');
  const out = resolve(dir, `${id}.zip`);
  const { pipeline } = await import('node:stream/promises');
  const { createWriteStream } = await import('node:fs');
  zip.end();
  await pipeline(zip.outputStream, createWriteStream(out));
  console.log(`Packed ${out}`);
}

async function addDirToZip(zip, dirPath, prefix) {
  const { stat } = await import('node:fs/promises');
  const entries = await readdir(dirPath, { withFileTypes: true });
  for (const entry of entries) {
    if (entry.name === '.gitignore') continue;
    const full = resolve(dirPath, entry.name);
    const name = prefix.length === 0 ? entry.name : `${prefix}/${entry.name}`;
    if (entry.isDirectory()) {
      await addDirToZip(zip, full, name);
    } else {
      const info = await stat(full);
      zip.addFile(full, name, { mode: info.mode });
    }
  }
}
