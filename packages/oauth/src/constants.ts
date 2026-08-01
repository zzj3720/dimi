import type { OAuthFlowConfig } from './types';

export const DEFAULT_DIMI_CODE_OAUTH_HOST = 'https://auth.kimi.com';

export const DIMI_CODE_FLOW_CONFIG: OAuthFlowConfig = {
  name: 'kimi-code',
  oauthHost:
    process.env['DIMI_CODE_OAUTH_HOST'] ??
    process.env['DIMI_OAUTH_HOST'] ??
    DEFAULT_DIMI_CODE_OAUTH_HOST,
  clientId: '17e5f671-d194-4dfb-9706-5516cb48c098',
};
