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
  assertDimiHostIdentity,
  createDimiDefaultHeaders,
  createDimiDeviceHeaders,
  createDimiDeviceId,
  createDimiUserAgent,
  DIMI_CODE_CUSTOM_HEADERS_ENV,
  DIMI_CODE_PLATFORM,
  parseDimiCodeCustomHeaders,
  readDimiDeviceId,
} from "./identity";
export type { DimiHostIdentity, DimiIdentityOptions } from "./identity";

export { DIMI_CODE_FLOW_CONFIG } from "./constants";

export { DIMI_CODE_OAUTH_KEY, resolveDimiCodeOAuthKey } from "./credential-key";

export {
  fetchManagedUsage,
  formatDuration,
  dimiCodeBaseUrl,
  dimiCodeUsageUrl,
  parseManagedUsagePayload,
} from "./managed-usage";
export type {
  FetchManagedUsageError,
  FetchManagedUsageResult,
  ParsedManagedUsage,
  UsageRow,
  UsageWindow,
} from "./managed-usage";

export { fetchSubmitFeedback, dimiCodeFeedbackUrl } from "./managed-feedback";
export type {
  FetchSubmitFeedbackError,
  FetchSubmitFeedbackOk,
  FetchSubmitFeedbackResult,
  SubmitFeedbackBody,
} from "./managed-feedback";

export {
  fetchCompleteFeedbackUpload,
  fetchCreateFeedbackUploadUrl,
  dimiCodeFeedbackUploadCompleteUrl,
  dimiCodeFeedbackUploadUrl,
} from "./managed-feedback-upload";
export type {
  CompleteFeedbackUploadBody,
  CreateFeedbackUploadUrlBody,
  CreateFeedbackUploadUrlResponse,
  FetchCompleteFeedbackUploadResult,
  FetchCreateFeedbackUploadUrlResult,
  FetchFeedbackUploadError,
} from "./managed-feedback-upload";
