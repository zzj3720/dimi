/**
 * Dimi process entry point.
 *
 * This file must stay dependency-free (no static project imports): ESM
 * static imports resolve and evaluate *before* any top-level code here runs,
 * and the agent-core-v2 OS backend modules read the legacy switch at module
 * load time. The process-wide `--legacy` flag therefore has to be set from
 * argv before anything else loads, then the real entry (`main-app`) is
 * loaded dynamically. Only node built-ins are imported here.
 */

import { existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.argv.includes('--legacy')) {
  process.env['DIMI_LEGACY'] = '1';
}

// Native-binding preflight: since M2 the Rust exec layer is the default, but
// the platform `dimi_bridge.node` may be absent (npm installs predating the
// native asset packaging, unsupported platforms, a missing `build:native` in
// dev checkouts). Instead of crashing at startup, fall back to the legacy
// TypeScript backend for this process.
if (process.env['DIMI_LEGACY'] !== '1' && !nativeBindingAvailable()) {
  process.env['DIMI_LEGACY'] = '1';
  process.stderr.write(
    'dimi: native runtime unavailable (dimi_bridge.node not found); ' +
      'falling back to the legacy TypeScript backend. Reinstall or run "dimi upgrade" to get the Rust runtime.\n',
  );
}

// Load the real entry. No top-level await: the native bundle (SEA) builds
// this entry as CJS, which does not support TLA.
import('./main-app').catch((error: unknown) => {
  process.stderr.write(
    `dimi: failed to start: ${error instanceof Error ? error.message : String(error)}\n`,
  );
  process.exit(1);
});

/** Locate the napi binding without loading it. */
function nativeBindingAvailable(): boolean {
  const require = createRequire(import.meta.url);
  // Bundled/npm install: `dist/dimi_bridge.node` next to `dist/main.mjs`
  // (the bundled `loadNative` resolves `../dist/dimi_bridge.node` from it).
  const candidates = [resolve(dirname(fileURLToPath(import.meta.url)), '../dist/dimi_bridge.node')];
  // Dev (tsx from src/): the workspace dimi-native package's own dist.
  try {
    const pkgPath = require.resolve('@dimi-agent/dimi-native/package.json');
    candidates.push(resolve(dirname(pkgPath), 'dist/dimi_bridge.node'));
  } catch {
    // not resolvable in the packaged runtime — the first candidate covers it
  }
  return candidates.some((candidate) => existsSync(candidate));
}
