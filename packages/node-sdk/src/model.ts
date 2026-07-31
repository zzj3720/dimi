import type { ModelAlias } from "./types";

/**
 * Models exposed by the SDK are already the runtime-resolved projection.
 * Kept as a named helper for UI callers that want one uniform read path.
 */
export function effectiveModelAlias(model: ModelAlias): ModelAlias {
  return model;
}
