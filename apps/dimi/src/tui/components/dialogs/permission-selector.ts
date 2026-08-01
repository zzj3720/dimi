import type { PermissionMode } from '@dimi-agent/dimi-sdk';

import { ChoicePickerComponent, type ChoiceOption } from './choice-picker';

const PERMISSION_OPTIONS: readonly ChoiceOption[] = [
  {
    value: 'manual',
    label: 'Manual',
    description: 'Approve every action yourself.',
  },
  {
    value: 'yolo',
    label: 'YOLO',
    description: 'Auto-approve tool actions, but the agent may still ask questions.',
  },
  {
    value: 'auto',
    label: 'Auto',
    description: 'Fully autonomous — agent decides everything without asking.',
  },
];

function isPermissionModeChoice(value: string): value is PermissionMode {
  return value === 'manual' || value === 'auto' || value === 'yolo';
}

export interface PermissionSelectorOptions {
  readonly currentValue: PermissionMode;
  /** Enter: applies to the current session and saves as the default for new sessions. */
  readonly onSelect: (mode: PermissionMode) => void;
  /** When provided, Alt+S applies the choice to the current session only. */
  readonly onSessionOnlySelect?: (mode: PermissionMode) => void;
  readonly onCancel: () => void;
}

export class PermissionSelectorComponent extends ChoicePickerComponent {
  constructor(opts: PermissionSelectorOptions) {
    super({
      title: 'Select permission mode',
      options: [...PERMISSION_OPTIONS],
      currentValue: opts.currentValue,
      onSelect: (value) => {
        if (isPermissionModeChoice(value)) opts.onSelect(value);
      },
      onSessionOnlySelect:
        opts.onSessionOnlySelect === undefined
          ? undefined
          : (value) => {
              if (isPermissionModeChoice(value)) opts.onSessionOnlySelect!(value);
            },
      onCancel: opts.onCancel,
    });
  }
}
