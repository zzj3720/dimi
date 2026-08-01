/**
 * OAuth protocol type definitions for Dimi Coding.
 *
 * Only Device Code Flow (RFC 8628) is supported, against
 * `https://auth.kimi.com`.
 *
 */

/** OAuth token bundle returned by the token endpoint. */
export interface TokenInfo {
  readonly accessToken: string;
  readonly refreshToken: string;
  /** Unix seconds when access_token expires. */
  readonly expiresAt: number;
  readonly scope: string;
  readonly tokenType: string;
  /** Original expires_in from server response (seconds). */
  readonly expiresIn: number;
}

/** RFC 8628 §3.2 device authorization response. */
export interface DeviceAuthorization {
  readonly userCode: string;
  readonly deviceCode: string;
  readonly verificationUri: string;
  readonly verificationUriComplete: string;
  /** Seconds until device_code expires (server-reported). May be null. */
  readonly expiresIn: number | null;
  /** Polling interval in seconds. */
  readonly interval: number;
}

/** OAuth flow endpoint + client configuration. */
export interface OAuthFlowConfig {
  /** Logical provider name for storage (e.g. "dimi"). */
  readonly name: string;
  /** Base URL of the OAuth server, no trailing slash. */
  readonly oauthHost: string;
  /** Client ID registered with the OAuth provider. */
  readonly clientId: string;
}

/** Device identification for `X-Msh-*` headers. */
export interface DeviceHeaders {
  readonly "X-Msh-Platform": string;
  readonly "X-Msh-Version": string;
  readonly "X-Msh-Device-Name": string;
  readonly "X-Msh-Device-Model": string;
  readonly "X-Msh-Os-Version": string;
  readonly "X-Msh-Device-Id": string;
}
