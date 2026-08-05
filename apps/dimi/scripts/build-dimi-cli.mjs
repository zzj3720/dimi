#!/usr/bin/env node
/**
 * Builds the Rust TUI binary (dimi-cli) and copies it to dist/dimi-cli.
 *
 * Run from the package root:
 *   pnpm --filter @dimi-agent/cli run build:cli
 *
 * The binary is a standalone executable (not a .node addon): `dimi`'s entry
 * (`dist/main.mjs`) spawns it as a child process when it is present next to
 * the bundle, inheriting the TTY, and falls back to the TypeScript TUI when
 * it is missing (see src/main.ts).
 *
 * Requires a Rust toolchain (cargo) — the repo's rust-version is 1.85+.
 *
 * Strict by default: a build failure exits non-zero so `pnpm build:cli` fails
 * loudly. Pass `--best-effort` (used by the `build` chain) to treat a failure
 * as "skip the Rust TUI" and exit 0 — the CLI then falls back to the TS TUI.
 * Set DIMI_SKIP_CLI_BUILD=1 to skip the cargo invocation entirely (fast local
 * builds / CI environments that don't want to spend minutes compiling the
 * Rust workspace).
 */
import { execFileSync } from 'node:child_process';
import { chmodSync, copyFileSync, mkdirSync, rmSync } from 'node:fs';
import { join } from 'node:path';

const packageRoot = join(import.meta.dirname, '..');
const repoRoot = join(packageRoot, '..', '..');
const bestEffort = process.argv.includes('--best-effort');
const skip = process.env.DIMI_SKIP_CLI_BUILD === '1';

// darwin/linux ship a bare executable; win32 appends `.exe`.
const binaryName = process.platform === 'win32' ? 'dimi-cli.exe' : 'dimi-cli';

function skipMessage() {
  console.warn(
    'dimi-cli build skipped: the `dimi` command will use the TypeScript TUI ' +
      '(run `pnpm --filter @dimi-agent/cli run build:cli` to build the Rust TUI).',
  );
}

if (skip) {
  skipMessage();
  process.exit(0);
}

try {
  const args = [
    'build',
    '--manifest-path',
    join(repoRoot, 'Cargo.toml'),
    '-p',
    'dimi-cli',
    '--release',
  ];
  console.log(`dimi-cli: cargo ${args.join(' ')}`);
  execFileSync('cargo', args, { stdio: 'inherit' });

  const targetDir = process.env.CARGO_TARGET_DIR ?? join(repoRoot, 'target');
  const sourceBinary = join(targetDir, 'release', binaryName);

  const outDir = join(packageRoot, 'dist');
  mkdirSync(outDir, { recursive: true });
  const targetBinary = join(outDir, binaryName);
  rmSync(targetBinary, { force: true });
  copyFileSync(sourceBinary, targetBinary);
  // cargo sets the exec bit on the source; make sure it survives the copy on
  // POSIX so the entry point can spawn it.
  if (process.platform !== 'win32') chmodSync(targetBinary, 0o755);
  console.log(`dimi-cli: ${targetBinary} ready`);
} catch (error) {
  if (bestEffort) {
    console.warn(
      `dimi-cli build skipped (best-effort): ${
        error instanceof Error ? error.message : String(error)
      }`,
    );
    skipMessage();
    process.exit(0);
  }
  throw error;
}
