/**
 * Test-environment repair for the node-pty native helper.
 *
 * node-pty ships `prebuilds/<platform>/spawn-helper` without the executable
 * bit in some package-manager stores (pnpm reproduces this consistently:
 * the tarball stores 644). `posix_spawnp` then fails with "posix_spawnp
 * failed." at pty spawn time. Restore the bit idempotently before the pty
 * differential suite runs.
 */
import { chmodSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);

function repairNodePtyHelper(): void {
  try {
    // Resolve the physical node-pty package root (through pnpm's symlinked
    // node_modules) and fix every platform helper we ship.
    const ptyRoot = join(require.resolve('node-pty/package.json'), '..');
    const prebuilds = join(ptyRoot, 'prebuilds');
    if (!existsSync(prebuilds)) return;
    for (const platform of ['darwin-arm64', 'darwin-x64', 'win32-arm64', 'win32-x64', 'linux-x64']) {
      const helper = join(prebuilds, platform, 'spawn-helper');
      if (existsSync(helper)) {
        try {
          chmodSync(helper, 0o755);
        } catch {
          // ignore: read-only store or already executable
        }
      }
    }
  } catch {
    // node-pty absent (e.g. production installs without dev deps) — nothing to repair.
  }
}

repairNodePtyHelper();
