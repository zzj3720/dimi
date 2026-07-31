import { z } from "zod";

import { modelCatalogItemSchema, providerCatalogItemSchema } from "../modelCatalog";

export const listModelsResponseSchema = z.object({
  items: z.array(modelCatalogItemSchema),
});
export type ListModelsResponse = z.infer<typeof listModelsResponseSchema>;

export const listProvidersResponseSchema = z.object({
  items: z.array(providerCatalogItemSchema),
});
export type ListProvidersResponse = z.infer<typeof listProvidersResponseSchema>;

export const getProviderResponseSchema = providerCatalogItemSchema;
export type GetProviderResponse = z.infer<typeof getProviderResponseSchema>;

export const setDefaultModelResponseSchema = z.object({
  default_model: z.string().min(1),
  model: modelCatalogItemSchema,
});
export type SetDefaultModelResponse = z.infer<typeof setDefaultModelResponseSchema>;

export const refreshProvidersResponseSchema = z.object({
  refreshed: z.array(z.string().min(1)),
  failed: z.array(
    z.object({
      provider: z.string().min(1),
      message: z.string().min(1),
    }),
  ),
});
export type RefreshProvidersResponse = z.infer<typeof refreshProvidersResponseSchema>;

export const providerActionRequestSchema = z
  .object({
    method: z.literal("api_key").optional(),
    value: z.string().min(1).optional(),
  })
  .optional();
export type ProviderActionRequest = z.infer<typeof providerActionRequestSchema>;

export const providerActionResponseSchema = z.union([
  refreshProvidersResponseSchema,
  providerCatalogItemSchema,
]);
export type ProviderActionResponse = z.infer<typeof providerActionResponseSchema>;
