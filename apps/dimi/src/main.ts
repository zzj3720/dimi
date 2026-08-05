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

import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** Flags the Rust TUI (`dimi-cli`) understands, plus the ones that take a value. */
const DIMI_CLI_FLAGS = new Set(['--wire', '--config', '--help', '-h', '--version', '-V']);
const DIMI_CLI_VALUE_FLAGS = new Set(['--wire', '--config']);

if (process.argv.includes('--legacy')) {
  process.env['DIMI_LEGACY'] = '1';
}

// Rust TUI dispatch (slice 6d): without `--legacy`, when the bundled dimi-cli
// binary is present, spawn it as the interactive TUI instead of the TypeScript
// TUI. `--legacy`, a missing binary, or args dimi-cli cannot handle all fall
// through to the TypeScript path below.
const dimiCliBinary = resolveDimiCliBinary();
let launchedDimiCli = false;
if (process.env['DIMI_LEGACY'] !== '1' && dimiCliBinary) {
  const forward = argsForDimiCli(process.argv.slice(2));
  if (forward.ok) {
    launchedDimiCli = true;
    spawnDimiCli(dimiCliBinary, forward.args);
  } else {
    process.stderr.write(
      `dimi: ${forward.reason}; the Rust TUI does not support it. ` +
        'Falling back to the TypeScript TUI.\n',
    );
  }
}

if (!launchedDimiCli) {
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
}

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

interface DimiCliForward {
  ok: boolean;
  args: string[];
  reason?: string;
}

/**
 * Locate the bundled dimi-cli binary next to the entry bundle
 * (`dist/dimi-cli`, `dist/dimi-cli.exe` on win32 — produced by
 * `scripts/build-dimi-cli.mjs`).
 *
 * The dev entry (`tsx src/main.ts`) returns `null` on purpose: it keeps the
 * TypeScript TUI as the dev surface so TS TUI development is never silently
 * replaced by the Rust TUI. Only the built product (`dist/main.mjs`)
 * dispatches to dimi-cli. The SEA executable also returns `null` — it ships
 * no sibling binary, so the bundled TS TUI remains its path.
 */
function resolveDimiCliBinary(): string | null {
  if (/[\\/]src[\\/]main\.ts$/.test(import.meta.url)) {
    return null;
  }
  const binaryName = process.platform === 'win32' ? 'dimi-cli.exe' : 'dimi-cli';
  const binaryPath = resolve(dirname(fileURLToPath(import.meta.url)), binaryName);
  return existsSync(binaryPath) ? binaryPath : null;
}

/**
 * Decide whether `args` can be handled entirely by dimi-cli. dimi-cli only
 * understands `--wire`/`--config`/`--help`/`--version` (plus `-h`/`-V`);
 * TS-only options (`-S`/`-p`/`--model`/subcommands…) are rejected so the
 * entry can fall back to the TypeScript TUI for them.
 */
function argsForDimiCli(args: string[]): DimiCliForward {
  const forwarded: string[] = [];
  let i = 0;
  while (i < args.length) {
    // `i < args.length` guarantees the element exists; `noUncheckedIndexedAccess`
    // still types the index as `string | undefined`.
    const arg = args[i]!;
    if (DIMI_CLI_FLAGS.has(arg)) {
      forwarded.push(arg);
      if (DIMI_CLI_VALUE_FLAGS.has(arg)) {
        const value = args[i + 1];
        if (value === undefined) {
          return { ok: false, args: forwarded, reason: `${arg} requires a value` };
        }
        forwarded.push(value);
        i += 2;
      } else {
        i += 1;
      }
    } else {
      return {
        ok: false,
        args: forwarded,
        reason: `option not supported by the Rust TUI: ${arg}`,
      };
    }
  }
  return { ok: true, args: forwarded };
}

/**
 * Spawn dimi-cli with inherited stdio (the interactive TTY) and forward its
 * exit code. The child is a standalone process in this process's foreground
 * group; it receives Ctrl+C/SIGTERM directly, so we install no-op handlers to
 * stay alive and report its real exit status instead of dying first with the
 * default 130.
 */
function spawnDimiCli(binaryPath: string, args: string[]): void {
  const child = spawn(binaryPath, args, { stdio: 'inherit' });
  for (const signal of ['SIGINT', 'SIGTERM'] as const) {
    process.on(signal, () => {});
  }
  child.on('error', (error: NodeJS.ErrnoException) => {
    process.stderr.write(
      `dimi: failed to launch the Rust TUI (${binaryPath}): ${error.message}\n`,
    );
    process.exit(1);
  });
  child.on('exit', (code, signal) => {
    if (signal !== null) {
      process.kill(process.pid, signal);
      return;
    }
    process.exit(code ?? 0);
  });
}
