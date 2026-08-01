import type {
  AuthInteraction,
  AuthPrompt,
  AuthType,
  CustomModelDefinition,
  CustomProviderDefinition,
  IProviderRuntime,
  Api,
  ProviderModel,
} from '@dimi-agent/agent-core-v2';
import {
  fetchCompleteFeedbackUpload,
  fetchCreateFeedbackUploadUrl,
  fetchManagedUsage,
  fetchSubmitFeedback,
  dimiCodeFeedbackUrl,
  dimiCodeUsageUrl,
  type FetchCompleteFeedbackUploadResult,
  type FetchFeedbackUploadError,
  type FetchManagedUsageError,
  type FetchSubmitFeedbackResult,
  type ParsedManagedUsage,
} from '@dimi-agent/dimi-oauth';

const DIMI_PROVIDER = 'kimi-coding';

export interface ProviderAuthMethod {
  readonly type: AuthType;
  readonly name: string;
  readonly label: string;
}

export interface ProviderAuthState {
  readonly id: string;
  readonly name: string;
  readonly configured: boolean;
  readonly credentialType?: AuthType;
  readonly source?: string;
  readonly methods: readonly ProviderAuthMethod[];
  readonly custom?: boolean;
}

export interface ProviderAuthStatus {
  readonly providers: readonly {
    readonly providerName: string;
    readonly hasToken: boolean;
  }[];
}

export interface ProviderLoginResult {
  readonly provider: string;
  readonly credentialType: AuthType;
  readonly models: readonly {
    readonly provider: string;
    readonly id: string;
    readonly name: string;
  }[];
}

export interface ProviderLogoutResult {
  readonly provider: string;
  readonly ok: true;
}

export type ProviderManagedUsageResult =
  | {
      readonly kind: 'ok';
      readonly summary: ParsedManagedUsage['summary'];
      readonly limits: ParsedManagedUsage['limits'];
      readonly extraUsage: ParsedManagedUsage['extraUsage'];
    }
  | FetchManagedUsageError;

export interface ProviderAuthFacadeOptions {
  readonly runtime: IProviderRuntime;
  /** Includes process-lifetime SDK extensions once the persisted catalog has loaded. */
  readonly ready?: Promise<void>;
}

export type CustomProviderInput = CustomProviderDefinition;

/** SDK facade over the runtime's provider, authentication, and model contracts. */
export class ProviderAuthFacade {
  private readonly ready: Promise<void>;

  constructor(private readonly options: ProviderAuthFacadeOptions) {
    this.ready = options.ready ?? options.runtime.ready;
  }

  async providers(): Promise<readonly ProviderAuthState[]> {
    await this.ready;
    await this.options.runtime.refreshProviderDefinitions();
    const customIds = new Set((await this.options.runtime.listCustomProviders()).map((item) => item.id));
    const credentials = new Map(
      (await this.options.runtime.listCredentials()).map((item) => [item.providerId, item.type]),
    );
    return Promise.all(
      this.options.runtime.getProviders().map(async (provider) => {
        const check = await this.options.runtime.checkAuth(provider.id);
        const methods: ProviderAuthMethod[] = [];
        if (provider.auth.oauth !== undefined) {
          methods.push({
            type: 'oauth',
            name: provider.auth.oauth.name,
            label: provider.auth.oauth.loginLabel ?? provider.auth.oauth.name,
          });
        }
        if (provider.auth.apiKey?.login !== undefined) {
          methods.push({
            type: 'api_key',
            name: provider.auth.apiKey.name,
            label: provider.auth.apiKey.name,
          });
        }
        return {
          id: provider.id,
          name: provider.name,
          configured: check !== undefined,
          credentialType: credentials.get(provider.id) ?? check?.type,
          source: check?.source,
          methods,
          custom: customIds.has(provider.id),
        };
      }),
    );
  }

  async status(providerId?: string): Promise<ProviderAuthStatus> {
    const providers = await this.providers();
    return {
      providers: providers
        .filter((provider) => providerId === undefined || provider.id === providerId)
        .map((provider) => ({
          providerName: provider.id,
          hasToken: provider.configured,
        })),
    };
  }

  async models(providerId?: string): Promise<readonly ProviderModel[]> {
    await this.ready;
    await this.options.runtime.refreshProviderDefinitions();
    return this.options.runtime.getAvailable(providerId);
  }

  async login(
    providerId: string,
    type: AuthType,
    interaction: AuthInteraction,
  ): Promise<ProviderLoginResult> {
    await this.ready;
    await this.options.runtime.login(providerId, type, interaction);
    await this.options.runtime.refresh({
      provider: providerId,
      allowNetwork: true,
      force: true,
      signal: interaction.signal,
    });
    const models = await this.options.runtime.getAvailable(providerId);
    return {
      provider: providerId,
      credentialType: type,
      models: models.map((model) => ({
        provider: model.provider,
        id: model.id,
        name: model.name,
      })),
    };
  }

  async logout(providerId: string): Promise<ProviderLogoutResult> {
    await this.options.runtime.logout(providerId);
    return { provider: providerId, ok: true };
  }

  async customProviders(): Promise<readonly CustomProviderDefinition[]> {
    await this.ready;
    return this.options.runtime.listCustomProviders();
  }

  async providerDefinitionDiagnostic(): Promise<string | undefined> {
    await this.ready;
    await this.options.runtime.refreshProviderDefinitions();
    return this.options.runtime.getProviderDefinitionDiagnostic();
  }

  /** Stream adapters accepted by the runtime that owns this harness. */
  providerApis(): readonly Api[] {
    return this.options.runtime.providerApis();
  }

  async upsertCustomProvider(definition: CustomProviderInput): Promise<void> {
    await this.options.runtime.upsertCustomProvider(definition);
  }

  async deleteCustomProvider(providerId: string): Promise<void> {
    await this.options.runtime.deleteCustomProvider(providerId);
  }

  async upsertCustomModel(providerId: string, model: CustomModelDefinition): Promise<void> {
    const provider = await this.requireCustomProvider(providerId);
    const models = new Map((provider.models ?? []).map((item) => [item.id, item]));
    const existing = models.get(model.id);
    // Public model update commands carry only the changed fields.  Preserve
    // every existing capability/request field unless the caller explicitly
    // supplies a new value; the runtime validates complete new models below.
    models.set(model.id, existing === undefined ? model : mergeModelDefinition(existing, model));
    await this.upsertCustomProvider({ ...provider, models: [...models.values()] });
  }

  async deleteCustomModel(providerId: string, modelId: string): Promise<void> {
    const provider = await this.requireCustomProvider(providerId);
    const existing = provider.models ?? [];
    const models = existing.filter((model) => model.id !== modelId);
    if (models.length === existing.length) return;
    // A built-in overlay may legitimately remove its final custom model and
    // fall back to the built-in catalog.  `upsertCustomProvider` is the sole
    // authority for whether an independent provider still has enough shape.
    await this.upsertCustomProvider({ ...provider, models });
  }

  async refreshModels(options?: {
    readonly provider?: string;
    readonly force?: boolean;
    readonly signal?: AbortSignal;
  }) {
    await this.ready;
    await this.options.runtime.refreshProviderDefinitions();
    return this.options.runtime.refresh({
      provider: options?.provider,
      allowNetwork: true,
      force: options?.force,
      signal: options?.signal,
    });
  }

  async getAccessToken(providerId: string = DIMI_PROVIDER): Promise<string | undefined> {
    const auth = (await this.options.runtime.getAuth(providerId))?.auth;
    if (auth?.apiKey !== undefined) return auth.apiKey;
    const authorization = Object.entries(auth?.headers ?? {}).find(
      ([name]) => name.toLowerCase() === 'authorization',
    )?.[1];
    return authorization?.match(/^Bearer\s+(.+)$/iu)?.[1];
  }

  readonly resolveTokenProvider = (
    providerId: string,
  ): {
    getAccessToken(options?: { readonly force?: boolean }): Promise<string>;
  } => ({
    getAccessToken: async () => {
      const token = await this.getAccessToken(providerId);
      if (token === undefined) throw new Error(`Provider is not authenticated: ${providerId}`);
      return token;
    },
  });

  private async requireCustomProvider(providerId: string): Promise<CustomProviderDefinition> {
    const provider = (await this.customProviders()).find((item) => item.id === providerId);
    if (provider === undefined) throw new Error(`Custom provider does not exist: ${providerId}`);
    return provider;
  }

  async getManagedUsage(providerId: string = DIMI_PROVIDER): Promise<ProviderManagedUsageResult> {
    try {
      const token = await this.requireToken(providerId);
      const result = await fetchManagedUsage(dimiCodeUsageUrl(), token);
      return result.kind === 'error'
        ? result
        : {
            kind: 'ok',
            summary: result.parsed.summary,
            limits: result.parsed.limits,
            extraUsage: result.parsed.extraUsage,
          };
    } catch (error) {
      return { kind: 'error', message: error instanceof Error ? error.message : String(error) };
    }
  }

  async submitFeedback(
    input: ProviderAuthSubmitFeedbackInput,
    providerId: string = DIMI_PROVIDER,
  ): Promise<FetchSubmitFeedbackResult> {
    try {
      return await fetchSubmitFeedback(dimiCodeFeedbackUrl(), await this.requireToken(providerId), {
        session_id: input.sessionId,
        content: input.content,
        version: input.version,
        os: input.os,
        model: input.model,
        contact: input.contact,
        info: input.info,
      });
    } catch (error) {
      return { kind: 'error', message: error instanceof Error ? error.message : String(error) };
    }
  }

  async createFeedbackUploadUrl(
    input: ProviderAuthCreateFeedbackUploadUrlInput,
    providerId: string = DIMI_PROVIDER,
  ): Promise<ProviderAuthCreateFeedbackUploadUrlResult> {
    try {
      const result = await fetchCreateFeedbackUploadUrl(await this.requireToken(providerId), {
        file_hash: input.sha256,
        file_name: input.filename,
        file_size: input.size,
        feedback_id: input.feedbackId,
      });
      if (result.kind === 'error') return result;
      return {
        kind: 'ok',
        uploadId: result.upload_id,
        parts: result.parts.map((part) => ({
          partNumber: part.part_number,
          url: part.url,
          method: part.method,
          size: part.size,
        })),
      };
    } catch (error) {
      return { kind: 'error', message: error instanceof Error ? error.message : String(error) };
    }
  }

  async completeFeedbackUpload(
    input: ProviderAuthCompleteFeedbackUploadInput,
    providerId: string = DIMI_PROVIDER,
  ): Promise<FetchCompleteFeedbackUploadResult> {
    try {
      return await fetchCompleteFeedbackUpload(await this.requireToken(providerId), {
        upload_id: input.uploadId,
        parts: input.parts.map((part) => ({
          part_number: part.partNumber,
          etag: part.etag,
        })),
      });
    } catch (error) {
      return { kind: 'error', message: error instanceof Error ? error.message : String(error) };
    }
  }

  private async requireToken(providerId: string): Promise<string> {
    const token = await this.getAccessToken(providerId);
    if (token === undefined) throw new Error(`Provider is not authenticated: ${providerId}`);
    return token;
  }
}

function mergeModelDefinition(
  existing: CustomModelDefinition,
  update: CustomModelDefinition,
): CustomModelDefinition {
  const merged: Record<string, unknown> = { ...existing };
  for (const [key, value] of Object.entries(update)) {
    if (value !== undefined) merged[key] = value;
  }
  return merged as unknown as CustomModelDefinition;
}

export type ProviderAuthPrompt = AuthPrompt;

export interface ProviderAuthSubmitFeedbackInput {
  readonly content: string;
  readonly sessionId: string;
  readonly version: string;
  readonly os: string;
  readonly model: string | null;
  readonly contact?: string;
  readonly info?: Record<string, unknown>;
}

export interface ProviderAuthCreateFeedbackUploadUrlInput {
  readonly feedbackId: number;
  readonly filename: string;
  readonly size: number;
  readonly sha256: string;
}

export interface ProviderAuthCompleteFeedbackUploadPart {
  readonly partNumber: number;
  readonly etag: string;
}

export interface ProviderAuthCompleteFeedbackUploadInput {
  readonly uploadId: number;
  readonly parts: readonly ProviderAuthCompleteFeedbackUploadPart[];
}

export interface ProviderAuthFeedbackUploadPart {
  readonly partNumber: number;
  readonly url: string;
  readonly method: string;
  readonly size: number;
}

export interface ProviderAuthCreateFeedbackUploadUrlOk {
  readonly kind: 'ok';
  readonly uploadId: number;
  readonly parts: readonly ProviderAuthFeedbackUploadPart[];
}

export type ProviderAuthCreateFeedbackUploadUrlResult =
  | ProviderAuthCreateFeedbackUploadUrlOk
  | FetchFeedbackUploadError;
