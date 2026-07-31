export { KimiHarness } from "#/kimi-harness";
export type { KimiHarnessRuntimeOptions } from "#/kimi-harness";
export { Session } from "#/session";
export { ProviderAuthFacade } from "#/auth";
export { createKimiHarness, SDKRpcClient, type SDKRpcClientOptions } from "#/sdk-rpc-client";
export {
  createKimiConfigRpc,
  KimiConfigRpcClient,
  type KimiConfigRpc,
  type KimiConfigValidationIssue,
  type KimiConfigValidationPathSegment,
  type ResolveKimiConfigPathInput,
  type ValidateKimiConfigTomlInput,
} from "#/config-rpc";
export { SDKRpcClientBase } from "#/rpc";
export { ImageLimits } from "#/image-limits";
export { effectiveModelAlias } from "#/model";

export * from "#/errors";

// Diagnostic logging — public surface only.
// RootLogger / getRootLogger / LoggingConfig stay inside agent-core.
export { flushDiagnosticLogs, flushDiagnosticLogsSync, log } from "#/logging";
export type { Logger, LogContext, LogLevel, LogPayload } from "#/logging";
export { resolveGlobalLogPath, resolveKimiHome } from "@moonshot-ai/agent-core-v2";

// Host-side config path resolution.
export { resolveConfigPath } from "@moonshot-ai/agent-core-v2";
export { limitAgentReplayByTurns } from "@moonshot-ai/agent-core-v2";
export { parseAgentFileText, resolveAgentPath } from "@moonshot-ai/agent-core-v2";
// Process-wide HTTP proxy bootstrap — installed once at CLI startup so all
// outbound fetch honors HTTP_PROXY / HTTPS_PROXY / NO_PROXY.
export { installGlobalProxyDispatcher } from "@moonshot-ai/agent-core-v2";
export type { AuthInteraction, AuthType, ProviderModel } from "@moonshot-ai/agent-core-v2";

// Image compression — ingestion sites (e.g. the CLI's clipboard paste, the ACP
// adapter) shrink oversized images while constructing the content part, before
// it enters a prompt. Best effort: returns the original on any failure.
// Compression is never silent: buildImageCompressionCaption renders the note
// placed next to a compressed image, and persistOriginalImage keeps the
// pre-compression bytes readable (ReadMediaFile + region) for detail.
export {
  buildImageCompressionCaption,
  buildUnsupportedImageNotice,
  compressImageForModel,
  compressBase64ForModel,
  gateImageFormatParts,
  isModelAcceptedImageMime,
  normalizeImageMime,
  parseImageDataUrl,
  persistOriginalImage,
  sessionMediaOriginalsDir,
  IMAGE_BYTE_BUDGET,
  MAX_IMAGE_EDGE_PX,
} from "@moonshot-ai/agent-core-v2";
export type {
  CompressImageOptions,
  CompressImageResult,
  CompressBase64Result,
  ImageCompressionCaptionInput,
  ImageCompressionTelemetry,
} from "@moonshot-ai/agent-core-v2";

// Experimental feature flags — types only. Resolved values come from
// `KimiHarness.getExperimentalFeatures()` over RPC, not from a re-exported runtime value.
export type {
  ExperimentalFeatureState,
  ExperimentalFlagMap,
  ExperimentalFlagSource,
  FlagDefinitionInput,
  FlagId,
  FlagSurface,
} from "@moonshot-ai/agent-core-v2";

export type {
  ProviderAuthCompleteFeedbackUploadInput,
  ProviderAuthCompleteFeedbackUploadPart,
  ProviderAuthCreateFeedbackUploadUrlInput,
  ProviderAuthCreateFeedbackUploadUrlOk,
  ProviderAuthCreateFeedbackUploadUrlResult,
  ProviderAuthFeedbackUploadPart,
  ProviderAuthMethod,
  ProviderAuthPrompt,
  ProviderAuthState,
  ProviderAuthStatus,
  ProviderAuthSubmitFeedbackInput,
  ProviderLoginResult,
  ProviderLogoutResult,
} from "#/auth";

export * from "#/events";
export type * from "#/types";
