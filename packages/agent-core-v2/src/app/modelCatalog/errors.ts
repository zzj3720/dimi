/**
 * `modelCatalog` domain (L2) — catalog error codes.
 *
 * Owns the public provider/model lookup errors shared by the model catalog
 * and provider runtime.
 */

import { registerErrorDomain, type ErrorDomain } from "#/_base/errors/codes";

export const ModelCatalogErrors = {
  codes: {
    PROVIDER_NOT_FOUND: "provider.not_found",
    MODEL_NOT_FOUND: "model.not_found",
  },
  info: {
    "provider.not_found": {
      title: "Provider not found",
      retryable: false,
      public: true,
      action: "Check the provider ID and connect the provider first.",
    },
    "model.not_found": {
      title: "Model not found",
      retryable: false,
      public: true,
      action: "Check the provider/model reference or refresh the model catalog.",
    },
  },
} as const satisfies ErrorDomain;

registerErrorDomain(ModelCatalogErrors);
