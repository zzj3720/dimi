/**
 * Native binding ↔ Rust source ↔ TS wrapper contract test.
 *
 * The `.d.ts`-equivalent typing (`NativeBinding` + wrapper signatures) is
 * hand-written; this test pins it to reality from three directions:
 *
 *   1. Rust source: every `#[napi] pub fn` in `crates/dimi-bridge/src/lib.rs`
 *      must exist on the loaded binding under its napi-rs JS name
 *      (snake_case → camelCase, unless `js_name` overrides).
 *   2. Binding: the loaded addon must export exactly that name set (extra or
 *      missing names are a drift signal).
 *   3. TS wrapper: every JS export must be re-exported by `#/index`.
 *
 * Skips itself when the native binding is not built (same policy as the
 * differential suite).
 */
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, test } from 'vitest';

import * as wrapper from '#/index';
import { loadNative } from '#/index';

const bindingPath = fileURLToPath(new URL('../dist/dimi_bridge.node', import.meta.url));
const bridgeSrcPath = fileURLToPath(new URL('../../../crates/dimi-bridge/src/lib.rs', import.meta.url));

/** napi-rs default JS export name: snake_case fn name → camelCase. */
function napiJsName(rustFnName: string): string {
  const [first, ...rest] = rustFnName.split('_');
  return first + rest.map((part) => part.slice(0, 1).toUpperCase() + part.slice(1)).join('');
}

/**
 * Extract `#[napi]`-annotated function names from the bridge source,
 * honoring `js_name = "..."` overrides when present.
 */
function napiFunctionNames(source: string): string[] {
  const names: string[] = [];
  for (const match of source.matchAll(/#\[napi(\([^\]]*\))?\]\s*pub fn\s+(\w+)/g)) {
    const attr = match[1] ?? '';
    const fnName = match[2] ?? '';
    const jsNameOverride = attr.match(/js_name\s*=\s*"([^"]+)"/);
    names.push(jsNameOverride ? (jsNameOverride[1] ?? '') : napiJsName(fnName));
  }
  return names;
}

const nativeAvailable = existsSync(bindingPath);
const suite = nativeAvailable ? describe : describe.skip;

suite('native binding ↔ Rust source ↔ TS wrapper', () => {
  test('binding exports exactly the #[napi] functions, under napi-rs JS names', () => {
    const source = readFileSync(bridgeSrcPath, 'utf8');
    const expected = napiFunctionNames(source).map(napiJsName).sort();
    expect(expected.length).toBeGreaterThan(0);

    const binding = loadNative();
    expect(Object.keys(binding).sort(), 'binding export set').toEqual(expected);
  });

  test('TS wrapper re-exports every binding function with a function value', () => {
    const source = readFileSync(bridgeSrcPath, 'utf8');
    for (const rustName of napiFunctionNames(source)) {
      const jsName = napiJsName(rustName);
      expect(typeof wrapper[jsName as keyof typeof wrapper], `wrapper.${jsName}`).toBe('function');
    }
  });
});

if (!nativeAvailable) {
  console.warn(
    '[dimi-native] native binding not built — binding-contract suite skipped. Run `pnpm --filter @dimi-agent/dimi-native run build:native` to enable it.',
  );
}
