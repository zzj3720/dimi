#!/usr/bin/env node
/**
 * Builds the dimi-bridge napi addon for the CURRENT platform and packs it as
 * an npm installable platform subpackage tarball:
 *
 *   @dimi-agent/dimi-native-<platform>-<arch>@<cli-version>.tgz
 *
 * The subpackage version follows apps/dimi/package.json so every CLI release
 * ships bindings at the same version (the CLI declares the six platform
 * subpackages as optionalDependencies and npm picks the matching one).
 *
 * Run from the package root:
 *   pnpm --filter @dimi-agent/dimi-native run pack:platform
 *
 * Requires a Rust toolchain (cargo) — the repo's rust-version is 1.85+.
 * Set DIMI_NATIVE_RELEASE=1 for an optimized build (CI does this).
 * Output: dist-platform/<name>-<version>.tgz
 */
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
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
const libPath = join(targetDir, profileDir, libName);

const platform = process.platform;
const arch = process.arch;
const packageName = `@dimi-agent/dimi-native-${platform}-${arch}`;

const cliPackage = JSON.parse(
  readFileSync(join(repoRoot, 'apps', 'dimi', 'package.json'), 'utf-8'),
);
const version = cliPackage.version;
if (typeof version !== 'string' || version.length === 0) {
  throw new Error(`dimi-native: invalid CLI version in apps/dimi/package.json: ${version}`);
}

const outDir = join(packageRoot, 'dist-platform', packageName);
rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });
copyFileSync(libPath, join(outDir, 'dimi_bridge.node'));

writeFileSync(
  join(outDir, 'package.json'),
  `${JSON.stringify(
    {
      name: packageName,
      version,
      description: `dimi native binding (dimi-bridge) for ${platform}-${arch}`,
      license: 'MIT',
      os: [platform],
      cpu: [arch],
      main: 'dimi_bridge.node',
      files: ['dimi_bridge.node'],
      publishConfig: { access: 'public' },
    },
    null,
    2,
  )}\n`,
);

const packDestination = join(packageRoot, 'dist-platform');
mkdirSync(packDestination, { recursive: true });
execFileSync('npm', ['pack', outDir, '--pack-destination', packDestination], {
  stdio: 'inherit',
});

const tgzName = `${packageName.replace('@', '').replace('/', '-')}-${version}.tgz`;
console.log(`dimi-native: packed ${join(packDestination, tgzName)}`);
