import { ConfigRegistry, resolveConfigPath } from '@moonshot-ai/agent-core-v2';
import { transformTomlData } from '@moonshot-ai/agent-core-v2/app/config/toml';
import { parse as parseToml } from 'smol-toml';
import { z } from 'zod';

import { ErrorCodes, KimiError } from '#/errors';

export type KimiConfigValidationPathSegment = string | number;

export interface KimiConfigValidationIssue {
  readonly path: readonly KimiConfigValidationPathSegment[];
  readonly message: string;
}

export interface ResolveKimiConfigPathInput {
  readonly homeDir?: string | undefined;
  readonly configPath?: string | undefined;
}

export interface ValidateKimiConfigTomlInput {
  readonly text: string;
  readonly filePath?: string | undefined;
}

export interface KimiConfigRpc {
  resolveConfigPath(input?: ResolveKimiConfigPathInput): Promise<string>;
  validateConfigToml(input: ValidateKimiConfigTomlInput): Promise<void>;
}

export class KimiConfigRpcClient implements KimiConfigRpc {
  async resolveConfigPath(input: ResolveKimiConfigPathInput = {}): Promise<string> {
    return resolveConfigPath(input);
  }

  async validateConfigToml(input: ValidateKimiConfigTomlInput): Promise<void> {
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

export function createKimiConfigRpc(): KimiConfigRpc {
  return new KimiConfigRpcClient();
}

function toConfigValidationError(
  error: unknown,
  validationIssues: readonly KimiConfigValidationIssue[],
): KimiError {
  const details =
    error instanceof KimiError && error.details !== undefined
      ? { ...error.details, validationIssues }
      : { validationIssues };

  if (error instanceof KimiError) {
    return new KimiError(error.code, error.message, { details });
  }

  const message = error instanceof Error ? error.message : String(error);
  return new KimiError(ErrorCodes.CONFIG_INVALID, message, { details });
}

function extractValidationIssues(error: unknown): readonly KimiConfigValidationIssue[] | undefined {
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
