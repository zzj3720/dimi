import { createHash } from "node:crypto";

import { DEFAULT_DIMI_CODE_OAUTH_HOST } from "./constants";
import { DEFAULT_DIMI_CODE_BASE_URL } from "./managed-usage";

export const DIMI_CODE_OAUTH_KEY = "oauth/kimi-code";

const DIMI_CODE_SCOPED_OAUTH_KEY_PREFIX = "oauth/kimi-code-env-";

function normalizeEndpoint(value: string): string {
  return value.trim().replace(/\/+$/, "");
}

export function resolveKimiCodeOAuthKey(options: {
  readonly oauthHost?: string;
  readonly baseUrl?: string;
}): string {
  const oauthHost = normalizeEndpoint(options.oauthHost ?? DEFAULT_DIMI_CODE_OAUTH_HOST);
  const baseUrl = normalizeEndpoint(options.baseUrl ?? DEFAULT_DIMI_CODE_BASE_URL);

  if (
    oauthHost === normalizeEndpoint(DEFAULT_DIMI_CODE_OAUTH_HOST) &&
    baseUrl === normalizeEndpoint(DEFAULT_DIMI_CODE_BASE_URL)
  ) {
    return DIMI_CODE_OAUTH_KEY;
  }

  const digest = createHash("sha256")
    .update(JSON.stringify({ oauthHost, baseUrl }))
    .digest("hex")
    .slice(0, 16);
  return `${DIMI_CODE_SCOPED_OAUTH_KEY_PREFIX}${digest}`;
}
