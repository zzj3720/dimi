import { access } from "node:fs/promises";

import { ProviderRuntimeErrors } from "./errors";
import { InMemoryCredentialStore, InMemoryModelsStore } from "./storage";
import { Error2 } from "#/_base/errors/errors";
import type {
  AssistantMessage,
  AssistantMessageEvent,
  AuthCheck,
  AuthContext,
  AuthInteraction,
  AuthResult,
  AuthType,
  Credential,
  CredentialStore,
  Model,
  Models,
  MutableModels,
  ModelsRefreshOptions,
  ModelsRefreshResult,
  ModelsSimpleStreamOptions,
  ModelsStore,
  Provider,
} from "./types";

const OAUTH_REFRESH_SKEW_MS = 5 * 60_000;

export interface CreateModelsOptions {
  providers?: readonly Provider[];
  credentials?: CredentialStore;
  modelsStore?: ModelsStore;
  authContext?: AuthContext;
}

export function createModels(options: CreateModelsOptions = {}): MutableModels {
  return new ProviderModels(
    options.providers ?? [],
    options.credentials ?? new InMemoryCredentialStore(),
    options.modelsStore ?? new InMemoryModelsStore(),
    options.authContext,
  );
}

export class ProviderModels implements MutableModels {
  private readonly providers = new Map<string, Provider>();

  constructor(
    providers: readonly Provider[],
    private readonly credentials: CredentialStore,
    private readonly modelsStore: ModelsStore,
    private readonly authContext: AuthContext = defaultAuthContext(),
  ) {
    for (const provider of providers) this.setProvider(provider);
  }

  setProvider(provider: Provider): void {
    this.providers.set(provider.id, provider);
  }

  deleteProvider(id: string): void {
    this.providers.delete(id);
  }

  clearProviders(): void {
    this.providers.clear();
  }

  getProviders(): readonly Provider[] {
    return [...this.providers.values()];
  }

  getProvider(id: string): Provider | undefined {
    return this.providers.get(id);
  }

  getModels(provider?: string): readonly Model[] {
    if (provider !== undefined) {
      const entry = this.providers.get(provider);
      if (entry === undefined) return [];
      try {
        return entry.getModels();
      } catch {
        return [];
      }
    }
    return [...this.providers.values()].flatMap((entry) => {
      try {
        return [...entry.getModels()];
      } catch {
        return [];
      }
    });
  }

  getModel(provider: string, id: string): Model | undefined {
    return this.getModels(provider).find((model) => model.id === id);
  }

  async refresh(options: ModelsRefreshOptions = {}): Promise<ModelsRefreshResult> {
    const errors = new Map<string, Error>();
    const allowNetwork = options.allowNetwork ?? true;
    const providers =
      options.provider === undefined
        ? [...this.providers.values()]
        : [this.providers.get(options.provider)].filter(
            (provider): provider is Provider => provider !== undefined,
          );
    await Promise.all(
      providers.map(async (provider) => {
        if (provider.refreshModels === undefined || options.signal?.aborted) return;
        const store = {
          read: () => this.modelsStore.read(provider.id),
          write: (entry: Parameters<ModelsStore["write"]>[1]) =>
            this.modelsStore.write(provider.id, entry),
          delete: () => this.modelsStore.delete(provider.id),
        };
        try {
          const auth = allowNetwork ? await this.getAuth(provider.id) : undefined;
          if (allowNetwork && auth === undefined) return;
          await provider.refreshModels({
            auth,
            store,
            allowNetwork,
            force: options.force,
            signal: options.signal,
          });
        } catch (error) {
          if (options.signal?.aborted) return;
          errors.set(provider.id, error instanceof Error ? error : new Error(String(error)));
          try {
            await provider.refreshModels({
              store,
              allowNetwork: false,
              signal: options.signal,
            });
          } catch {
            // Keep the original auth/network failure; cache restoration is best-effort.
          }
        }
      }),
    );
    return { aborted: options.signal?.aborted ?? false, errors };
  }

  async checkAuth(providerId: string): Promise<AuthCheck | undefined> {
    const provider = this.providers.get(providerId);
    if (provider === undefined) return undefined;
    return this.checkProviderAuth(provider, await this.readCredential(providerId));
  }

  async getAvailable(providerId?: string): Promise<readonly Model[]> {
    const providers =
      providerId === undefined
        ? [...this.providers.values()]
        : [this.providers.get(providerId)].filter(
            (provider): provider is Provider => provider !== undefined,
          );
    const available = await Promise.all(
      providers.map(async (provider) => {
        const credential = await this.readCredential(provider.id);
        if ((await this.checkProviderAuth(provider, credential)) === undefined) return [];
        const models = this.getModels(provider.id);
        return provider.filterModels?.(models, credential) ?? models;
      }),
    );
    return available.flat();
  }

  private async checkProviderAuth(
    provider: Provider,
    credential: Credential | undefined,
  ): Promise<AuthCheck | undefined> {
    if (credential !== undefined) {
      if (credential.type === "oauth") {
        return provider.auth.oauth === undefined ? undefined : { type: "oauth", source: "OAuth" };
      }
      if (provider.auth.apiKey === undefined) return undefined;
      return this.checkApiKeyAuth(provider, credential);
    }
    const apiKey = provider.auth.apiKey;
    if (apiKey === undefined) return undefined;
    return this.checkApiKeyAuth(provider, undefined);
  }

  private async checkApiKeyAuth(
    provider: Provider,
    credential: Extract<Credential, { type: "api_key" }> | undefined,
  ): Promise<AuthCheck | undefined> {
    try {
      const apiKey = provider.auth.apiKey!;
      const checked = await apiKey.check?.({
        ctx: this.authContext,
        credential,
      });
      if (checked !== undefined) return checked;
      const resolved = await apiKey.resolve({
        ctx: this.authContext,
        credential,
      });
      return resolved === undefined
        ? undefined
        : { type: "api_key", source: resolved.source ?? "API key" };
    } catch (error) {
      throw authError(`API key authentication check failed for ${provider.id}`, error);
    }
  }

  getAuth(providerId: string): Promise<AuthResult | undefined>;
  getAuth(model: Model): Promise<AuthResult | undefined>;
  async getAuth(providerOrModel: string | Model): Promise<AuthResult | undefined> {
    const providerId =
      typeof providerOrModel === "string" ? providerOrModel : providerOrModel.provider;
    const provider = this.providers.get(providerId);
    if (provider === undefined) return undefined;
    const credential = await this.readCredential(providerId);
    if (credential !== undefined) {
      if (credential.type === "oauth") {
        if (provider.auth.oauth === undefined) return undefined;
        let resolved: Credential | undefined = credential;
        if (credential.expires <= Date.now() + OAUTH_REFRESH_SKEW_MS) {
          try {
            resolved = await this.credentials.modify(providerId, async (current) => {
              if (current?.type !== "oauth") return undefined;
              if (current.expires > Date.now() + OAUTH_REFRESH_SKEW_MS) return undefined;
              try {
                return await provider.auth.oauth!.refresh(current);
              } catch (error) {
                throw authError(`OAuth refresh failed for ${providerId}`, error);
              }
            });
          } catch (error) {
            throw authError(`Credential update failed for ${providerId}`, error);
          }
        }
        if (resolved?.type !== "oauth") return undefined;
        try {
          return {
            auth: await provider.auth.oauth.toAuth(resolved),
            source: "OAuth",
          };
        } catch (error) {
          throw authError(`OAuth authentication failed for ${providerId}`, error);
        }
      }
      if (provider.auth.apiKey === undefined) return undefined;
      return this.resolveApiKeyAuth(provider, credential);
    }
    const apiKey = provider.auth.apiKey;
    if (apiKey === undefined) return undefined;
    return this.resolveApiKeyAuth(provider, undefined);
  }

  private async resolveApiKeyAuth(
    provider: Provider,
    credential: Extract<Credential, { type: "api_key" }> | undefined,
  ): Promise<AuthResult | undefined> {
    try {
      return await provider.auth.apiKey!.resolve({
        ctx: this.authContext,
        credential,
      });
    } catch (error) {
      throw authError(`API key authentication failed for ${provider.id}`, error);
    }
  }

  private async readCredential(providerId: string): Promise<Credential | undefined> {
    try {
      return await this.credentials.read(providerId);
    } catch (error) {
      throw authError(`Credential read failed for ${providerId}`, error);
    }
  }

  async login(
    providerId: string,
    type: AuthType,
    interaction: AuthInteraction,
  ): Promise<Credential> {
    const provider = this.providers.get(providerId);
    if (provider === undefined) {
      throw new Error2(
        ProviderRuntimeErrors.codes.PROVIDER_API_ERROR,
        `Unknown provider: ${providerId}`,
      );
    }
    const credential =
      type === "oauth"
        ? await provider.auth.oauth?.login(interaction)
        : await provider.auth.apiKey?.login?.(interaction);
    if (credential === undefined) {
      throw new Error2(
        ProviderRuntimeErrors.codes.PROVIDER_API_ERROR,
        `Provider ${providerId} does not support ${type} login`,
      );
    }
    try {
      await this.credentials.modify(providerId, async () => credential);
    } catch (error) {
      throw authError(`Credential update failed for ${providerId}`, error);
    }
    return credential;
  }

  async logout(providerId: string): Promise<void> {
    try {
      await this.credentials.delete(providerId);
    } catch (error) {
      throw authError(`Credential delete failed for ${providerId}`, error);
    }
  }

  async *streamSimple(
    model: Model,
    context: Parameters<Models["streamSimple"]>[1],
    options?: ModelsSimpleStreamOptions,
  ): AsyncIterable<AssistantMessageEvent> {
    const provider = this.providers.get(model.provider);
    if (provider === undefined) {
      throw new Error2(
        ProviderRuntimeErrors.codes.PROVIDER_API_ERROR,
        `Unknown provider: ${model.provider}`,
      );
    }
    const auth = await this.getAuth(model);
    if (auth === undefined) {
      throw new Error2(
        ProviderRuntimeErrors.codes.AUTH_LOGIN_REQUIRED,
        `Provider ${model.provider} is not authenticated`,
        { details: { provider_id: model.provider, model_id: model.id } },
      );
    }
    yield* provider.stream(model, context, auth, options);
  }

  async completeSimple(
    model: Model,
    context: Parameters<Models["completeSimple"]>[1],
    options?: ModelsSimpleStreamOptions,
  ): Promise<AssistantMessage> {
    let final: AssistantMessage | undefined;
    for await (const event of this.streamSimple(model, context, options)) {
      if (event.type === "done") final = event.message;
      if (event.type === "error") throw new Error(event.error.errorMessage ?? "Provider error");
    }
    if (final === undefined) throw new Error("Provider stream ended without a final message");
    return final;
  }
}

function authError(message: string, cause: unknown): Error2 {
  if (cause instanceof Error2 && cause.code === ProviderRuntimeErrors.codes.PROVIDER_AUTH_ERROR) {
    return cause;
  }
  const reason = cause instanceof Error ? cause.message : String(cause);
  return new Error2(ProviderRuntimeErrors.codes.PROVIDER_AUTH_ERROR, `${message}: ${reason}`, {
    cause,
  });
}

function defaultAuthContext(): AuthContext {
  return {
    env: async (name) => process.env[name],
    fileExists: async (path) => {
      try {
        await access(path.replace(/^~(?=\/)/, process.env["HOME"] ?? "~"));
        return true;
      } catch {
        return false;
      }
    },
  };
}
