/**
 * GET /v1/auth
 *   Reply: AuthSummary {
 *     ready,
 *     providers_count,
 *     default_model,
 *     authenticated_providers
 *   }
 */
import { z } from "zod";

export const authenticatedProviderSchema = z.object({
  id: z.string().min(1),
  type: z.enum(["oauth", "api_key"]),
  source: z.string().min(1),
});
export type AuthenticatedProvider = z.infer<typeof authenticatedProviderSchema>;

export const authSummarySchema = z.object({
  ready: z.boolean(),
  providers_count: z.number().int().nonnegative(),
  default_model: z.string().nullable(),
  authenticated_providers: z.array(authenticatedProviderSchema),
});
export type AuthSummary = z.infer<typeof authSummarySchema>;
