import { hostname } from 'node:os';
import { join } from 'node:path';

import {
  getLiveServerInstance,
  startServer,
  type RunningServer,
} from '@moonshot-ai/kap-server';
import {
  startRemoteBridge,
  type RemoteBridgeStatus,
  type RunningRemoteBridge,
} from '@k-3720/remote/bridge';

import { getVersion } from './cli/version';
import { getDataDir } from './utils/paths';
import {
  DEFAULT_SERVER_HOST,
  DEFAULT_SERVER_PORT,
  normalizeServerOrigin,
  resolveServerToken,
  serverOrigin,
} from './cli/sub/web/shared';

export interface StartRemoteAccessOptions {
  readonly relayUrl?: string;
  readonly localOrigin?: string;
  readonly runtimeName?: string;
  readonly onStatus?: (status: RemoteBridgeStatus) => void;
}

export interface RunningRemoteAccess {
  readonly runtimeId: string;
  createPairingUri(): string;
  close(): Promise<void>;
}

export async function startRemoteAccess(
  options: StartRemoteAccessOptions = {},
): Promise<RunningRemoteAccess> {
  const homeDir = getDataDir();
  const local = await resolveLocalServer(homeDir, options.localOrigin);
  let bridge: RunningRemoteBridge;
  try {
    bridge = await startRemoteBridge({
      relayUrl: options.relayUrl ?? process.env['AGENT_RELAY_URL'] ?? 'wss://relay.k.3720.org',
      localOrigin: local.origin,
      localToken: local.token,
      runtimeName: options.runtimeName ?? hostname(),
      statePath: join(homeDir, 'remote', 'bridge.json'),
      onStatus: options.onStatus,
    });
  } catch (error) {
    await local.owned?.close();
    throw error;
  }

  return {
    runtimeId: bridge.runtimeId,
    createPairingUri: () => bridge.createPairingUri(),
    close: async () => {
      try {
        await bridge.close();
      } finally {
        await local.owned?.close();
      }
    },
  };
}

async function resolveLocalServer(
  homeDir: string,
  requestedOrigin: string | undefined,
): Promise<{ origin: string; token: string; owned?: RunningServer }> {
  if (requestedOrigin !== undefined) {
    return {
      origin: normalizeServerOrigin(requestedOrigin),
      token: resolveServerToken(homeDir),
    };
  }
  const live = await getLiveServerInstance(homeDir);
  if (live !== undefined) {
    return {
      origin: serverOrigin(live.host, live.port),
      token: resolveServerToken(homeDir),
    };
  }
  const owned = await startServer({
    host: DEFAULT_SERVER_HOST,
    port: DEFAULT_SERVER_PORT,
    homeDir,
    version: getVersion(),
    logLevel: 'warn',
    telemetry: true,
  });
  return {
    origin: serverOrigin(owned.host, owned.port),
    token: owned.authTokenService.getToken(),
    owned,
  };
}
