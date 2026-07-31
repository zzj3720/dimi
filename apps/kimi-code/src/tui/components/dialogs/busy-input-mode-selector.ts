import type { BusyInputMode } from '../../config';
import { ChoicePickerComponent, type ChoiceOption } from './choice-picker';

const BUSY_INPUT_MODE_OPTIONS: readonly ChoiceOption[] = [
  {
    value: 'steer',
    label: 'Steer (default)',
    description: 'Enter injects into the current turn immediately (same as Ctrl-S).',
  },
  {
    value: 'queue',
    label: 'Queue',
    description: 'Enter queues for after the current task; Ctrl-S steers immediately.',
  },
];

function isBusyInputMode(value: string): value is BusyInputMode {
  return value === 'queue' || value === 'steer';
}

export interface BusyInputModeSelectorOptions {
  readonly currentValue: BusyInputMode;
  readonly onSelect: (value: BusyInputMode) => void;
  readonly onCancel: () => void;
}

export class BusyInputModeSelectorComponent extends ChoicePickerComponent {
  constructor(opts: BusyInputModeSelectorOptions) {
    super({
      title: 'Busy input',
      options: [...BUSY_INPUT_MODE_OPTIONS],
      currentValue: opts.currentValue,
      onSelect: (value) => {
        if (isBusyInputMode(value)) opts.onSelect(value);
      },
      onCancel: opts.onCancel,
    });
  }
}
