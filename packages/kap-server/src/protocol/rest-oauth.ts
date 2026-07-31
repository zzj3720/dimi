import { z } from "zod";

export const oauthLoginStartRequestSchema = z.object({
  provider: z.string().min(1),
});

export const oauthLoginQuerySchema = z.object({
  provider: z.string().min(1),
});

export const oauthFlowStatusSchema = z.enum([
  "pending",
  "authenticated",
  "denied",
  "expired",
  "cancelled",
]);

export const oauthFlowStartSchema = z.union([
  z.object({
    flow_id: z.string(),
    provider: z.string(),
    status: z.literal("authenticated"),
  }),
  z.object({
    flow_id: z.string(),
    provider: z.string(),
    status: z.literal("pending"),
    verification_uri: z.string().url(),
    verification_uri_complete: z.string().url(),
    user_code: z.string(),
    expires_in: z.number().int().positive(),
    interval: z.number().positive(),
    expires_at: z.string(),
  }),
]);

export const oauthFlowSnapshotSchema = z.object({
  flow_id: z.string(),
  provider: z.string(),
  status: oauthFlowStatusSchema,
  verification_uri: z.string().url().optional(),
  verification_uri_complete: z.string().url().optional(),
  user_code: z.string().optional(),
  expires_in: z.number().int().positive().optional(),
  interval: z.number().positive().optional(),
  expires_at: z.string().optional(),
  resolved_at: z.string().optional(),
  error_message: z.string().optional(),
});

export const oauthLoginCancelResponseSchema = z.object({
  cancelled: z.boolean(),
  status: oauthFlowStatusSchema,
});
