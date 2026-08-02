import { existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join } from 'node:path';

import { getNativePackageRoot } from './native-assets';

type ModuleLoad = (request: string, parent: unknown, isMain: boolean) => unknown;

interface ModuleWithLoad {
  _load?: ModuleLoad;
}

const nodeRequire = createRequire(import.meta.url);
let installed = false;

// pi-tui loads its platform-specific native helpers via an absolute-path
// require() computed from import.meta.url / process.execPath
// (see pi-tui dist/terminal.js and dist/native-modifiers.js). In a SEA binary
// those .node files live in the native-asset cache, so redirect any absolute
// require of a pi-tui native helper to the cached copy.
//
// Path shape: native/<darwin|win32>/prebuilds/<arch>/<file>.node — note the
// two path segments after "prebuilds", so ".+" (not "[^/]+") is required.
const PI_TUI_NATIVE_PATTERN = /native[\\/](?:win32|darwin)[\\/]prebuilds[\\/].+\.node$/;

// dimi-native's binding subpackage, e.g. `@dimi-agent/dimi-native-darwin-arm64`.
// loadNative requires it by name (falling back from the workspace dist copy);
// in a SEA binary no node_modules exists, so redirect it into the
// native-asset cache where the per-platform binding is materialized.
const DIMI_NATIVE_SUBPACKAGE = /^@dimi-agent\/dimi-native-(?:darwin|linux|win32)-(?:arm64|x64)$/;

/**
 * The cached dimi_bridge.node for the requested binding subpackage, or null
 * when the package resolves normally (dev / npm installs — the real package
 * wins) or the native-asset tree has no such package.
 */
function dimiNativeBindingPath(request: string): string | null {
  try {
    nodeRequire.resolve(request);
    return null; // resolvable outside SEA — nothing to redirect
  } catch {
    // not installed — fall through to the native-asset cache (SEA)
  }
  const pkgRoot = getNativePackageRoot(request);
  if (pkgRoot === null) return null;
  return join(pkgRoot, 'dimi_bridge.node');
}

export function installNativeModuleHook(): void {
  if (installed) return;
  installed = true;

  const moduleBuiltin = nodeRequire('node:module') as ModuleWithLoad;
  const originalLoad = moduleBuiltin._load;
  if (originalLoad === undefined) return;

  moduleBuiltin._load = function loadWithNativeAssets(
    this: unknown,
    request: string,
    parent: unknown,
    isMain: boolean,
  ): unknown {
    if (typeof request === 'string') {
      if (PI_TUI_NATIVE_PATTERN.test(request) && !existsSync(request)) {
        const pkgRoot = getNativePackageRoot('@dimi-agent/pi-tui');
        if (pkgRoot !== null) {
          const match = request.match(PI_TUI_NATIVE_PATTERN);
          if (match !== null) {
            const redirected = join(pkgRoot, match[0]);
            return originalLoad.call(this, redirected, parent, isMain);
          }
        }
      } else if (DIMI_NATIVE_SUBPACKAGE.test(request)) {
        const redirected = dimiNativeBindingPath(request);
        if (redirected !== null) {
          return originalLoad.call(this, redirected, parent, isMain);
        }
      }
    }
    return originalLoad.call(this, request, parent, isMain);
  };
}
