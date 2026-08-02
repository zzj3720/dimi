#!/usr/bin/env node
/**
 * Builds the dimi-bridge napi addon and copies it to dist/dimi_bridge.node.
 *
 * Run from the package root:
 *   pnpm --filter @dimi-agent/dimi-native run build:native
 *
 * Requires a Rust toolchain (cargo) — the repo's rust-version is 1.85+.
 * Set DIMI_NATIVE_RELEASE=1 for an optimized build.
 */
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, rmSync } from 'node:fs';
import { join } from 'node:path';

const packageRoot = join(import.meta.dirname, '..');
const repoRoot = join(packageRoot, '..', '..');
const release = process.env.DIMI_NATIVE_RELEASE === '1';

const args = ['build', '--manifest-path', join(repoRoot, 'Cargo.toml'), '-p', 'dimi-bridge'];
if (release) args.push('--release');
console.log(`dimi-native: cargo ${args.join(' ')}`);
execFileSync('cargo', args, { stdio: 'inherit' });

const libNames = {
  darwin: 'libdimi_bridge.dylib',
  linux: 'libdimi_bridge.so',
  win32: 'dimi_bridge.dll',
};
const libName = libNames[process.platform];
if (!libName) {
  throw new Error(`dimi-native: unsupported platform ${process.platform}`);
}

const targetDir = process.env.CARGO_TARGET_DIR ?? join(repoRoot, 'target');
const profileDir = release ? 'release' : 'debug';
const outDir = join(packageRoot, 'dist');
mkdirSync(outDir, { recursive: true });
rmSync(join(outDir, 'dimi_bridge.node'), { force: true });
copyFileSync(join(targetDir, profileDir, libName), join(outDir, 'dimi_bridge.node'));
console.log('dimi-native: dist/dimi_bridge.node ready');
