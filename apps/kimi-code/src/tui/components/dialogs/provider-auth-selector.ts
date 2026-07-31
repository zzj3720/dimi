import type { AuthType, ProviderAuthMethod, ProviderAuthState } from '@moonshot-ai/kimi-code-sdk';

import { ChoicePickerComponent, type ChoiceOption } from './choice-picker';

export interface ProviderAuthSelectorOptions {
  readonly title: string;
  readonly providers: readonly ProviderAuthState[];
  readonly authType?: AuthType;
  readonly onSelect: (providerId: string) => void;
  readonly onCancel: () => void;
}

export class ProviderAuthSelectorComponent extends ChoicePickerComponent {
  constructor(options: ProviderAuthSelectorOptions) {
    super({
      title: options.title,
      options: options.providers
        .filter(
          (provider) =>
            options.authType === undefined ||
            provider.methods.some((method) => method.type === options.authType),
        )
        .map((provider) => providerOption(provider, options.authType)),
      searchable: true,
      onSelect: options.onSelect,
      onCancel: options.onCancel,
    });
  }
}

export interface AuthTypeSelectorOptions {
  readonly providerName?: string;
  readonly methods: readonly ProviderAuthMethod[];
  readonly onSelect: (authType: AuthType) => void;
  readonly onCancel: () => void;
}

export class AuthTypeSelectorComponent extends ChoicePickerComponent {
  constructor(options: AuthTypeSelectorOptions) {
    const methods = uniqueMethods(options.methods);
    super({
      title:
        options.providerName === undefined
          ? 'Select authentication method'
          : `Select authentication method for ${options.providerName}`,
      options: methods.map((method) => ({
        value: method.type,
        label:
          options.providerName === undefined
            ? method.type === 'oauth'
              ? 'Sign in with an account'
              : 'Sign in with an API key'
            : method.label,
        description: method.type === 'oauth' ? 'OAuth subscription' : 'Stored API key',
      })),
      onSelect: (value) => {
        options.onSelect(value as AuthType);
      },
      onCancel: options.onCancel,
    });
  }
}

function providerOption(provider: ProviderAuthState, authType?: AuthType): ChoiceOption {
  const methods =
    authType === undefined
      ? provider.methods
      : provider.methods.filter((method) => method.type === authType);
  const methodLabel = methods.map((method) => method.label).join(' / ');
  const status = provider.configured
    ? `connected via ${provider.credentialType === 'oauth' ? 'OAuth' : 'API key'}${
        provider.source === undefined ? '' : ` (${provider.source})`
      }`
    : 'not connected';
  return {
    value: provider.id,
    label: provider.name,
    description: [provider.id, methodLabel, status].filter(Boolean).join(' · '),
  };
}

function uniqueMethods(methods: readonly ProviderAuthMethod[]): readonly ProviderAuthMethod[] {
  const seen = new Set<AuthType>();
  return methods.filter((method) => {
    if (seen.has(method.type)) return false;
    seen.add(method.type);
    return true;
  });
}
