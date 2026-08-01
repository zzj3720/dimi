import { IModelCatalog, IProviderRuntime, isError2, type Scope } from "@dimi-agent/agent-core-v2";
import { setDefaultModelResponseSchema } from "@dimi-agent/agent-core-v2/app/modelCatalog/catalog";
import { z } from "zod";

import { errEnvelope, okEnvelope } from "../envelope";
import { defineRoute } from "../middleware/defineRoute";
import { ErrorCode } from "../protocol/error-codes";
import {
  getProviderResponseSchema,
  listModelsResponseSchema,
  listProvidersResponseSchema,
  providerActionRequestSchema,
  providerActionResponseSchema,
  refreshProvidersResponseSchema,
} from "../protocol/rest-modelCatalog";
import { parseActionSuffix } from "./action-suffix";

interface RouteHost {
  get(path: string, options: object, handler: (req: any, reply: any) => unknown): unknown;
  post(path: string, options: object, handler: (req: any, reply: any) => unknown): unknown;
}

const providerIdParamSchema = z.object({ provider_id: z.string().min(1) });
const tailParamSchema = z.object({ tail: z.string().min(1) });
const actionParamSchema = z.object({ action: z.string().min(1) });

export function registerModelCatalogRoutes(app: RouteHost, core: Scope): void {
  const listModels = defineRoute(
    {
      method: "GET",
      path: "/models",
      success: { data: listModelsResponseSchema },
      description: "List models available through authenticated providers",
      tags: ["models"],
    },
    async (req, reply) => {
      reply.send(
        okEnvelope({ items: await core.accessor.get(IModelCatalog).listModels() }, req.id),
      );
    },
  );
  app.get(listModels.path, listModels.options, listModels.handler);

  const setDefault = defineRoute(
    {
      method: "POST",
      path: "/models/{tail}",
      params: tailParamSchema,
      success: { data: setDefaultModelResponseSchema },
      errors: {
        [ErrorCode.VALIDATION_FAILED]: {},
        [ErrorCode.MODEL_NOT_FOUND]: {},
      },
      description: "Set the default provider/model reference",
      tags: ["models"],
    },
    async (req, reply) => {
      const parsed = parseActionSuffix({
        tail: req.params.tail,
        allowedActions: ["set_default"] as const,
        resourceLabel: "model",
      });
      if (parsed.kind !== "action") {
        reply.send(
          errEnvelope(
            ErrorCode.VALIDATION_FAILED,
            parsed.kind === "invalid" ? parsed.reason : "set_default action required",
            req.id,
          ),
        );
        return;
      }
      try {
        reply.send(
          okEnvelope(await core.accessor.get(IModelCatalog).setDefaultModel(parsed.id), req.id),
        );
      } catch (error) {
        if (sendCatalogError(reply, req.id, error)) return;
        throw error;
      }
    },
  );
  app.post(setDefault.path, setDefault.options, setDefault.handler);

  const listProviders = defineRoute(
    {
      method: "GET",
      path: "/providers",
      success: { data: listProvidersResponseSchema },
      description: "List built-in providers and their connection status",
      tags: ["providers"],
    },
    async (req, reply) => {
      reply.send(
        okEnvelope({ items: await core.accessor.get(IModelCatalog).listProviders() }, req.id),
      );
    },
  );
  app.get(listProviders.path, listProviders.options, listProviders.handler);

  const getProvider = defineRoute(
    {
      method: "GET",
      path: "/providers/{provider_id}",
      params: providerIdParamSchema,
      success: { data: getProviderResponseSchema },
      errors: { [ErrorCode.PROVIDER_NOT_FOUND]: {} },
      description: "Get one built-in provider",
      tags: ["providers"],
    },
    async (req, reply) => {
      try {
        reply.send(
          okEnvelope(
            await core.accessor.get(IModelCatalog).getProvider(req.params.provider_id),
            req.id,
          ),
        );
      } catch (error) {
        if (sendCatalogError(reply, req.id, error)) return;
        throw error;
      }
    },
  );
  app.get(getProvider.path, getProvider.options, getProvider.handler);

  const refreshAll = defineRoute(
    {
      method: "POST",
      path: "/providers{action}",
      params: actionParamSchema,
      success: { data: refreshProvidersResponseSchema },
      errors: { [ErrorCode.VALIDATION_FAILED]: {} },
      description: "Refresh dynamic provider model catalogs",
      tags: ["providers"],
    },
    async (req, reply) => {
      if (req.params.action !== ":refresh") {
        reply.send(
          errEnvelope(
            ErrorCode.VALIDATION_FAILED,
            `unsupported action: ${req.params.action}`,
            req.id,
          ),
        );
        return;
      }
      reply.send(okEnvelope(await refresh(core), req.id));
    },
  );
  app.post(refreshAll.path, refreshAll.options, refreshAll.handler);

  const refreshOne = defineRoute(
    {
      method: "POST",
      path: "/providers/{tail}",
      params: tailParamSchema,
      body: providerActionRequestSchema,
      success: { data: providerActionResponseSchema },
      errors: {
        [ErrorCode.VALIDATION_FAILED]: {},
        [ErrorCode.PROVIDER_NOT_FOUND]: {},
      },
      description: "Login, logout, or refresh one provider",
      tags: ["providers"],
    },
    async (req, reply) => {
      const parsed = parseActionSuffix({
        tail: req.params.tail,
        allowedActions: ["login", "logout", "refresh"] as const,
        resourceLabel: "provider",
      });
      if (parsed.kind !== "action") {
        reply.send(
          errEnvelope(
            ErrorCode.VALIDATION_FAILED,
            parsed.kind === "invalid" ? parsed.reason : "refresh action required",
            req.id,
          ),
        );
        return;
      }
      const runtime = core.accessor.get(IProviderRuntime);
      if (runtime.getProvider(parsed.id) === undefined) {
        reply.send(
          errEnvelope(ErrorCode.PROVIDER_NOT_FOUND, `provider ${parsed.id} does not exist`, req.id),
        );
        return;
      }
      if (parsed.action === "refresh") {
        reply.send(okEnvelope(await refresh(core, parsed.id), req.id));
        return;
      }
      if (parsed.action === "login") {
        if (req.body?.method !== "api_key" || req.body.value === undefined) {
          reply.send(
            errEnvelope(
              ErrorCode.VALIDATION_FAILED,
              "API-key login requires method=api_key and a non-empty value",
              req.id,
            ),
          );
          return;
        }
        const value = req.body.value;
        await runtime.login(parsed.id, "api_key", {
          prompt: async () => value,
          notify: () => {},
        });
        await runtime.refresh({ provider: parsed.id, allowNetwork: true, force: true });
      } else {
        await runtime.logout(parsed.id);
      }
      reply.send(okEnvelope(await core.accessor.get(IModelCatalog).getProvider(parsed.id), req.id));
    },
  );
  app.post(refreshOne.path, refreshOne.options, refreshOne.handler);
}

async function refresh(core: Scope, providerId?: string) {
  const runtime = core.accessor.get(IProviderRuntime);
  const candidates =
    providerId === undefined
      ? runtime.getProviders()
      : [runtime.getProvider(providerId)].filter((value) => value !== undefined);
  const selected = (
    await Promise.all(
      candidates.map(async (provider) =>
        provider.refreshModels !== undefined && (await runtime.checkAuth(provider.id)) !== undefined
          ? provider
          : undefined,
      ),
    )
  ).filter((provider) => provider !== undefined);
  const result = await runtime.refresh({
    provider: providerId,
    allowNetwork: true,
    force: true,
  });
  return {
    refreshed: selected
      .filter((provider) => !result.errors.has(provider.id))
      .map((provider) => provider.id),
    failed: [...result.errors]
      .filter(([id]) => providerId === undefined || id === providerId)
      .map(([provider, error]) => ({ provider, message: error.message })),
  };
}

function sendCatalogError(reply: { send(payload: unknown): unknown }, id: string, error: unknown) {
  if (!isError2(error)) return false;
  if (error.code === "provider.not_found") {
    reply.send(errEnvelope(ErrorCode.PROVIDER_NOT_FOUND, error.message, id));
    return true;
  }
  if (error.code === "model.not_found") {
    reply.send(errEnvelope(ErrorCode.MODEL_NOT_FOUND, error.message, id));
    return true;
  }
  return false;
}
