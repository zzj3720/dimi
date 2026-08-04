# CHANGELOG

## 0.6.12

### Patch Changes

- [`7354c99`](https://github.com/zzj3720/dimi/commit/7354c99cd02a473488e33b76a63beebc29e2a39f) - Fix subagents (Agent tool) not receiving the tool definitions in their LLM request, so they did not know the available tools existed (e.g. claimed there were no file-reading tools); subagent requests now advertise the parent's tool set.

## 0.6.11

### Patch Changes

- [`128a849`](https://github.com/zzj3720/dimi/commit/128a849c939f40d0dabdb5fafac3ce6c7cfc0117) - Fix every Bash tool call failing with "No such file or directory" when the session working directory resolved to an empty string; the shell now falls back to the process working directory.

## 0.6.10

### Patch Changes

- [`f195cc0`](https://github.com/zzj3720/dimi/commit/f195cc0afb8d608042551dea629b0efc4418c85a) - Fix tool definitions being dropped from LLM requests in the Rust engine, which made models write tool calls as literal XML text instead of calling tools; tools are now sent in the format the request path parses.

## 0.6.9

### Patch Changes

- [`6f335e1`](https://github.com/zzj3720/dimi/commit/6f335e14e63f5412989950929cc844d7fb88d74c) - Fix the Rust engine sending the qualified "provider/model" alias as the request model id; strict providers (e.g. OpenCode) rejected it with HTTP 401, so the bare catalog model id is sent instead.

## 0.6.8

### Patch Changes

- [`91da4d2`](https://github.com/zzj3720/dimi/commit/91da4d2689cb4e8b7829144e67f433e898d073d4) - Fix the Rust engine not retrying some transient provider errors (rate limits, overloads, connection failures) that should be retried per step.

- [`91da4d2`](https://github.com/zzj3720/dimi/commit/91da4d2689cb4e8b7829144e67f433e898d073d4) - Fix token usage accounting across tool steps in the Rust engine, suppress repeated identical tool calls, and stop exposing disabled tools to the model.

## 0.6.7

### Patch Changes

- [`005d37e`](https://github.com/zzj3720/dimi/commit/005d37ebeab4f70eccd566916802892e73fe957a) - Warn when a Bash command leaves processes running outside dimi control (e.g. via `nohup` or `&`): the result lists them and explains they are not tracked, cannot be stopped with TaskStop, and WaitFor only wakes on timeout.

## 0.6.6

### Patch Changes

- [`bf0de20`](https://github.com/zzj3720/dimi/commit/bf0de204049a93e66a9828e80045d38b29e0f62a) - Require external-state watchers to run as dimi background tasks so their completion wakes the wait; detached processes (nohup, cron) are invisible to dimi and no longer suggested in the WaitFor guidance.

## 0.6.5

### Patch Changes

- [`18f506a`](https://github.com/zzj3720/dimi/commit/18f506ac591d2a18eaee60cb8fdfeb95eaa1ac3c) - Make the WaitFor guidance surface watcher errors: a background poll script must print failures and exit non-zero so the wait wakes with the real reason instead of silently timing out.

## 0.6.4

### Patch Changes

- [`59b471d`](https://github.com/zzj3720/dimi/commit/59b471d606d4584d31a0c7d40eb1dc699b1735e2) - Guide WaitFor to poll external state changes through a background watcher so waits wake as soon as the monitored condition changes instead of on a blind timeout.

## 0.6.3

### Patch Changes

- [`67c5a66`](https://github.com/zzj3720/dimi/commit/67c5a66607979ca2ebbfb942630f86af227d3543) - Fix resumed sessions showing a compaction summary as a user message and hiding pre-compaction history after context compaction.

## 0.6.2

### Patch Changes

- [`af12ae9`](https://github.com/zzj3720/dimi/commit/af12ae9925be69b4c9be2fd917b3e8f7b54667e4) - Fold tool calls progressively in the TUI: while a tool-call run is still growing, only the newest two calls stay expanded and older finished ones merge into the summary immediately, instead of everything staying expanded until the run ends. Running calls (streaming args, live subagents, pending reads) are never folded away.

## 0.6.1

### Patch Changes

- [`ad98cbe`](https://github.com/zzj3720/dimi/commit/ad98cbe222edbd96500b945c274bf43790754f8a) - Fix the WaitFor wait card keeping its elapsed timer running after a notification wakes the wait; the elapsed time is now frozen when the wait ends.

## 0.6.0

### Minor Changes

- [`be16a51`](https://github.com/zzj3720/dimi/commit/be16a515d61d62b4315a818e3e9cf8ad3fa3d87a) - Add a Context size setting that caps the conversation context window as a percentage of the model's default, in 5% steps with a 200k token floor (models below 200k keep their window). Set it under Settings → Context size in the TUI, or via `loop_control.context_size_percent` in config.toml.

## 0.5.5

### Patch Changes

- [`ff33753`](https://github.com/zzj3720/dimi/commit/ff33753b39a6d4119abaae6ac69b6b58dddfaab0) - Make subagents fully asynchronous, add the AgentOutput tool so the agent can inspect a subagent's recent transcript-style output (assistant text, thinking, tool calls, progress), and let it message a running subagent via resume — the prompt is steered into its current turn, like a human steering the agent.

## 0.5.4

### Patch Changes

- [`9b87918`](https://github.com/zzj3720/dimi/commit/9b87918a1019d6e9d62de07c87f2e5a6d45996f9) - Ship the Rust runtime with npm installs: the platform-specific native binding is now installed for every supported platform, and native (SEA) release binaries embed it instead of falling back to the legacy TypeScript backend.

## 0.5.3

### Patch Changes

- [`fa5569c`](https://github.com/zzj3720/dimi/commit/fa5569c26ac1dbfdb38752dfa5aa18fcd6f9c93e) - Ship the Rust runtime binary in the npm package, and fall back to the TypeScript backends with a warning when it is unavailable instead of crashing at startup.

- [`fa5569c`](https://github.com/zzj3720/dimi/commit/fa5569c26ac1dbfdb38752dfa5aa18fcd6f9c93e) - Use the Rust runtime for process, file, environment, file-watch and terminal operations by default. Pass `--legacy` to keep the TypeScript backends.

## 0.5.2

### Patch Changes

- [`4cc2ef8`](https://github.com/zzj3720/dimi/commit/4cc2ef822bc15431f30c04abb446d20a07a084bf) - Merge consecutive tool calls into one summary line when only invisible content (notifications, whitespace deltas) separates them, including across notification-driven turns.

## 0.5.1

### Patch Changes

- [`d1726cd`](https://github.com/zzj3720/dimi/commit/d1726cdcfe70661df6107dc93e2148dc65c0ed51) - Clarify the AllDone tool guidance so agents call WaitFor directly when waiting on the user instead of replying with repeated status text.

## 0.5.0

### Minor Changes

- [`c4234fa`](https://github.com/zzj3720/dimi/commit/c4234fa70603cfe1d3e3f0c8511897b4996247b4) - Remove goal mode: the `/goal` command, the goal tools (CreateGoal, GetGoal, SetGoalBudget, UpdateGoal), the session goal API, and the `GET /sessions/{id}/goal` route are gone.

## 0.4.4

### Patch Changes

- [`2914a0f`](https://github.com/zzj3720/dimi/commit/2914a0ff07648c38904825435926e8022cfb7923) - Allow /model, /effort, and other configuration or read-only slash commands while the agent is streaming or compacting.

## 0.4.3

### Patch Changes

- [`7d06026`](https://github.com/zzj3720/dimi/commit/7d0602616cb56da9314b554b90722301468556e5) - Add a --legacy flag to use the TypeScript backend instead of the Rust runtime. Pass --legacy to opt out.

## 0.4.2

### Patch Changes

- [`42e5854`](https://github.com/zzj3720/dimi/commit/42e58548ab505e1aa7b3c0d734e7f0e69bed32bf) - Keep the AllDone and WaitFor control cards visible in the transcript: AllDone renders as its own "Work complete" card and WaitFor keeps its live timer card, instead of both being folded into the tool-run summary line.

- [`42e5854`](https://github.com/zzj3720/dimi/commit/42e58548ab505e1aa7b3c0d734e7f0e69bed32bf) - Remember the chosen permission mode and thinking effort for new sessions. /permission, /yolo, and /auto save the mode as the default, and thinking effort is remembered per model ([model_efforts]) — every declared level including the highest — so switching models or starting a new session resumes the effort you picked.

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
