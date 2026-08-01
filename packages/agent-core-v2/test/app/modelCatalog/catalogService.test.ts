/**
 * Scenario: model requester resolution follows the current provider catalog.
 * Responsibilities: resolve requesters through the public catalog service and
 * discard stale model metadata after the provider catalog changes. Wiring:
 * the real ModelCatalog resolved through DI with a provider-runtime boundary
 * stub. Run: vp test -- --run test/app/modelCatalog/catalogService.test.ts
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { DisposableStore } from "#/_base/di/lifecycle";
import { createServices, type TestInstantiationService } from "#/_base/di/test";
import { IConfigService } from "#/app/config/config";
import {
  IModelCatalog,
  modelThinkingLevels,
  toProtocolModel,
  type Model,
} from "#/app/modelCatalog/catalog";
import { ModelCatalog } from "#/app/modelCatalog/catalogService";
import { IProviderRuntime } from "#/app/providerRuntime/providerRuntime";

function model(maxTokens: number): Model {
  return {
    id: "catalog-model",
    name: "Catalog model",
    api: "openai-completions",
    provider: "catalog-provider",
    baseUrl: "https://api.example.test/v1",
    reasoning: true,
    input: ["text"],
    cost: { input: 1, output: 2, cacheRead: 0.1, cacheWrite: 0 },
    contextWindow: 1_000_000,
    maxTokens,
  };
}

describe("ModelCatalog requester resolution", () => {
  let disposables: DisposableStore;
  let ix: TestInstantiationService;
  let current: Model;

  beforeEach(() => {
    current = model(32_000);
    disposables = new DisposableStore();
    ix = createServices(disposables, {
      additionalServices: (reg) => {
        reg.definePartialInstance(IProviderRuntime, {
          getModel: () => current,
          getModels: () => [current],
        });
        reg.definePartialInstance(IConfigService, {});
        reg.define(IModelCatalog, ModelCatalog);
      },
    });
  });

  afterEach(() => disposables.dispose());

  it("uses refreshed metadata when the same model id is requested again", () => {
    const catalog = ix.get(IModelCatalog);
    expect(catalog.getRequester("catalog-provider/catalog-model").model.maxTokens).toBe(32_000);

    current = model(384_000);

    expect(catalog.getRequester("catalog-provider/catalog-model").model.maxTokens).toBe(384_000);
  });
});

describe("ModelCatalog thinking capabilities", () => {
  it("does not invent effort levels for a reasoning model without a level map", () => {
    const reasoningModel = model(32_000);

    expect(modelThinkingLevels(reasoningModel)).toEqual([]);
    expect(toProtocolModel(reasoningModel)).toMatchObject({ support_efforts: [] });
  });
});
