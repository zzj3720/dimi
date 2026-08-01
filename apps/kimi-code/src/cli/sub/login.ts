/**
 * `kimi login` — drive the OAuth device-code flow non-interactively.
 * The `authMethods.terminal-auth.args=['login']` (legacy `_meta` path)
 * advertised by the ACP server points clients at this entry point. The
 * first-class ACP `args=['--login']` path enters the same flow via
 * `kimi acp --login`.
 */

import type { Command } from "commander";

import { runLoginFlow } from "./login-flow";

export function registerLoginCommand(parent: Command): void {
  parent
    .command("login [provider]")
    .description("Connect an LLM provider with OAuth or an API key.")
    .option("--method <method>", "Authentication method: oauth or api-key.")
    .action(async (provider: string | undefined, options: { method?: string }) => {
      await runLoginFlow(provider, options.method);
    });
}
