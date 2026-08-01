# CHANGELOG

## 0.4.1

### Patch Changes

- [`4c6b7ec`](https://github.com/zzj3720/dimi/commit/4c6b7ec31a5d513b0c1d05902a3bf0a4398e58ab) - Fix the footer CH badge and /usage panel under-reporting the prompt cache hit rate for OpenAI-compatible providers, where cached input tokens were counted twice.

- [`5db585a`](https://github.com/zzj3720/dimi/commit/5db585a6e32602eb4d3f2356a30595564457619e) - Render WaitFor wait cards with a live count-up timer, the max wait duration, and the wait reason instead of the generic tool output.

## 0.4.0

### Minor Changes

- [#5](https://github.com/zzj3720/dimi/pull/5) [`00e8096`](https://github.com/zzj3720/dimi/commit/00e80969518caeec23ddec29c84d93f788dd8fbe) Thanks [@zzj3720](https://github.com/zzj3720)! - Add the remote mobile bridge: a relay client that pairs native mobile clients with a running Kap server over an encrypted WebSocket relay. Run `dimi remote --relay <wss-url>` to connect, and pair the Android client from `apps/mobile`. Includes the Cloudflare Worker relay (`apps/relay`), the Android Expo client (`apps/mobile`), and the Android distribution page (`apps/distribution`). Remote questions also stay pending across the end of their originating turn so a paired client can answer them later.

## 0.3.0

### Minor Changes

- [`9fc5cfa`](https://github.com/zzj3720/dimi/commit/9fc5cfa86e6713756f9b90847b68eb10718ce61b) - Announce the `AllDone` completion protocol in the base system prompt and require agents to review every tool-free response and explicitly call `AllDone` before ending, while refusing completion until background tasks have settled.

## 0.2.1

### Patch Changes

- [`9cc3700`](https://github.com/zzj3720/dimi/commit/9cc37008861e47dc3a25fb1aae7e2145097114d8) - Remove the bundled VS Code extension from the repository.

- [`eb96994`](https://github.com/zzj3720/dimi/commit/eb9699455769c83e9ba9a76cd8b37f318a5a6af5) - Stop folding assistant messages into the transcript step summary, so model output stays visible instead of being collapsed into a message count.

## 0.2.0

### Minor Changes

- [`23da84f`](https://github.com/zzj3720/dimi/commit/23da84fed3e850fa7b94ce5d40b598f63d01cfa7) Thanks [@zzj3720](https://github.com/zzj3720)! - Add experimental stdin support for background Bash tasks. Enable `background-bash-stdin`, start Bash with `stdin_mode="pipe"`, and let the agent use `TaskInput` to write input or send EOF.

- [#2365](https://github.com/MoonshotAI/kimi-code/pull/2365) [`fa2c5ce`](https://github.com/zzj3720/dimi/commit/fa2c5ce18b70577fa3ada4eb8bdd4993891994ce) Thanks [@7Sageer](https://github.com/7Sageer)! - Add support for plugin-contributed custom agents, discovered automatically and available for sub-agent delegation. Ship an `agents/` directory in the plugin (or declare `agents` paths in the plugin manifest) to provide them.

- [#2314](https://github.com/MoonshotAI/kimi-code/pull/2314) [`02d77b2`](https://github.com/zzj3720/dimi/commit/02d77b20d941873563f14890e049ffe40cec76e4) Thanks [@7Sageer](https://github.com/7Sageer)! - Allow enabled plugins to contribute agent system-prompt instructions through `systemPrompt` or `systemPromptPath` in `dimi.plugin.json`, effective on both agent engines (the TUI, `dimi -p`, and `dimi web`).

- [#3](https://github.com/zzj3720/dimi/pull/3) [`508c9d5`](https://github.com/zzj3720/dimi/commit/508c9d5eb01f484f12fb193e2bdfa468f68f93ea) Thanks [@zzj3720](https://github.com/zzj3720)! - Add a shared provider runtime with dynamic model catalogs, account connections, and `models.json` overlays across the CLI, TUI, and SDK. Run `dimi provider list` or `/provider` to start.

- [#2232](https://github.com/MoonshotAI/kimi-code/pull/2232) [`efac96c`](https://github.com/zzj3720/dimi/commit/efac96c8a95a3c3ca4e1ae9bce38082498a02b2e) Thanks [@7Sageer](https://github.com/7Sageer)! - Support Markdown-defined custom agents on agent-core.

- [#2232](https://github.com/MoonshotAI/kimi-code/pull/2232) [`efac96c`](https://github.com/zzj3720/dimi/commit/efac96c8a95a3c3ca4e1ae9bce38082498a02b2e) Thanks [@7Sageer](https://github.com/7Sageer)! - Add the /secondary_model slash command to configure the secondary model used by subagents.

- [#3](https://github.com/zzj3720/dimi/pull/3) [`508c9d5`](https://github.com/zzj3720/dimi/commit/508c9d5eb01f484f12fb193e2bdfa468f68f93ea) Thanks [@zzj3720](https://github.com/zzj3720)! - web: Add provider account connection, model refresh, and logout controls.

### Patch Changes

- [`abd16b7`](https://github.com/zzj3720/dimi/commit/abd16b757ed0e069ef6347231445abb8fe0b57c2) - Remove product self-identification from the default prompt and keep runtime instructions concise.

- [`0e2001d`](https://github.com/zzj3720/dimi/commit/0e2001db17c568e4f14b0285edbe3b1add07f806) - Fix DeepSeek V4 thinking controls to expose and send the supported effort levels.

- [`143ec82`](https://github.com/zzj3720/dimi/commit/143ec82b32bff142552e4de1a23ff27c80b3293a) - Enable Dimi's own update channel backed by GitHub Releases. `dimi upgrade` and the startup check now read `latest.json` from the newest GitHub Release (published automatically by the `publish-update-channel` workflow on `v*` tags), and source builds get a `git pull --ff-only && vp install` manual upgrade hint instead of pointing at the pre-fork npm package.

- [#3](https://github.com/zzj3720/dimi/pull/3) [`255b905`](https://github.com/zzj3720/dimi/commit/255b905a2cf67d72bfba634021b55eb1ba7b12c8) Thanks [@zzj3720](https://github.com/zzj3720)! - Disable automatic upgrades and remote banners until this source repository has its own release channel, so a 0.1.x build cannot install or advertise an unrelated pre-fork release.

- [`c902edc`](https://github.com/zzj3720/dimi/commit/c902edcdecac4a7ec1be3c29402bcc292e1a7ddb) - Expose the max thinking effort for DeepSeek V4 models through the OpenCode Go provider instead of degrading to high.

- [#2379](https://github.com/MoonshotAI/kimi-code/pull/2379) [`691ec46`](https://github.com/zzj3720/dimi/commit/691ec4679ea19d6be8ac18f359088384ed3e446d) Thanks [@RealKai42](https://github.com/RealKai42)! - Remove the blocking `block`/`timeout` wait from the TaskOutput tool so checking a background task can no longer stall the conversation; it now always returns an immediate snapshot, and completion still arrives via automatic notification.

- [`c47e5f7`](https://github.com/zzj3720/dimi/commit/c47e5f7868b98036a3ba401baa887afd16741670) - Fold consecutive tool-call rounds without visible assistant text into a single summary line.

Forked from [MoonshotAI/kimi-code](https://github.com/MoonshotAI/kimi-code).
Historical changelog: <https://github.com/MoonshotAI/kimi-code/blob/main/CHANGELOG.md>
