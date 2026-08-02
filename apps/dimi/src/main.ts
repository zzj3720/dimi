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
  // NB: do not name this binding `require` — rolldown's CJS output references
  // the injected `require` inside the createRequire initializer, and a
  // `const require` shadowing it dies with a TDZ ReferenceError at startup.
  const nodeRequire = createRequire(import.meta.url);
  // SEA binary: the platform binding is embedded in the executable, so the
  // Rust runtime is available by construction (bundle and assets are atomic).
  try {
    const sea = nodeRequire('node:sea') as { isSea?: () => boolean } | undefined;
    if (typeof sea?.isSea === 'function' && sea.isSea()) return true;
  } catch {
    // not a SEA runtime
  }
  // Local dev build: `dist/dimi_bridge.node` next to the bundle
  // (the bundled `loadNative` resolves `../dist/dimi_bridge.node` from it).
  if (existsSync(resolve(dirname(fileURLToPath(import.meta.url)), '../dist/dimi_bridge.node'))) {
    return true;
  }
  // npm install (>=0.5.4): the platform binding subpackage installed by npm.
  try {
    nodeRequire.resolve(`@dimi-agent/dimi-native-${process.platform}-${process.arch}`);
    return true;
  } catch {
    // binding not installed for this platform
  }
  // Dev (tsx from src/): the workspace dimi-native package's own dist.
  try {
    const pkgPath = nodeRequire.resolve('@dimi-agent/dimi-native/package.json');
    if (existsSync(resolve(dirname(pkgPath), 'dist/dimi_bridge.node'))) return true;
  } catch {
    // not resolvable in the packaged runtime — no binding available
  }
  return false;
}
