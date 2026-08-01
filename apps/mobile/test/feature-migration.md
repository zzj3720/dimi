# Android client feature migration matrix

This matrix is a manual review of every TUI and Web test present on 2026-07-31.
It classifies user outcomes, not implementation files: Android must preserve useful
session behavior without copying terminal, browser, credential, or host-control
surfaces.

Status:

- **Now** — covered by the current Android client or its Remote E2E.
- **Next** — belongs on Android, but needs a named protocol or UI prerequisite.
- **Deferred** — useful mobile behavior outside the first usable release.
- **Native** — preserve the outcome with Android UI, not the source platform mechanism.
- **Host only** — intentionally remains on the runtime host.

## TUI tests

### Conversation, transcript, and interactions

- **Now** `kimi-tui-message-flow.test.ts` — prompt, busy-state steer, cancel, error notice, and draft-preserving failure semantics.
- **Now** `message-replay.test.ts` — session reload, reconnect, resync, and transcript ordering without duplication.
- **Now** `components/dialogs/session-picker.test.ts` — select an existing session; terminal row layout does not migrate.
- **Now** `components/messages/user-message.test.ts` — stable user-message rendering.
- **Now** `components/messages/assistant-message.test.ts` — stable Markdown assistant messages; no token streaming.
- **Now** `components/messages/notice.test.ts` — ordered notice and error presentation.
- **Now** `components/messages/step-summary.test.ts` — stable assistant result text without exposing raw event frames.
- **Now** `components/messages/tool-call-sequence.test.ts` — consecutive tool calls collapse into one summary row.
- **Now** `components/messages/tool-call.test.ts` — individual tool name, state, and safe summary remain available after expansion.
- **Now** `components/messages/tool-renderers/chip.test.ts` — compact tool identity and state.
- **Next** `components/messages/tool-renderers/media.test.ts` — render only explicitly transferred, size-limited media metadata.
- **Next** `components/messages/tool-renderers/registry.test.ts` — add Android-native renderers only when they reduce ambiguity over the generic tool card.
- **Now** `components/messages/tool-renderers/truncated.test.ts` — bounded preview with a separate long-content screen.
- **Now** `components/media/code-highlight.test.ts` — long code opens on a dedicated screen instead of expanding the timeline.
- **Next** `components/media/diff-preview.test.ts` — add a read-only, bounded Android diff view for approval and tool results.
- **Next** `components/dialogs/compaction.test.ts` — expose compacting/completed/failed as read-only session activity.
- **Deferred** `components/panels/todo-panel.test.ts` — read-only agent checklist; the phone never owns todo state.
- **Deferred** `components/panels/plan-box.test.ts` — show plan content only when attached to a structured approval.
- **Host only** `components/messages/thinking.test.ts` — raw reasoning and token-level thinking are intentionally not rendered.
- **Host only** `components/messages/usage-panel.test.ts` — provider account usage and token accounting stay with the host.
- **Deferred** `components/messages/agent-group.test.ts` — separate per-agent transcript navigation before exposing subagent groups.
- **Deferred** `components/messages/background-agent-status.test.ts` — read-only aggregate after per-agent transcript isolation exists.
- **Deferred** `components/messages/agent-swarm-progress.test.ts` — read-only swarm result/activity, not swarm control.
- **Deferred** `components/messages/status-panel.test.ts` — small read-only session diagnostics only if connection state is insufficient.
- **Host only** `components/messages/shell-execution.test.ts` — only a safe generic tool summary migrates; shell control/output does not.
- **Host only** `components/messages/shell-run.test.ts` — direct bash mode remains on the host.
- **Host only** `components/messages/mcp-status-panel.test.ts` — MCP control and credentials remain on the host.
- **Deferred** `components/messages/goal-markers.test.ts` — read-only goal state may migrate with goal projection.
- **Deferred** `components/messages/goal-panel.test.ts` — read-only goal result may migrate; goal lifecycle control stays on the host.

### Approval and question flows

- **Now** `reverse-rpc/approval.test.ts` — approval is session-scoped, idempotent, reconnect-safe, and serial.
- **Now** `reverse-rpc/question.test.ts` — structured answers, Other/free text, dismiss, reconnect, and serial pending requests.
- **Now** `reverse-rpc/approval-adapter.test.ts` — Android submits the protocol response, never parses tool text.
- **Host only** `reverse-rpc/base-controller.test.ts` — TUI controller implementation.
- **Host only** `reverse-rpc/index.test.ts` — TUI reverse-RPC registration.
- **Now** `components/dialogs/approval-panel.test.ts` — action, safe input preview, feedback, approve once/session, and reject.
- **Now** `components/dialogs/approval-preview.test.ts` — bounded safe preview before a decision.
- **Now** `components/dialogs/question-dialog.test.ts` — options, multiple selection, Other, descriptions, submit, and dismiss.
- **Native** `components/dialogs/choice-picker.test.ts` — Android buttons, checkbox rows, and modal focus replace terminal key handling.
- **Native** `components/dialogs/feedback-input-dialog.test.ts` — feedback is embedded in the approval card.

### Input and attachments

- **Next** `input/image-attachment-store.test.ts` — requires encrypted bounded binary transfer, MIME/quantity/size limits, cancellation, and Android URI permissions.
- **Next** `input/image-placeholder.test.ts` — show selected local attachment order and upload state after the binary protocol exists.
- **Next** `controllers/editor-keyboard-image-paste.test.ts` — replace paste with Android document/photo picker; preserve text/attachment order and failed-upload draft.
- **Host only** `controllers/clipboard-image-hint.test.ts` — terminal clipboard polling and focus hints.
- **Host only** `components/media/image-thumbnail.test.ts` — Android will use a native preview after safe attachment transfer.
- **Host only** `media-url.test.ts` — never expose base64 or reuse TUI media URL helpers.
- **Next** `components/editor/file-mention-provider.test.ts` — only after a privacy-preserving, allowlisted remote file-reference API exists.
- **Host only** `components/editor/custom-editor.test.ts` — terminal editor and completion lifecycle.
- **Host only** `components/editor/normalize-caps-locked-ctrl.test.ts` — terminal key normalization.
- **Host only** `components/editor/prompt-symbol.test.ts` — terminal glyph.
- **Host only** `components/editor/side-borders.test.ts` — terminal layout.
- **Host only** `components/editor/slash-highlight.test.ts` — terminal slash highlighting.
- **Host only** `components/editor/wrapping-select-list.test.ts` — terminal list measurement and wrapping.
- **Host only** `controllers/editor-keyboard.test.ts` — terminal key map.
- **Native** `commands/copy.test.ts` — Android selection/long-press copy replaces `/copy`.

### Queue, task, agent, and goal results

- **Now** `components/panes/queue-pane.test.ts` — preserve host-authoritative queued prompt and steer outcomes; do not duplicate a local queue state machine.
- **Deferred** `background-task-status.test.ts` — show read-only task results after a stable task projection exists.
- **Deferred** `tasks-browser.test.ts` — read-only task list/result, no start/stop/control surface.
- **Deferred** `task-output-viewer.test.ts` — bounded read-only task result page.
- **Host only** `utils/foreground-task.test.ts` — terminal foreground-task helper.
- **Deferred** `commands/swarm.test.ts` — only read-only swarm status/results; swarm mode and permission controls remain host-side.
- **Host only** `controllers/session-event-handler-goal-queue.test.ts` — host owns goal queue progression.
- **Host only** `goal-queue-store.test.ts` — host filesystem owns durable upcoming goals.
- **Host only** `components/dialogs/goal-queue-manager.test.ts` — mobile does not reorder host goals.
- **Deferred** `commands/goal.test.ts` — a later structured goal API may expose read-only status and explicit cancel; no file-backed queue replication.
- **Deferred** `utils/goal-completion.test.ts` — consume a structured goal result, not terminal markup.
- **Deferred** `components/panels/footer-goal-badge.test.ts` — a compact read-only goal indicator may replace the terminal badge.
- **Deferred** `components/panels/footer-bg-agents.test.ts` — a compact read-only background-agent indicator may migrate after agent isolation.

### Session, startup, pagination, and notifications

- **Now** `kimi-tui-startup.test.ts` — only the user outcome of reopening an existing session migrates; CLI args, OAuth, and terminal startup do not.
- **Next** `utils/paging.test.ts` — Android must load older sessions and transcript pages; it does not reuse the TUI pager.
- **Next** `utils/transcript-window.test.ts` — replace in-memory terminal clipping with explicit server pagination.
- **Native** `terminal-notification.test.ts` — Android notifications require a reliable background/push design before being enabled.
- **Host only** `terminal-focus.test.ts` — terminal focus detection.
- **Host only** `signal-handlers.test.ts` — POSIX signals and terminal cleanup.
- **Host only** `utils/dead-terminal.test.ts` — terminal death detection.
- **Host only** `printable-key-guard.test.ts` — terminal printable-key protection.
- **Host only** `tmux-keyboard.test.ts` — tmux key sequences.
- **Host only** `terminal-theme.test.ts` — terminal theme detection.
- **Host only** `theme/custom-theme-loader.test.ts` — terminal theme files.

### Models, providers, extensions, credentials, and host capabilities

- **Deferred** `commands/secondary-model.test.ts` — model configuration is explicitly outside this mobile-client milestone.
- **Deferred** `components/dialogs/model-selector.test.ts` — later structured model selection, without exposing provider credentials.
- **Deferred** `components/dialogs/tabbed-model-selector.test.ts` — same outcome as model selection; terminal tabs do not migrate.
- **Deferred** `components/dialogs/effort-selector.test.ts` — later structured reasoning-effort selection.
- **Host only** `components/dialogs/api-key-input-dialog.test.ts` — provider credentials never move to the phone.
- **Host only** `components/dialogs/custom-registry-import.test.ts` — registry import and credentials remain host-side.
- **Host only** `components/dialogs/provider-manager.test.ts` — provider management remains host-side.
- **Host only** `utils/refresh-providers.test.ts` — provider discovery, OAuth, and model metadata remain host-side.
- **Host only** `commands/plugin-commands.test.ts` — installing or executing plugins changes host capability.
- **Host only** `components/dialogs/plugins-selector.test.ts` — plugin control remains host-side.
- **Host only** `controllers/plugin-update-notifier.test.ts` — host plugin update state.
- **Host only** `controllers/session-event-handler-plugin-updates.test.ts` — host plugin update lifecycle.
- **Host only** `commands/skills.test.ts` — skill installation and environment changes remain host-side.
- **Host only** `commands/experiments.test.ts` — experimental host switches.
- **Host only** `components/dialogs/experiments-selector.test.ts` — host experiment selection.
- **Host only** `commands/reload.test.ts` — runtime reload.
- **Host only** `commands/add-dir.test.ts` — expanding host filesystem scope is not a phone action.
- **Host only** `components/messages/mcp-status-panel.test.ts` — MCP state and control remain host-side.
- **Host only** `utils/mcp-oauth.test.ts` — OAuth callback and local server stay on the host.

### TUI implementation and presentation-only tests

- **Host only** `activity-pane.test.ts` — terminal activity pane layout.
- **Host only** `components/panes/activity-pane.test.ts` — terminal pane rendering.
- **Host only** `banner/banner-provider.test.ts` — CLI banner selection and cooldown.
- **Host only** `banner/state.test.ts` — CLI banner cache and version state.
- **Host only** `chalk-named-color-guard.test.ts` — ANSI color guard.
- **Host only** `commands/registry.test.ts` — TUI slash-command registry.
- **Host only** `commands/resolve.test.ts` — TUI slash-command parser.
- **Host only** `commands/update-preferences.test.ts` — TUI preference persistence.
- **Host only** `commands/web.test.ts` — starts or takes over a local Web server.
- **Host only** `components/chrome/banner.test.ts` — terminal banner rendering.
- **Host only** `components/chrome/device-code-box.test.ts` — CLI device-code OAuth.
- **Host only** `components/chrome/footer-status-line.test.ts` — terminal footer line.
- **Host only** `components/chrome/footer.test.ts` — terminal footer.
- **Host only** `components/chrome/gutter-container.test.ts` — terminal gutter.
- **Host only** `components/chrome/moon-loader.test.ts` — ANSI loader.
- **Host only** `components/chrome/welcome.test.ts` — CLI welcome screen.
- **Host only** `components/panels/footer-context.test.ts` — terminal context footer.
- **Host only** `components/panels/help-panel.test.ts` — TUI keybinding help.
- **Host only** `config.test.ts` — local TUI configuration and keybindings.
- **Host only** `constant/tips.test.ts` — terminal tips.
- **Host only** `create-tui-state.test.ts` — TUI object wiring.
- **Host only** `easter-eggs/dance.test.ts` — terminal easter egg.
- **Deferred** `export-markdown.test.ts` — a later explicit, redacted Android share/export flow; never write hidden tool data to shared storage.
- **Host only** `render-memo.bench.ts` — terminal renderer benchmark.
- **Host only** `tui-frame.bench.ts` — terminal frame benchmark.
- **Host only** `utils/event-payload.test.ts` — TUI event helper.
- **Host only** `utils/searchable-list.test.ts` — TUI list implementation.
- **Host only** `utils/session-picker-rows.test.ts` — terminal session-row layout.
- **Host only** `utils/shell-output.test.ts` — ANSI shell-output formatting.
- **Host only** `utils/tab-strip.test.ts` — terminal tabs.
- **Host only** `utils/thinking-config.test.ts` — host thinking configuration.
- **Host only** `working-tips.test.ts` — rotating TUI tips.

## Web tests

- **Host only** `agent-event-projector.test.ts` — Android consumes stable transcript operations and deliberately does not project token deltas.
- **Host only** `ask-user-tool-parse.test.ts` — Android consumes the structured `/questions` API, not legacy tool-text parsing.
- **Next** `attachment-upload.test.ts` — Android picker plus encrypted bounded binary transport, cancellation, and MIME/size limits.
- **Now** `chat-turn-rendering.test.ts` — message ordering, Markdown, long-code page, and consecutive-tool summary; Web-specific media/export shapes do not migrate.
- **Native** `clipboard.test.ts` — Android long-press or explicit native copy; no browser fallback.
- **Next** `composer-draft.test.ts` — persist a draft per session and clear it transactionally after accepted submission.
- **Native** `confirm-dialog.test.ts` — Android confirmation for destructive local actions such as forgetting a runtime.
- **Host only** `daemon-client.test.ts` — Android has one authoritative `RemoteClient`, not the Web REST/WS adapter.
- **Host only** `event-batcher.test.ts` — no requestAnimationFrame or token-delta rendering.
- **Now** `event-reducer.test.ts` — equivalent outcomes are covered through stable transcript/session state, not a copied reducer.
- **Host only** `index-html.test.ts` — Web HTML and CSP.
- **Next** `input-history.test.ts` — migrate through an explicit touch-friendly recent-input UI, not hidden arrow-key behavior.
- **Deferred** `lib-logic.test.ts` — reconsider its individual user outcomes only; never copy the Web debug/export/path utility bundle wholesale.
- **Next** `mention-menu.test.ts` — only after a privacy-preserving, allowlisted remote file-reference API exists.
- **Next** `notification-logic.test.ts` — Android-native reliable background/push notifications; a live foreground socket is insufficient.
- **Next** `open-file-attachment.test.ts` — private temporary storage, content URI, strict MIME allowlist, and explicit user action.
- **Host only** `server-auth.test.ts` — encrypted device pairing replaces bearer fragments and localStorage.
- **Deferred** `side-chat.test.ts` — a later explicit subagent conversation UX; not part of the first usable release.
- **Deferred** `slash-menu.test.ts` — reconsider after the mobile feature set has structured actions; do not expose unsupported host commands.
- **Native** `sound-notification.test.ts` — use Android notification channels after reliable notifications exist.
- **Host only** `storage-logic.test.ts` — browser localStorage compatibility, cross-tab state, and workspace rail state do not migrate.
- **Deferred** `swarm-card-rows.test.ts` — read-only swarm participant/result rows after per-agent isolation.
- **Deferred** `swarm-groups.test.ts` — read-only grouped swarm state after per-agent isolation.
- **Deferred** `swarm-result.test.ts` — consume a structured result rather than parsing Web display markup.
- **Deferred** `task-poller.test.ts` — read-only task results should arrive through stable server projection, not phone polling.
- **Now** `turn-logic.test.ts` — stable user/assistant/notice/tool ordering, resync, and deduplication; no Web-only cron/thinking parser.
- **Host only** `workspace-order.test.ts` — phone does not manage or reorder host workspaces.
- **Now** `workspace-state.test.ts` — selected session, cancel, approvals, questions, reload, and reconnect; workspace CRUD, goal, skill, and Web queue stay out.
- **Now** `ws-lifecycle.test.ts` — reconnect, stale-socket isolation, bridge restart, and `resync_required` through `RemoteClient`.

## First usable Android release boundary

The release includes pairing, encrypted reconnect, existing-session selection,
stable main-agent transcript, prompt/steer/cancel, safe tool summaries, approval,
questions, and long-code navigation. It explicitly excludes remote shell/filesystem
control, credentials, provider/plugin/MCP management, token streaming, and automatic
external Markdown image loading.

Attachments, pagination, drafts/history, notifications, and read-only multi-agent
results are the next product slices. Each requires its own protocol or native
lifecycle design and must add a real Android E2E before shipping.
