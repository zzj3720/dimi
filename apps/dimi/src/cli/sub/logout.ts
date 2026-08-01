import { createDimiHarness } from "@dimi-agent/dimi-sdk";
import type { Command } from "commander";

import { createDimiCodeHostIdentity } from "#/cli/version";

/** Register the non-interactive counterpart to the TUI's `/logout` command. */
export function registerLogoutCommand(parent: Command): void {
  parent
    .command("logout <provider>")
    .description("Disconnect a configured LLM provider.")
    .action(async (provider: string) => {
      const harness = createDimiHarness({ identity: createDimiCodeHostIdentity(), uiMode: "cli" });
      try {
        await harness.auth.logout(provider);
        process.stdout.write(`Disconnected from ${provider}.\n`);
      } catch (error) {
        process.exitCode = 1;
        process.stderr.write(
          `Logout failed: ${error instanceof Error ? error.message : String(error)}\n`,
        );
      } finally {
        await harness.close();
      }
    });
}
