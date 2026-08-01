import { registerErrorDomain, type ErrorDomain } from "#/_base/errors/codes";
import { Error2 } from "#/_base/errors/errors";

export const ProviderRuntimeErrors = {
  codes: {
    AUTH_LOGIN_REQUIRED: "auth.login_required",
    PROVIDER_API_ERROR: "provider.api_error",
    PROVIDER_FILTERED: "provider.filtered",
    PROVIDER_RATE_LIMIT: "provider.rate_limit",
    PROVIDER_AUTH_ERROR: "provider.auth_error",
    PROVIDER_CONNECTION_ERROR: "provider.connection_error",
    PROVIDER_INVALID_DEFINITION: "provider.invalid_definition",
    PROVIDER_CONFLICT: "provider.conflict",
    PROVIDER_DEFINITION_STORE_INVALID: "provider.definition_store_invalid",
    PROVIDER_OVERLOADED: "provider.overloaded",
    CONTEXT_OVERFLOW: "context.overflow",
  },
  retryable: [
    "provider.rate_limit",
    "provider.connection_error",
    "provider.overloaded",
    "context.overflow",
  ],
  info: {
    "auth.login_required": {
      title: "Login required",
      retryable: false,
      public: true,
      action: "Run /login and choose the provider used by this model.",
    },
    "provider.rate_limit": {
      title: "Provider rate limit",
      retryable: true,
      public: true,
      action: "Retry after the provider rate limit resets.",
    },
    "provider.filtered": {
      title: "Provider filtered response",
      retryable: false,
      public: true,
      action: "Revise the prompt or choose another model.",
    },
    "provider.auth_error": {
      title: "Provider authentication failed",
      retryable: false,
      public: true,
      action: "Run /login and reconnect this provider.",
    },
    "provider.overloaded": {
      title: "Provider overloaded",
      retryable: true,
      public: true,
      action: "Retry after the provider recovers.",
    },
    "context.overflow": {
      title: "Context overflow",
      retryable: true,
      public: true,
      action: "Compact the conversation or retry with fewer tokens.",
    },
  },
} as const satisfies ErrorDomain;

registerErrorDomain(ProviderRuntimeErrors);

export function providerRuntimeError(message: string, metadata?: unknown, cause?: unknown): Error2 {
  const normalized = message.toLowerCase();
  const statusCode = Number(/\bHTTP\s+(\d{3})\b/iu.exec(message)?.[1]);
  const traceId =
    typeof metadata === "object" &&
    metadata !== null &&
    typeof (metadata as Record<string, unknown>)["traceId"] === "string"
      ? ((metadata as Record<string, unknown>)["traceId"] as string)
      : undefined;
  const code = /\b(401|403)\b|unauthori[sz]ed|invalid api key|authentication/.test(normalized)
    ? ProviderRuntimeErrors.codes.PROVIDER_AUTH_ERROR
    : /\b429\b|rate.?limit/.test(normalized)
      ? ProviderRuntimeErrors.codes.PROVIDER_RATE_LIMIT
      : /\b529\b|overload|capacity/.test(normalized)
        ? ProviderRuntimeErrors.codes.PROVIDER_OVERLOADED
        : /context.{0,24}(length|window|token)|too many tokens/.test(normalized)
          ? ProviderRuntimeErrors.codes.CONTEXT_OVERFLOW
          : /fetch failed|network|connection|timed? ?out|socket/.test(normalized)
            ? ProviderRuntimeErrors.codes.PROVIDER_CONNECTION_ERROR
            : ProviderRuntimeErrors.codes.PROVIDER_API_ERROR;
  return new Error2(code, message, {
    cause,
    details: {
      statusCode: Number.isFinite(statusCode) ? statusCode : undefined,
      traceId,
    },
  });
}
