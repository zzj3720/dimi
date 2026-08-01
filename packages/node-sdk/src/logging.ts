import {
  BoundLogger,
  createFileLogWriter,
  resolveDimiHome,
  resolveLoggingConfig,
  type ILogger,
  type LogContext,
  type LogLevel,
  type LogPayload,
} from '@moonshot-ai/agent-core-v2';

const config = resolveLoggingConfig({ homeDir: resolveDimiHome(), env: process.env });
const writer = createFileLogWriter({
  path: config.globalLogPath,
  maxBytes: config.globalMaxBytes,
  files: config.globalFiles,
});
const logger = new BoundLogger(writer, { level: config.level });

export const log: ILogger = logger;

export function flushDiagnosticLogs(): Promise<void> {
  return writer.flush();
}

export function flushDiagnosticLogsSync(): void {
  writer.flushSync();
}

export type Logger = ILogger;
export type { LogContext, LogLevel, LogPayload };
