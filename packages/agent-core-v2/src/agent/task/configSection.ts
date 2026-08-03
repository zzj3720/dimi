/**
 * `task` domain (L5) — task config-section schema and env bindings.
 *
 * Owns the `[task]` configuration section (task limits and lifecycle tuning).
 * `maxRunningTasks` also accepts the
 * `DIMI_CODE_BACKGROUND_MAX_RUNNING_TASKS` environment override
 * (applied live by the config env overlay; while a field's env var is set,
 * `stripEnvBoundFields` restores its env-free raw value before persistence, so
 * env values never leak into `config.toml`). Also owns the
 * `dimi -p` print-mode background policy (`printBackgroundMode` /
 * `printWaitCeilingS` / `printMaxTurns`), resolved by
 * `resolvePrintBackgroundMode`. Self-registered
 * at module load via `registerConfigSection`, so the `config` domain never
 * imports this domain's types.
 */

import { z } from 'zod';

import {
  type EnvBindings,
  envBindings,
  stripEnvBoundFields,
  type IConfigService,
} from '#/app/config/config';
import { registerConfigSection } from '#/app/config/configSectionContributions';

export const TASK_SECTION = 'task';

/** Default TaskStop SIGTERM grace (the `task` section's `killGracePeriodMs`
 *  fallback). **MUST stay equal to the Rust constant**
 *  `DEFAULT_KILL_GRACE_MS` in `crates/dimi-engine/src/tool.rs` — the two are
 *  only comment-linked; if one side changes, change the other. The
 *  Rust-engine runner wires it into the engine turn input so a deployment
 *  that raises the grace is honored end-to-end. */
export const DEFAULT_KILL_GRACE_MS = 5_000;

export const PrintBackgroundModeSchema = z.enum(['exit', 'drain', 'steer']);

export type PrintBackgroundMode = z.infer<typeof PrintBackgroundModeSchema>;

export const AgentTaskConfigSchema = z.object({
  maxRunningTasks: z.number().int().min(1).optional(),
  bashAutoBackgroundOnTimeout: z.boolean().optional(),
  bashTaskTimeoutS: z.number().int().min(0).optional(),
  killGracePeriodMs: z.number().int().min(0).optional(),
  printWaitCeilingS: z.number().int().min(1).optional(),
  printBackgroundMode: PrintBackgroundModeSchema.optional(),
  printMaxTurns: z.number().int().min(1).optional(),
});

export type AgentTaskConfig = z.infer<typeof AgentTaskConfigSchema>;

export function resolveAgentTaskConfig(config: IConfigService): AgentTaskConfig | undefined {
  return config.get<AgentTaskConfig | undefined>(TASK_SECTION);
}

export function resolvePrintBackgroundMode(config: IConfigService): PrintBackgroundMode {
  return resolveAgentTaskConfig(config)?.printBackgroundMode ?? 'steer';
}

export const MAX_RUNNING_TASKS_ENV = 'DIMI_CODE_BACKGROUND_MAX_RUNNING_TASKS';

function parsePositiveInt(raw: string): number | undefined {
  const value = raw.trim();
  if (value.length === 0 || !/^\d+$/.test(value)) return undefined;
  const parsed = Number(value);
  return Number.isInteger(parsed) && parsed > 0 ? parsed : undefined;
}

export const taskEnvBindings: EnvBindings<AgentTaskConfig> = envBindings(AgentTaskConfigSchema, {
  maxRunningTasks: { env: MAX_RUNNING_TASKS_ENV, parse: parsePositiveInt },
});

export const stripTaskEnv = stripEnvBoundFields(taskEnvBindings);

registerConfigSection(TASK_SECTION, AgentTaskConfigSchema, {
  env: taskEnvBindings,
  stripEnv: stripTaskEnv,
});
