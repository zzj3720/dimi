/**
 * Dimi telemetry — deliberately a no-op.
 *
 * Dimi does not collect or upload usage telemetry. These functions exist only
 * so existing call sites (CLI bootstrap, harness wiring, crash handlers) keep
 * their shape; every call is a no-op.
 */

import type { TelemetryProperties as TelemetryPropertiesType } from './types';

export type { TelemetryPrimitive, TelemetryProperties } from './types';

export function track(_event: string, _properties: TelemetryPropertiesType = {}): void {}

export interface TelemetryContextIds {
  readonly deviceId?: string | null;
  readonly sessionId?: string | null;
}

export interface TelemetryClient {
  track(event: string, properties?: TelemetryPropertiesType): void;
  withContext(patch: TelemetryContextIds): TelemetryClient;
  setContext(patch: TelemetryContextIds): void;
}

const noopClient: TelemetryClient = {
  track: () => {},
  withContext: () => noopClient,
  setContext: () => {},
};

export function setTelemetryContext(_patch: TelemetryContextIds): void {}

export function withTelemetryContext(_patch: TelemetryContextIds): TelemetryClient {
  return noopClient;
}

export function flushTelemetrySync(): void {}

export async function shutdownTelemetry(
  _options: { readonly timeoutMs?: number } = {},
): Promise<void> {}

export interface TelemetryBootstrapOptions {
  readonly enabled?: boolean;
  readonly homeDir: string;
  readonly deviceId: string;
  readonly sessionId?: string;
  readonly appName: string;
  readonly version: string;
  readonly uiMode?: string;
  readonly model?: string;
  readonly buildSha?: string;
  readonly terminal?: string;
  readonly locale?: string;
  readonly getAccessToken?: () => string | null | Promise<string | null>;
}

export function initializeTelemetry(_options: TelemetryBootstrapOptions): void {}

export type CrashPhase = 'startup' | 'runtime' | 'shutdown';

export function setCrashPhase(_nextPhase: CrashPhase): void {}

export function installCrashHandlers(): () => void {
  return () => {};
}

export { normalizeRemote } from './remote';
