import * as vscode from "vscode";
import type { DimiHarness } from "@dimi-agent/dimi-sdk";

export async function updateLoginContext(harness: DimiHarness): Promise<boolean> {
  const status = await harness.auth.status();
  const loggedIn = status.providers.some((provider) => provider.hasToken);
  await vscode.commands.executeCommand("setContext", "dimi.isLoggedIn", loggedIn);
  return loggedIn;
}
