import * as vscode from "vscode";

import { Events, Methods } from "../../shared/bridge";
import type { LoginResult } from "../../shared/legacy-sdk";
import type { LoginRequest, LoginStatus } from "../../shared/types";
import { updateLoginContext } from "../utils/context";
import type { Handler } from "./types";

export const authHandlers: Record<string, Handler<any, any>> = {
  [Methods.CheckLoginStatus]: async (_, ctx): Promise<LoginStatus> => {
    const providers = await ctx.harness.auth.providers();
    return {
      loggedIn: await updateLoginContext(ctx.harness),
      providers: providers.map((provider) => ({
        id: provider.id,
        name: provider.name,
        configured: provider.configured,
        methods: provider.methods.map((method) => ({
          type: method.type,
          label: method.label,
        })),
      })),
    };
  },

  [Methods.Login]: async (params: LoginRequest, ctx): Promise<LoginResult> => {
    try {
      await ctx.harness.auth.login(params.providerId, params.method, {
        prompt: async () => {
          if (params.value !== undefined) return params.value;
          throw new Error(`${params.method} login requires interactive input`);
        },
        notify: (event) => {
          if (event.type !== "device_code" && event.type !== "auth_url") return;
          const url = event.type === "device_code" ? event.verificationUri : event.url;
          ctx.broadcast(Events.LoginUrl, { url }, ctx.webviewId);
          void vscode.env.openExternal(vscode.Uri.parse(url));
        },
      });
      await updateLoginContext(ctx.harness);
      return { success: true };
    } catch (error) {
      ctx.logError(`Provider login failed: ${params.providerId}`, error);
      await updateLoginContext(ctx.harness).catch((statusError: unknown) => {
        ctx.logError("Unable to refresh login status after a failed login", statusError);
      });
      return {
        success: false,
        error: error instanceof Error ? error.message : String(error),
      };
    }
  },

  [Methods.Logout]: async (_, ctx): Promise<LoginResult> => {
    try {
      const providers = await ctx.harness.auth.providers();
      await Promise.all(
        providers
          .filter((provider) => provider.configured)
          .map((provider) => ctx.harness.auth.logout(provider.id)),
      );
      await updateLoginContext(ctx.harness);
      return { success: true };
    } catch (error) {
      ctx.logError("Provider logout failed", error);
      await updateLoginContext(ctx.harness).catch((statusError: unknown) => {
        ctx.logError("Unable to refresh login status after a failed logout", statusError);
      });
      return {
        success: false,
        error: error instanceof Error ? error.message : String(error),
      };
    }
  },
};
