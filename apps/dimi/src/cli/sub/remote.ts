import { hostname } from "node:os";
import { join } from "node:path";

import { getLiveServerInstance, startServer, type RunningServer } from "@dimi-agent/kap-server";
import { startRemoteBridge } from "@dimi-agent/remote/bridge";
import type { Command } from "commander";
import QRCode from "qrcode";

import { getVersion } from "#/cli/version";
import { getDataDir } from "#/utils/paths";
import {
  DEFAULT_SERVER_HOST,
  DEFAULT_SERVER_PORT,
  normalizeServerOrigin,
  resolveServerToken,
  serverOrigin,
} from "./web/shared";

interface RemoteCliOptions {
  relay: string;
  server?: string;
  name: string;
  /** Use the legacy TypeScript backend instead of the Rust runtime. */
  legacy?: boolean;
}

export function registerRemoteCommand(parent: Command): void {
  parent
    .command("remote")
    .description("Connect this runtime to the mobile app through an encrypted relay.")
    .option(
      "--relay <url>",
      "Relay WebSocket URL.",
      process.env["AGENT_RELAY_URL"] ?? "wss://relay.k.3720.org",
    )
    .option("--server <url>", "Existing local server URL. Starts one when omitted.")
    .option("--name <name>", "Name shown on paired devices.", hostname())
    .option("--legacy", "Use the legacy TypeScript backend instead of the Rust runtime.", false)
    .action(async (options: RemoteCliOptions) => {
      await runRemoteCommand(options);
    });
}

async function runRemoteCommand(options: RemoteCliOptions): Promise<void> {
  const homeDir = getDataDir();
  const local = await resolveLocalServer(homeDir, options.server, options.legacy);
  const bridge = await startRemoteBridge({
    relayUrl: options.relay,
    localOrigin: local.origin,
    localToken: local.token,
    runtimeName: options.name,
    statePath: join(homeDir, "remote", "bridge.json"),
    onStatus: (status) => process.stdout.write(`Relay ${status}\n`),
  });

  process.stdout.write(`Runtime: ${bridge.runtimeId}\n`);
  process.stdout.write("Scan with the mobile app:\n\n");
  const code = await QRCode.toString(bridge.pairingUri, { type: "terminal", small: true });
  process.stdout.write(`${code}\n`);
  process.stdout.write(`${bridge.pairingUri}\n\n`);
  process.stdout.write("Press Ctrl+C to stop remote access.\n");

  await new Promise<void>((resolve) => {
    process.once("SIGINT", resolve);
    process.once("SIGTERM", resolve);
  });
  await bridge.close();
  await local.owned?.close();
}

async function resolveLocalServer(
  homeDir: string,
  requestedOrigin: string | undefined,
  legacy: boolean | undefined,
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
    logLevel: "warn",
    telemetry: true,
    legacyStore: legacy === true,
  });
  return {
    origin: serverOrigin(owned.host, owned.port),
    token: owned.authTokenService.getToken(),
    owned,
  };
}
