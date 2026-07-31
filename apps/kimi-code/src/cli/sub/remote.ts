import type { Command } from "commander";
import QRCode from "qrcode";

import { startRemoteAccess } from "../../remote-access";

interface RemoteCliOptions {
  relay: string;
  server?: string;
  name?: string;
}

export function registerRemoteCommand(parent: Command): void {
  parent
    .command("remote")
    .description("Connect this runtime to the mobile app through an encrypted relay.")
    .argument("[action]", "Action: start or pair.", "start")
    .option(
      "--relay <url>",
      "Relay WebSocket URL.",
      process.env["AGENT_RELAY_URL"] ?? "wss://relay.k.3720.org",
    )
    .option("--server <url>", "Existing local server URL. Starts one when omitted.")
    .option("--name <name>", "Name shown on paired devices.")
    .action(async (action: string, options: RemoteCliOptions) => {
      await runRemoteCommand(action, options);
    });
}

async function runRemoteCommand(action: string, options: RemoteCliOptions): Promise<void> {
  if (action !== "start" && action !== "pair") {
    throw new Error("Usage: kimi remote [start|pair]");
  }
  const remote = await startRemoteAccess({
    relayUrl: options.relay,
    localOrigin: options.server,
    runtimeName: options.name,
    onStatus: (status) => process.stdout.write(`Relay ${status}\n`),
  });

  process.stdout.write(`Runtime: ${remote.runtimeId}\n`);
  if (action === "pair") {
    const pairingUri = remote.createPairingUri();
    process.stdout.write("Scan with the mobile app:\n\n");
    process.stdout.write(
      `${await QRCode.toString(pairingUri, { type: "terminal", small: true })}\n`,
    );
    process.stdout.write(`${pairingUri}\n\n`);
  } else {
    process.stdout.write("Paired devices reconnect automatically.\n");
  }
  process.stdout.write("Press Ctrl+C to stop remote access.\n");

  await new Promise<void>((resolve) => {
    process.once("SIGINT", resolve);
    process.once("SIGTERM", resolve);
  });
  await remote.close();
}
