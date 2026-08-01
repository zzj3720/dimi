import * as vscode from "vscode";

import { Events } from "../shared/bridge";
import { DimiWebviewProvider } from "./DimiWebviewProvider";
import { onSettingsChange, VSCodeSettings } from "./config/vscode-settings";
import { updateLoginContext } from "./utils/context";

let outputChannel: vscode.OutputChannel | undefined;
let provider: DimiWebviewProvider | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  outputChannel = vscode.window.createOutputChannel("Dimi");
  const remoteInfo = vscode.env.remoteName ? ` (remote: ${vscode.env.remoteName})` : "";
  log(`Dimi ${VSCodeSettings.getExtensionConfig().version} activating${remoteInfo}`);

  provider = new DimiWebviewProvider(
    context.extensionUri,
    context,
    () => outputChannel?.show(),
    (message) => log(message),
  );
  context.subscriptions.push(provider, outputChannel);

  try {
    await updateLoginContext(provider.harness);
  } catch (error) {
    logError("Unable to determine login status", error);
  }

  context.subscriptions.push(
    vscode.workspace.registerTextDocumentContentProvider("dimi-baseline", {
      provideTextDocumentContent: async (uri) => {
        const sessionId = new URLSearchParams(uri.query).get("sessionId");
        if (!sessionId || !provider) return "";
        const relativePath = decodeURIComponent(uri.path.replace(/^\//, ""));
        try {
          return await provider.getBaselineContent(sessionId, relativePath);
        } catch (error) {
          logError("Unable to open baseline content", error);
          return "";
        }
      },
    }),
  );

  context.subscriptions.push(
    onSettingsChange((changedKeys) => {
      provider?.broadcast(Events.ExtensionConfigChanged, {
        config: VSCodeSettings.getExtensionConfig(),
        changedKeys,
      });
      if (changedKeys.includes("yoloMode")) {
        void provider
          ?.setYoloModeForActiveSessions(VSCodeSettings.yoloMode)
          .catch((error) => logError("Unable to update session permission", error));
      }
    }),
    vscode.window.registerWebviewViewProvider("dimi.webview", provider, {
      webviewOptions: { retainContextWhenHidden: true },
    }),
  );

  const commands: Record<string, () => void | Promise<void>> = {
    "dimi.clearAllState": async () => {
      await context.globalState.update("dimi.config", undefined);
      await context.globalState.update("dimi.mcpServers", undefined);
      await context.workspaceState.update("dimi.mcpEnabled", undefined);
      await vscode.window.showInformationMessage("Dimi: Extension UI state cleared.");
    },
    "dimi.openInTab": () => {
      provider?.createPanel();
    },
    "dimi.openInSideBar": async () => {
      await vscode.commands.executeCommand("dimi.webview.focus");
    },
    "dimi.focusInput": async () => {
      await vscode.commands.executeCommand("dimi.webview.focus");
      provider?.broadcast(Events.FocusInput, {});
    },
    "dimi.insertMention": async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) {
        await vscode.window.showWarningMessage("No active editor");
        return;
      }
      await vscode.commands.executeCommand("dimi.webview.focus");
      if (!(await provider?.insertEditorMention(editor.document.uri, editor.selection))) {
        await vscode.window.showWarningMessage("The active file is outside the selected working directory.");
      }
    },
    "dimi.newConversation": async () => {
      await vscode.commands.executeCommand("dimi.webview.focus");
      provider?.broadcast(Events.NewConversation, {});
    },
    "dimi.showLogs": () => outputChannel?.show(),
    "dimi.resetKimi": () => provider?.resetAllWebviews(),
    "dimi.logout": async () => {
      await vscode.commands.executeCommand("dimi.webview.focus");
      await vscode.window.showInformationMessage("Use the logout button in Dimi settings.");
    },
  };

  for (const [id, handler] of Object.entries(commands)) {
    context.subscriptions.push(vscode.commands.registerCommand(id, handler));
  }

  log("Dimi activated");
}

export async function deactivate(): Promise<void> {
  log("Dimi deactivating");
  await provider?.shutdown();
  provider = undefined;
}

function log(message: string): void {
  outputChannel?.appendLine(`[${new Date().toISOString()}] ${message}`);
}

function logError(message: string, error: unknown): void {
  const detail = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
  log(`${message}: ${detail}`);
}

export { log };
