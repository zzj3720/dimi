import { createDecorator, type ServiceIdentifier } from "#/_base/di/instantiation";

import type {
  Api,
  CredentialInfo,
  CustomProviderDefinition,
  Model,
  MutableModels,
} from "./types";

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
  CustomModelDefinition,
  CustomProviderDefinition,
  Context,
  ModelAuth,
  ModelCost,
  ModelCostTier,
  ModelInput,
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
  ProviderStreams,
  RefreshModelsContext,
} from "./types";
export type ProviderModel<TApi extends Api = Api> = Model<TApi>;

/**
 * App-wide provider runtime. Provider, model, auth and streaming all use the
 * native contracts in this domain; edges only add serialization/UI adapters.
 */
export interface IProviderRuntime extends MutableModels {
  readonly _serviceBrand: undefined;
  readonly ready: Promise<void>;
  listCredentials(): Promise<readonly CredentialInfo[]>;
  providerApis(): readonly Api[];
  listCustomProviders(): Promise<readonly CustomProviderDefinition[]>;
  /** The latest recoverable models.json load failure, if any. */
  getProviderDefinitionDiagnostic(): string | undefined;
  /** Reload the user-owned models.json layer before presenting a catalog. */
  refreshProviderDefinitions(): Promise<void>;
  upsertCustomProvider(definition: CustomProviderDefinition): Promise<void>;
  deleteCustomProvider(id: string): Promise<void>;
}

export const IProviderRuntime: ServiceIdentifier<IProviderRuntime> =
  createDecorator<IProviderRuntime>("providerRuntime");
