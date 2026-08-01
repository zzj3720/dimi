---
"@dimi-agent/cli": minor
---

Add the remote mobile bridge: a relay client that pairs native mobile clients with a running Kap server over an encrypted WebSocket relay. Run `dimi remote --relay <wss-url>` to connect, and pair the Android client from `apps/mobile`. Includes the Cloudflare Worker relay (`apps/relay`), the Android Expo client (`apps/mobile`), and the Android distribution page (`apps/distribution`). Remote questions also stay pending across the end of their originating turn so a paired client can answer them later.
