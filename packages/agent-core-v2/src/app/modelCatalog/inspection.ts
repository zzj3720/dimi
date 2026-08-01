import type { Model, Provider } from "#/app/providerRuntime/types";

/**
 * Serializable inspector projection. `model` is the runtime model object; only
 * Provider functions are projected because debug transports cannot serialize
 * executable callbacks.
 */
export interface ModelInspection {
  readonly id: string;
  readonly model: Model;
  readonly provider: {
    readonly id: Provider["id"];
    readonly name: Provider["name"];
    readonly auth: {
      readonly oauth?: {
        readonly name: string;
        readonly loginLabel?: string;
      };
      readonly apiKey?: {
        readonly name: string;
        readonly interactive: boolean;
      };
    };
    readonly dynamicModels: boolean;
  };
}
