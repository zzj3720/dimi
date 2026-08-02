import {
  CONTEXT_SIZE_FLOOR_TOKENS,
} from '@dimi-agent/agent-core-v2';

import { formatDecimalTokenCount } from '#/utils/usage/usage-format';

import { ChoicePickerComponent, type ChoiceOption } from './choice-picker';

export interface ContextSizeSelectorOptions {
  /** The model's default context window in tokens (100% base). */
  readonly contextWindow: number;
  /** Percentage levels offered by the picker (from `contextSizePercentOptions`). */
  readonly percentOptions: readonly number[];
  /** Currently configured percentage (defaults to 100). */
  readonly currentPercent: number;
  readonly onSelect: (percent: number) => void;
  readonly onCancel: () => void;
}

function buildOptions(
  contextWindow: number,
  percentOptions: readonly number[],
): readonly ChoiceOption[] {
  return percentOptions.map((percent) => ({
    value: String(percent),
    // The model's fixed window is always the 100% default; a user-selected
    // percentage only changes the effective size, never the default itself.
    label: percent === 100 ? '100% (default)' : `${percent}%`,
    description: `${formatDecimalTokenCount(Math.floor((contextWindow * percent) / 100))} tokens`,
  }));
}

/**
 * Vertical list of context-size levels for the current model, one entry per
 * 5% step from 100% down while the scaled window stays at or above
 * `CONTEXT_SIZE_FLOOR_TOKENS` (200k). The model's fixed window is the 100%
 * default; models whose window is already below the floor are not offered
 * here at all (the caller shows a notice instead).
 */
export class ContextSizeSelectorComponent extends ChoicePickerComponent {
  constructor(opts: ContextSizeSelectorOptions) {
    super({
      title: 'Context size',
      hint: `Model window ${formatDecimalTokenCount(opts.contextWindow)} · floor ${formatDecimalTokenCount(CONTEXT_SIZE_FLOOR_TOKENS)}`,
      options: buildOptions(opts.contextWindow, opts.percentOptions),
      currentValue: String(opts.currentPercent),
      onSelect: (value) => {
        opts.onSelect(Number(value));
      },
      onCancel: opts.onCancel,
    });
  }
}
