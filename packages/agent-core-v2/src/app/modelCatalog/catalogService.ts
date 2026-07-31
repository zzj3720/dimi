import { Error2 } from "#/_base/errors/errors";
import { LifecycleScope, ScopeActivation, registerScopedService } from "#/_base/di/scope";
import { IConfigService } from "#/app/config/config";
import { IProviderRuntime } from "#/app/providerRuntime/providerRuntime";
import type { TokenUsage } from "#/llmProtocol/usage";

import {
  IModelCatalog,
  type Model,
  type ModelCatalogItem,
  type ModelPingResult,
  type ModelReference,
  type ProviderCatalogItem,
  type SetDefaultModelResponse,
  modelReference,
  toProtocolModel,
} from "./catalog";
import { ModelCatalogErrors } from "./errors";
import type { ModelInspection } from "./inspection";
import type { ModelRequester } from "./modelRequester";
import { ModelRequesterImpl } from "./modelRequesterImpl";

export class ModelCatalog implements IModelCatalog {
  declare readonly _serviceBrand: undefined;

  private readonly requesters = new Map<string, ModelRequester>();

  constructor(
    @IProviderRuntime private readonly runtime: IProviderRuntime,
    @IConfigService private readonly config: IConfigService,
  ) {}

  notifyConfigChanged(): void {
    this.requesters.clear();
  }

  get(reference: string): Model {
    const model = this.resolve(reference);
    if (model !== undefined) return model;
    throw new Error2(
      ModelCatalogErrors.codes.MODEL_NOT_FOUND,
      `model ${formatReference(reference)} does not exist`,
    );
  }

  getRequester(reference: string): ModelRequester {
    const model = this.get(reference);
    const key = modelReference(model);
    const existing = this.requesters.get(key);
    if (existing !== undefined) return existing;
    const requester = new ModelRequesterImpl(model, this.runtime);
    this.requesters.set(key, requester);
    return requester;
  }

  findByName(name: string): readonly string[] {
    const normalized = name.toLowerCase();
    return this.runtime
      .getModels()
      .filter(
        (model) =>
          model.id.toLowerCase() === normalized ||
          model.name.toLowerCase() === normalized ||
          modelReference(model).toLowerCase() === normalized,
      )
      .map(modelReference);
  }

  inspect(reference: string): ModelInspection {
    const model = this.get(reference);
    const provider = this.runtime.getProvider(model.provider);
    if (provider === undefined) {
      throw new Error2(
        ModelCatalogErrors.codes.PROVIDER_NOT_FOUND,
        `provider ${model.provider} does not exist`,
      );
    }
    return {
      id: modelReference(model),
      model,
      provider: {
        id: provider.id,
        name: provider.name,
        auth: {
          oauth:
            provider.auth.oauth === undefined
              ? undefined
              : {
                  name: provider.auth.oauth.name,
                  loginLabel: provider.auth.oauth.loginLabel,
                },
          apiKey:
            provider.auth.apiKey === undefined
              ? undefined
              : {
                  name: provider.auth.apiKey.name,
                  interactive: provider.auth.apiKey.login !== undefined,
                },
        },
        dynamicModels: provider.refreshModels !== undefined,
      },
    };
  }

  async ping(reference: string): Promise<ModelPingResult> {
    const requester = this.getRequester(reference);
    const startedAt = Date.now();
    try {
      let text = "";
      let usage: TokenUsage | undefined;
      let finishReason: string | undefined;
      for await (const event of requester.request(
        {
          systemPrompt: 'Answer with the single word "pong".',
          tools: [],
          messages: [
            {
              role: "user",
              content: [{ type: "text", text: "ping" }],
              toolCalls: [],
            },
          ],
        },
        undefined,
        { maxCompletionTokens: 32 },
      )) {
        if (event.type === "part" && event.part.type === "text") text += event.part.text;
        else if (event.type === "usage") usage = event.usage;
        else if (event.type === "finish") {
          finishReason = event.providerFinishReason ?? event.rawFinishReason;
        }
      }
      return {
        ok: true,
        durationMs: Date.now() - startedAt,
        text: text.trim(),
        finishReason,
        usage,
      };
    } catch (error) {
      return {
        ok: false,
        durationMs: Date.now() - startedAt,
        error: error instanceof Error ? error.message : String(error),
      };
    }
  }

  async listModels(): Promise<readonly ModelCatalogItem[]> {
    await this.runtime.ready;
    const available = await this.runtime.getAvailable();
    return available.map(toProtocolModel);
  }

  async listProviders(): Promise<readonly ProviderCatalogItem[]> {
    await this.runtime.ready;
    return Promise.all(
      this.runtime.getProviders().map((provider) => this.projectProvider(provider.id)),
    );
  }

  async getProvider(providerId: string): Promise<ProviderCatalogItem> {
    await this.runtime.ready;
    if (this.runtime.getProvider(providerId) === undefined) {
      throw new Error2(
        ModelCatalogErrors.codes.PROVIDER_NOT_FOUND,
        `provider ${providerId} does not exist`,
      );
    }
    return this.projectProvider(providerId);
  }

  async setDefaultModel(reference: string): Promise<SetDefaultModelResponse> {
    const model = this.get(reference);
    await this.config.replace("defaultProvider", model.provider);
    await this.config.replace("defaultModel", model.id);
    return {
      default_model: modelReference(model),
      model: toProtocolModel(model),
    };
  }

  private resolve(reference: string | ModelReference): Model | undefined {
    if (typeof reference !== "string") {
      return this.runtime.getModel(reference.provider, reference.model);
    }
    const exact = this.runtime.getModels().find((model) => modelReference(model) === reference);
    if (exact !== undefined) return exact;
    const matches = this.runtime
      .getModels()
      .filter((model) => model.id === reference || model.name === reference);
    return matches.length === 1 ? matches[0] : undefined;
  }

  private async projectProvider(providerId: string): Promise<ProviderCatalogItem> {
    const provider = this.runtime.getProvider(providerId);
    if (provider === undefined) {
      throw new Error2(
        ModelCatalogErrors.codes.PROVIDER_NOT_FOUND,
        `provider ${providerId} does not exist`,
      );
    }
    let status: ProviderCatalogItem["status"] = "unconfigured";
    let credentialType: ProviderCatalogItem["credential_type"];
    try {
      const auth = await this.runtime.checkAuth(providerId);
      status = auth === undefined ? "unconfigured" : "connected";
      credentialType = auth?.type;
    } catch {
      status = "error";
    }
    const defaultProvider = this.config.get<string>("defaultProvider");
    const defaultModel = this.config.get<string>("defaultModel");
    return {
      id: provider.id,
      name: provider.name,
      base_url: provider.baseUrl,
      default_model: defaultProvider === provider.id ? defaultModel : undefined,
      auth_methods: [
        provider.auth.oauth === undefined ? undefined : "oauth",
        provider.auth.apiKey?.login === undefined ? undefined : "api_key",
      ].filter((method): method is "oauth" | "api_key" => method !== undefined),
      credential_type: credentialType,
      status,
      models: provider.getModels().map((model) => model.id),
    };
  }
}

function formatReference(reference: string | ModelReference): string {
  return typeof reference === "string" ? reference : `${reference.provider}/${reference.model}`;
}

registerScopedService(
  LifecycleScope.App,
  IModelCatalog,
  ModelCatalog,
  ScopeActivation.OnScopeCreated,
  "modelCatalog",
);
