import type { DimiHarness, ProviderModel } from "@dimi-agent/dimi-sdk";

/** One runtime model projected to ACP's model picker. */
export interface AcpModelEntry {
  readonly id: string;
  readonly name: string;
  readonly description?: string;
  readonly thinkingSupported: boolean;
  readonly alwaysThinking?: boolean;
  readonly supportEfforts: readonly string[];
  readonly defaultThinkingEffort: string;
}

export function deriveSupportEfforts(model: ProviderModel): readonly string[] {
  const configured = Object.entries(model.thinkingLevelMap ?? {})
    .filter(([level, value]) => level !== "off" && value !== null)
    .map(([level]) => level);
  return configured.length > 0 ? configured : model.reasoning ? ["low", "medium", "high"] : [];
}

export function deriveThinkingSupported(model: ProviderModel): boolean {
  return model.reasoning;
}

export function deriveAlwaysThinking(model: ProviderModel): boolean {
  return model.reasoning && model.thinkingLevelMap?.["off"] === null;
}

export function deriveDefaultThinkingEffort(model: ProviderModel): string {
  const efforts = deriveSupportEfforts(model);
  return efforts[Math.floor(efforts.length / 2)] ?? "on";
}

/** Read the live, authenticated runtime catalog. Config stores only selections. */
export async function listModelsFromHarness(
  harness: DimiHarness,
): Promise<readonly AcpModelEntry[]> {
  const models = await harness.auth.models();
  return models.map((model) => ({
    id: `${model.provider}/${model.id}`,
    name: model.name,
    thinkingSupported: deriveThinkingSupported(model),
    alwaysThinking: deriveAlwaysThinking(model),
    supportEfforts: deriveSupportEfforts(model),
    defaultThinkingEffort: deriveDefaultThinkingEffort(model),
  }));
}
