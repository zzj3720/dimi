#!/usr/bin/env node
/**
 * Publishes the platform binding subpackage tarballs produced by
 * packages/dimi-native/scripts/pack-platform.mjs. Idempotent: versions that
 * already exist on the registry are skipped, so re-running a release does not
 * fail on previously published bindings.
 *
 * Usage:
 *   node apps/dimi/scripts/native/publish-bindings.mjs <tgz-dir>
 *
 * Requires npm auth (NODE_AUTH_TOKEN / .npmrc registry token).
 */
import { execFileSync } from 'node:child_process';
import { readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';

const inputDir = resolve(process.argv[2] ?? '.');
const tgzs = readdirSync(inputDir)
  .filter((f) => f.endsWith('.tgz'))
  .sort();

if (tgzs.length === 0) {
  console.error(`publish-bindings: no .tgz files in ${inputDir}`);
  process.exit(1);
}

let published = 0;
let skipped = 0;
for (const file of tgzs) {
  const pkgJson = JSON.parse(
    execFileSync('tar', ['-xOf', join(inputDir, file), 'package/package.json'], {
      encoding: 'utf8',
    }),
  );
  const spec = `${pkgJson.name}@${pkgJson.version}`;
  try {
    execFileSync('npm', ['view', spec, 'version'], { stdio: 'pipe' });
    console.log(`publish-bindings: skip (already published): ${spec}`);
    skipped += 1;
    continue;
  } catch {
    // not on the registry yet — publish below
  }
  console.log(`publish-bindings: publishing ${spec} (${file})`);
  execFileSync('npm', ['publish', join(inputDir, file), '--access', 'public'], {
    stdio: 'inherit',
  });
  published += 1;
}

console.log(`publish-bindings: done (published=${published}, skipped=${skipped})`);
if (published === 0 && skipped === 0) {
  process.exit(1);
}
