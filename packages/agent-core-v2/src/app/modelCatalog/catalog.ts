import { z } from "zod";

import { createDecorator, type ServiceIdentifier } from "#/_base/di/instantiation";
import type { Model as ProviderModel } from "#/app/providerRuntime/types";
import type { ModelCapability } from "#/llmProtocol/capability";
import type { TokenUsage } from "#/llmProtocol/usage";

import type { ModelInspection } from "./inspection";
import type { ModelRequester } from "./modelRequester";

/** The provider runtime owns the canonical model shape. */
export type Model = ProviderModel;

export interface ModelReference {
  readonly provider: string;
  readonly model: string;
}

export function modelReference(model: Pick<Model, "provider" | "id">): string {
  return `${model.provider}/${model.id}`;
}

export function modelCapabilities(model: Model): ModelCapability {
  return {
    image_in: model.input.includes("image"),
    video_in: false,
    audio_in: false,
    thinking: model.reasoning,
    tool_use: true,
    max_context_tokens: model.contextWindow,
    dynamically_loaded_tools: model.dynamicTools,
  };
}

export function modelThinkingLevels(model: Model): readonly string[] {
  const configured = Object.entries(model.thinkingLevelMap ?? {})
    .filter(([level, value]) => level !== "off" && value !== null)
    .map(([level]) => level);
  return configured.length > 0 ? configured : model.reasoning ? ["low", "medium", "high"] : [];
}

export function modelDefaultThinkingLevel(model: Model): string | undefined {
  const levels = modelThinkingLevels(model);
  if (model.defaultThinkingLevel !== undefined && levels.includes(model.defaultThinkingLevel)) {
    return model.defaultThinkingLevel;
  }
  return levels[Math.floor(levels.length / 2)];
}

export function modelAlwaysThinking(model: Model): boolean {
  return model.reasoning && model.thinkingLevelMap?.["off"] === null;
}

export interface ModelPingResult {
  readonly ok: boolean;
  readonly durationMs: number;
  readonly text?: string;
  readonly finishReason?: string;
  readonly usage?: TokenUsage;
  readonly error?: string;
}

export const modelCatalogItemSchema = z.object({
  provider: z.string().min(1),
  model: z.string().min(1),
  display_name: z.string().min(1).optional(),
  max_context_size: z.number().int().min(1),
  capabilities: z.array(z.string()).optional(),
  support_efforts: z.array(z.string()).optional(),
  default_effort: z.string().optional(),
});
export type ModelCatalogItem = z.infer<typeof modelCatalogItemSchema>;

export const providerCatalogStatusSchema = z.enum(["connected", "error", "unconfigured"]);
export type ProviderCatalogStatus = z.infer<typeof providerCatalogStatusSchema>;

export const providerCatalogItemSchema = z.object({
  id: z.string().min(1),
  name: z.string().min(1),
  base_url: z.string().min(1).optional(),
  default_model: z.string().min(1).optional(),
  auth_methods: z.array(z.enum(["oauth", "api_key"])),
  credential_type: z.enum(["oauth", "api_key"]).optional(),
  status: providerCatalogStatusSchema,
  models: z.array(z.string().min(1)).optional(),
});
export type ProviderCatalogItem = z.infer<typeof providerCatalogItemSchema>;

export const setDefaultModelResponseSchema = z.object({
  default_model: z.string().min(1),
  model: modelCatalogItemSchema,
});
export type SetDefaultModelResponse = z.infer<typeof setDefaultModelResponseSchema>;

export function toProtocolModel(model: Model): ModelCatalogItem {
  const capabilities = modelCapabilities(model);
  const capabilityNames = [
    capabilities.image_in ? "image_in" : undefined,
    capabilities.thinking ? "thinking" : undefined,
    capabilities.tool_use ? "tool_use" : undefined,
  ].filter((value): value is string => value !== undefined);
  return {
    provider: model.provider,
    model: model.id,
    display_name: model.name,
    max_context_size: model.contextWindow,
    capabilities: capabilityNames.length === 0 ? undefined : capabilityNames,
    support_efforts: [...modelThinkingLevels(model)],
    default_effort: modelDefaultThinkingLevel(model),
  };
}

export interface IModelCatalog {
  readonly _serviceBrand: undefined;

  /** Resolve either a canonical `provider/model` reference or a unique model id. */
  get(reference: string): Model;
  getRequester(reference: string): ModelRequester;
  inspect(reference: string): ModelInspection;
  ping(reference: string): Promise<ModelPingResult>;
  findByName(name: string): readonly string[];
  listModels(): Promise<readonly ModelCatalogItem[]>;
  listProviders(): Promise<readonly ProviderCatalogItem[]>;
  getProvider(providerId: string): Promise<ProviderCatalogItem>;
  setDefaultModel(reference: string): Promise<SetDefaultModelResponse>;
}

export const IModelCatalog: ServiceIdentifier<IModelCatalog> =
  createDecorator<IModelCatalog>("modelResolver");
