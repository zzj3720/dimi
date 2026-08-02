/**
 * Scenario: a user starts and connects a provider through the production interactive TUI.
 * Responsibilities: tolerate unavailable environment defaults, accept /login, collect a secret,
 * select a runtime model, and persist both.
 * Wiring: production main entry with an isolated home and documented paste-burst setting.
 * Run: vp exec vitest run test/e2e/interactive-provider-login.e2e.test.ts
 */
import { createRequire } from 'node:module';
import { execFileSync, spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

const require = createRequire(import.meta.url);
const appRoot = join(import.meta.dirname, '..', '..');
const repoRoot = join(appRoot, '..', '..');
const tsxCli = require.resolve('tsx/cli');
const rawTextLoader = join(appRoot, '..', '..', 'build', 'register-raw-text-loader.mjs');
const main = join(appRoot, 'src', 'main.ts');
const tempHomes: string[] = [];

/** The dev-wrapper scenarios need the local `vp` launcher; skip without it. */
function hasVpLauncher(): boolean {
  try {
    execFileSync('which', ['vp'], { stdio: 'pipe' });
    return true;
  } catch {
    return false;
  }
}
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
  // TUI teardown flushes its state files asynchronously on macOS. Retrying an
  // ENOTEMPTY removal is deliberate recovery for that verified close race;
  // every spawned terminal is awaited below rather than abandoned.
  for (const home of tempHomes.splice(0)) {
    await rm(home, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
  }
});

describe('interactive provider login', () => {
  it.skipIf(process.platform === 'win32' || !hasVpLauncher())(
    'restarts the actual dev wrapper with Grok 4.5 thinking and context metadata',
    async () => {
      const home = await mkdtemp(join(tmpdir(), 'dimi-grok-dev-wrapper-e2e-'));
      tempHomes.push(home);
      const requests: unknown[] = [];
      const server = createServer((request, response) => {
        void (async () => {
          if (request.url !== '/v1/responses') {
            response.writeHead(404).end();
            return;
          }
          const chunks: Buffer[] = [];
          for await (const chunk of request) chunks.push(Buffer.from(chunk));
          requests.push(JSON.parse(Buffer.concat(chunks).toString('utf8')));
          response.writeHead(200, { 'content-type': 'text/event-stream' });
          response.end([
            'data: {"type":"response.output_text.delta","delta":"wrapper grok reply"}',
            '',
            'data: {"type":"response.completed","response":{"id":"resp-wrapper","model":"grok-4.5","status":"completed","usage":{"input_tokens":3,"output_tokens":2}}}',
            '',
          ].join('\n'));
        })().catch((error: unknown) => response.destroy(error instanceof Error ? error : new Error(String(error))));
      });
      await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
      const address = server.address();
      if (address === null || typeof address === 'string') throw new Error('fake provider did not bind a TCP port');
      const baseUrl = `http://127.0.0.1:${String(address.port)}/v1`;

      try {
        await writeFile(join(home, 'models.json'), JSON.stringify({
          providers: { xai: { baseUrl } },
        }), 'utf8');
        await writeFile(join(home, 'config.toml'), [
          'default_provider = "xai"',
          'default_model = "grok-4.5"',
          '',
          '[thinking]',
          'enabled = true',
          'effort = "high"',
          '',
        ].join('\n'), 'utf8');
        await writeFile(join(home, 'tui.toml'), 'disable_paste_burst = true\n', 'utf8');

        const first = startDevTui(home, { XAI_API_KEY: 'test-xai-key' });
        try {
          await waitForText(first.terminal, first.read, 'Grok 4.5');
          await writeCommand(first.terminal, first.read, '/usage');
          await waitForText(first.terminal, first.read, 'Context window');
          expect(stripTerminalControls(first.read())).toContain('(0 / 488k)');
          await writeLine(first.terminal, first.read, 'hello from dev wrapper');
          await waitForText(first.terminal, first.read, 'wrapper grok reply');
          await writeCommand(first.terminal, first.read, '/exit');
          await waitForExit(first.terminal);
        } catch (error) {
          throw new Error(
            `${String(error)}\nFirst dev-wrapper TUI output:\n${stripTerminalControls(first.read()).slice(-12_000)}`,
            { cause: error },
          );
        } finally {
          if (first.terminal.exitCode === null) first.terminal.kill();
          await waitForExit(first.terminal);
        }

        const restarted = startDevTui(home, { XAI_API_KEY: 'test-xai-key' });
        try {
          await waitForText(restarted.terminal, restarted.read, 'Grok 4.5');
          await writeCommand(restarted.terminal, restarted.read, '/usage');
          await waitForText(restarted.terminal, restarted.read, 'Context window');
          expect(stripTerminalControls(restarted.read())).toContain('(0 / 488k)');
          await writeCommand(restarted.terminal, restarted.read, '/exit');
          await waitForExit(restarted.terminal);
        } catch (error) {
          throw new Error(
            `${String(error)}\nRestarted dev-wrapper TUI output:\n${stripTerminalControls(restarted.read()).slice(-12_000)}`,
            { cause: error },
          );
        } finally {
          if (restarted.terminal.exitCode === null) restarted.terminal.kill();
          await waitForExit(restarted.terminal);
        }

        expect(requests).toHaveLength(1);
        expect(requests[0]).toMatchObject({
          model: 'grok-4.5',
          reasoning: { effort: 'high', summary: 'auto' },
        });
      } finally {
        await new Promise<void>((resolve, reject) => server.close((error) => error === undefined ? resolve() : reject(error)));
      }
    },
    120_000,
  );

  it.skipIf(process.platform === 'win32')(
    'starts the production TUI without consuming a stale legacy update cache or remote banner',
    async () => {
      const home = await mkdtemp(join(tmpdir(), 'dimi-update-channel-tui-e2e-'));
      tempHomes.push(home);
      await mkdir(join(home, 'updates'), { recursive: true });
      await writeFile(join(home, 'updates', 'latest.json'), JSON.stringify({
        source: 'cdn',
        checkedAt: '2026-07-01T00:00:00.000Z',
        latest: '0.30.0',
        manifest: null,
      }), 'utf8');
      await writeFile(join(home, 'tui.toml'), 'disable_paste_burst = true\n', 'utf8');
      const { terminal, read } = startTui(home);

      try {
        await waitForText(terminal, read, 'Run /login to connect a provider.');
        await writeCommand(terminal, read, '/exit');
        await waitForExit(terminal);
        expect(stripTerminalControls(read())).not.toMatch(/new version available|updated to v0\.30|tip[s]? banner/iu);
      } catch (error) {
        throw new Error(
          `${String(error)}\nTUI output:\n${stripTerminalControls(read()).slice(-12_000)}`,
          { cause: error },
        );
      } finally {
        if (terminal.exitCode === null) terminal.kill();
        await waitForExit(terminal);
      }
    },
    60_000,
  );

  it.skipIf(process.platform === 'win32')(
    'keeps /login and /model usable while showing a malformed models.json diagnostic',
    async () => {
      const home = await mkdtemp(join(tmpdir(), 'dimi-malformed-models-tui-e2e-'));
      tempHomes.push(home);
      const malformed = '{ "providers": ';
      await writeFile(join(home, 'models.json'), malformed, 'utf8');
      await writeFile(join(home, 'tui.toml'), 'disable_paste_burst = true\n', 'utf8');
      const modelTui = startTui(home);

      try {
        await waitForText(modelTui.terminal, modelTui.read, 'Run /login to connect a provider.');
        await writeCommand(modelTui.terminal, modelTui.read, '/model');
        await waitForText(modelTui.terminal, modelTui.read, 'Failed to load models.json');
        // The empty picker owns the editor, so end this instance as a real
        // restart would; a second production instance proves /login remains
        // reachable without accidentally turning `/exit` into picker search.
        modelTui.terminal.kill();
        await waitForExit(modelTui.terminal);

        const loginTui = startTui(home);
        try {
          await waitForText(loginTui.terminal, loginTui.read, 'Run /login to connect a provider.');
          await writeCommand(loginTui.terminal, loginTui.read, '/login openai');
          await waitForText(loginTui.terminal, loginTui.read, 'Enter OpenAI API key:');
        } finally {
          if (loginTui.terminal.exitCode === null) loginTui.terminal.kill();
          await waitForExit(loginTui.terminal);
        }
      } catch (error) {
        throw new Error(
          `${String(error)}\nTUI output:\n${stripTerminalControls(modelTui.read()).slice(-12_000)}`,
          { cause: error },
        );
      } finally {
        if (modelTui.terminal.exitCode === null) modelTui.terminal.kill();
        await waitForExit(modelTui.terminal);
      }

      await expect(readFile(join(home, 'models.json'), 'utf8')).resolves.toBe(malformed);
    },
    60_000,
  );

  it.skipIf(process.platform === 'win32')(
    'persists the selected OpenAI model after a production TUI login',
    async () => {
      const secret = 'TUI_PROVIDER_SECRET_SENTINEL_9f4d';
      const home = await mkdtemp(join(tmpdir(), 'dimi-provider-tui-e2e-'));
      tempHomes.push(home);
      await writeFile(join(home, 'tui.toml'), 'disable_paste_burst = true\n', 'utf8');
      const { terminal, read } = startTui(home);

      try {
        await waitForText(terminal, read, 'Run /login to connect a provider.');
        await writeCommand(terminal, read, '/login openai');
        await waitForText(terminal, read, 'Connect to OpenAI');
        await waitForText(terminal, read, 'Enter OpenAI API key:');
        await writeLine(terminal, read, secret);
        await waitForText(terminal, read, 'Select a model');
        expect(stripTerminalControls(read())).not.toContain(secret);
        terminal.stdin.write('\r');
        await waitForText(terminal, read, 'Connected to OpenAI');
        await writeCommand(terminal, read, '/exit');
        await waitForExit(terminal);
      } catch (error) {
        throw new Error(
          `${String(error)}\nTUI output:\n${stripTerminalControls(read()).slice(-12_000)}`,
          { cause: error },
        );
      } finally {
        if (terminal.exitCode === null) terminal.kill();
        await waitForExit(terminal);
      }

      await expect(readFile(join(home, 'auth.json'), 'utf8')).resolves.toContain(
        `"key": "${secret}"`,
      );
      const config = await readFile(join(home, 'config.toml'), 'utf8');
      expect(config).toContain('default_provider = "openai"');
      expect(config).toMatch(/default_model = ".+"/);
    },
    60_000,
  );

  it.skipIf(process.platform === 'win32')(
    'starts the production TUI when an environment default is absent from the dynamic catalog',
    async () => {
      const home = await mkdtemp(join(tmpdir(), 'dimi-retired-model-tui-e2e-'));
      tempHomes.push(home);
      await writeFile(join(home, 'tui.toml'), 'disable_paste_burst = true\n', 'utf8');
      const { terminal, read } = startTui(home, { DIMI_MODEL_NAME: 'retired-model' });

      try {
        await waitForText(terminal, read, 'Run /login to connect a provider.');
        expect(stripTerminalControls(read())).toContain('Run /login to connect a provider.');
        await writeCommand(terminal, read, '/exit');
        await waitForExit(terminal);
      } catch (error) {
        throw new Error(
          `${String(error)}\nTUI output:\n${stripTerminalControls(read()).slice(-12_000)}`,
          { cause: error },
        );
      } finally {
        if (terminal.exitCode === null) terminal.kill();
        await waitForExit(terminal);
      }
    },
    60_000,
  );

  it.skipIf(process.platform === 'win32')(
    'persists /provider add form data and exposes its model after an interactive restart',
    async () => {
      const home = await mkdtemp(join(tmpdir(), 'dimi-custom-provider-tui-e2e-'));
      tempHomes.push(home);
      await writeFile(join(home, 'tui.toml'), 'disable_paste_burst = true\n', 'utf8');
      const first = startTui(home);

      try {
        await waitForText(first.terminal, first.read, 'Run /login to connect a provider.');
        await writeCommand(first.terminal, first.read, '/provider add');
        await waitForText(first.terminal, first.read, 'Add custom provider');
        for (const value of [
          'tui-local',
          'TUI Local',
          'http://127.0.0.1:9876/v1',
          'openai-completions',
          'tui-chat',
          '131072',
          '8192',
          'text,image',
          'low,high',
        ]) {
          await writeLine(first.terminal, first.read, value);
        }
        await waitForText(first.terminal, first.read, 'Added TUI Local.');

        await writeCommand(first.terminal, first.read, '/login tui-local');
        await waitForText(first.terminal, first.read, 'Enter TUI Local API key:');
        await writeLine(first.terminal, first.read, 'tui-local-key');
        await waitForText(first.terminal, first.read, 'Select a model');
        first.terminal.stdin.write('\r');
        await waitForText(first.terminal, first.read, 'Connected to TUI Local');
        await writeCommand(first.terminal, first.read, '/exit');
        await waitForExit(first.terminal);
      } catch (error) {
        throw new Error(
          `${String(error)}\nCustom provider TUI output:\n${stripTerminalControls(first.read()).slice(-12_000)}`,
          { cause: error },
        );
      } finally {
        if (first.terminal.exitCode === null) first.terminal.kill();
        await waitForExit(first.terminal);
      }

      await expect(readFile(join(home, 'models.json'), 'utf8')).resolves.toContain('"tui-local"');
      await expect(readFile(join(home, 'models.json'), 'utf8')).resolves.toContain('"contextWindow": 131072');

      const restarted = startTui(home);
      try {
        await waitForPlainPattern(restarted.terminal, restarted.read, /Model:\s+tui-chat/u);
        const beforePicker = restarted.read().length;
        await writeCommand(restarted.terminal, restarted.read, '/model');
        await waitForPlainTextAfter(restarted.terminal, restarted.read, beforePicker, 'Select a model');
        // The picker rows render after the title on slow CI PTYs; wait for the
        // custom provider row instead of asserting on a race-prone snapshot.
        await waitForPlainTextAfter(restarted.terminal, restarted.read, beforePicker, 'tui-local');
        const pickerOutput = stripTerminalControls(restarted.read().slice(beforePicker));
        expect(pickerOutput).toContain('tui-local');
        expect(pickerOutput).toContain('tui-chat');
        expect(pickerOutput).toContain('← current');

        const beforeCancel = restarted.read().length;
        restarted.terminal.stdin.write('\u001b');
        await waitForPlainPatternAfter(
          restarted.terminal,
          restarted.read,
          beforeCancel,
          />\s{2,}│/u,
        );
        await writeCommand(restarted.terminal, restarted.read, '/exit');
        await waitForExit(restarted.terminal);
      } catch (error) {
        throw new Error(
          `${String(error)}\nRestarted custom-provider TUI output:\n${stripTerminalControls(restarted.read()).slice(-12_000)}`,
          { cause: error },
        );
      } finally {
        if (restarted.terminal.exitCode === null) restarted.terminal.kill();
        await waitForExit(restarted.terminal);
      }
    },
    90_000,
  );
});

function startTui(
  home: string,
  envOverrides: Readonly<Record<string, string>> = {},
): { terminal: ChildProcessWithoutNullStreams; read: () => string } {
  const env = sanitizedProviderEnv(envOverrides);
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
      DIMI_CODE_HOME: home,
      DIMI_LOG_LEVEL: 'off',
      DIMI_LEGACY: '1',
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
  return { terminal, read: () => output };
}

/** Runs the same `vp run dev:cli` wrapper the developer invokes locally. */
function startDevTui(
  home: string,
  envOverrides: Readonly<Record<string, string>> = {},
): { terminal: ChildProcessWithoutNullStreams; read: () => string } {
  const env = sanitizedProviderEnv(envOverrides);
  const terminal = spawn('python3', ['-u', '-c', PTY_BRIDGE, 'vp', 'run', 'dev:cli'], {
    cwd: repoRoot,
    env: {
      ...env,
      TERM: 'xterm-256color',
      DIMI_CODE_HOME: home,
      DIMI_LOG_LEVEL: 'off',
      DIMI_LEGACY: '1',
    },
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  let output = '';
  terminal.stdout.setEncoding('utf8');
  terminal.stderr.setEncoding('utf8');
  terminal.stdout.on('data', (chunk: string) => { output += chunk; });
  terminal.stderr.on('data', (chunk: string) => { output += chunk; });
  return { terminal, read: () => output };
}

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

function waitForPlainText(
  terminal: ChildProcessWithoutNullStreams,
  read: () => string,
  expected: string,
  timeoutMs = 15_000,
): Promise<void> {
  return waitForCondition(
    terminal,
    () => stripTerminalControls(read()).includes(expected),
    `plain TUI text: ${expected}`,
    timeoutMs,
  );
}

function waitForPlainTextAfter(
  terminal: ChildProcessWithoutNullStreams,
  read: () => string,
  offset: number,
  expected: string,
  timeoutMs = 15_000,
): Promise<void> {
  return waitForCondition(
    terminal,
    () => stripTerminalControls(read().slice(offset)).includes(expected),
    `plain TUI text after command: ${expected}`,
    timeoutMs,
  );
}

function waitForPlainPattern(
  terminal: ChildProcessWithoutNullStreams,
  read: () => string,
  expected: RegExp,
  timeoutMs = 15_000,
): Promise<void> {
  return waitForCondition(
    terminal,
    () => expected.test(stripTerminalControls(read())),
    `plain TUI pattern: ${String(expected)}`,
    timeoutMs,
  );
}

function waitForPlainPatternAfter(
  terminal: ChildProcessWithoutNullStreams,
  read: () => string,
  offset: number,
  expected: RegExp,
  timeoutMs = 15_000,
): Promise<void> {
  return waitForCondition(
    terminal,
    () => expected.test(stripTerminalControls(read().slice(offset))),
    `plain TUI pattern after command: ${String(expected)}`,
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
  if (terminal.exitCode !== null || terminal.signalCode !== null) return Promise.resolve();
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

/** Interactive scenarios must not inherit an unrelated logged-in provider from the developer shell. */
function sanitizedProviderEnv(overrides: Readonly<Record<string, string>> = {}): NodeJS.ProcessEnv {
  const env = { ...process.env };
  for (const name of [
    'DIMI_MODEL_NAME', 'DIMI_MODEL_PROVIDER', 'ANT_LING_API_KEY', 'ANTHROPIC_API_KEY', 'ANTHROPIC_AUTH_TOKEN',
    'ANTHROPIC_OAUTH_TOKEN', 'AZURE_OPENAI_API_KEY', 'CEREBRAS_API_KEY', 'CLOUDFLARE_API_KEY',
    'COPILOT_GITHUB_TOKEN', 'DEEPSEEK_API_KEY', 'FIREWORKS_API_KEY', 'GEMINI_API_KEY', 'GOOGLE_CLOUD_API_KEY',
    'GOOGLE_CLOUD_PROJECT', 'GOOGLE_CLOUD_LOCATION', 'GOOGLE_APPLICATION_CREDENTIALS', 'GCLOUD_PROJECT',
    'GROQ_API_KEY', 'HF_TOKEN', 'DIMI_API_KEY', 'MINIMAX_API_KEY', 'MINIMAX_CN_API_KEY', 'MISTRAL_API_KEY',
    'MOONSHOT_API_KEY', 'NVIDIA_API_KEY', 'OPENAI_API_KEY', 'OPENCODE_API_KEY', 'OPENROUTER_API_KEY',
    'QWEN_TOKEN_PLAN_API_KEY', 'QWEN_TOKEN_PLAN_CN_API_KEY', 'RADIUS_API_KEY', 'TOGETHER_API_KEY',
    'AI_GATEWAY_API_KEY', 'XAI_API_KEY', 'XIAOMI_API_KEY', 'XIAOMI_TOKEN_PLAN_AMS_API_KEY',
    'XIAOMI_TOKEN_PLAN_CN_API_KEY', 'XIAOMI_TOKEN_PLAN_SGP_API_KEY', 'ZAI_API_KEY', 'ZAI_CODING_CN_API_KEY',
    'AWS_PROFILE', 'AWS_REGION', 'AWS_DEFAULT_REGION',
  ]) delete env[name];
  return { ...env, ...overrides };
}
