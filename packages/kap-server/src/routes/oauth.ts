import { randomUUID } from "node:crypto";

import {
  IProviderRuntime,
  type AuthEvent,
  type AuthInteraction,
  type Scope,
} from "@dimi-agent/agent-core-v2";
import { z } from "zod";

import { okEnvelope } from "../envelope";
import { requestLog } from "../lib/requestLog";
import { defineRoute } from "../middleware/defineRoute";
import {
  oauthFlowSnapshotSchema,
  oauthFlowStartSchema,
  oauthLoginCancelResponseSchema,
  oauthLoginQuerySchema,
  oauthLoginStartRequestSchema,
} from "../protocol/rest-oauth";

interface RouteHost {
  get(path: string, options: object, handler: (req: any, reply: any) => unknown): unknown;
  post(path: string, options: object, handler: (req: any, reply: any) => unknown): unknown;
  delete(path: string, options: object, handler: (req: any, reply: any) => unknown): unknown;
}

type FlowStatus = "pending" | "authenticated" | "denied" | "expired" | "cancelled";

interface LoginFlow {
  flow_id: string;
  provider: string;
  status: FlowStatus;
  controller: AbortController;
  verification_uri?: string;
  verification_uri_complete?: string;
  user_code?: string;
  expires_in?: number;
  interval?: number;
  expires_at?: string;
  resolved_at?: string;
  error_message?: string;
  announced: Promise<void>;
  announce(): void;
}

const flows = new Map<string, LoginFlow>();

export function registerOAuthRoutes(app: RouteHost, core: Scope): void {
  const start = defineRoute(
    {
      method: "POST",
      path: "/oauth/login",
      body: oauthLoginStartRequestSchema,
      success: { data: oauthFlowStartSchema },
      description: "Start an OAuth flow for a provider",
      tags: ["auth"],
    },
    async (req, reply) => {
      const provider = req.body.provider;
      const runtime = core.accessor.get(IProviderRuntime);
      await runtime.ready;
      if ((await runtime.checkAuth(provider))?.type === "oauth") {
        reply.send(
          okEnvelope({ flow_id: randomUUID(), provider, status: "authenticated" as const }, req.id),
        );
        return;
      }
      flows.get(provider)?.controller.abort();
      const flow = createFlow(provider);
      flows.set(provider, flow);
      const interaction = interactionFor(flow);
      void runtime
        .login(provider, "oauth", interaction)
        .then(async () => {
          await runtime.refresh({ provider, allowNetwork: true, force: true });
          settle(flow, "authenticated");
        })
        .catch((error: unknown) => {
          if (flow.controller.signal.aborted) {
            settle(flow, "cancelled");
            return;
          }
          const message = error instanceof Error ? error.message : String(error);
          settle(flow, /expired|timed out/iu.test(message) ? "expired" : "denied", message);
        });
      await flow.announced;
      if (flow.status === "authenticated") {
        reply.send(
          okEnvelope({ flow_id: flow.flow_id, provider, status: "authenticated" as const }, req.id),
        );
        return;
      }
      if (flow.status !== "pending") {
        throw new Error(flow.error_message ?? `OAuth login ${flow.status} for ${provider}`);
      }
      requestLog(req)?.info({ provider, action: "login" }, "oauth login started");
      reply.send(okEnvelope(toPendingStart(flow), req.id));
    },
  );
  app.post(start.path, start.options, start.handler);

  const poll = defineRoute(
    {
      method: "GET",
      path: "/oauth/login",
      querystring: oauthLoginQuerySchema,
      success: { data: z.union([oauthFlowSnapshotSchema, z.null()]) },
      description: "Poll an OAuth flow",
      tags: ["auth"],
    },
    (req, reply) => {
      const provider = req.query.provider;
      const flow = flows.get(provider);
      reply.send(okEnvelope(flow === undefined ? null : snapshot(flow), req.id));
    },
  );
  app.get(poll.path, poll.options, poll.handler);

  const cancel = defineRoute(
    {
      method: "DELETE",
      path: "/oauth/login",
      querystring: oauthLoginQuerySchema,
      success: { data: oauthLoginCancelResponseSchema },
      description: "Cancel an OAuth flow",
      tags: ["auth"],
    },
    (req, reply) => {
      const provider = req.query.provider;
      const flow = flows.get(provider);
      const cancelled = flow?.status === "pending";
      if (cancelled) {
        flow.controller.abort();
        settle(flow, "cancelled");
      }
      reply.send(okEnvelope({ cancelled, status: flow?.status ?? ("cancelled" as const) }, req.id));
    },
  );
  app.delete(cancel.path, cancel.options, cancel.handler);
}

function createFlow(provider: string): LoginFlow {
  let announce = () => {};
  const announced = new Promise<void>((resolve) => {
    announce = resolve;
  });
  return {
    flow_id: randomUUID(),
    provider,
    status: "pending",
    controller: new AbortController(),
    announced,
    announce,
  };
}

function interactionFor(flow: LoginFlow): AuthInteraction {
  return {
    signal: flow.controller.signal,
    prompt: async () => {
      throw new Error("This OAuth flow requires an interactive client");
    },
    notify: (event: AuthEvent) => {
      if (event.type !== "device_code") return;
      const expiresIn = event.expiresInSeconds ?? 15 * 60;
      flow.verification_uri = event.verificationUri;
      flow.verification_uri_complete = event.verificationUri;
      flow.user_code = event.userCode;
      flow.expires_in = expiresIn;
      flow.interval = event.intervalSeconds ?? 5;
      flow.expires_at = new Date(Date.now() + expiresIn * 1_000).toISOString();
      flow.announce();
    },
  };
}

function settle(flow: LoginFlow, status: Exclude<FlowStatus, "pending">, message?: string): void {
  flow.status = status;
  flow.resolved_at = new Date().toISOString();
  flow.error_message = message;
  flow.announce();
}

function toPendingStart(flow: LoginFlow) {
  if (
    flow.verification_uri === undefined ||
    flow.verification_uri_complete === undefined ||
    flow.user_code === undefined ||
    flow.expires_in === undefined ||
    flow.interval === undefined ||
    flow.expires_at === undefined
  ) {
    throw new Error(flow.error_message ?? `OAuth login did not start for ${flow.provider}`);
  }
  return {
    flow_id: flow.flow_id,
    provider: flow.provider,
    status: "pending" as const,
    verification_uri: flow.verification_uri,
    verification_uri_complete: flow.verification_uri_complete,
    user_code: flow.user_code,
    expires_in: flow.expires_in,
    interval: flow.interval,
    expires_at: flow.expires_at,
  };
}

function snapshot(flow: LoginFlow) {
  return {
    flow_id: flow.flow_id,
    provider: flow.provider,
    status: flow.status,
    verification_uri: flow.verification_uri,
    verification_uri_complete: flow.verification_uri_complete,
    user_code: flow.user_code,
    expires_in: flow.expires_in,
    interval: flow.interval,
    expires_at: flow.expires_at,
    resolved_at: flow.resolved_at,
    error_message: flow.error_message,
  };
}
