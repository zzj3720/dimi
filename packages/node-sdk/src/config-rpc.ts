import { ConfigRegistry, resolveConfigPath } from '@dimi-agent/agent-core-v2';
import { transformTomlData } from '@dimi-agent/agent-core-v2/app/config/toml';
import { parse as parseToml } from 'smol-toml';
import { z } from 'zod';

import { ErrorCodes, DimiError } from '#/errors';

export type DimiConfigValidationPathSegment = string | number;

export interface DimiConfigValidationIssue {
  readonly path: readonly DimiConfigValidationPathSegment[];
  readonly message: string;
}

export interface ResolveDimiConfigPathInput {
  readonly homeDir?: string | undefined;
  readonly configPath?: string | undefined;
}

export interface ValidateDimiConfigTomlInput {
  readonly text: string;
  readonly filePath?: string | undefined;
}

export interface DimiConfigRpc {
  resolveConfigPath(input?: ResolveDimiConfigPathInput): Promise<string>;
  validateConfigToml(input: ValidateDimiConfigTomlInput): Promise<void>;
}

export class DimiConfigRpcClient implements DimiConfigRpc {
  async resolveConfigPath(input: ResolveDimiConfigPathInput = {}): Promise<string> {
    return resolveConfigPath(input);
  }

  async validateConfigToml(input: ValidateDimiConfigTomlInput): Promise<void> {
    try {
      const registry = new ConfigRegistry();
      const config = transformTomlData(parseToml(input.text), registry);
      for (const [domain, value] of Object.entries(config)) {
        registry.validate(domain, value);
      }
    } catch (error) {
      const issues = extractValidationIssues(error);
      if (issues !== undefined) throw toConfigValidationError(error, issues);
      throw error;
    }
  }
}

export function createDimiConfigRpc(): DimiConfigRpc {
  return new DimiConfigRpcClient();
}

function toConfigValidationError(
  error: unknown,
  validationIssues: readonly DimiConfigValidationIssue[],
): DimiError {
  const details =
    error instanceof DimiError && error.details !== undefined
      ? { ...error.details, validationIssues }
      : { validationIssues };

  if (error instanceof DimiError) {
    return new DimiError(error.code, error.message, { details });
  }

  const message = error instanceof Error ? error.message : String(error);
  return new DimiError(ErrorCodes.CONFIG_INVALID, message, { details });
}

function extractValidationIssues(error: unknown): readonly DimiConfigValidationIssue[] | undefined {
  const zodError = findZodError(error);
  if (zodError === undefined) return undefined;
  return zodError.issues.map((issue) => ({
    path: issue.path.map((segment) =>
      typeof segment === 'number' ? segment : String(segment),
    ),
    message: issue.message,
  }));
}

function findZodError(error: unknown): z.ZodError | undefined {
  if (error instanceof z.ZodError) return error;
  if (error instanceof Error && error.cause instanceof z.ZodError) return error.cause;
  return undefined;
}
