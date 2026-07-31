import type {
  AuthType,
  ModelAlias,
  ProviderAuthMethod,
  ProviderAuthState,
  ThinkingEffort,
} from '@moonshot-ai/kimi-code-sdk';

import { ChoicePickerComponent, type ChoiceOption } from '../components/dialogs/choice-picker';
import {
  FeedbackInputDialogComponent,
  type FeedbackInputDialogResult,
} from '../components/dialogs/feedback-input-dialog';
import { ModelSelectorComponent } from '../components/dialogs/model-selector';
import {
  AuthTypeSelectorComponent,
  ProviderAuthSelectorComponent,
} from '../components/dialogs/provider-auth-selector';
import type { SlashCommandHost } from './dispatch';

export function promptLogoutProviderSelection(
  host: SlashCommandHost,
  options: readonly ChoiceOption[],
  currentValue: string | undefined,
): Promise<string | undefined> {
  return new Promise((resolve) => {
    const picker = new ChoicePickerComponent({
      title: 'Select a provider to log out',
      options,
      currentValue,
      onSelect: (value) => {
        host.restoreEditor();
        resolve(value);
      },
      onCancel: () => {
        host.restoreEditor();
        resolve(undefined);
      },
    });
    host.mountEditorReplacement(picker);
  });
}

export function promptAuthTypeSelection(
  host: SlashCommandHost,
  methods: readonly ProviderAuthMethod[],
  providerName?: string,
): Promise<AuthType | undefined> {
  return new Promise((resolve) => {
    host.mountEditorReplacement(
      new AuthTypeSelectorComponent({
        providerName,
        methods,
        onSelect: (value) => {
          host.restoreEditor();
          resolve(value);
        },
        onCancel: () => {
          host.restoreEditor();
          resolve(undefined);
        },
      }),
    );
  });
}

export function promptProviderAuthSelection(
  host: SlashCommandHost,
  providers: readonly ProviderAuthState[],
  authType?: AuthType,
  title = authType === undefined
    ? 'Select a provider'
    : authType === 'oauth'
    ? 'Select an account provider'
    : 'Select an API key provider',
): Promise<string | undefined> {
  return new Promise((resolve) => {
    host.mountEditorReplacement(
      new ProviderAuthSelectorComponent({
        title,
        providers,
        authType,
        onSelect: (value) => {
          host.restoreEditor();
          resolve(value);
        },
        onCancel: () => {
          host.restoreEditor();
          resolve(undefined);
        },
      }),
    );
  });
}

export function promptProviderSelection(
  host: SlashCommandHost,
  providers: readonly ProviderAuthState[],
): Promise<string | undefined> {
  return new Promise((resolve) => {
    host.mountEditorReplacement(
      new ProviderAuthSelectorComponent({
        title: 'Providers',
        providers,
        onSelect: (value) => {
          host.restoreEditor();
          resolve(value);
        },
        onCancel: () => {
          host.restoreEditor();
          resolve(undefined);
        },
      }),
    );
  });
}

export interface FeedbackPromptResult {
  readonly value: string;
}

export function promptFeedbackInput(
  host: SlashCommandHost,
): Promise<FeedbackPromptResult | undefined> {
  return new Promise((resolve) => {
    const dialog = new FeedbackInputDialogComponent((result: FeedbackInputDialogResult) => {
      host.restoreEditor();
      resolve(result.kind === 'ok' ? { value: result.value } : undefined);
    });
    host.mountEditorReplacement(dialog);
  });
}

export type FeedbackAttachmentLevel = 'none' | 'logs' | 'logs+codebase';

const FEEDBACK_ATTACHMENT_OPTIONS: readonly ChoiceOption[] = [
  { value: 'none', label: 'No attachment', description: 'Text feedback only' },
  {
    value: 'logs',
    label: 'Logs only',
    description: 'Upload wire events and diagnostic logs from this session',
  },
  {
    value: 'logs+codebase',
    label: 'Logs + codebase',
    description:
      'Include your codebase for deeper diagnosis. Sensitive files are automatically excluded — e.g. .env, config files, secret keys. We use attachments only for diagnosis and never share them.',
    descriptionTone: 'warning',
  },
];

export function promptFeedbackAttachment(
  host: SlashCommandHost,
): Promise<FeedbackAttachmentLevel | undefined> {
  return new Promise((resolve) => {
    const picker = new ChoicePickerComponent({
      title: 'Share diagnostic info to help us investigate?',
      options: FEEDBACK_ATTACHMENT_OPTIONS,
      onSelect: (value) => {
        host.restoreEditor();
        resolve(value as FeedbackAttachmentLevel);
      },
      onCancel: () => {
        host.restoreEditor();
        resolve(undefined);
      },
    });
    host.mountEditorReplacement(picker);
  });
}

export function runModelSelector(
  host: SlashCommandHost,
  modelDict: Record<string, ModelAlias>,
): Promise<{ alias: string; thinking: ThinkingEffort } | undefined> {
  return new Promise((resolve) => {
    const firstAlias = Object.keys(modelDict)[0] ?? '';
    const firstModel = modelDict[firstAlias];
    const caps = firstModel?.capabilities ?? [];
    const initialThinking = caps.includes('always_thinking') || caps.includes('thinking');
    const selector = new ModelSelectorComponent({
      models: modelDict,
      currentValue: firstAlias,
      currentThinkingEffort: initialThinking ? firstModel?.defaultEffort ?? 'on' : 'off',
      searchable: true,
      onSelect: ({ alias, thinking }) => {
        host.restoreEditor();
        resolve({ alias, thinking });
      },
      onCancel: () => {
        host.restoreEditor();
        resolve(undefined);
      },
    });
    host.mountEditorReplacement(selector);
  });
}
