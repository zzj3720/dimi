/**
 * Server telemetry bootstrap — deliberately a no-op.
 *
 * Dimi does not collect or upload usage telemetry, so nothing is attached to
 * the App-scoped `ITelemetryService`. The function exists only to preserve the
 * start.ts wiring shape; it returns an empty handle and never touches the
 * network.
 */

export interface ServerTelemetry {
  readonly appender?: undefined;
  readonly registration?: undefined;
}

export async function initializeServerTelemetry(
  _core: unknown,
  _homeDir: string,
): Promise<ServerTelemetry> {
  return {};
}

export async function shutdownServerTelemetry(
  _telemetry: ServerTelemetry,
  _deadlineMs = 0,
): Promise<void> {}
