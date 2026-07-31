import type {
  AuthInteraction,
  AuthPrompt,
  AuthType,
  IProviderRuntime,
  ProviderModel,
} from '@moonshot-ai/agent-core-v2';
import {
  fetchCompleteFeedbackUpload,
  fetchCreateFeedbackUploadUrl,
  fetchManagedUsage,
  fetchSubmitFeedback,
  kimiCodeFeedbackUrl,
  kimiCodeUsageUrl,
  type FetchCompleteFeedbackUploadResult,
  type FetchFeedbackUploadError,
  type FetchManagedUsageError,
  type FetchSubmitFeedbackResult,
  type ParsedManagedUsage,
} from '@moonshot-ai/kimi-code-oauth';

const KIMI_PROVIDER = 'kimi-coding';

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
}

/** SDK facade over the runtime's provider, authentication, and model contracts. */
export class ProviderAuthFacade {
  constructor(private readonly options: ProviderAuthFacadeOptions) {}

  async providers(): Promise<readonly ProviderAuthState[]> {
    await this.options.runtime.ready;
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
    await this.options.runtime.ready;
    return this.options.runtime.getAvailable(providerId);
  }

  async login(
    providerId: string,
    type: AuthType,
    interaction: AuthInteraction,
  ): Promise<ProviderLoginResult> {
    await this.options.runtime.ready;
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

  async refreshModels(options?: {
    readonly provider?: string;
    readonly force?: boolean;
    readonly signal?: AbortSignal;
  }) {
    await this.options.runtime.ready;
    return this.options.runtime.refresh({
      provider: options?.provider,
      allowNetwork: true,
      force: options?.force,
      signal: options?.signal,
    });
  }

  async getAccessToken(providerId: string = KIMI_PROVIDER): Promise<string | undefined> {
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

  async getManagedUsage(providerId: string = KIMI_PROVIDER): Promise<ProviderManagedUsageResult> {
    try {
      const token = await this.requireToken(providerId);
      const result = await fetchManagedUsage(kimiCodeUsageUrl(), token);
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
    providerId: string = KIMI_PROVIDER,
  ): Promise<FetchSubmitFeedbackResult> {
    try {
      return await fetchSubmitFeedback(kimiCodeFeedbackUrl(), await this.requireToken(providerId), {
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
    providerId: string = KIMI_PROVIDER,
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
    providerId: string = KIMI_PROVIDER,
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
