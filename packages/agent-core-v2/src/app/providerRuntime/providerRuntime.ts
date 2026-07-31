import { createDecorator, type ServiceIdentifier } from "#/_base/di/instantiation";

import type { Api, CredentialInfo, Model, MutableModels } from "./types";

export { createProvider, hasApi, type CreateProviderOptions } from "./createProvider";
export type {
  Api,
  ApiKeyCredential,
  AssistantMessage,
  AssistantMessageEvent,
  AuthCheck,
  AuthContext,
  AuthEvent,
  AuthInfoLink,
  AuthInteraction,
  AuthPrompt,
  AuthResult,
  AuthType,
  Credential,
  CredentialInfo,
  CredentialStore,
  Context,
  ModelAuth,
  KnownApi,
  Models,
  ModelsRefreshOptions,
  ModelsRefreshResult,
  ModelsSimpleStreamOptions,
  ModelsStore,
  MutableModels,
  OAuthCredential,
  Provider,
  ProviderHeaders,
  RefreshModelsContext,
} from "./types";
export { createModels, ProviderModels, type CreateModelsOptions } from "./models";
export { InMemoryCredentialStore, InMemoryModelsStore } from "./storage";
export type ProviderModel<TApi extends Api = Api> = Model<TApi>;

/**
 * App-wide provider runtime. Provider, model, auth and streaming all use the
 * native contracts in this domain; edges only add serialization/UI adapters.
 */
export interface IProviderRuntime extends MutableModels {
  readonly _serviceBrand: undefined;
  readonly ready: Promise<void>;
  listCredentials(): Promise<readonly CredentialInfo[]>;
}

export const IProviderRuntime: ServiceIdentifier<IProviderRuntime> =
  createDecorator<IProviderRuntime>("providerRuntime");
