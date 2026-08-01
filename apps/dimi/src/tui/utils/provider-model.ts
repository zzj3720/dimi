import type { ModelAlias, ProviderModel } from '@dimi-agent/dimi-sdk';

export function providerModelToAlias(model: ProviderModel): ModelAlias {
  const configuredEfforts = Object.entries(model.thinkingLevelMap ?? {})
    .filter(([level, value]) => level !== 'off' && value !== null)
    .map(([level]) => level);
  const efforts =
    configuredEfforts.length > 0
      ? configuredEfforts
      : [];
  return {
    provider: model.provider,
    model: model.id,
    displayName: model.name,
    maxContextSize: model.contextWindow,
    maxOutputSize: model.maxTokens,
    capabilities: [
      model.input.includes('image') ? 'image_in' : undefined,
      model.reasoning
        ? model.thinkingLevelMap?.['off'] === null
          ? 'always_thinking'
          : 'thinking'
        : undefined,
      'tool_use',
    ].filter((value): value is string => value !== undefined),
    supportEfforts: efforts,
    defaultEffort: efforts.includes(model.defaultThinkingLevel ?? '')
      ? model.defaultThinkingLevel
      : efforts[Math.floor(efforts.length / 2)],
  };
}
