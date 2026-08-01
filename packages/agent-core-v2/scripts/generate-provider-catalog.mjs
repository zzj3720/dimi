#!/usr/bin/env node
import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const output = resolve(packageRoot, "src/app/providerRuntime/builtinCatalog.generated.ts");
const source = process.env.DIMI_MODELS_CATALOG_SOURCE ?? "https://models.dev/api.json";

const providers = [
  ["amazon-bedrock", "amazon-bedrock", "bedrock-converse-stream"],
  ["ant-ling", undefined, "openai-completions"],
  ["anthropic", "anthropic", "anthropic-messages"],
  ["azure-openai-responses", undefined, "azure-openai-responses"],
  ["cerebras", "cerebras", "openai-completions"],
  ["cloudflare-ai-gateway", undefined, "openai-completions"],
  ["cloudflare-workers-ai", "cloudflare-workers-ai", "openai-completions"],
  ["deepseek", "deepseek", "openai-completions"],
  ["fireworks", "fireworks-ai", "openai-completions"],
  ["github-copilot", "github-copilot", "openai-completions"],
  ["google", "google", "google-generative-ai"],
  ["google-vertex", "google-vertex", "google-vertex"],
  ["groq", "groq", "openai-completions"],
  ["huggingface", "huggingface", "openai-completions"],
  ["kimi-coding", undefined, "anthropic-messages"],
  ["minimax", "minimax", "anthropic-messages"],
  ["minimax-cn", "minimax-cn", "anthropic-messages"],
  ["mistral", "mistral", "mistral-conversations"],
  ["moonshotai", "moonshotai", "openai-completions"],
  ["moonshotai-cn", "moonshotai-cn", "openai-completions"],
  ["nvidia", "nvidia", "openai-completions"],
  ["openai", "openai", "openai-responses"],
  ["openai-codex", undefined, "openai-codex-responses"],
  ["opencode", "opencode", "openai-completions"],
  ["opencode-go", "opencode-go", "openai-completions"],
  ["openrouter", "openrouter", "openai-completions"],
  ["qwen-token-plan", "alibaba", "openai-completions"],
  ["qwen-token-plan-cn", "alibaba-coding-plan-cn", "openai-completions"],
  ["radius", undefined, "openai-completions"],
  ["together", "togetherai", "openai-completions"],
  ["vercel-ai-gateway", undefined, "openai-completions"],
  ["xai", "xai", "openai-completions"],
  ["xiaomi", "xiaomi", "openai-completions"],
  ["xiaomi-token-plan-ams", "xiaomi-token-plan-ams", "openai-completions"],
  ["xiaomi-token-plan-cn", "xiaomi-token-plan-cn", "openai-completions"],
  ["xiaomi-token-plan-sgp", "xiaomi-token-plan-sgp", "openai-completions"],
  ["zai", "zai", "openai-completions"],
  ["zai-coding-cn", undefined, "openai-completions"],
];

const providerCorrections = {
  "amazon-bedrock": { name: "Amazon Bedrock", baseUrl: "", envNames: [], models: [] },
  anthropic: { baseUrl: "https://api.anthropic.com/v1", envNames: ["ANTHROPIC_API_KEY"], models: [] },
  "openai-codex": {
    name: "OpenAI Codex",
    baseUrl: "https://chatgpt.com/backend-api",
    envNames: [],
    models: [
      {
        id: "gpt-5.4",
        name: "GPT-5.4",
        reasoning: true,
        input: ["text", "image"],
        cost: { input: 2.5, output: 15, cacheRead: 0.25, cacheWrite: 0 },
        contextWindow: 272_000,
        maxTokens: 128_000,
      },
      {
        id: "gpt-5.4-mini",
        name: "GPT-5.4 mini",
        reasoning: true,
        input: ["text", "image"],
        cost: { input: 0.75, output: 4.5, cacheRead: 0.075, cacheWrite: 0 },
        contextWindow: 272_000,
        maxTokens: 128_000,
      },
    ],
  },
  "kimi-coding": {
    name: "Dimi",
    baseUrl: "https://api.kimi.com/coding/v1",
    envNames: ["DIMI_API_KEY"],
    models: [
      {
        id: "kimi-for-coding",
        name: "Dimi for Coding",
        reasoning: true,
        input: ["text"],
        cost: { input: 0.95, output: 4, cacheRead: 0.19, cacheWrite: 0 },
        contextWindow: 262_144,
        maxTokens: 128_000,
      },
    ],
  },
  "ant-ling": {
    name: "Ant Ling",
    baseUrl: "https://api.ant-ling.com/v1",
    envNames: ["ANT_LING_API_KEY"],
    models: [],
  },
  "azure-openai-responses": {
    name: "Azure OpenAI",
    envNames: ["AZURE_OPENAI_API_KEY"],
    models: [],
  },
  cerebras: { baseUrl: "https://api.cerebras.ai/v1", envNames: ["CEREBRAS_API_KEY"], models: [] },
  "cloudflare-ai-gateway": {
    name: "Cloudflare AI Gateway",
    envNames: ["CLOUDFLARE_API_KEY"],
    models: [],
  },
  "cloudflare-workers-ai": { envNames: ["CLOUDFLARE_API_KEY"], models: [] },
  deepseek: { baseUrl: "https://api.deepseek.com/v1", envNames: ["DEEPSEEK_API_KEY"], models: [] },
  fireworks: { baseUrl: "https://api.fireworks.ai/inference/v1", envNames: ["FIREWORKS_API_KEY"], models: [] },
  "github-copilot": { baseUrl: "https://api.individual.githubcopilot.com", envNames: [], models: [] },
  google: { baseUrl: "https://generativelanguage.googleapis.com/v1beta", envNames: ["GEMINI_API_KEY"], models: [] },
  "google-vertex": {
    baseUrl: "https://{location}-aiplatform.googleapis.com",
    envNames: ["GOOGLE_CLOUD_API_KEY"],
    models: [],
  },
  groq: { baseUrl: "https://api.groq.com/openai/v1", envNames: ["GROQ_API_KEY"], models: [] },
  huggingface: { baseUrl: "https://router.huggingface.co/v1", envNames: ["HF_TOKEN"], models: [] },
  minimax: { baseUrl: "https://api.minimax.io/anthropic", envNames: ["MINIMAX_API_KEY"], models: [] },
  "minimax-cn": { baseUrl: "https://api.minimaxi.com/anthropic", envNames: ["MINIMAX_CN_API_KEY"], models: [] },
  mistral: { baseUrl: "https://api.mistral.ai/v1", envNames: ["MISTRAL_API_KEY"], models: [] },
  moonshotai: { baseUrl: "https://api.moonshot.ai/v1", envNames: ["MOONSHOT_API_KEY", "DIMI_API_KEY"], models: [] },
  "moonshotai-cn": { baseUrl: "https://api.moonshot.cn/v1", envNames: ["MOONSHOT_API_KEY"], models: [] },
  nvidia: { baseUrl: "https://integrate.api.nvidia.com/v1", envNames: ["NVIDIA_API_KEY"], models: [] },
  openai: { baseUrl: "https://api.openai.com/v1", envNames: ["OPENAI_API_KEY"], models: [] },
  opencode: { baseUrl: "https://opencode.ai/zen/v1", envNames: ["OPENCODE_API_KEY"], models: [] },
  "opencode-go": { baseUrl: "https://opencode.ai/zen/go/v1", envNames: ["OPENCODE_API_KEY"], models: [] },
  openrouter: { baseUrl: "https://openrouter.ai/api/v1", envNames: ["OPENROUTER_API_KEY"], models: [] },
  "qwen-token-plan": {
    baseUrl: "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
    envNames: ["QWEN_TOKEN_PLAN_API_KEY"],
    models: [],
  },
  "qwen-token-plan-cn": {
    baseUrl: "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1",
    envNames: ["QWEN_TOKEN_PLAN_CN_API_KEY"],
    models: [],
  },
  together: { baseUrl: "https://api.together.xyz/v1", envNames: ["TOGETHER_API_KEY"], models: [] },
  "radius": { name: "Radius", envNames: ["RADIUS_API_KEY"], models: [] },
  "vercel-ai-gateway": {
    name: "Vercel AI Gateway",
    baseUrl: "https://ai-gateway.vercel.sh",
    envNames: ["AI_GATEWAY_API_KEY"],
    models: [],
  },
  xai: { baseUrl: "https://api.x.ai/v1", envNames: ["XAI_API_KEY"], models: [] },
  xiaomi: { baseUrl: "https://api.xiaomimimo.com/v1", envNames: ["XIAOMI_API_KEY"], models: [] },
  "xiaomi-token-plan-ams": {
    baseUrl: "https://token-plan-ams.xiaomimimo.com/v1",
    envNames: ["XIAOMI_TOKEN_PLAN_AMS_API_KEY"],
    models: [],
  },
  "xiaomi-token-plan-cn": {
    baseUrl: "https://token-plan-cn.xiaomimimo.com/v1",
    envNames: ["XIAOMI_TOKEN_PLAN_CN_API_KEY"],
    models: [],
  },
  "xiaomi-token-plan-sgp": {
    baseUrl: "https://token-plan-sgp.xiaomimimo.com/v1",
    envNames: ["XIAOMI_TOKEN_PLAN_SGP_API_KEY"],
    models: [],
  },
  zai: { baseUrl: "https://api.z.ai/api/paas/v4", envNames: ["ZAI_API_KEY"], models: [] },
  "zai-coding-cn": {
    name: "Z.AI Coding CN",
    baseUrl: "https://open.bigmodel.cn/api/coding/paas/v4",
    envNames: ["ZAI_CODING_CN_API_KEY"],
    models: [],
  },
};

const modelCorrections = {
  "deepseek/deepseek-v4-flash": {
    thinkingLevelMap: {
      low: "low",
      high: "high",
      max: "max",
    },
    defaultThinkingLevel: "high",
    compat: {
      requiresReasoningContentOnAssistantMessages: true,
      supportsReasoningEffort: true,
      thinkingFormat: "deepseek",
    },
  },
  "deepseek/deepseek-v4-pro": {
    thinkingLevelMap: {
      high: "high",
      max: "max",
    },
    defaultThinkingLevel: "high",
    compat: {
      requiresReasoningContentOnAssistantMessages: true,
      supportsReasoningEffort: true,
      thinkingFormat: "deepseek",
    },
  },
  "xai/grok-4.5": {
    api: "openai-responses",
    compat: { supportsLongCacheRetention: false },
    thinkingLevelMap: { off: null, minimal: null, low: "low", medium: "medium", high: "high" },
  },
};

const response = await fetch(source);
if (!response.ok) throw new Error(`models.dev catalog failed (HTTP ${response.status})`);
const upstream = await response.json();
const snapshot = {
  generatedAt: new Date().toISOString(),
  providers: providers.map(([id, sourceId, api]) => {
    const upstreamProvider = sourceId === undefined ? undefined : upstream[sourceId];
    const correction = providerCorrections[id];
    return {
      id,
      source: sourceId ?? "manual",
      name: upstreamProvider?.name ?? correction?.name ?? id,
      api,
      baseUrl: correction?.baseUrl ?? upstreamProvider?.api,
      envNames: correction?.envNames ?? upstreamProvider?.env ?? [],
      models: [...Object.values(upstreamProvider?.models ?? {})
        .flatMap((model) => normalizeModel(`${sourceId}/${model?.id}`, model)), ...(correction?.models ?? [])]
        .sort((left, right) => left.name.localeCompare(right.name)),
    };
  }),
};

const contents = `/**\n * Generated by scripts/generate-provider-catalog.mjs from models.dev.\n * Do not edit manually; run \`vp run gen:provider-catalog\`.\n */\nimport type { BuiltinCatalogSnapshot } from "./builtinCatalog";\n\nexport const BUILTIN_CATALOG = ${JSON.stringify(snapshot, null, 2)} as const satisfies BuiltinCatalogSnapshot;\n`;
await mkdir(dirname(output), { recursive: true });
await writeFile(output, contents);

function normalizeModel(key, model) {
  const input = Array.isArray(model?.modalities?.input)
    ? model.modalities.input.filter((value) => value === "text" || value === "image")
    : undefined;
  const output = model?.modalities?.output;
  const contextWindow = model?.limit?.context;
  const maxTokens = model?.limit?.output;
  if (
    typeof model?.id !== "string" ||
    typeof model?.name !== "string" ||
    model?.tool_call !== true ||
    model?.status === "deprecated" ||
    model?.status === "alpha" ||
    model?.experimental === true ||
    !Array.isArray(input) ||
    !input.includes("text") ||
    !Array.isArray(output) ||
    !output.includes("text") ||
    /(?:^|[-_/])(?:embedding|rerank|moderation|tts|whisper|transcri(?:be|ption)|dall-e)(?:$|[-_/])/iu.test(
      model.id,
    ) ||
    !Number.isInteger(contextWindow) ||
    contextWindow < 1 ||
    !Number.isInteger(maxTokens) ||
    maxTokens < 1
  ) {
    return [];
  }
  const cost = model.cost ?? {};
  const correction = modelCorrections[key];
  const value = {
    id: model.id,
    name: model.name,
    reasoning: model.reasoning === true,
    input,
    cost: {
      input: number(cost.input),
      output: number(cost.output),
      cacheRead: number(cost.cache_read),
      cacheWrite: number(cost.cache_write),
      tiers: costTiers(cost.tiers),
    },
    contextWindow,
    maxTokens,
    dynamicTools: model?.dynamically_loaded_tools === true || undefined,
    ...thinkingMetadata(model?.reasoning_options),
    ...correction,
  };
  return [value];
}

function thinkingMetadata(options) {
  const values = Array.isArray(options)
    ? options
        .filter((option) => option?.type === "effort" && Array.isArray(option.values))
        .flatMap((option) => option.values)
        .filter((value) => typeof value === "string" || typeof value === "number")
    : [];
  const thinkingLevelMap = Object.fromEntries(
    values.flatMap((value) => {
      if (value === "none") return [["off", null]];
      if (
        value === "minimal" ||
        value === "low" ||
        value === "medium" ||
        value === "high" ||
        value === "xhigh" ||
        value === "max"
      ) {
        return [[value, value]];
      }
      return [];
    }),
  );
  const defaultThinkingLevel = ["medium", "high", "low", "minimal"].find((value) =>
    Object.hasOwn(thinkingLevelMap, value),
  );
  if (defaultThinkingLevel === undefined) {
    return Object.keys(thinkingLevelMap).length === 0 ? {} : { thinkingLevelMap };
  }
  return { thinkingLevelMap, defaultThinkingLevel };
}

function number(value) {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function costTiers(value) {
  if (!Array.isArray(value)) return undefined;
  const tiers = value.flatMap((tier) => {
    const threshold = tier?.tier?.type === "context" ? tier.tier.size : undefined;
    if (!Number.isInteger(threshold) || threshold < 1) return [];
    return [
      {
        inputTokensAbove: threshold,
        input: number(tier.input),
        output: number(tier.output),
        cacheRead: number(tier.cache_read),
        cacheWrite: number(tier.cache_write),
      },
    ];
  });
  return tiers.length === 0 ? undefined : tiers;
}
