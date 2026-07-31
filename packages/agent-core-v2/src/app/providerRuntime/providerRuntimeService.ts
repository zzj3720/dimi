import { join } from "node:path";

import { LifecycleScope, ScopeActivation, registerScopedService } from "#/_base/di/scope";
import { IBootstrapService } from "#/app/bootstrap/bootstrap";

import { createModels } from "./models";
import { builtinProviders } from "./providers";
import { IProviderRuntime } from "./providerRuntime";
import { FileCredentialStore, FileModelsStore } from "./storage";
import { IHostRequestHeaders } from "./hostRequestHeaders";
import type {
  AuthInteraction,
  AuthType,
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
  private readonly models: MutableModels;
  private readyPromise: Promise<void> | undefined;

  constructor(
    @IBootstrapService bootstrap: IBootstrapService,
    @IHostRequestHeaders hostHeaders: IHostRequestHeaders,
  ) {
    this.credentials = new FileCredentialStore(join(bootstrap.homeDir, "auth.json"));
    this.models = createModels({
      providers: builtinProviders(hostHeaders.headers),
      credentials: this.credentials,
      modelsStore: new FileModelsStore(join(bootstrap.homeDir, "models-store.json")),
    });
  }

  get ready(): Promise<void> {
    this.readyPromise ??= this.models.refresh({ allowNetwork: false }).then(() => undefined);
    return this.readyPromise;
  }

  listCredentials() {
    return this.credentials.list();
  }

  getProviders(): readonly Provider[] {
    return this.models.getProviders();
  }

  setProvider(provider: Provider): void {
    this.models.setProvider(provider);
  }

  deleteProvider(id: string): void {
    this.models.deleteProvider(id);
  }

  clearProviders(): void {
    this.models.clearProviders();
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
}

registerScopedService(
  LifecycleScope.App,
  IProviderRuntime,
  ProviderRuntimeService,
  ScopeActivation.OnScopeCreated,
  "provider-runtime",
);
