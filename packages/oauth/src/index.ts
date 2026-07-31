export {
  DeviceCodeExpiredError,
  DeviceCodeTimeoutError,
  OAuthConnectionError,
  OAuthError,
  OAuthUnauthorizedError,
  RetryableRefreshError,
} from "./errors";

export type { DeviceAuthorization, DeviceHeaders, OAuthFlowConfig, TokenInfo } from "./types";

export type { DevicePollResult, RefreshOptions } from "./oauth";
export { pollDeviceToken, refreshAccessToken, requestDeviceAuthorization } from "./oauth";

export {
  assertKimiHostIdentity,
  createKimiDefaultHeaders,
  createKimiDeviceHeaders,
  createKimiDeviceId,
  createKimiUserAgent,
  KIMI_CODE_CUSTOM_HEADERS_ENV,
  KIMI_CODE_PLATFORM,
  parseKimiCodeCustomHeaders,
  readKimiDeviceId,
} from "./identity";
export type { KimiHostIdentity, KimiIdentityOptions } from "./identity";

export { KIMI_CODE_FLOW_CONFIG } from "./constants";

export { KIMI_CODE_OAUTH_KEY, resolveKimiCodeOAuthKey } from "./credential-key";

export {
  fetchManagedUsage,
  formatDuration,
  kimiCodeBaseUrl,
  kimiCodeUsageUrl,
  parseManagedUsagePayload,
} from "./managed-usage";
export type {
  FetchManagedUsageError,
  FetchManagedUsageResult,
  ParsedManagedUsage,
  UsageRow,
  UsageWindow,
} from "./managed-usage";

export { fetchSubmitFeedback, kimiCodeFeedbackUrl } from "./managed-feedback";
export type {
  FetchSubmitFeedbackError,
  FetchSubmitFeedbackOk,
  FetchSubmitFeedbackResult,
  SubmitFeedbackBody,
} from "./managed-feedback";

export {
  fetchCompleteFeedbackUpload,
  fetchCreateFeedbackUploadUrl,
  kimiCodeFeedbackUploadCompleteUrl,
  kimiCodeFeedbackUploadUrl,
} from "./managed-feedback-upload";
export type {
  CompleteFeedbackUploadBody,
  CreateFeedbackUploadUrlBody,
  CreateFeedbackUploadUrlResponse,
  FetchCompleteFeedbackUploadResult,
  FetchCreateFeedbackUploadUrlResult,
  FetchFeedbackUploadError,
} from "./managed-feedback-upload";
