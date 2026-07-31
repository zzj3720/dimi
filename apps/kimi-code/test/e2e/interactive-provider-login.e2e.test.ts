/**
 * Scenario: a user connects a provider through the production interactive TUI.
 * Responsibilities: accept /login, collect a secret, select a runtime model, and persist both.
 * Wiring: production main entry with an isolated home and documented paste-burst setting.
 * Run: vp exec vitest run test/e2e/interactive-provider-login.e2e.test.ts
 */
import { createRequire } from 'node:module';
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

const require = createRequire(import.meta.url);
const appRoot = join(import.meta.dirname, '..', '..');
const tsxCli = require.resolve('tsx/cli');
const rawTextLoader = join(appRoot, '..', '..', 'build', 'register-raw-text-loader.mjs');
const main = join(appRoot, 'src', 'main.ts');
const tempHomes: string[] = [];
const PTY_BRIDGE = `
import os, pty, select, sys
pid, master = pty.fork()
if pid == 0:
    os.execvpe(sys.argv[1], sys.argv[1:], os.environ)
readers = [master, sys.stdin.fileno()]
while True:
    readable, _, _ = select.select(readers, [], [])
    if master in readable:
        try:
            data = os.read(master, 4096)
        except OSError:
            break
        if not data:
            break
        os.write(sys.stdout.fileno(), data)
    if sys.stdin.fileno() in readable:
        data = os.read(sys.stdin.fileno(), 4096)
        if data:
            os.write(master, data)
        else:
            readers.remove(sys.stdin.fileno())
_, status = os.waitpid(pid, 0)
if os.WIFEXITED(status):
    sys.exit(os.WEXITSTATUS(status))
sys.exit(128 + os.WTERMSIG(status))
`;

afterEach(async () => {
  for (const home of tempHomes.splice(0)) await rm(home, { recursive: true, force: true });
});

describe('interactive provider login', () => {
  it.skipIf(process.platform === 'win32')(
    'persists the selected OpenAI model after a production TUI login',
    async () => {
      const home = await mkdtemp(join(tmpdir(), 'kimi-provider-tui-e2e-'));
      tempHomes.push(home);
      await writeFile(join(home, 'tui.toml'), 'disable_paste_burst = true\n', 'utf8');
      const env = { ...process.env };
      delete env['OPENAI_API_KEY'];
      const command = [
        process.execPath,
        tsxCli,
        '--tsconfig',
        join(appRoot, 'tsconfig.dev.json'),
        '--import',
        rawTextLoader,
        main,
      ];
      const terminal = spawn('python3', ['-u', '-c', PTY_BRIDGE, ...command], {
        cwd: appRoot,
        env: {
          ...env,
          TERM: 'xterm-256color',
          KIMI_CODE_HOME: home,
          KIMI_LOG_LEVEL: 'off',
        },
        stdio: ['pipe', 'pipe', 'pipe'],
      });
      let output = '';
      terminal.stdout.setEncoding('utf8');
      terminal.stderr.setEncoding('utf8');
      terminal.stdout.on('data', (chunk: string) => {
        output += chunk;
      });
      terminal.stderr.on('data', (chunk: string) => {
        output += chunk;
      });

      try {
        await waitForText(terminal, () => output, 'Run /login to connect a provider.');
        await writeCommand(terminal, () => output, '/login openai');
        await waitForText(terminal, () => output, 'Connect to OpenAI');
        await waitForText(terminal, () => output, 'Enter OpenAI API key:');
        await writeLine(terminal, () => output, 'YOUR_API_KEY');
        await waitForText(terminal, () => output, 'Select a model');
        terminal.stdin.write('\r');
        await waitForText(terminal, () => output, 'Connected to OpenAI');
        await writeCommand(terminal, () => output, '/exit');
        await waitForExit(terminal);
      } catch (error) {
        throw new Error(
          `${String(error)}\nTUI output:\n${stripTerminalControls(output).slice(-12_000)}`,
          { cause: error },
        );
      } finally {
        if (terminal.exitCode === null) terminal.kill();
      }

      await expect(readFile(join(home, 'auth.json'), 'utf8')).resolves.toContain(
        '"key": "YOUR_API_KEY"',
      );
      const config = await readFile(join(home, 'config.toml'), 'utf8');
      expect(config).toContain('default_provider = "openai"');
      expect(config).toMatch(/default_model = ".+"/);
    },
    60_000,
  );
});

async function writeLine(
  terminal: ChildProcessWithoutNullStreams,
  read: () => string,
  line: string,
): Promise<void> {
  const beforeInput = read().length;
  terminal.stdin.write(line);
  await waitForCondition(
    terminal,
    () => read().length > beforeInput,
    `TUI to render input for ${line}`,
  );
  terminal.stdin.write('\r');
}

async function writeCommand(
  terminal: ChildProcessWithoutNullStreams,
  read: () => string,
  command: string,
): Promise<void> {
  const beforeInput = read().length;
  terminal.stdin.write(command);
  await waitForCondition(terminal, () => read().length > beforeInput, `TUI to render ${command}`);
  const beforeAutocompleteAccept = read().length;
  terminal.stdin.write('\r');
  await waitForCondition(
    terminal,
    () => read().length > beforeAutocompleteAccept,
    `TUI to accept autocomplete for ${command}`,
  );
  terminal.stdin.write('\r');
}

function waitForText(
  terminal: ChildProcessWithoutNullStreams,
  read: () => string,
  expected: string,
  timeoutMs = 15_000,
): Promise<void> {
  return waitForCondition(
    terminal,
    () => read().includes(expected),
    `TUI text: ${expected}`,
    timeoutMs,
  );
}

function waitForCondition(
  terminal: ChildProcessWithoutNullStreams,
  condition: () => boolean,
  description: string,
  timeoutMs = 15_000,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const check = (): void => {
      if (condition()) {
        cleanup();
        resolve();
      }
    };
    const onError = (error: Error): void => {
      cleanup();
      reject(error);
    };
    const timer = setTimeout(() => {
      cleanup();
      reject(new Error(`Timed out waiting for ${description}`));
    }, timeoutMs);
    const cleanup = (): void => {
      clearTimeout(timer);
      terminal.stdout.off('data', check);
      terminal.stderr.off('data', check);
      terminal.off('error', onError);
    };
    terminal.stdout.on('data', check);
    terminal.stderr.on('data', check);
    terminal.once('error', onError);
    check();
  });
}

function stripTerminalControls(value: string): string {
  return value
    .replaceAll(/\u001B\][^\u0007]*(?:\u0007|\u001B\\)/g, '')
    .replaceAll(/\u001B\[[0-?]*[ -/]*[@-~]/g, '')
    .replaceAll('\r', '');
}

function waitForExit(terminal: ChildProcessWithoutNullStreams, timeoutMs = 15_000): Promise<void> {
  if (terminal.exitCode !== null) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error('Timed out waiting for TUI exit'));
    }, timeoutMs);
    terminal.once('exit', () => {
      clearTimeout(timer);
      resolve();
    });
  });
}
