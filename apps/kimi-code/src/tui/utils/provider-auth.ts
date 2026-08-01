import type { AuthInteraction } from '@moonshot-ai/kimi-code-sdk';

import { openUrl } from '#/utils/open-url';

type ProviderAuthEvent = Parameters<AuthInteraction['notify']>[0];

export function openAuthEventUrl(event: ProviderAuthEvent): void {
  if (event.type === 'auth_url') {
    openUrl(event.url);
  } else if (event.type === 'device_code') {
    openUrl(event.verificationUri);
  }
}
