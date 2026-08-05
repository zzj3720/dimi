import { execFile } from 'node:child_process';
import { readFile, stat } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const bundlePath = resolve(appRoot, 'dist', 'main.mjs');
const webIndexPath = resolve(appRoot, 'dist-web', 'index.html');
const packageJson = JSON.parse(await readFile(resolve(appRoot, 'package.json'), 'utf-8'));
const expectedVersion = packageJson.version;
// The Rust TUI binary the bundle spawns by default (scripts/build-dimi-cli.mjs).
const dimiCliBinary = process.platform === 'win32' ? 'dimi-cli.exe' : 'dimi-cli';
const dimiCliPath = resolve(appRoot, 'dist', dimiCliBinary);

function fail(message) {
  console.error(message);
  process.exit(1);
}

async function ensureBundleExists() {
  try {
    await stat(bundlePath);
  } catch {
    fail(`Bundle not found at ${bundlePath}. Run \`pnpm build\` first.`);
  }
}

async function ensureRuntimeAssetsExist() {
  try {
    await stat(webIndexPath);
  } catch {
    fail(`Runtime asset not found at ${webIndexPath}. Run \`pnpm build\` first.`);
  }
}

async function runBundle(args) {
  try {
    const { stdout, stderr } = await execFileAsync(process.execPath, [bundlePath, ...args], {
      cwd: appRoot,
      maxBuffer: 1024 * 1024 * 16,
    });
    return `${stdout}${stderr}`;
  } catch (error) {
    const detail = [error.stdout?.trim(), error.stderr?.trim(), error.message]
      .filter(Boolean)
      .join('\n');
    fail(`Bundle smoke failed: node ${bundlePath} ${args.join(' ')}\n${detail}`);
  }
}

function assertIncludes(output, expected, command) {
  if (!output.includes(expected)) {
    fail(`Bundle smoke output for "${command}" did not include "${expected}".\n${output}`);
  }
}

await ensureBundleExists();
await ensureRuntimeAssetsExist();

// TS TUI path: `--legacy` forces the TypeScript TUI, so these stay
// deterministic whether or not the Rust TUI binary is present.
const versionOutput = await runBundle(['--legacy', '--version']);
assertIncludes(versionOutput, expectedVersion, '--legacy --version');

const helpOutput = await runBundle(['--legacy', '--help']);
assertIncludes(helpOutput, 'Usage: dimi', '--legacy --help');

const exportHelpOutput = await runBundle(['--legacy', 'export', '--help']);
assertIncludes(exportHelpOutput, 'Usage: dimi export', '--legacy export --help');

const webHelpOutput = await runBundle(['--legacy', 'web', '--help']);
assertIncludes(webHelpOutput, 'Usage: dimi web', '--legacy web --help');

// Rust TUI dispatch: with the binary shipped and no `--legacy`, `dimi` spawns
// dimi-cli instead of the TS TUI. Verify it surfaces dimi-cli's own version.
const dimiCliVersion = await runDimiCliVersion();
if (dimiCliVersion !== null) {
  const rustOutput = await runBundle(['--version']);
  assertIncludes(rustOutput, dimiCliVersion, '--version (dimi-cli dispatch)');
  if (rustOutput.includes(expectedVersion)) {
    fail(
      `Rust TUI dispatch unexpectedly reported the TS version (${expectedVersion}).\n${rustOutput}`,
    );
  }
  console.log(`Rust TUI dispatch smoke passed: ${dimiCliPath} -> ${dimiCliVersion}`);
}

console.log(`Bundle smoke passed: ${bundlePath}`);

/** dimi-cli's own --version output, or null when the binary isn't built. */
async function runDimiCliVersion() {
  try {
    const { stdout } = await execFileAsync(dimiCliPath, ['--version'], { cwd: appRoot });
    return stdout.trim();
  } catch {
    return null; // binary not built — the dispatch can't be verified, TS fallback covers it
  }
}
