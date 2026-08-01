import { describe, expect, it } from 'vitest';

import {
  isThinkingOn,
  modelEffortKey,
  rememberedEffortFromConfig,
  thinkingEffortFromConfig,
  thinkingEffortToConfig,
} from '@/tui/utils/thinking-config';

describe('thinkingEffortToConfig', () => {
  it.each([
    ['off', { enabled: false }],
    // 'on' is the boolean-model on-signal, not a declared effort. It must not
    // be persisted as `thinking.effort` — boolean models have no effort concept
    // and resolve back to 'on' at runtime via defaultThinkingEffortFor.
    ['on', { enabled: true }],
    ['low', { enabled: true, effort: 'low' }],
    ['high', { enabled: true, effort: 'high' }],
    ['max', { enabled: true, effort: 'max' }],
  ] as const)('maps %s → %o without model efforts', (effort, expected) => {
    expect(thinkingEffortToConfig(effort)).toEqual(expected);
  });

  it.each([
    // Every concrete level — including the model's highest declared tier —
    // persists as the global default so new sessions resume the chosen effort.
    ['low', { enabled: true, effort: 'low' }],
    ['high', { enabled: true, effort: 'high' }],
    ['max', { enabled: true, effort: 'max' }],
    // Undeclared values persist as-is (the provider validates them).
    ['ultra', { enabled: true, effort: 'ultra' }],
  ] as const)('maps %s → %o for [low, high, max]', (effort, expected) => {
    expect(thinkingEffortToConfig(effort)).toEqual(expected);
  });

  it('persists a single declared level instead of treating it as session-only', () => {
    expect(thinkingEffortToConfig('max')).toEqual({ enabled: true, effort: 'max' });
  });
});

describe('isThinkingOn', () => {
  it.each([
    ['off', false],
    ['on', true],
    ['low', true],
    ['high', true],
    ['max', true],
  ] as const)('%s → %s', (effort, expected) => {
    expect(isThinkingOn(effort)).toBe(expected);
  });
});

describe('thinkingEffortFromConfig', () => {
  it.each([
    [undefined, undefined],
    [{}, undefined],
    // enabled with no concrete effort → let the model's own default apply.
    [{ enabled: true }, undefined],
    [{ enabled: false }, 'off'],
    [{ enabled: true, effort: 'high' }, 'high'],
    // effort is honored even when enabled is not explicitly set.
    [{ effort: 'max' }, 'max'],
  ] as const)('%o → %s', (config, expected) => {
    expect(thinkingEffortFromConfig(config)).toBe(expected);
  });
});

describe('modelEffortKey', () => {
  it('joins provider and model into the [model_efforts] key', () => {
    expect(modelEffortKey('anthropic', 'claude-sonnet-4-5')).toBe('anthropic/claude-sonnet-4-5');
  });
});

describe('rememberedEffortFromConfig', () => {
  it('returns the effort recorded for the (provider, model) pair', () => {
    const config = {
      modelEfforts: { 'anthropic/claude-sonnet-4-5': 'high', 'kimi-coding/kimi-k2.5': 'off' },
    };
    expect(
      rememberedEffortFromConfig(config, { provider: 'anthropic', model: 'claude-sonnet-4-5' }),
    ).toBe('high');
    expect(rememberedEffortFromConfig(config, { provider: 'kimi-coding', model: 'kimi-k2.5' })).toBe(
      'off',
    );
  });

  it('returns undefined for models without a recorded effort or an unknown model', () => {
    const config = { modelEfforts: { 'anthropic/claude-sonnet-4-5': 'high' } };
    expect(rememberedEffortFromConfig(config, { provider: 'anthropic', model: 'other' })).toBe(
      undefined,
    );
    expect(rememberedEffortFromConfig(config, undefined)).toBe(undefined);
    expect(rememberedEffortFromConfig({}, { provider: 'a', model: 'b' })).toBe(undefined);
  });
});
