import { z } from "zod";

import {
  modelCatalogItemSchema,
  providerCatalogItemSchema,
} from "@moonshot-ai/agent-core-v2/app/modelCatalog/catalog";

export const listModelsResponseSchema = z.object({
  items: z.array(modelCatalogItemSchema),
});

export const listProvidersResponseSchema = z.object({
  items: z.array(providerCatalogItemSchema),
});

export const getProviderResponseSchema = providerCatalogItemSchema;

export const refreshProvidersResponseSchema = z.object({
  refreshed: z.array(z.string().min(1)),
  failed: z.array(
    z.object({
      provider: z.string().min(1),
      message: z.string().min(1),
    }),
  ),
});

export const providerActionRequestSchema = z
  .object({
    method: z.literal("api_key").optional(),
    value: z.string().min(1).optional(),
  })
  .optional();

export const providerActionResponseSchema = z.union([
  refreshProvidersResponseSchema,
  providerCatalogItemSchema,
]);
