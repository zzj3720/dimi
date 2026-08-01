import { describe, expect, it, vi } from 'vitest';

import {
  flushTelemetrySync,
  initializeTelemetry,
  installCrashHandlers,
  setCrashPhase,
  setTelemetryContext,
  shutdownTelemetry,
  track,
  withTelemetryContext,
} from '../src';

// Dimi does not collect telemetry: every API must be a safe no-op and must not
// throw or touch the network. These tests pin that contract.
describe('telemetry no-op contract', () => {
  it('track is a no-op', () => {
    expect(() => track('some_event', { value: 1 })).not.toThrow();
  });

  it('initializeTelemetry is a no-op', () => {
    expect(() =>
      initializeTelemetry({
        homeDir: '/tmp/dimi-home',
        deviceId: 'device-1',
        appName: 'dimi',
        version: '1.0.0',
      }),
    ).not.toThrow();
  });

  it('context helpers are no-ops', () => {
    expect(() => setTelemetryContext({ deviceId: 'device-1' })).not.toThrow();
    expect(() => setCrashPhase('runtime')).not.toThrow();
  });

  it('withTelemetryContext returns a no-op client', () => {
    const client = withTelemetryContext({ sessionId: 'session-1' });
    expect(() => client.track('event')).not.toThrow();
    const nested = client.withContext({ deviceId: 'device-1' });
    expect(() => nested.track('event', { x: 1 })).not.toThrow();
    expect(() => client.setContext({})).not.toThrow();
  });

  it('flush and shutdown resolve without doing anything', async () => {
    await expect(shutdownTelemetry()).resolves.toBeUndefined();
    expect(() => flushTelemetrySync()).not.toThrow();
  });

  it('installCrashHandlers returns an uninstall no-op', () => {
    const uninstall = installCrashHandlers();
    expect(() => uninstall()).not.toThrow();
  });

  it('does not perform network calls', async () => {
    const fetchSpy = vi.spyOn(globalThis, 'fetch').mockRejectedValue(new Error('must not fetch'));
    track('event');
    await shutdownTelemetry();
    flushTelemetrySync();
    expect(fetchSpy).not.toHaveBeenCalled();
    fetchSpy.mockRestore();
  });
});
