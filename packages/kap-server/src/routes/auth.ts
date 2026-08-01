import { IConfigService, IProviderRuntime, type Scope } from "@dimi-agent/agent-core-v2";
import { z } from "zod";

import { okEnvelope } from "../envelope";
import { defineRoute } from "../middleware/defineRoute";

interface RouteHost {
  get(
    path: string,
    options: { schema?: Record<string, unknown> },
    handler: (req: { id: string }, reply: { send(payload: unknown): void }) => unknown,
  ): unknown;
}

const authSummarySchema = z.object({
  ready: z.boolean(),
  providers_count: z.number().int().min(0),
  default_model: z.string().nullable(),
  authenticated_providers: z.array(
    z.object({
      id: z.string().min(1),
      type: z.enum(["oauth", "api_key"]),
      source: z.string().min(1),
    }),
  ),
});

export function registerAuthRoute(app: RouteHost, core: Scope): void {
  const route = defineRoute(
    {
      method: "GET",
      path: "/auth",
      success: { data: authSummarySchema },
      description: "Get provider authentication readiness",
      tags: ["auth"],
    },
    async (req, reply) => {
      const runtime = core.accessor.get(IProviderRuntime);
      const config = core.accessor.get(IConfigService);
      await Promise.all([runtime.ready, config.ready]);
      const connected = (
        await Promise.all(
          runtime.getProviders().map(async (provider) => {
            const auth = await runtime.checkAuth(provider.id);
            return auth === undefined
              ? undefined
              : { id: provider.id, type: auth.type, source: auth.source };
          }),
        )
      ).filter((provider) => provider !== undefined);
      const defaultProvider = config.get<string>("defaultProvider") ?? undefined;
      const defaultModel = config.get<string>("defaultModel") ?? undefined;
      const defaultReference =
        defaultProvider !== undefined && defaultModel !== undefined
          ? `${defaultProvider}/${defaultModel}`
          : (defaultModel ?? null);
      reply.send(
        okEnvelope(
          {
            ready:
              defaultModel !== undefined &&
              (defaultProvider === undefined ||
                connected.some((provider) => provider.id === defaultProvider)),
            providers_count: connected.length,
            default_model: defaultReference,
            authenticated_providers: connected,
          },
          req.id,
        ),
      );
    },
  );
  app.get(route.path, route.options, route.handler as Parameters<RouteHost["get"]>[2]);
}
