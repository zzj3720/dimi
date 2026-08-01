import { readFile, mkdir, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import * as zlib from 'node:zlib';

import { Command } from 'commander';
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';

import { registerExportCommand } from '#/cli/sub/export';
import { createDimiCodeHostIdentity } from '#/cli/version';
import {
  createDimiHarness,
  flushDiagnosticLogs,
  log,
} from '@dimi-agent/dimi-sdk';

const SESSION_LOG = 'logs/dimi.log';
const GLOBAL_LOG = 'logs/global/dimi.log';
const ENABLED = process.env['DIMI_E2E'] === '1';

const loggingEnv = vi.hoisted(() => {
  const tempRoot = process.env['TMPDIR'] ?? process.env['TEMP'] ?? process.env['TMP'] ?? '/tmp';
  const homeDir = `${tempRoot.replace(/[\\/]$/, '')}/dimi-cli-log-home-${String(process.pid)}`;
  const previousHome = process.env['DIMI_CODE_HOME'];
  const previousLogLevel = process.env['DIMI_LOG_LEVEL'];
  process.env['DIMI_CODE_HOME'] = homeDir;
  process.env['DIMI_LOG_LEVEL'] = 'info';
  return { homeDir, previousHome, previousLogLevel };
});

const homeDir = loggingEnv.homeDir;
let workDir: string;

beforeAll(async () => {
  await rm(homeDir, { recursive: true, force: true });
  await mkdir(homeDir, { recursive: true });
  workDir = await mkdtemp(join(tmpdir(), 'dimi-cli-log-work-'));
});

afterAll(async () => {
  await flushDiagnosticLogs();
  if (loggingEnv.previousHome === undefined) {
    delete process.env['DIMI_CODE_HOME'];
  } else {
    process.env['DIMI_CODE_HOME'] = loggingEnv.previousHome;
  }
  if (loggingEnv.previousLogLevel === undefined) {
    delete process.env['DIMI_LOG_LEVEL'];
  } else {
    process.env['DIMI_LOG_LEVEL'] = loggingEnv.previousLogLevel;
  }
  await rm(homeDir, { recursive: true, force: true });
  await rm(workDir, { recursive: true, force: true });
});

describe.skipIf(!ENABLED)('local logging export e2e', () => {
  it('exports session log and global log by default, and allows skipping global log', async () => {
    const harness = createDimiHarness({
      homeDir,
      identity: createDimiCodeHostIdentity('0.1.1'),
    });
    try {
      const session = await harness.createSession({
        id: 'ses_cli_logging_export',
        workDir,
        mcpServers: {
          missing: {
            transport: 'stdio',
            command: '/definitely/not/a/real/mcp-executable',
          },
        },
      });
      await session.listMcpServers();
      log.warn('cli global marker');
      await flushDiagnosticLogs();
      await session.close();

      const defaultZip = join(workDir, 'default.zip');
      await runDimiExport([session.id, '-o', defaultZip]);
      const defaultEntries = readZipEntries(await readFile(defaultZip));
      expect(defaultEntries.has(SESSION_LOG)).toBe(true);
      expect(defaultEntries.has(GLOBAL_LOG)).toBe(true);
      expect(defaultEntries.get(SESSION_LOG)!.toString('utf-8')).toContain('mcp server unavailable');
      expect(defaultEntries.get(GLOBAL_LOG)!.toString('utf-8')).toContain('cli global marker');
      const defaultManifest = JSON.parse(
        defaultEntries.get('manifest.json')!.toString('utf-8'),
      ) as Record<string, unknown>;
      expect(defaultManifest['sessionLogPath']).toBe(SESSION_LOG);
      expect(defaultManifest['globalLogPath']).toBe(GLOBAL_LOG);

      const noGlobalZip = join(workDir, 'no-global.zip');
      await runDimiExport([session.id, '-o', noGlobalZip, '--no-include-global-log']);
      const noGlobalEntries = readZipEntries(await readFile(noGlobalZip));
      expect(noGlobalEntries.has(GLOBAL_LOG)).toBe(false);
      const noGlobalManifest = JSON.parse(
        noGlobalEntries.get('manifest.json')!.toString('utf-8'),
      ) as Record<string, unknown>;
      expect(noGlobalManifest['globalLogPath']).toBeUndefined();
    } finally {
      await harness.close().catch(() => {});
    }
  }, 15_000);
});

async function runDimiExport(args: string[]): Promise<void> {
  const program = new Command('dimi');
  const stdout: string[] = [];
  const stderr: string[] = [];
  registerExportCommand(program, {
    stdout: {
      write: (chunk) => {
        stdout.push(chunk);
        return true;
      },
    },
    stderr: {
      write: (chunk) => {
        stderr.push(chunk);
        return true;
      },
    },
    exit: (code: number): never => {
      throw new Error(`dimi export exited ${code}: ${stderr.join('')}`);
    },
  });
  await program.parseAsync(['node', 'dimi', 'export', ...args]);
}

function readZipEntries(buf: Buffer): Map<string, Buffer> {
  let eocd = -1;
  for (let i = buf.length - 22; i >= Math.max(0, buf.length - 65_557); i--) {
    if (buf.readUInt32LE(i) === 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd === -1) throw new Error('zip eocd not found');
  const entryCount = buf.readUInt16LE(eocd + 10);
  const cdOffset = buf.readUInt32LE(eocd + 16);
  const entries = new Map<string, Buffer>();
  let pos = cdOffset;
  for (let i = 0; i < entryCount; i++) {
    if (buf.readUInt32LE(pos) !== 0x02014b50) throw new Error('bad cd entry');
    const method = buf.readUInt16LE(pos + 10);
    const compressedSize = buf.readUInt32LE(pos + 20);
    const fnameLen = buf.readUInt16LE(pos + 28);
    const extraLen = buf.readUInt16LE(pos + 30);
    const commentLen = buf.readUInt16LE(pos + 32);
    const lfhOffset = buf.readUInt32LE(pos + 42);
    const filename = buf.toString('utf8', pos + 46, pos + 46 + fnameLen);
    if (buf.readUInt32LE(lfhOffset) !== 0x04034b50) throw new Error('bad lfh');
    const lfhFnameLen = buf.readUInt16LE(lfhOffset + 26);
    const lfhExtraLen = buf.readUInt16LE(lfhOffset + 28);
    const dataStart = lfhOffset + 30 + lfhFnameLen + lfhExtraLen;
    const compressed = buf.subarray(dataStart, dataStart + compressedSize);
    const data = method === 0 ? compressed : method === 8 ? zlib.inflateRawSync(compressed) : null;
    if (data === null) throw new Error('unsupported compression');
    entries.set(filename, data);
    pos += 46 + fnameLen + extraLen + commentLen;
  }
  return entries;
}
