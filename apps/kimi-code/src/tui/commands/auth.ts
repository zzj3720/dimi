import type { ProviderAuthMethod, ProviderAuthState } from '@moonshot-ai/kimi-code-sdk';
import { log } from '@moonshot-ai/kimi-code-sdk';

import { ProviderLoginDialogComponent } from '../components/dialogs/provider-login-dialog';
import { formatErrorMessage } from '../utils/event-payload';
import { openAuthEventUrl } from '../utils/provider-auth';
import { providerModelToAlias } from '../utils/provider-model';
import {
  promptAuthTypeSelection,
  promptLogoutProviderSelection,
  promptProviderAuthSelection,
  runModelSelector,
} from './prompts';
import type { SlashCommandHost } from './dispatch';

interface LoginTarget {
  readonly provider: ProviderAuthState;
  readonly method: ProviderAuthMethod;
}

export async function handleLoginCommand(
  host: SlashCommandHost,
  providerReference = '',
): Promise<void> {
  const providers = (await host.harness.auth.providers()).filter(
    (provider) => provider.methods.length > 0,
  );
  const target = await selectLoginTarget(host, providers, providerReference);
  if (target === undefined) return;
  await login(host, target);
}

export async function handleLogoutCommand(host: SlashCommandHost): Promise<void> {
  const providers = (await host.harness.auth.providers()).filter((provider) => provider.configured);
  if (providers.length === 0) {
    host.showStatus(
      'No stored credentials to remove. Environment-provided credentials are unchanged.',
    );
    return;
  }

  const currentAlias = host.state.appState.model;
  const currentProvider = host.state.appState.availableModels[currentAlias]?.provider;
  const target = await promptLogoutProviderSelection(
    host,
    providers.map((provider) => ({
      value: provider.id,
      label: provider.name,
      description: [provider.credentialType === 'oauth' ? 'OAuth' : 'API key', provider.source]
        .filter(Boolean)
        .join(' · '),
    })),
    currentProvider,
  );
  if (target === undefined) return;

  await host.harness.auth.logout(target);
  await host.authFlow.refreshAvailableModels();
  if (target === currentProvider) {
    await host.authFlow.refreshConfigAfterLogout();
    await host.authFlow.clearActiveSessionAfterLogout();
  }
  host.track('logout', { provider: target });
  host.showStatus(
    `Logged out from ${providers.find((provider) => provider.id === target)?.name ?? target}.`,
  );
}

async function selectLoginTarget(
  host: SlashCommandHost,
  providers: readonly ProviderAuthState[],
  providerReference: string,
): Promise<LoginTarget | undefined> {
  const reference = providerReference.trim().toLowerCase();
  if (reference.length > 0) {
    const provider = providers.find(
      (entry) => entry.id.toLowerCase() === reference || entry.name.toLowerCase() === reference,
    );
    if (provider === undefined) {
      host.showError(`Unknown provider: ${providerReference.trim()}`);
      return undefined;
    }
    const method = await selectMethod(host, provider.methods, provider.name);
    return method === undefined ? undefined : { provider, method };
  }

  const authType = await promptAuthTypeSelection(
    host,
    providers.flatMap((provider) => provider.methods),
  );
  if (authType === undefined) return undefined;
  const providerId = await promptProviderAuthSelection(host, providers, authType);
  if (providerId === undefined) return undefined;
  const provider = providers.find((entry) => entry.id === providerId);
  const method = provider?.methods.find((entry) => entry.type === authType);
  return provider === undefined || method === undefined ? undefined : { provider, method };
}

async function selectMethod(
  host: SlashCommandHost,
  methods: readonly ProviderAuthMethod[],
  providerName: string,
): Promise<ProviderAuthMethod | undefined> {
  if (methods.length === 1) return methods[0];
  const authType = await promptAuthTypeSelection(host, methods, providerName);
  return methods.find((method) => method.type === authType);
}

async function login(host: SlashCommandHost, target: LoginTarget): Promise<void> {
  const { provider, method } = target;
  const controller = new AbortController();
  const cancelLogin = (): void => {
    controller.abort();
  };
  host.cancelInFlight = cancelLogin;
  let dialogMounted = true;
  const dialog = new ProviderLoginDialogComponent({
    providerName: provider.name,
    methodLabel: method.label,
    requestRender: () => {
      host.state.ui.requestRender();
    },
    onCancel: cancelLogin,
  });
  host.mountEditorReplacement(dialog);

  try {
    await host.harness.auth.login(provider.id, method.type, {
      signal: controller.signal,
      prompt: (prompt) => dialog.prompt(prompt),
      notify: (event) => {
        openAuthEventUrl(event);
        dialog.notify(event);
      },
    });
    host.restoreEditor();
    dialogMounted = false;
    host.track('login', { provider: provider.id, method: method.type });

    const models = await host.harness.auth.models(provider.id);
    if (models.length === 0) {
      host.showStatus(`Connected to ${provider.name}, but no models are currently available.`);
      await host.authFlow.refreshAvailableModels();
      return;
    }
    const modelDict = Object.fromEntries(
      models.map((model) => [`${model.provider}/${model.id}`, providerModelToAlias(model)]),
    );
    const selection = await runModelSelector(host, modelDict);
    if (selection === undefined) {
      await host.authFlow.refreshAvailableModels();
      host.showStatus(`Connected to ${provider.name}. Use /model to select a model.`);
      return;
    }
    const selected = models.find((model) => `${model.provider}/${model.id}` === selection.alias);
    if (selected === undefined) return;
    await host.harness.setConfig({
      defaultProvider: selected.provider,
      defaultModel: selected.id,
      thinking: {
        enabled: selection.thinking !== 'off',
        effort:
          selection.thinking === 'off' || selection.thinking === 'on'
            ? undefined
            : selection.thinking,
      },
    });
    await host.authFlow.refreshAvailableModels();
    await host.authFlow.activateModelAfterLogin(selection.alias, selection.thinking);
    host.setAppState({
      model: selection.alias,
      thinkingEffort: selection.thinking,
      maxContextTokens: selected.contextWindow,
    });
    host.showStatus(`Connected to ${provider.name} · ${selected.name}.`);
  } catch (error) {
    if (dialogMounted) {
      host.restoreEditor();
      dialogMounted = false;
    }
    if (controller.signal.aborted) {
      host.showStatus(`Login to ${provider.name} cancelled.`);
      return;
    }
    log.warn('provider login failed', {
      providerId: provider.id,
      methodType: method.type,
      sessionId: host.session?.id,
      error,
    });
    host.showError(`Login failed: ${formatErrorMessage(error)}`);
  } finally {
    if (dialogMounted) host.restoreEditor();
    if (host.cancelInFlight === cancelLogin) host.cancelInFlight = undefined;
  }
}
