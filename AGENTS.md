# Repository-level Agent Guide

Reply in the same language as the user.

This is a TypeScript monorepo built for agent-assisted development. Keep the root `AGENTS.md` limited to hot-path rules: the project map, hard constraints, and workflow requirements — things every task needs to know.

## Fork Policy

This repository is a fork of `MoonshotAI/kimi-code` (first commit `Kimi For Coding`). We no longer merge upstream commits: `main` evolves only through our own changes, and features we need are built here, never pulled from upstream. The `upstream` remote is kept read-only for reference while we replace kimi's original designs; never merge or cherry-pick from it.

## Working Principles

- Think from first principles. Start from real requirements, code facts, and verification results; if the goal is unclear, discuss it with the user first.
- Treat code, not documentation, as the source of truth. Unless the user explicitly says otherwise, do not read ordinary Markdown just to understand the implementation.
- Before making code changes, read the relevant code and the most recent constraints, and follow the nearest `AGENTS.md` in the directory tree.
- Keep changes focused. Do not slip in unrelated refactors along the way.
- When committing, do not add any co-author attribution, and do not reveal the identity of the agent in commit messages, PR descriptions, or any explanatory text.

## Project Map

- `apps/dimi`: the CLI / TUI application. It consumes core capabilities through `@dimi-agent/dimi-sdk`; the runtime is composed below the SDK boundary. When writing or modifying its terminal UI, use the `write-tui` skill (`.agents/skills/write-tui/SKILL.md`).
- `apps/dimi-web`: the browser web UI, a peer to the TUI. Vue 3 + Vite + vue-i18n; talks to the server over REST + WebSocket under `/api/v1`. It is decoupled from the runtime and re-implements wire types locally. Run the server with `pnpm dev:server`; Vite proxies to `DIMI_SERVER_URL`. See `apps/dimi-web/AGENTS.md`.
- `packages/agent-core-v2`: the sole agent runtime, organized as DI scopes and domain services for Agent, Session, tools, tasks, permissions, persistence, and providers.
- `packages/node-sdk`: the public TypeScript SDK and harness.
- `packages/agent-core-v2/src/app/providerRuntime`: the project-owned provider,
  model catalog, authentication, and streaming runtime.
- `packages/oauth`: Dimi OAuth and managed auth utilities.
- `packages/telemetry`: shared client-side telemetry infrastructure.
- `packages/transcript`: the isomorphic transcript rendering data layer — agent-granular L1 store, idempotent L2 operations, `off/turn/block/delta` L3 subscription granularity, framework-free L4 view registry, and turn-cursor pagination. Pure TypeScript (browser-safe, no engine imports) and the sole owner of all transcript contract types (`src/contract/`); consumed by `packages/kap-server` (engine events → transcript, REST + WS surface; live stores backfill history from the persisted per-agent wire records — main on first attach, any agent on demand, cold sessions rebuild any agent — with 0-based turn ordinals matching the engine's). The cold rebuild is a two-level fold over `wire.jsonl` as the single source of truth: `history/groupTurns.ts` (context messages → turn tree) plus `history/foldFacts.ts` (non-context records → tasks, interactions, todos, goal/plan/swarm meta, and end-appended markers/taskrefs; interactions left pending at shutdown fold to `cancelled`). Plan content is a recorded fact too: each ExitPlanMode review submission offloads the document to `agents/<agentId>/plan/<planId>/v<N>.md` and persists a reference-only `plan.revision` record (`{id, version, path, sha256, bytes}`), which projects — live and cold — to a `plan.revision` marker and the `modes.plan` badge (`{reviewPath, version}`). It also owns the op-batch sequencing contract (`transcriptSeqSchema` in `contract/schema.ts`): a per-(session, agent) monotonic batch `seq` on `transcript.ops` / `transcript.reset` / the REST transcript response, the `transcript_since` subscription cursor, and the `GET .../transcript/ops` catch-up response shape — every field optional so pre-seq peers fall back to loss-signal-driven refreshes. Beyond the timeline, the model carries wire-equivalent detail: steps carry `usage` / `finishReason` / `timing` (LLM latencies) / `retry` / interrupt reason, turns carry `durationMs` / `error` / `usage`, tool frames carry the streamed `inputText` and the latest `progress`, tasks carry subagent `resultSummary` / `error` / `stateReason` / `usage`, `meta.agent` mirrors the agent status slices (model / usage / context / permission / phase), a global `prompts` entity (op `prompt.upsert`) tracks the prompt queue, and `hook.result` lands as a `'hook'` marker. These live-projected fields are NOT backfilled by the cold rebuild (known limitation).
- `packages/kap-server`: the Dimi server, backed by the DI × Scope agent engine (`@dimi-agent/agent-core-v2`). Exposes sessions over REST + WebSocket (`/api/v1` + `/api/v1/ws`); bootstrapped from `src/start.ts` and consumed by `apps/dimi`. The RPC surface is `/api/v1/debug/*` — a reflection dispatcher over the ENTIRE scoped DI registry (every Service callable, no whitelist; `src/transport/registerDebugRoutes.ts` + `serviceDispatcherRoutes.ts`), mounted only with `--debug-endpoints` on a loopback bind and gated by the global bearer auth; repo dev scripts pass the flag. Its transcript surface implements the op-batch sequencing contract: `TranscriptService.dispatchOps` assigns every dispatched batch a per-agent consecutive `seq` and retains it in a bounded in-memory journal (`TRANSCRIPT_OPS_JOURNAL_CAPACITY`, dies with the live store); WS `transcript.ops`/`transcript.reset` payloads carry the seq/watermark, a `transcript_since` subscription cursor (carried, with the per-agent grades, by the `subscribe_v2` control frame — the only transcript subscription channel; its agent-grained counterpart `unsubscribe_v2` detaches listed agents' streams, or the whole session's when `agent_ids` is absent, letting the detached agents' legacy events flow again) replays journaled batches instead of a baseline reset when the journal covers it, and `GET /sessions/{id}/transcript/ops?since_seq=` serves point-to-point catch-up (`complete: false` = journal can't cover or session cold → caller falls back to a full refresh). Beside the paged route, `GET /sessions/{id}/transcript/plan?agent_id=[&tool_call_id=]` projects an agent's ExitPlanMode plan info (content / path / options / review outcome; `tool_call_id` narrows to one call, omitted lists every recoverable plan) from the first available fact — the linked approval interaction's persisted request display, the live tool frame's display, or the tool result output text. The baseline `transcript.reset` itself is items-empty (`TRANSCRIPT_RESET_TAIL_TURNS = 0`): it carries only global state + the watermark + `has_more_older`, because history always pages in over REST. When a WS connection subscribes to the transcript protocol (grade ≠ `off` for an agent), the broadcaster suppresses the transcript-projected `session_event` types for that connection × agent (`TRANSCRIPT_PROJECTED_EVENT_TYPES` + `suppressedByTranscript` in `sessionEventBroadcaster.ts`; cursor replay via `getBufferedSince` applies the same filter). Suppression is only a per-connection send view — the journal still records everything, and connections without transcript grades are unaffected. The session's work aggregate behind `event.session.work_changed` (`busy` / `main_turn_active` / `pending_interaction` / `last_turn_reason`) is owned by the core's `ISessionActivityView` (`sessionActivity` domain, Session scope): the broadcaster only schedules the wire emission around turn frames (`busy:false` lands after `turn.ended`), and `resolveSessionFacts` (`src/routes/sessions.ts`) reads the same view — never fold per-agent activity at the edge. Delivery split on `/api/v1/ws`: global events (`session.meta.updated` and the `event.session.*` / `event.workspace.*` / `event.config.*` families, including every activated session's `event.session.work_changed`) fan out to EVERY established connection — `WsConnectionV1` registers itself via `broadcaster.addGlobalTarget` on construction and unregisters on close — while session/agent-grained events only reach connections subscribed to that session (subject to `agent_filter` and the transcript suppression above); transcript frames are a separate channel governed by the per-agent grades alone and bypass `agent_filter` entirely.
- `packages/klient`: the client SDK — a contract-driven facade over agent-core-v2 with aggregated `global.*` / `session(id).*` / `agent(id).*` methods, zod validation on every call, and klient-level typed event forwarding. Transport is chosen once at creation via subpath entry (`@dimi-agent/klient/ipc|memory`); both return the same `Klient`. The package also hosts the e2e suites: the legacy `/api/v1` live suites (`test/e2e/legacy/`) and the docker e2e runner (`pnpm --filter @dimi-agent/klient docker:e2e`). See `packages/klient/AGENTS.md`.
- `packages/server-e2e`: live e2e tests and scenarios against a running server (`DIMI_SERVER_URL`, default `http://127.0.0.1:58627`). See `packages/server-e2e/AGENTS.md`.

## Environment Requirements

- **Node.js**: `>=24.15.0` (from the root `package.json` `engines`; `.nvmrc` is `24.15.0`, used by nvm / fnm / mise to pick the minimum recommended version).
- **pnpm**: `10.33.0` (from the root `package.json` `packageManager`).
- `pnpm install` will fail when the Node version is not satisfied, because `.npmrc` sets `engine-strict=true`.

## Monorepo Workspace Maintenance

- `pnpm-workspace.yaml` is the source of truth for workspace membership, but `flake.nix` also contains **hardcoded** `workspacePaths` and `workspaceNames` lists.
- **Whenever you add or remove a workspace package, you MUST update both `pnpm-workspace.yaml` and `flake.nix` — for every package, including leaf / test / e2e packages that nothing depends on.**
  - `pnpm-workspace.yaml` uses globs (`packages/*`, `apps/*`), so most packages land there automatically; `flake.nix` is fully manual and is where omissions happen.
  - Missing a path in `flake.nix`'s `workspacePaths` will silently drop files from the Nix build's `src` fileset.
  - Missing a name in `flake.nix`'s `workspaceNames` will break `pnpmConfigHook` because dependencies for that workspace will not be fetched.
- The automated "Check flake.nix workspace sync" (`scripts/check-nix-workspace.mjs`) only validates the transitive dependency **closure of `@dimi-agent/cli`**. A leaf package outside that closure (e.g. an e2e package nobody imports) slips through even when it is missing from `flake.nix`. A green check is therefore NOT proof that `flake.nix` is fully in sync — keep it updated by hand on every add/remove, do not rely on the check to catch omissions.

## General Coding Rules

- For optional object properties, pass `undefined` directly instead of using conditional spread.
  - YES: `{ user }`
  - NO: `{ ...(user ? { user } : undefined) }`
- Optional object properties do not need to additionally allow `undefined` in the type.
  - YES: `interface Options { user?: User }`
  - NO: `interface Options { user?: User | undefined }`
- Internal methods with only a single parameter should not be turned into options objects just for stylistic uniformity.
- Except for a package's `index.ts`, other `index.ts` files should prefer `export * from './module';`.
- Do not add too many new test files. Prefer adding tests to the existing test file of the corresponding component or module.
- When a test fails because of a user modification, default to fixing the test first; do not change the implementation to satisfy an old test unless the implementation truly has a bug.
- Do not sacrifice code quality for external compatibility unless the user explicitly asks for it. Breaking changes go through changesets and a `major` bump, gated by the rule below.

## Experimental Features

- Gate a not-yet-public feature behind an experimental flag. Define it beside its owning domain with `registerFlagDefinition`, then check it through `IFlagService`. Flags are env-driven and default off: `DIMI_CODE_EXPERIMENTAL_<NAME>` toggles one, `DIMI_CODE_EXPERIMENTAL_FLAG` enables all. Release by changing the definition's default.

## Where to Update Instructions

- Hard rules that affect almost every task: update the root `AGENTS.md`.
- Rules that only affect a specific directory: update the nearest sub-directory `AGENTS.md`.
- Keep instruction updates focused and supported by code facts.

## Release & Update Channel

Releasing is automated from `main` via `.github/workflows/release.yml`; do not publish manually. A push to `main` triggers:

1. `changeset version` consumes every pending `.changeset/*.md` (version bump + changelog + delete). No pending changesets → no version change.
2. The version bump is committed as `ci: release packages` and pushed to `main`.
3. `changeset publish` publishes to npm. Only `@dimi-agent/cli` is actually published — every other workspace package is `private: true` (changesets versions them, but they are never published). Publishing uses the `NPM_TOKEN` repo secret (OIDC Trusted Publisher is also configured, but the token is the reliable path).
4. The "Report publish outputs" step gates the native release: if a GitHub Release tagged `@dimi-agent/cli@<version>` does not exist, the pipeline builds **unsigned** native binaries for 6 platforms and uploads them to a new GitHub Release — including the `latest.json` update manifest, `install.sh`/`install.ps1`, and the plugin marketplace. Existing release → native step is skipped (idempotent).
5. Docs deploy to GitHub Pages.

The CLI's update channel is `DIMI_CODE_UPDATE_CHANNEL_URL` (the `latest.json` asset on the newest GitHub Release), consumed by `dimi upgrade` / `dimi update` (aliases). Install source is detected (`npm` vs native script); native installs self-update from the GitHub Release assets.

Gotchas:

- The workflow `release` job needs `pull-requests: write` (changelog generation resolves PR info via the API) and must pass `GITHUB_TOKEN` explicitly to the `changeset version` step; it also needs `id-token: write` for npm OIDC and `contents: write` for the commit/push.
- The fork has no Apple secrets, so macOS binaries are unsigned (`sign-macos: false`).
- The native release gate reads the step id `changesets` ("Report publish outputs"), whose `publishedPackages` output feeds `apps/dimi/scripts/native/resolve-release.mjs` — keep those ids/outputs in sync when editing the workflow.
- The update-channel design lives in `apps/dimi/src/constant/app.ts` (`DIMI_CODE_UPDATE_CHANNEL_URL`, `DIMI_CODE_CDN_BASE`).

## Workflow Requirements

- Prefer `rg` / `rg --files` when reading code.
- When designing changes, follow existing boundaries and local patterns first.
- In public text and test data, replace real internal identifiers with neutral placeholders such as `example.com`, `example.test`, and `YOUR_API_KEY`. Before opening a PR, ask a read-only agent to audit the diff for context-specific internal identifiers.
- When creating a PR, the PR title must follow Conventional Commit style, e.g. `chore: remove legacy format commands`.
- When an AI agent opens or updates a PR, fill in `.github/pull_request_template.md` — link the related issue or explain the problem, then describe what changed. Do not leave placeholder text or submit a generic summary of the diff.
- Do not submit vague AI-generated PR text. The human author must understand the change well enough to explain the code, edge cases, and why the approach fits this repository.
- After finishing a task and before submitting a PR, you must run the `gen-changesets` skill (see `.agents/skills/gen-changesets/SKILL.md`) and generate a changeset under `.changeset/` according to its rules.
- When generating a changeset, **never** decide on a `major` bump on your own. When you judge a change to meet the major criteria (breaking changes, incompatible user configuration, renamed or removed commands/arguments, changed behavior semantics, etc.), you must stop and explain it to the user and ask for confirmation. **Only write `major` after the user has explicitly agreed.** Otherwise default to `minor` (and fall back to `patch` if `minor` is unclear). See the "Hard rule: confirm with the user before writing `major`" section in `.agents/skills/gen-changesets/SKILL.md` for details.
- Prefer importing via `import ... from '#/...'`, which serves the same purpose as `import ... from '@/...'`.
- Do not commit throwaway scratch or exploratory files. Never stage:
  - Agent working notes or handoff/summary documents (e.g. `HANDOVER-*.md`, `HANDOFF-*.md`, `handoff.md`).
  - Throwaway UI/UX prototypes or design mockups (e.g. `*-designs.html`, `*-mockup.html`, `*-demo(s).html`) at the repo root or under a `design/` folder. The only tracked `.html` files should be Vite `index.html` entrypoints.
    Before committing or opening a PR, run `git status` and `git diff --staged --stat` and remove anything matching these patterns. Put scratch work under `.tmp/` (gitignored) instead of the repo root or the source tree.
