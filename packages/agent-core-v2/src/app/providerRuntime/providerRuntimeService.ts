import { join } from "node:path";

import { LifecycleScope, ScopeActivation, registerScopedService } from "#/_base/di/scope";
import { IBootstrapService } from "#/app/bootstrap/bootstrap";

import { createModels } from "./models";
import { composeProvider, parseCustomProviderDefinition } from "./customProviders";
import { builtinProviders } from "./providers";
import { IProviderRuntime } from "./providerRuntime";
import { providerApis } from "./stream";
import { FileCredentialStore, FileCustomProvidersStore, FileModelsStore } from "./storage";
import { IHostRequestHeaders } from "./hostRequestHeaders";
import type {
  Api,
  AuthInteraction,
  AuthType,
  CustomProviderDefinition,
  Model,
  Models,
  MutableModels,
  ModelsRefreshOptions,
  ModelsSimpleStreamOptions,
  Provider,
} from "./types";

export class ProviderRuntimeService implements IProviderRuntime {
  declare readonly _serviceBrand: undefined;

  private readonly credentials: FileCredentialStore;
  private readonly customProviders: FileCustomProvidersStore;
  private readonly models: MutableModels;
  private readonly builtins: ReadonlyMap<string, Provider>;
  private readonly processProviders = new Map<string, Provider>();
  private readonly definitions = new Map<string, CustomProviderDefinition>();
  private definitionDiagnostic: string | undefined;
  private readyPromise: Promise<void> | undefined;

  constructor(
    @IBootstrapService bootstrap: IBootstrapService,
    @IHostRequestHeaders hostHeaders: IHostRequestHeaders,
  ) {
    this.credentials = new FileCredentialStore(join(bootstrap.homeDir, "auth.json"));
    const providers = builtinProviders(hostHeaders.headers);
    this.builtins = new Map(providers.map((provider) => [provider.id, provider]));
    this.models = createModels({
      providers,
      credentials: this.credentials,
      modelsStore: new FileModelsStore(join(bootstrap.homeDir, "models-store.json")),
    });
    this.customProviders = new FileCustomProvidersStore(join(bootstrap.homeDir, "models.json"));
  }

  get ready(): Promise<void> {
    this.readyPromise ??= this.syncCustomProviders().then(() =>
      this.models.refresh({ allowNetwork: false }).then(() => undefined),
    );
    return this.readyPromise;
  }

  listCredentials() {
    return this.credentials.list();
  }

  providerApis(): readonly Api[] {
    return providerApis();
  }

  async listCustomProviders(): Promise<readonly CustomProviderDefinition[]> {
    await this.ready;
    await this.syncCustomProviders();
    return [...this.definitions.values()];
  }

  getProviderDefinitionDiagnostic(): string | undefined {
    return this.definitionDiagnostic;
  }

  async upsertCustomProvider(definition: CustomProviderDefinition): Promise<void> {
    await this.ready;
    const parsed = parseCustomProviderDefinition(definition);
    // Validate the entire candidate while the store lock is held.  A valid
    // replacement must not paper over a different invalid provider, and a
    // failed CLI/TUI mutation must leave the file byte-for-byte unchanged.
    await this.customProviders.set(parsed, (definitions) => this.validateDefinitions(definitions));
    await this.syncCustomProviders();
  }

  async deleteCustomProvider(id: string): Promise<void> {
    await this.ready;
    await this.customProviders.delete(id);
    await this.syncCustomProviders();
    if (!this.builtins.has(id) && !this.processProviders.has(id)) await this.credentials.delete(id);
  }

  async refreshProviderDefinitions(): Promise<void> {
    await this.ready;
    await this.syncCustomProviders();
  }

  getProviders(): readonly Provider[] {
    return this.models.getProviders();
  }

  setProvider(provider: Provider): void {
    const previous = this.processProviders.get(provider.id);
    this.processProviders.set(provider.id, provider);
    try {
      this.recomposeProvider(provider.id);
    } catch (error) {
      if (previous === undefined) this.processProviders.delete(provider.id);
      else this.processProviders.set(provider.id, previous);
      throw error;
    }
  }

  deleteProvider(id: string): void {
    this.processProviders.delete(id);
    this.recomposeAfterBaseRemoval([id]);
  }

  clearProviders(): void {
    const ids = new Set([...this.processProviders.keys(), ...this.builtins.keys(), ...this.definitions.keys()]);
    this.processProviders.clear();
    this.recomposeAfterBaseRemoval(ids);
  }

  getProvider(id: string): Provider | undefined {
    return this.models.getProvider(id);
  }

  getModels(provider?: string): readonly Model[] {
    return this.models.getModels(provider);
  }

  getModel(provider: string, id: string): Model | undefined {
    return this.models.getModel(provider, id);
  }

  refresh(options?: ModelsRefreshOptions) {
    return this.models.refresh(options);
  }

  checkAuth(providerId: string) {
    return this.models.checkAuth(providerId);
  }

  getAvailable(providerId?: string) {
    return this.models.getAvailable(providerId);
  }

  getAuth(providerId: string): ReturnType<Models["getAuth"]>;
  getAuth(model: Model): ReturnType<Models["getAuth"]>;
  getAuth(providerOrModel: string | Model): ReturnType<Models["getAuth"]> {
    return typeof providerOrModel === "string"
      ? this.models.getAuth(providerOrModel)
      : this.models.getAuth(providerOrModel);
  }

  login(providerId: string, type: AuthType, interaction: AuthInteraction) {
    return this.models.login(providerId, type, interaction);
  }

  logout(providerId: string): Promise<void> {
    return this.models.logout(providerId);
  }

  streamSimple(
    model: Model,
    context: Parameters<Models["streamSimple"]>[1],
    options?: ModelsSimpleStreamOptions,
  ) {
    return this.models.streamSimple(model, context, options);
  }

  completeSimple(
    model: Model,
    context: Parameters<Models["completeSimple"]>[1],
    options?: ModelsSimpleStreamOptions,
  ) {
    return this.models.completeSimple(model, context, options);
  }

  private async syncCustomProviders(): Promise<void> {
    const loaded = await this.customProviders.load();
    if (loaded.error !== undefined) {
      this.definitionDiagnostic = `Failed to load models.json: ${loaded.error.message}`;
      return;
    }
    let next: Map<string, CustomProviderDefinition>;
    try {
      next = this.validateDefinitions(loaded.providers);
    } catch (error) {
      this.definitionDiagnostic = error instanceof Error ? error.message : String(error);
      return;
    }
    this.definitionDiagnostic = undefined;
    // Re-reading models.json is part of normal CLI/TUI discovery.  Keep an
    // unchanged composed provider instance intact so its dynamic catalog is
    // not thrown away on every list/model operation.
    const changed = new Set<string>();
    for (const id of this.definitions.keys()) {
      if (!next.has(id)) changed.add(id);
    }
    for (const [id, definition] of next) {
      if (!sameDefinition(this.definitions.get(id), definition)) changed.add(id);
    }
    for (const id of changed) {
      const definition = next.get(id);
      if (definition === undefined) this.definitions.delete(id);
      else this.definitions.set(id, definition);
    }
    for (const id of changed) this.recomposeProvider(id);
  }

  private recomposeProvider(id: string): void {
    const base = this.baseProvider(id);
    const definition = this.definitions.get(id);
    if (definition !== undefined) {
      this.models.setProvider(composeProvider(definition, base));
      return;
    }
    if (base !== undefined) {
      this.models.setProvider(base);
      return;
    }
    this.models.deleteProvider(id);
  }

  /**
   * Process providers are ephemeral bases.  If removing one exposes a sparse
   * models.json overlay, the old composed provider cannot linger as a ghost:
   * retain the editable definition, surface a diagnostic, and remove it from
   * the live catalog until a compatible base is registered again.
   */
  private recomposeAfterBaseRemoval(ids: Iterable<string>): void {
    const errors: string[] = [];
    for (const id of ids) {
      try {
        this.recomposeProvider(id);
      } catch (error) {
        this.models.deleteProvider(id);
        errors.push(error instanceof Error ? error.message : String(error));
      }
    }
    if (errors.length > 0) this.definitionDiagnostic = `Invalid models.json: ${errors.join("; ")}`;
  }

  /** Parse and compose every definition before any live or persisted mutation. */
  private validateDefinitions(
    definitions: readonly CustomProviderDefinition[],
  ): Map<string, CustomProviderDefinition> {
    const next = new Map<string, CustomProviderDefinition>();
    const errors: string[] = [];
    for (const definition of definitions) {
      try {
        const parsed = parseCustomProviderDefinition(definition);
        next.set(parsed.id, parsed);
      } catch (error) {
        errors.push(error instanceof Error ? error.message : String(error));
      }
    }
    for (const [id, definition] of next) {
      try {
        composeProvider(definition, this.baseProvider(id));
      } catch (error) {
        errors.push(error instanceof Error ? error.message : String(error));
      }
    }
    if (errors.length > 0) throw new Error(`Invalid models.json: ${errors.join("; ")}`);
    return next;
  }

  private baseProvider(id: string): Provider | undefined {
    return this.processProviders.get(id) ?? this.builtins.get(id);
  }
}

function sameDefinition(
  left: CustomProviderDefinition | undefined,
  right: CustomProviderDefinition,
): boolean {
  return left !== undefined && JSON.stringify(left) === JSON.stringify(right);
}

registerScopedService(
  LifecycleScope.App,
  IProviderRuntime,
  ProviderRuntimeService,
  ScopeActivation.OnScopeCreated,
  "provider-runtime",
);
