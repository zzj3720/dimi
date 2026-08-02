import { cp, mkdir, rm, stat } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(appRoot, '../..');
const source = resolve(repoRoot, 'packages/pi-tui/native');
const target = resolve(appRoot, 'native');

// pi-tui ships platform-specific native helpers only for darwin/win32;
// Linux has no native helper, so there is nothing to copy for it.
const PLATFORMS = ['darwin', 'win32'];

async function assertPrebuilds(platform) {
  const dir = resolve(source, platform, 'prebuilds');
  try {
    const info = await stat(dir);
    if (!info.isDirectory()) {
      throw new Error('not a directory');
    }
  } catch {
    throw new Error(
      `pi-tui native prebuilds were not found at ${dir}. Build or restore packages/pi-tui first.`,
    );
  }
  return dir;
}

await rm(target, { recursive: true, force: true });
await mkdir(target, { recursive: true });

for (const platform of PLATFORMS) {
  const srcPrebuilds = await assertPrebuilds(platform);
  const dstPrebuilds = resolve(target, platform, 'prebuilds');
  await cp(srcPrebuilds, dstPrebuilds, { recursive: true });
}

console.log(`Copied pi-tui native prebuilds to ${target}`);

// dimi-native's napi binding — the Rust exec layer (M2+). The npm package
// ships the build machine's platform binary at `dist/dimi_bridge.node`
// (`loadNative` resolves it relative to the bundled `dist/main.mjs`); other
// platforms fall back to the legacy backend via the entry-point preflight.
const bridgeSource = resolve(repoRoot, 'packages/dimi-native/dist/dimi_bridge.node');
const bridgeTargetDir = resolve(appRoot, 'dist');
const bridgeTarget = resolve(bridgeTargetDir, 'dimi_bridge.node');
try {
  const info = await stat(bridgeSource);
  if (!info.isFile()) {
    throw new Error('not a file');
  }
} catch {
  throw new Error(
    `dimi-native binding was not found at ${bridgeSource}. Run "pnpm --filter @dimi-agent/dimi-native run build:native" first.`,
  );
}
await mkdir(bridgeTargetDir, { recursive: true });
await cp(bridgeSource, bridgeTarget);
console.log(`Copied dimi-native binding to ${bridgeTarget}`);
