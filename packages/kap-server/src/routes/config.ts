/**
 * `/config` route handlers — server-v2 port.
 *
 * Implements `/api/v1/config` on top of `agent-core-v2`'s
 * section-registry `IConfigService`:
 *   GET  /config   — global Kimi configuration, secrets redacted
 *   POST /config   — update global configuration (merge semantics)
 *
 * The config service is a per-domain registry (`get(domain)` /
 * `set(domain, patch)`), so this edge:
 *   - projects `getAll()` (camelCase resolved config) into the snake_case
 *     `ConfigResponse`;
 *   - splits v1's flat multi-domain `POST /config` patch into per-domain
 *     `IConfigService.set(domain, value)` calls (snake_case → camelCase);
 *   - republishes the change as a v2 `DomainEvent` on `IEventService`.
 *
 * **Event shape**: v2's `DomainEvent` is `{ type, payload }`, and the Core
 * `events` WS stream forwards it as-is. The config-changed notification is
 * therefore emitted as `{ type: 'event.config.changed', payload: { changedFields,
 * config } }` rather than v1's flat `{ type, changedFields, config }`. The HTTP
 * response (the schema contract) is unaffected.
 */

import { IConfigService, IEventService, type Scope } from "@moonshot-ai/agent-core-v2";

import { errEnvelope, okEnvelope } from "../envelope";
import { requestLog } from "../lib/requestLog";
import { defineRoute } from "../middleware/defineRoute";
import { ErrorCode } from "../protocol/error-codes";
import { configResponseSchema, patchConfigRequestSchema } from "../protocol/rest-config";
import type { ConfigResponse } from "../protocol/rest-config";

interface ConfigRouteHost {
  get(
    path: string,
    options: { schema?: Record<string, unknown> },
    handler: (req: { id: string }, reply: { send(payload: unknown): void }) => Promise<void> | void,
  ): unknown;
  post(
    path: string,
    options: { schema?: Record<string, unknown> },
    handler: (
      req: { id: string; body: unknown },
      reply: { send(payload: unknown): void },
    ) => Promise<void> | void,
  ): unknown;
}

export function registerConfigRoutes(app: ConfigRouteHost, core: Scope): void {
  const getRoute = defineRoute(
    {
      method: "GET",
      path: "/config",
      success: { data: configResponseSchema },
      description: "Get the global Kimi configuration (secrets redacted)",
      tags: ["config"],
    },
    async (req, reply) => {
      const config = core.accessor.get(IConfigService);
      await config.ready;
      reply.send(okEnvelope(toConfigResponse(config.getAll()), req.id));
    },
  );
  app.get(
    getRoute.path,
    getRoute.options,
    getRoute.handler as Parameters<ConfigRouteHost["get"]>[2],
  );

  const setRoute = defineRoute(
    {
      method: "POST",
      path: "/config",
      body: patchConfigRequestSchema,
      success: { data: configResponseSchema },
      errors: {
        [ErrorCode.VALIDATION_FAILED]: {},
      },
      description: "Update the global Kimi configuration (merge semantics)",
      tags: ["config"],
    },
    async (req, reply) => {
      try {
        const config = core.accessor.get(IConfigService);
        await config.ready;
        const camelPatch = convertKeysSnakeToCamel(req.body) as Record<string, unknown>;
        // v1 wire sugar: `yolo: true` is an alias for
        // `default_permission_mode = 'yolo'`. Fold it into the canonical domain and
        // drop the key so `yolo` is never a config domain and never persisted.
        if (camelPatch["yolo"] === true) {
          camelPatch["defaultPermissionMode"] = "yolo";
        }
        delete camelPatch["yolo"];
        for (const domain of Object.keys(camelPatch)) {
          await config.set(domain, camelPatch[domain]);
        }
        const response = toConfigResponse(config.getAll());
        const changedFields = Object.keys(req.body as Record<string, unknown>);
        core.accessor.get(IEventService).publish({
          type: "event.config.changed",
          payload: {
            changedFields,
            config: response,
          },
        });
        // Only the changed field *names* — values may carry secrets.
        requestLog(req)?.info({ changedFields }, "config updated");
        reply.send(okEnvelope(response, req.id));
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        requestLog(req)?.error({ err: error }, "config update failed");
        reply.send(errEnvelope(ErrorCode.VALIDATION_FAILED, message, req.id));
      }
    },
  );
  app.post(
    setRoute.path,
    setRoute.options,
    setRoute.handler as Parameters<ConfigRouteHost["post"]>[2],
  );
}

// ---------------------------------------------------------------------------
// Edge facade — project resolved config into the public `ConfigResponse`.
// Provider definitions and model metadata are intentionally absent: the
// provider runtime and its dynamic catalog are their sole owners.
// ---------------------------------------------------------------------------

function toConfigResponse(resolved: Record<string, unknown>): ConfigResponse {
  const wire: Record<string, unknown> = {};
  for (const [domain, value] of Object.entries(resolved)) {
    wire[camelToSnake(domain)] = value;
  }
  // v1 wire echo: surface `yolo` as a derived boolean of the effective default
  // permission mode. `yolo` is not a config domain; it is computed here so the
  // v1 `/config` shape is preserved without persisting a parallel field.
  const defaultPermissionMode = resolved["defaultPermissionMode"];
  if (typeof defaultPermissionMode === "string") {
    wire["yolo"] = defaultPermissionMode === "yolo";
  }
  return wire as ConfigResponse;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function convertKeysSnakeToCamel(obj: unknown): unknown {
  if (Array.isArray(obj)) {
    return obj.map(convertKeysSnakeToCamel);
  }
  if (isPlainObject(obj)) {
    const result: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(obj)) {
      result[snakeToCamel(key)] = convertKeysSnakeToCamel(value);
    }
    return result;
  }
  return obj;
}

function snakeToCamel(str: string): string {
  return str.replaceAll(/_([a-z])/g, (_, ch: string) => ch.toUpperCase());
}

function camelToSnake(str: string): string {
  return str.replaceAll(/[A-Z]/g, (ch) => `_${ch.toLowerCase()}`);
}
