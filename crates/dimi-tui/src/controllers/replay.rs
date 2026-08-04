//! Session replay — port of `apps/dimi/src/tui/controllers/session-replay.ts`
//! and the pure projection helpers in
//! `apps/dimi/src/tui/utils/message-replay.ts` (plus the small formatters it
//! leans on: `hook-result-format`, `background-task-status`,
//! `background-agent-status`, `shell-output`).
//!
//! The renderer drives the live streaming hooks through the replay records,
//! flushing accumulated assistant/thinking parts into the transcript. The
//! SDK resume-state fetch (`session.getResumeState`) and the component
//! tree are `// TODO(legacy)`.

use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;

use crate::chrome::{TodoItem, TodoStatus};
use crate::controllers::event_handler::UiState;
use crate::controllers::events::PromptOrigin;
use crate::controllers::{
    Args, BackgroundAgentMetadata, BackgroundTaskInfo, BackgroundTaskKind, CompactionData,
    CompactionResultKind, PermissionMode, RenderMode, SkillActivationProjection,
    SkillActivationTrigger, ToolCallBlockData, ToolResultBlockData, TranscriptEntry,
    TranscriptEntryKind,
};
use crate::theme::{ColorToken, current_theme};

/// `REPLAY_TURN_LIMIT`.
pub const REPLAY_TURN_LIMIT: usize = 10;

// ---------------------------------------------------------------------------
// Replay record types (the subset the renderer reads)
// ---------------------------------------------------------------------------

/// `Message['role']`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    Tool,
    System,
}

/// `ContentPart` (the renderer reads text/think and flattens media to text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentPart {
    Text(String),
    Think(String),
    ImageUrl { url: String },
    VideoUrl { url: String },
    AudioUrl { url: String },
}

/// `ToolCall` (replay).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Option<String>,
}

/// `ContextMessage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextMessage {
    pub role: MessageRole,
    pub content: Vec<ContentPart>,
    pub tool_calls: Vec<ReplayToolCall>,
    pub tool_call_id: Option<String>,
    pub origin: Option<PromptOrigin>,
    pub is_error: bool,
}

/// `ApprovalResponse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalResponse {
    pub decision: ApprovalDecision,
    /// `scope` — `'session'` when set.
    pub scope: Option<String>,
    pub feedback: Option<String>,
    pub selected_label: Option<String>,
}

/// `ApprovalResponse['decision']`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approved,
    Rejected,
    Cancelled,
}

/// `PermissionApprovalResultRecord`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalResultRecord {
    pub tool_call_id: String,
    pub tool_name: String,
    pub action: String,
    pub result: ApprovalResponse,
}

/// `CompactionResult` / `'cancelled'`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactionReplayResult {
    Cancelled,
    Completed {
        summary: String,
        tokens_before: u64,
        tokens_after: u64,
    },
}

/// `AgentReplayRecord`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayRecord {
    Message(ContextMessage),
    Compaction {
        result: Option<CompactionReplayResult>,
        instruction: Option<String>,
    },
    PlanUpdated {
        enabled: bool,
    },
    ConfigUpdated,
    PermissionUpdated {
        mode: PermissionMode,
    },
    ApprovalResult(ApprovalResultRecord),
}

/// `AgentConfigData` (subset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConfigData {
    pub model_alias: String,
    pub max_context_tokens: u64,
}

/// `AgentContextData` (subset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentContextData {
    pub token_count: u64,
}

/// `ResumedAgentState` (subset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumedAgentState {
    pub config: AgentConfigData,
    pub context: AgentContextData,
    pub replay: Vec<ReplayRecord>,
    pub plan: Option<String>,
    pub swarm_mode: Option<bool>,
    pub permission_mode: PermissionMode,
    pub tasks: Vec<BackgroundTaskInfo>,
    pub todos: Vec<TodoItem>,
}

// ---------------------------------------------------------------------------
// Replay render context
// ---------------------------------------------------------------------------

/// `ReplayRenderContext`.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayContext {
    pub turn_index: u64,
    pub step_index: u32,
    pub current_turn_id: Option<String>,
    pub assistant_thinking: Vec<String>,
    pub assistant_text: Vec<String>,
    pub tool_calls: BTreeMap<String, ToolCallBlockData>,
    pub completed_tool_call_ids: BTreeSet<String>,
    pub skill_activation_ids: BTreeSet<String>,
    pub plugin_command_activation_ids: BTreeSet<String>,
    pub suppress_next_plan_mode_off_notice: bool,
}

/// `createReplayRenderContext`.
pub fn create_replay_render_context() -> ReplayContext {
    ReplayContext {
        turn_index: 0,
        step_index: 0,
        current_turn_id: None,
        assistant_thinking: Vec::new(),
        assistant_text: Vec::new(),
        tool_calls: BTreeMap::new(),
        completed_tool_call_ids: BTreeSet::new(),
        skill_activation_ids: BTreeSet::new(),
        plugin_command_activation_ids: BTreeSet::new(),
        suppress_next_plan_mode_off_notice: false,
    }
}

// ---------------------------------------------------------------------------
// Pure projection helpers (`message-replay.ts`)
// ---------------------------------------------------------------------------

/// `isUserVisiblePromptOrigin`.
pub fn is_user_visible_prompt_origin(origin: &Option<PromptOrigin>) -> bool {
    match origin {
        None | Some(PromptOrigin::User) => true,
        Some(PromptOrigin::SkillActivation { trigger, .. }) => {
            *trigger == SkillActivationTrigger::UserSlash
        }
        Some(PromptOrigin::PluginCommand { .. }) => true,
        Some(PromptOrigin::ShellCommand { phase, .. }) => {
            *phase == crate::controllers::events::ShellPhase::Input
        }
        Some(PromptOrigin::SystemTrigger { .. }) => false,
        _ => false,
    }
}

/// `isAgentReplayUserTurnRecord`.
pub fn is_agent_replay_user_turn_record(record: &ReplayRecord) -> bool {
    match record {
        ReplayRecord::Message(message) => {
            message.role == MessageRole::User && is_user_visible_prompt_origin(&message.origin)
        }
        _ => false,
    }
}

/// `limitAgentReplayByTurns` — keep only the most recent `maxTurns` user
/// turns.
pub fn limit_replay_records_by_turn(
    records: &[ReplayRecord],
    max_turns: usize,
) -> Vec<ReplayRecord> {
    if max_turns == 0 {
        return Vec::new();
    }
    let starts: Vec<usize> = records
        .iter()
        .enumerate()
        .filter_map(|(i, r)| is_agent_replay_user_turn_record(r).then_some(i))
        .collect();
    if starts.len() <= max_turns {
        return records.to_vec();
    }
    let cut = starts[starts.len() - max_turns];
    records[cut..].to_vec()
}

/// `collectReplayMessageContent` — fold think/text parts into the draft.
pub fn collect_replay_message_content(context: &mut ReplayContext, content: &[ContentPart]) {
    for part in content {
        match part {
            ContentPart::Think(text) => context.assistant_thinking.push(text.clone()),
            ContentPart::Text(text) => context.assistant_text.push(text.clone()),
            ContentPart::ImageUrl { .. }
            | ContentPart::VideoUrl { .. }
            | ContentPart::AudioUrl { .. } => {}
        }
    }
}

/// `contentPartsToText` — join parts to text (media parts flatten to their
/// markdown form).
pub fn content_parts_to_text(content: &[ContentPart]) -> String {
    content.iter().map(content_part_to_text).collect()
}

fn content_part_to_text(part: &ContentPart) -> String {
    match part {
        ContentPart::Text(text) => text.clone(),
        ContentPart::Think(text) => text.clone(),
        ContentPart::ImageUrl { url } => format!("![image]({url})"),
        ContentPart::VideoUrl { url } => format!("![video]({url})"),
        ContentPart::AudioUrl { url } => format!("![audio]({url})"),
    }
}

/// `toolResultOutput`.
pub fn tool_result_output(content: &[ContentPart]) -> String {
    if content.iter().any(|p| !matches!(p, ContentPart::Text(_))) {
        // JSON.stringify(content) — media parts render as their wire shape.
        serde_json::json!(content.iter().map(content_part_to_text).collect::<Vec<_>>()).to_string()
    } else {
        content_parts_to_text(content)
    }
}

/// `parseReplayToolArguments`.
fn parse_replay_tool_arguments(value: Option<&str>) -> Args {
    let Some(value) = value else {
        return Args::new();
    };
    if value.is_empty() {
        return Args::new();
    }
    match serde_json::from_str::<serde_json::Value>(value) {
        Ok(parsed) => match parsed.as_object() {
            Some(map) => map.clone(),
            None => Args::new(),
        },
        Err(_) => Args::new(),
    }
}

/// `toolCallFromReplayMessage`.
pub fn tool_call_from_replay_message(
    raw: &ReplayToolCall,
    context: &ReplayContext,
) -> Option<ToolCallBlockData> {
    let id = &raw.id;
    let name = &raw.name;
    if id.is_empty() || name.is_empty() {
        return None;
    }
    Some(ToolCallBlockData {
        id: id.clone(),
        name: name.clone(),
        args: parse_replay_tool_arguments(raw.arguments.as_deref()),
        description: None,
        streaming_arguments: None,
        streaming_started_at_ms: None,
        result: None,
        step: Some(context.step_index),
        turn_id: context.current_turn_id.clone(),
        truncated: false,
    })
}

/// `skillActivationFromOrigin`.
pub fn skill_activation_from_origin(
    origin: &Option<PromptOrigin>,
) -> Option<SkillActivationProjection> {
    match origin {
        Some(PromptOrigin::SkillActivation {
            activation_id,
            skill_name,
            skill_args,
            trigger,
        }) => Some(SkillActivationProjection {
            activation_id: activation_id.clone(),
            skill_name: skill_name.clone(),
            skill_args: skill_args.clone(),
            trigger: *trigger,
        }),
        _ => None,
    }
}

/// `pluginCommandFromOrigin`.
pub fn plugin_command_from_origin(
    origin: &Option<PromptOrigin>,
) -> Option<crate::controllers::PluginCommandProjection> {
    match origin {
        Some(PromptOrigin::PluginCommand {
            activation_id,
            plugin_id,
            command_name,
            command_args,
        }) => Some(crate::controllers::PluginCommandProjection {
            activation_id: activation_id.clone(),
            plugin_id: plugin_id.clone(),
            command_name: command_name.clone(),
            command_args: command_args.clone(),
        }),
        _ => None,
    }
}

/// `countActiveBackgroundTasks`.
pub fn count_active_background_tasks(tasks: &BTreeMap<String, BackgroundTaskInfo>) -> (u64, u64) {
    let mut bash_tasks = 0u64;
    let mut agent_tasks = 0u64;
    for info in tasks.values() {
        if info.status.is_terminal() {
            continue;
        }
        if info.kind == BackgroundTaskKind::Agent {
            agent_tasks += 1;
        } else {
            bash_tasks += 1;
        }
    }
    (bash_tasks, agent_tasks)
}

/// `replayBackgroundProjection` — non-terminal agent tasks → metadata map.
pub fn replay_background_projection(
    tasks: &[BackgroundTaskInfo],
) -> BTreeMap<String, BackgroundAgentMetadata> {
    let mut out = BTreeMap::new();
    for info in tasks {
        if info.kind != BackgroundTaskKind::Agent {
            continue;
        }
        if info.status.is_terminal() {
            continue;
        }
        let agent_id = info
            .agent_id
            .clone()
            .unwrap_or_else(|| info.task_id.clone());
        out.insert(
            agent_id.clone(),
            BackgroundAgentMetadata {
                agent_id,
                parent_tool_call_id: info.task_id.clone(),
                agent_name: None,
                description: info.description.clone(),
            },
        );
    }
    out
}

// ---------------------------------------------------------------------------
// Formatters the renderer leans on (`hook-result-format.ts`, `shell-output.ts`)
// ---------------------------------------------------------------------------

fn hook_result_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"<hook_result\s+hook_event="([^"]+)">\n?([\s\S]*?)\n?</hook_result>"#)
            .expect("valid hook-result regex")
    })
}

/// `formatHookResultBlock`.
fn format_hook_result_block(event: &str, body: &str, blocked: bool) -> String {
    let suffix = if blocked { " blocked" } else { "" };
    let body = body.trim();
    let body = if body.is_empty() { "(empty)" } else { body };
    format!("*{event} hook{suffix}*\n\n{body}")
}

/// `formatHookResultMessageForTranscript`.
pub fn format_hook_result_message_for_transcript(
    text: &str,
    fallback_event: &str,
    blocked: bool,
) -> String {
    let re = hook_result_re();
    let mut results: Vec<(String, String)> = Vec::new();
    let mut last_index = 0usize;
    for cap in re.captures_iter(text) {
        let m = cap.get(0).expect("whole match");
        if !text[last_index..m.start()].trim().is_empty() {
            return format_hook_result_block(fallback_event, text, blocked);
        }
        let (Some(event), Some(body)) = (cap.get(1), cap.get(2)) else {
            return format_hook_result_block(fallback_event, text, blocked);
        };
        results.push((event.as_str().to_owned(), body.as_str().to_owned()));
        last_index = m.end();
    }
    if results.is_empty() || !text[last_index..].trim().is_empty() {
        return format_hook_result_block(fallback_event, text, blocked);
    }
    results
        .iter()
        .map(|(event, body)| format_hook_result_block(event, body, blocked))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn osc_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\x1b\][\s\S]*?(?:\x07|\x1b\\)").expect("valid OSC regex"))
}

fn csi_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\x1b\[[0-9:;<=>?]*[ -/]*[@-~]").expect("valid CSI regex"))
}

fn esc_single_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\x1b(?:[ -/][0-~]|[0-~])").expect("valid ESC-single regex"))
}

fn c0_control_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new("[\u{0000}-\u{0008}\u{000B}-\u{001B}\u{001C}-\u{001F}]").expect("valid C0 regex")
    })
}

/// `sanitizeShellOutput` — strip every terminal control sequence.
pub fn sanitize_shell_output(text: &str) -> String {
    let text = osc_re().replace_all(text, "").into_owned();
    let text = csi_re().replace_all(&text, "").into_owned();
    let text = esc_single_re().replace_all(&text, "").into_owned();
    c0_control_re().replace_all(&text, "").into_owned()
}

/// `formatBashOutputForDisplay`.
pub fn format_bash_output_for_display(stdout: &str, stderr: &str, is_error: bool) -> String {
    let theme = current_theme();
    let clean_stdout = sanitize_shell_output(stdout).trim_end().to_owned();
    let clean_stderr = sanitize_shell_output(stderr).trim_end().to_owned();
    let mut parts: Vec<String> = Vec::new();
    if !clean_stdout.is_empty() {
        parts.push(theme.fg(ColorToken::TextDim, &clean_stdout));
    }
    if !clean_stderr.is_empty() {
        parts.push(if is_error {
            theme.fg(ColorToken::Error, &clean_stderr)
        } else {
            theme.fg(ColorToken::TextDim, &clean_stderr)
        });
    }
    if parts.is_empty() {
        parts.push(theme.fg(ColorToken::TextDim, "(no output)"));
    }
    parts.join("\n")
}

/// `extractBashTag`.
pub fn extract_bash_tag(text: &str, tag: &str) -> Option<String> {
    let pattern = format!("<{tag}>([\\s\\S]*?)</{tag}>");
    let re = Regex::new(&pattern).ok()?;
    let cap = re.captures(text)?;
    cap.get(1).map(|m| unescape_bash_xml(m.as_str()))
}

/// `unescapeBashXml`.
pub fn unescape_bash_xml(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

/// `extractCronPrompt`.
pub fn extract_cron_prompt(text: &str) -> String {
    let open = "<prompt>\n";
    let close = "\n</prompt>";
    if let Some(start) = text.find(open) {
        let after = start + open.len();
        if let Some(end_rel) = text[after..].find(close) {
            let end = after + end_rel;
            if end >= after {
                return text[after..end].to_owned();
            }
        }
    }
    strip_cron_envelope(text)
}

/// `stripCronEnvelope`.
pub fn strip_cron_envelope(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.len() >= 2
        && lines[0].starts_with("<cron-fire ")
        && lines[lines.len() - 1] == "</cron-fire>"
    {
        return lines[1..lines.len() - 1].join("\n");
    }
    text.to_owned()
}

// ---------------------------------------------------------------------------
// Replay renderer
// ---------------------------------------------------------------------------

/// Outcome of [`SessionReplayRenderer::hydrate_replay`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayOutcome {
    Ok,
    /// `session.getResumeState()?.agents['main']` was missing.
    NoAgentState,
}

/// The replay renderer (port of `SessionReplayRenderer`).
#[derive(Debug, Default)]
pub struct SessionReplayRenderer;

impl SessionReplayRenderer {
    pub fn new() -> Self {
        SessionReplayRenderer
    }

    /// `hydrateFromReplay` — the pure half (the resume-state fetch is legacy).
    pub fn hydrate_replay(
        &self,
        state: &mut UiState,
        agent: Option<&ResumedAgentState>,
    ) -> ReplayOutcome {
        state.app.is_replaying = true;
        let outcome = match agent {
            None => {
                // TODO(legacy): host.showError('Session history is unavailable…')
                ReplayOutcome::NoAgentState
            }
            Some(agent) => {
                self.hydrate_snapshot(state, agent);
                self.render_records(state, agent);
                ReplayOutcome::Ok
            }
        };
        state.app.is_replaying = false;
        // TODO(legacy): host.flushQueuedMessages()
        outcome
    }

    fn hydrate_snapshot(&self, state: &mut UiState, agent: &ResumedAgentState) {
        // appStateFromResumeAgent
        let max_context_tokens = agent.config.max_context_tokens;
        let context_tokens = agent.context.token_count;
        let context_usage = if max_context_tokens > 0 {
            context_tokens as f64 / max_context_tokens as f64
        } else {
            0.0
        };
        state.app.model = agent.config.model_alias.clone();
        state.app.context_tokens = Some(context_tokens);
        state.app.max_context_tokens = Some(max_context_tokens);
        state.app.context_usage = Some(context_usage);
        state.app.plan_mode = agent.plan.is_some();
        state.app.swarm_mode = agent.swarm_mode.unwrap_or(false);
        state.app.permission_mode = agent.permission_mode;

        self.hydrate_todo_panel(state, &agent.todos);
        self.hydrate_background_state(state, &agent.tasks);
    }

    fn hydrate_todo_panel(&self, state: &mut UiState, todos: &[TodoItem]) {
        let items: Vec<TodoItem> = todos
            .iter()
            .filter(|t| !t.title.is_empty())
            .cloned()
            .collect();
        if !items.is_empty() && items.iter().all(|t| t.status == TodoStatus::Done) {
            state
                .streaming
                .set_todo_list(&mut state.effects, Vec::new());
            return;
        }
        state.streaming.set_todo_list(&mut state.effects, items);
    }

    fn hydrate_background_state(&self, state: &mut UiState, tasks: &[BackgroundTaskInfo]) {
        let projection = replay_background_projection(tasks);
        state.background_agent_metadata = projection;
        state.background_tasks.clear();
        for info in tasks {
            state
                .background_tasks
                .insert(info.task_id.clone(), info.clone());
        }
        state.background_tasks_transcripted_terminal.clear();
        for info in tasks {
            if info.status.is_terminal() {
                state
                    .background_tasks_transcripted_terminal
                    .insert(info.task_id.clone());
            }
        }
        let (bash, agent) = count_active_background_tasks(&state.background_tasks);
        state.background_badge = (bash, agent);
    }

    // -----------------------------------------------------------------------
    // Record rendering
    // -----------------------------------------------------------------------

    fn render_records(&self, state: &mut UiState, agent: &ResumedAgentState) {
        let mut context = create_replay_render_context();
        for record in limit_replay_records_by_turn(&agent.replay, REPLAY_TURN_LIMIT) {
            self.render_record(state, &mut context, &record);
            state.sync_streaming_transcript();
        }
        self.cleanup_runtime(state, &mut context);
        state.sync_streaming_transcript();
    }

    fn render_record(
        &self,
        state: &mut UiState,
        context: &mut ReplayContext,
        record: &ReplayRecord,
    ) {
        match record {
            ReplayRecord::Message(message) => self.render_message(state, context, message),
            ReplayRecord::Compaction {
                result,
                instruction,
            } => {
                self.render_compaction(state, context, result.as_ref(), instruction.as_deref());
            }
            ReplayRecord::PlanUpdated { enabled } => {
                self.flush_assistant(state, context);
                if !*enabled && context.suppress_next_plan_mode_off_notice {
                    context.suppress_next_plan_mode_off_notice = false;
                    return;
                }
                context.suppress_next_plan_mode_off_notice = false;
                let content = if *enabled {
                    "Plan mode: ON"
                } else {
                    "Plan mode: OFF"
                };
                let entry = self.replay_entry(
                    state,
                    context,
                    TranscriptEntryKind::Status,
                    content,
                    RenderMode::Notice,
                    &[],
                );
                state.transcript.push(entry);
            }
            ReplayRecord::PermissionUpdated { mode } => {
                self.flush_assistant(state, context);
                self.render_permission_update(state, context, *mode);
            }
            ReplayRecord::ApprovalResult(record) => {
                self.flush_assistant(state, context);
                self.render_approval_result(state, context, record);
            }
            ReplayRecord::ConfigUpdated => {}
        }
    }

    fn render_message(
        &self,
        state: &mut UiState,
        context: &mut ReplayContext,
        message: &ContextMessage,
    ) {
        match message.role {
            MessageRole::User => self.render_user_message(state, context, message),
            MessageRole::Assistant => {
                if message
                    .origin
                    .as_ref()
                    .is_some_and(|o| matches!(o, PromptOrigin::HookResult { .. }))
                {
                    self.render_hook_result(state, context, message);
                    self.render_tool_calls(state, context, &message.tool_calls);
                    return;
                }
                collect_replay_message_content(context, &message.content);
                self.flush_assistant(state, context);
                self.render_tool_calls(state, context, &message.tool_calls);
            }
            MessageRole::Tool => {
                self.flush_assistant(state, context);
                self.render_tool_result(state, context, message);
            }
            MessageRole::System => {}
        }
    }

    fn render_user_message(
        &self,
        state: &mut UiState,
        context: &mut ReplayContext,
        message: &ContextMessage,
    ) {
        match &message.origin {
            Some(PromptOrigin::HookResult { .. }) => {
                self.render_hook_result(state, context, message);
                return;
            }
            Some(PromptOrigin::Injection { .. }) => return,
            Some(PromptOrigin::CompactionSummary) => {
                self.render_compaction_summary(state, context, message);
                return;
            }
            Some(PromptOrigin::Task { task_id }) => {
                self.flush_assistant(state, context);
                let Some(info) = state.background_tasks.get(task_id) else {
                    return;
                };
                if !crate::controllers::event_handler::should_show_background_task_transcript(info)
                {
                    return;
                }
                let status =
                    crate::controllers::event_handler::format_background_task_transcript(info);
                let mut entry = self.replay_entry(
                    state,
                    context,
                    TranscriptEntryKind::Status,
                    &status.headline,
                    RenderMode::Plain,
                    &[],
                );
                entry.detail = status.detail.clone();
                entry.background_agent_status = Some(status);
                state.transcript.push(entry);
                return;
            }
            Some(PromptOrigin::ShellCommand { phase, is_error }) => {
                // A `!` command, replayed from records.
                self.flush_assistant(state, context);
                let text = content_parts_to_text(&message.content);
                if *phase == crate::controllers::events::ShellPhase::Input {
                    let cmd = extract_bash_tag(&text, "bash-input").unwrap_or_else(|| text.clone());
                    let cmd = cmd.trim().to_owned();
                    self.advance_turn(state, context);
                    let theme = current_theme();
                    let rendered = theme.fg(ColorToken::ShellMode, &format!("$ {cmd}"));
                    let entry = self.replay_entry(
                        state,
                        context,
                        TranscriptEntryKind::User,
                        &rendered,
                        RenderMode::Plain,
                        &[("bullet", "")],
                    );
                    state.transcript.push(entry);
                } else {
                    let stdout = extract_bash_tag(&text, "bash-stdout").unwrap_or_default();
                    let stderr = extract_bash_tag(&text, "bash-stderr").unwrap_or_default();
                    let out = format_bash_output_for_display(
                        stdout.trim(),
                        stderr.trim(),
                        *is_error == Some(true),
                    );
                    let entry = self.replay_entry(
                        state,
                        context,
                        TranscriptEntryKind::Status,
                        &out,
                        RenderMode::Plain,
                        &[],
                    );
                    state.transcript.push(entry);
                }
                return;
            }
            Some(PromptOrigin::CronJob { .. }) => {
                self.render_cron_job(state, context, message);
                return;
            }
            Some(PromptOrigin::CronMissed { .. }) => {
                self.render_cron_missed(state, context, message);
                return;
            }
            // System-trigger messages are model-facing only: the live stream
            // never renders them, so replay must not leak them either.
            Some(PromptOrigin::SystemTrigger { .. }) => return,
            Some(PromptOrigin::SkillActivation { .. }) => {
                self.flush_assistant(state, context);
                let skill = skill_activation_from_origin(&message.origin);
                if let Some(skill) = skill {
                    self.render_skill_activation(state, context, &skill);
                    if message.origin.as_ref().is_some_and(|o| {
                        matches!(
                            o,
                            PromptOrigin::SkillActivation {
                                trigger: SkillActivationTrigger::UserSlash,
                                ..
                            }
                        )
                    }) {
                        self.advance_turn(state, context);
                    }
                }
                return;
            }
            Some(PromptOrigin::PluginCommand { .. }) => {
                self.flush_assistant(state, context);
                let command = plugin_command_from_origin(&message.origin);
                if let Some(command) = command {
                    self.render_plugin_command(state, context, &command);
                    if message
                        .origin
                        .as_ref()
                        .is_some_and(|o| matches!(o, PromptOrigin::PluginCommand { .. }))
                    {
                        self.advance_turn(state, context);
                    }
                }
                return;
            }
            // Plain user / no origin / retry.
            _ => {}
        }

        self.flush_assistant(state, context);
        self.advance_turn(state, context);
        let entry = self.replay_entry(
            state,
            context,
            TranscriptEntryKind::User,
            &content_parts_to_text(&message.content),
            RenderMode::Plain,
            &[],
        );
        state.transcript.push(entry);
    }

    fn render_tool_calls(
        &self,
        state: &mut UiState,
        context: &mut ReplayContext,
        tool_calls: &[ReplayToolCall],
    ) {
        if tool_calls.is_empty() {
            return;
        }
        context.step_index += 1;
        self.apply_step_context(state, context);
        for raw in tool_calls {
            let Some(tool_call) = tool_call_from_replay_message(raw, context) else {
                continue;
            };
            context
                .tool_calls
                .insert(tool_call.id.clone(), tool_call.clone());
            state
                .streaming
                .set_active_tool_call(&tool_call.id, tool_call.clone());
            state
                .streaming
                .on_tool_call_start(&mut state.effects, &tool_call);
        }
    }

    fn render_tool_result(
        &self,
        state: &mut UiState,
        context: &mut ReplayContext,
        message: &ContextMessage,
    ) {
        let Some(tool_call_id) = &message.tool_call_id else {
            return;
        };
        if !context.tool_calls.contains_key(tool_call_id) {
            return;
        }
        let result = ToolResultBlockData {
            tool_call_id: tool_call_id.clone(),
            output: tool_result_output(&message.content),
            is_error: message.is_error,
            synthetic: false,
        };
        self.apply_step_context(state, context);
        state
            .streaming
            .on_tool_call_end(&mut state.effects, tool_call_id, &result);
        state.streaming.remove_active_tool_call(tool_call_id);
        context.completed_tool_call_ids.insert(tool_call_id.clone());
    }

    fn advance_turn(&self, state: &mut UiState, context: &mut ReplayContext) {
        context.turn_index += 1;
        context.step_index = 0;
        context.current_turn_id = Some(format!("replay:{}", context.turn_index));
        // A new turn in the history means the previous WaitFor wait ended;
        // freeze its count-up at the actual elapsed duration.
        state.streaming.finalize_active_wait();
        self.apply_step_context(state, context);
    }

    fn apply_step_context(&self, state: &mut UiState, context: &ReplayContext) {
        state.streaming.set_turn_id(context.current_turn_id.clone());
        state.streaming.set_step(context.step_index);
    }

    fn flush_assistant(&self, state: &mut UiState, context: &mut ReplayContext) {
        let thinking = context.assistant_thinking.join("");
        let text = context.assistant_text.join("");
        context.assistant_thinking.clear();
        context.assistant_text.clear();
        self.apply_step_context(state, context);

        if !thinking.is_empty() {
            state
                .streaming
                .on_thinking_update(&mut state.effects, &thinking);
            state.streaming.on_thinking_end(&mut state.effects);
        }
        if !text.is_empty() {
            state.streaming.on_streaming_text_start(&mut state.effects);
            state
                .streaming
                .on_streaming_text_update(&mut state.effects, &text);
            state.streaming.on_streaming_text_end(&mut state.effects);
            state.streaming.clear_assistant_draft();
        }
        state.sync_streaming_transcript();
    }

    fn cleanup_runtime(&self, state: &mut UiState, context: &mut ReplayContext) {
        self.flush_assistant(state, context);
        state
            .streaming
            .cleanup_after_replay(&context.completed_tool_call_ids);
    }

    // -----------------------------------------------------------------------
    // Special content renderers
    // -----------------------------------------------------------------------

    fn render_skill_activation(
        &self,
        state: &mut UiState,
        context: &mut ReplayContext,
        skill: &SkillActivationProjection,
    ) {
        if context.skill_activation_ids.contains(&skill.activation_id) {
            return;
        }
        if state
            .rendered_skill_activation_ids
            .contains(&skill.activation_id)
        {
            return;
        }
        context
            .skill_activation_ids
            .insert(skill.activation_id.clone());
        state
            .rendered_skill_activation_ids
            .insert(skill.activation_id.clone());
        let mut entry = self.replay_entry(
            state,
            context,
            TranscriptEntryKind::SkillActivation,
            &format!("Activated skill: {}", skill.skill_name),
            RenderMode::Plain,
            &[],
        );
        entry.skill_activation_id = Some(skill.activation_id.clone());
        entry.skill_name = Some(skill.skill_name.clone());
        entry.skill_args = skill.skill_args.clone();
        entry.skill_trigger = Some(skill.trigger);
        state.transcript.push(entry);
    }

    fn render_plugin_command(
        &self,
        state: &mut UiState,
        context: &mut ReplayContext,
        command: &crate::controllers::PluginCommandProjection,
    ) {
        if context
            .plugin_command_activation_ids
            .contains(&command.activation_id)
        {
            return;
        }
        if state
            .rendered_plugin_command_activation_ids
            .contains(&command.activation_id)
        {
            return;
        }
        context
            .plugin_command_activation_ids
            .insert(command.activation_id.clone());
        state
            .rendered_plugin_command_activation_ids
            .insert(command.activation_id.clone());
        let mut entry = self.replay_entry(
            state,
            context,
            TranscriptEntryKind::PluginCommand,
            &format!("/{}:{}", command.plugin_id, command.command_name),
            RenderMode::Plain,
            &[],
        );
        entry.plugin_command_data = Some(command.clone());
        state.transcript.push(entry);
    }

    fn render_compaction(
        &self,
        state: &mut UiState,
        context: &mut ReplayContext,
        result: Option<&CompactionReplayResult>,
        instruction: Option<&str>,
    ) {
        self.flush_assistant(state, context);
        let Some(result) = result else {
            return;
        };
        let (content, data) = match result {
            CompactionReplayResult::Cancelled => (
                "Compaction cancelled",
                CompactionData {
                    result: Some(CompactionResultKind::Cancelled),
                    instruction: instruction.map(str::to_owned),
                    ..Default::default()
                },
            ),
            CompactionReplayResult::Completed {
                summary,
                tokens_before,
                tokens_after,
            } => (
                "Compaction complete",
                CompactionData {
                    summary: Some(summary.clone()),
                    tokens_before: Some(*tokens_before),
                    tokens_after: Some(*tokens_after),
                    instruction: instruction.map(str::to_owned),
                    ..Default::default()
                },
            ),
        };
        let mut entry = self.replay_entry(
            state,
            context,
            TranscriptEntryKind::Status,
            content,
            RenderMode::Plain,
            &[],
        );
        entry.compaction_data = Some(data);
        state.transcript.push(entry);
    }

    /// `renderCompactionSummary` — a compaction folded into the model context.
    fn render_compaction_summary(
        &self,
        state: &mut UiState,
        context: &mut ReplayContext,
        message: &ContextMessage,
    ) {
        self.flush_assistant(state, context);
        let mut entry = self.replay_entry(
            state,
            context,
            TranscriptEntryKind::Status,
            "Compaction complete",
            RenderMode::Plain,
            &[],
        );
        entry.compaction_data = Some(CompactionData {
            summary: Some(content_parts_to_text(&message.content)),
            ..Default::default()
        });
        state.transcript.push(entry);
    }

    fn render_hook_result(
        &self,
        state: &mut UiState,
        context: &mut ReplayContext,
        message: &ContextMessage,
    ) {
        let Some(PromptOrigin::HookResult { event, blocked }) = &message.origin else {
            return;
        };
        self.flush_assistant(state, context);
        let entry = self.replay_entry(
            state,
            context,
            TranscriptEntryKind::Assistant,
            &format_hook_result_message_for_transcript(
                &content_parts_to_text(&message.content),
                event,
                *blocked,
            ),
            RenderMode::Markdown,
            &[],
        );
        state.transcript.push(entry);
    }

    fn render_cron_job(
        &self,
        state: &mut UiState,
        context: &mut ReplayContext,
        message: &ContextMessage,
    ) {
        let Some(PromptOrigin::CronJob {
            job_id,
            cron,
            recurring,
            coalesced_count,
            stale,
        }) = &message.origin
        else {
            return;
        };
        self.flush_assistant(state, context);
        let mut entry = self.replay_entry(
            state,
            context,
            TranscriptEntryKind::Cron,
            &extract_cron_prompt(&content_parts_to_text(&message.content)),
            RenderMode::Plain,
            &[],
        );
        entry.cron_data = Some(crate::controllers::CronData {
            job_id: Some(job_id.clone()),
            cron: Some(cron.clone()),
            recurring: Some(*recurring),
            coalesced_count: Some(*coalesced_count),
            stale: Some(*stale),
            missed_count: None,
        });
        state.transcript.push(entry);
    }

    fn render_cron_missed(
        &self,
        state: &mut UiState,
        context: &mut ReplayContext,
        message: &ContextMessage,
    ) {
        let Some(PromptOrigin::CronMissed { count }) = &message.origin else {
            return;
        };
        self.flush_assistant(state, context);
        let mut entry = self.replay_entry(
            state,
            context,
            TranscriptEntryKind::Cron,
            &strip_cron_envelope(&content_parts_to_text(&message.content)),
            RenderMode::Plain,
            &[],
        );
        entry.cron_data = Some(crate::controllers::CronData {
            missed_count: Some(*count),
            ..crate::controllers::CronData::default()
        });
        state.transcript.push(entry);
    }

    fn render_permission_update(
        &self,
        state: &mut UiState,
        context: &mut ReplayContext,
        mode: PermissionMode,
    ) {
        if mode == PermissionMode::Yolo {
            let entry = self.replay_entry(
                state,
                context,
                TranscriptEntryKind::Status,
                "YOLO mode: ON",
                RenderMode::Notice,
                &[(
                    "detail",
                    "Tool actions auto-approved; the agent may still ask you questions.",
                )],
            );
            state.transcript.push(entry);
            return;
        }
        let content = if mode == PermissionMode::Manual {
            "YOLO mode: OFF"
        } else {
            match mode {
                PermissionMode::Auto => "Permission mode: auto",
                PermissionMode::Yolo => "YOLO mode: ON",
                PermissionMode::Manual => "YOLO mode: OFF",
            }
        };
        let entry = self.replay_entry(
            state,
            context,
            TranscriptEntryKind::Status,
            content,
            RenderMode::Notice,
            &[],
        );
        state.transcript.push(entry);
    }

    fn render_approval_result(
        &self,
        state: &mut UiState,
        context: &mut ReplayContext,
        record: &ApprovalResultRecord,
    ) {
        if record.tool_name == "ExitPlanMode" {
            self.render_plan_review_result(state, context, record);
            return;
        }
        let mut parts: Vec<String> = Vec::new();
        match record.result.decision {
            ApprovalDecision::Approved => {
                parts.push(if record.result.scope.as_deref() == Some("session") {
                    "Approved for session".to_owned()
                } else {
                    "Approved".to_owned()
                });
            }
            ApprovalDecision::Rejected => parts.push("Rejected".to_owned()),
            ApprovalDecision::Cancelled => parts.push("Cancelled".to_owned()),
        }
        parts.push(format!(": {}", record.action));
        if let Some(feedback) = &record.result.feedback {
            if !feedback.is_empty() {
                parts.push(format!(" — \"{feedback}\""));
            }
        }
        let entry = self.replay_entry(
            state,
            context,
            TranscriptEntryKind::Status,
            &parts.join(""),
            RenderMode::Notice,
            &[],
        );
        state.transcript.push(entry);
    }

    fn render_plan_review_result(
        &self,
        state: &mut UiState,
        context: &mut ReplayContext,
        record: &ApprovalResultRecord,
    ) {
        if record.result.decision == ApprovalDecision::Approved {
            context.suppress_next_plan_mode_off_notice = true;
            return;
        }
        self.remove_tool_call(state, context, &record.tool_call_id);

        let content = match record.result.decision {
            ApprovalDecision::Rejected => {
                if record.result.selected_label.as_deref() == Some("Revise") {
                    "Plan sent back for revision"
                } else {
                    "Plan review rejected"
                }
            }
            ApprovalDecision::Cancelled => "Plan review cancelled",
            ApprovalDecision::Approved => "Plan approved",
        };
        let detail = record
            .result
            .feedback
            .as_deref()
            .filter(|f| !f.is_empty())
            .map(|f| format!("Feedback: {f}"));
        let entry = self.replay_entry(
            state,
            context,
            TranscriptEntryKind::Status,
            content,
            RenderMode::Notice,
            &[],
        );
        let mut entry = entry;
        entry.detail = detail;
        state.transcript.push(entry);
    }

    fn remove_tool_call(
        &self,
        state: &mut UiState,
        context: &mut ReplayContext,
        tool_call_id: &str,
    ) {
        state.streaming.remove_active_tool_call(tool_call_id);
        state.streaming.remove_tool_component(tool_call_id);
        state.transcript.retain(|entry| {
            entry.tool_call_data.as_ref().map(|t| t.id.as_str()) != Some(tool_call_id)
        });
        context.tool_calls.remove(tool_call_id);
    }

    /// `replayEntry`.
    fn replay_entry(
        &self,
        state: &mut UiState,
        context: &ReplayContext,
        kind: TranscriptEntryKind,
        content: &str,
        render_mode: RenderMode,
        extras: &[(&str, &str)],
    ) -> TranscriptEntry {
        let mut entry = TranscriptEntry::new(
            state.next_entry_id(),
            kind,
            context.current_turn_id.clone(),
            render_mode,
            content,
        );
        for (key, value) in extras {
            match *key {
                "detail" => entry.detail = Some(value.to_string()),
                "bullet" => entry.bullet = Some(value.to_string()),
                _ => {}
            }
        }
        entry
    }
}

/// Build a test-friendly assistant message with text parts.
pub fn text_message(role: MessageRole, text: &str, origin: Option<PromptOrigin>) -> ReplayRecord {
    ReplayRecord::Message(ContextMessage {
        role,
        content: vec![ContentPart::Text(text.to_owned())],
        tool_calls: Vec::new(),
        tool_call_id: None,
        origin,
        is_error: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::BackgroundTaskStatus;
    use crate::controllers::event_handler::SessionEventHandler;

    fn agent(replay: Vec<ReplayRecord>, tasks: Vec<BackgroundTaskInfo>) -> ResumedAgentState {
        ResumedAgentState {
            config: AgentConfigData {
                model_alias: "claude".to_owned(),
                max_context_tokens: 10000,
            },
            context: AgentContextData { token_count: 2000 },
            replay,
            plan: None,
            swarm_mode: None,
            permission_mode: PermissionMode::Auto,
            tasks,
            todos: Vec::new(),
        }
    }

    #[test]
    fn limit_replay_records_keeps_recent_turns() {
        let mut records = Vec::new();
        for i in 0..15 {
            records.push(text_message(
                MessageRole::User,
                &format!("msg {i}"),
                Some(PromptOrigin::User),
            ));
        }
        let limited = limit_replay_records_by_turn(&records, REPLAY_TURN_LIMIT);
        assert_eq!(limited.len(), 10);
        assert_eq!(
            limited[0],
            text_message(MessageRole::User, "msg 5", Some(PromptOrigin::User))
        );
        // More turns than limit but under it → keep all.
        assert_eq!(limit_replay_records_by_turn(&records[..3], 10).len(), 3);
        // maxTurns 0 → empty.
        assert!(limit_replay_records_by_turn(&records, 0).is_empty());
    }

    #[test]
    fn limit_replay_records_ignores_system_trigger_user_messages() {
        let records = vec![
            text_message(
                MessageRole::User,
                "system",
                Some(PromptOrigin::SystemTrigger {
                    name: "wait_timeout".to_owned(),
                }),
            ),
            text_message(MessageRole::User, "real", Some(PromptOrigin::User)),
        ];
        // The system-trigger message is not a user turn, so both stay.
        assert_eq!(limit_replay_records_by_turn(&records, 1).len(), 2);
    }

    #[test]
    fn collect_replay_message_content_folds_parts() {
        let mut context = create_replay_render_context();
        collect_replay_message_content(
            &mut context,
            &[
                ContentPart::Think("hmm".to_owned()),
                ContentPart::Text("hello".to_owned()),
                ContentPart::ImageUrl {
                    url: "x.png".to_owned(),
                },
            ],
        );
        assert_eq!(context.assistant_thinking, vec!["hmm".to_owned()]);
        assert_eq!(context.assistant_text, vec!["hello".to_owned()]);
    }

    #[test]
    fn tool_call_from_replay_message_projects() {
        let context = ReplayContext {
            turn_index: 3,
            step_index: 2,
            current_turn_id: Some("replay:3".to_owned()),
            ..create_replay_render_context()
        };
        let raw = ReplayToolCall {
            id: "t1".to_owned(),
            name: "Read".to_owned(),
            arguments: Some(r#"{"path":"/a"}"#.to_owned()),
        };
        let tc = tool_call_from_replay_message(&raw, &context).unwrap();
        assert_eq!(tc.id, "t1");
        assert_eq!(tc.step, Some(2));
        assert_eq!(tc.turn_id.as_deref(), Some("replay:3"));
        assert_eq!(tc.args.get("path"), Some(&serde_json::json!("/a")));

        // Empty id → None.
        let raw = ReplayToolCall {
            id: String::new(),
            name: "Read".to_owned(),
            arguments: None,
        };
        assert!(tool_call_from_replay_message(&raw, &context).is_none());
    }

    #[test]
    fn content_parts_to_text_and_tool_result_output() {
        assert_eq!(
            content_parts_to_text(&[
                ContentPart::Text("a".to_owned()),
                ContentPart::Think("b".to_owned())
            ]),
            "ab"
        );
        // Media parts make toolResultOutput stringify.
        let out = tool_result_output(&[
            ContentPart::Text("x".to_owned()),
            ContentPart::ImageUrl {
                url: "y.png".to_owned(),
            },
        ]);
        assert!(out.contains("y.png"));
        // All-text → joined text.
        assert_eq!(
            tool_result_output(&[ContentPart::Text("plain".to_owned())]),
            "plain"
        );
    }

    #[test]
    fn replay_projection_helpers() {
        let mut task = BackgroundTaskInfo::new(
            "t1",
            BackgroundTaskKind::Agent,
            BackgroundTaskStatus::Running,
        );
        task.agent_id = Some("a1".to_owned());
        task.description = Some("d".to_owned());
        let projection = replay_background_projection(&[task.clone()]);
        let meta = projection.get("a1").unwrap();
        assert_eq!(meta.parent_tool_call_id, "t1");
        assert_eq!(meta.description.as_deref(), Some("d"));

        // Terminal tasks are excluded from the projection.
        let mut done = BackgroundTaskInfo::new(
            "t2",
            BackgroundTaskKind::Agent,
            BackgroundTaskStatus::Completed,
        );
        done.agent_id = Some("a2".to_owned());
        let projection = replay_background_projection(&[task.clone(), done]);
        assert!(!projection.contains_key("a2"));
    }

    #[test]
    fn bash_output_sanitizes_control_sequences() {
        let clean = sanitize_shell_output("\x1b[31mred\x1b[0m \x07");
        assert_eq!(clean, "red ");
        assert_eq!(sanitize_shell_output("plain\n\ttext"), "plain\n\ttext");
    }

    #[test]
    fn format_bash_output_for_display_colors_stderr_on_error() {
        let out = format_bash_output_for_display("out", "err", true);
        assert!(out.contains("out"));
        assert!(out.contains("err"));
        let out = format_bash_output_for_display("", "", false);
        assert!(out.contains("(no output)"));
    }

    #[test]
    fn hook_result_message_formatting() {
        let text = "<hook_result hook_event=\"pre_tool_use\">\nbody here\n</hook_result>";
        let formatted = format_hook_result_message_for_transcript(text, "fallback", false);
        assert_eq!(formatted, "*pre_tool_use hook*\n\nbody here");

        // Multiple hook results join with blank lines.
        let multi = "<hook_result hook_event=\"a\">one</hook_result><hook_result hook_event=\"b\">two</hook_result>";
        let formatted = format_hook_result_message_for_transcript(multi, "fallback", true);
        assert_eq!(
            formatted,
            "*a hook blocked*\n\none\n\n*b hook blocked*\n\ntwo"
        );

        // Leading garbage → fallback block with the whole text.
        let formatted = format_hook_result_message_for_transcript(
            "garbage <hook_result hook_event=\"a\">x</hook_result>",
            "fallback",
            false,
        );
        assert_eq!(
            formatted,
            "*fallback hook*\n\ngarbage <hook_result hook_event=\"a\">x</hook_result>"
        );
    }

    #[test]
    fn extract_bash_tag_and_unescape() {
        assert_eq!(
            extract_bash_tag("<bash-input>ls &lt;x&gt;</bash-input>", "bash-input"),
            Some("ls <x>".to_owned())
        );
        assert_eq!(extract_bash_tag("no tags", "bash-input"), None);
    }

    #[test]
    fn cron_envelope_helpers() {
        let text = "<cron-fire job_id=\"j\">\ncheck\n</cron-fire>";
        assert_eq!(strip_cron_envelope(text), "check");
        assert_eq!(strip_cron_envelope("plain"), "plain");

        let with_prompt = "<prompt>\ncheck status\n</prompt>";
        assert_eq!(extract_cron_prompt(with_prompt), "check status");
        // No prompt envelope → falls back to stripping the cron-fire envelope.
        let text = "<cron-fire job_id=\"j\">\ncheck\n</cron-fire>";
        assert_eq!(extract_cron_prompt(text), "check");
    }

    #[test]
    fn hydrate_replay_renders_transcript_in_order() {
        let mut h = SessionEventHandler::new();
        let renderer = SessionReplayRenderer::new();
        let records = vec![
            text_message(MessageRole::User, "hello", Some(PromptOrigin::User)),
            ReplayRecord::Message(ContextMessage {
                role: MessageRole::Assistant,
                content: vec![
                    ContentPart::Think("thinking here".to_owned()),
                    ContentPart::Text("the answer".to_owned()),
                ],
                tool_calls: Vec::new(),
                tool_call_id: None,
                origin: None,
                is_error: false,
            }),
            text_message(MessageRole::User, "again", Some(PromptOrigin::User)),
        ];
        let outcome = renderer.hydrate_replay(&mut h.state, Some(&agent(records, vec![])));
        assert_eq!(outcome, ReplayOutcome::Ok);
        assert!(!h.state.app.is_replaying);

        for e in &h.state.transcript {
            eprintln!(
                "DBG {:?}: {:?} bullet={:?} turn={:?}",
                e.kind, e.content, e.bullet, e.turn_id
            );
        }
        let kinds: Vec<TranscriptEntryKind> = h.state.transcript.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TranscriptEntryKind::User,
                TranscriptEntryKind::Assistant,
                TranscriptEntryKind::User,
            ]
        );
        assert_eq!(h.state.transcript[0].content, "hello");
        assert_eq!(h.state.transcript[1].content, "the answer");
        assert!(h.state.transcript[1].model_text);
        assert_eq!(h.state.transcript[1].turn_id.as_deref(), Some("replay:1"));
        assert_eq!(h.state.transcript[2].turn_id.as_deref(), Some("replay:2"));
    }

    #[test]
    fn hydrate_replay_missing_agent_reports_no_agent_state() {
        let mut h = SessionEventHandler::new();
        let renderer = SessionReplayRenderer::new();
        assert_eq!(
            renderer.hydrate_replay(&mut h.state, None),
            ReplayOutcome::NoAgentState
        );
        assert!(!h.state.app.is_replaying);
    }

    #[test]
    fn hydrate_replay_snapshot_patches_app_state() {
        let mut h = SessionEventHandler::new();
        let renderer = SessionReplayRenderer::new();
        let a = agent(vec![], vec![]);
        renderer.hydrate_replay(&mut h.state, Some(&a));
        assert_eq!(h.state.app.model, "claude");
        assert_eq!(h.state.app.context_tokens, Some(2000));
        assert_eq!(h.state.app.max_context_tokens, Some(10000));
        assert_eq!(h.state.app.context_usage, Some(0.2));
        assert!(!h.state.app.plan_mode);
    }

    #[test]
    fn hydrate_replay_renders_compaction_and_permission() {
        let mut h = SessionEventHandler::new();
        let renderer = SessionReplayRenderer::new();
        let records = vec![
            ReplayRecord::Compaction {
                result: Some(CompactionReplayResult::Completed {
                    summary: "earlier".to_owned(),
                    tokens_before: 100,
                    tokens_after: 20,
                }),
                instruction: Some("summarize".to_owned()),
            },
            ReplayRecord::PermissionUpdated {
                mode: PermissionMode::Yolo,
            },
        ];
        renderer.hydrate_replay(&mut h.state, Some(&agent(records, vec![])));
        let kinds: Vec<TranscriptEntryKind> = h.state.transcript.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![TranscriptEntryKind::Status, TranscriptEntryKind::Status]
        );
        assert_eq!(h.state.transcript[0].content, "Compaction complete");
        assert_eq!(
            h.state.transcript[0]
                .compaction_data
                .as_ref()
                .unwrap()
                .summary
                .as_deref(),
            Some("earlier")
        );
        assert_eq!(h.state.transcript[1].content, "YOLO mode: ON");
    }

    #[test]
    fn hydrate_replay_plan_updated_and_suppression() {
        let mut h = SessionEventHandler::new();
        let renderer = SessionReplayRenderer::new();
        // Plan ON then OFF; the OFF after an approved plan review is
        // suppressed when suppressNextPlanModeOffNotice is set.
        let records = vec![
            ReplayRecord::PlanUpdated { enabled: true },
            ReplayRecord::PlanUpdated { enabled: false },
        ];
        renderer.hydrate_replay(&mut h.state, Some(&agent(records, vec![])));
        assert_eq!(h.state.transcript.len(), 2); // ON + OFF both rendered

        let mut h2 = SessionEventHandler::new();
        let records = vec![
            ReplayRecord::PlanUpdated { enabled: true },
            ReplayRecord::ApprovalResult(ApprovalResultRecord {
                tool_call_id: "plan".to_owned(),
                tool_name: "ExitPlanMode".to_owned(),
                action: "approve plan".to_owned(),
                result: ApprovalResponse {
                    decision: ApprovalDecision::Approved,
                    scope: None,
                    feedback: None,
                    selected_label: None,
                },
            }),
            ReplayRecord::PlanUpdated { enabled: false },
        ];
        renderer.hydrate_replay(&mut h2.state, Some(&agent(records, vec![])));
        // Plan ON rendered; the OFF is suppressed after the approved review.
        let kinds: Vec<&str> = h2
            .state
            .transcript
            .iter()
            .map(|e| e.content.as_str())
            .collect();
        assert_eq!(kinds, vec!["Plan mode: ON"]);
    }

    #[test]
    fn hydrate_replay_renders_tool_calls_and_results() {
        let mut h = SessionEventHandler::new();
        let renderer = SessionReplayRenderer::new();
        let records = vec![
            ReplayRecord::Message(ContextMessage {
                role: MessageRole::Assistant,
                content: Vec::new(),
                tool_calls: vec![ReplayToolCall {
                    id: "t1".to_owned(),
                    name: "Read".to_owned(),
                    arguments: Some(r#"{"path":"/a"}"#.to_owned()),
                }],
                tool_call_id: None,
                origin: None,
                is_error: false,
            }),
            ReplayRecord::Message(ContextMessage {
                role: MessageRole::Tool,
                content: vec![ContentPart::Text("file contents".to_owned())],
                tool_calls: Vec::new(),
                tool_call_id: Some("t1".to_owned()),
                origin: None,
                is_error: false,
            }),
        ];
        renderer.hydrate_replay(&mut h.state, Some(&agent(records, vec![])));
        // The tool call card state is tracked but the transcript only holds
        // the assistant entry (empty text → no assistant entry).
        // cleanup_after_replay clears completed tool calls.
        assert!(!h.state.streaming.has_active_tool_call("t1"));
        assert!(!h.state.streaming.has_tool_component("t1"));
    }

    #[test]
    fn hydrate_replay_shell_command_renders_dollar_line() {
        let mut h = SessionEventHandler::new();
        let renderer = SessionReplayRenderer::new();
        let records = vec![ReplayRecord::Message(ContextMessage {
            role: MessageRole::User,
            content: vec![ContentPart::Text("<bash-input>ls</bash-input>".to_owned())],
            tool_calls: Vec::new(),
            tool_call_id: None,
            origin: Some(PromptOrigin::ShellCommand {
                phase: crate::controllers::events::ShellPhase::Input,
                is_error: None,
            }),
            is_error: false,
        })];
        renderer.hydrate_replay(&mut h.state, Some(&agent(records, vec![])));
        assert_eq!(h.state.transcript.len(), 1);
        assert!(h.state.transcript[0].content.contains("$ ls"));
        assert_eq!(h.state.transcript[0].bullet.as_deref(), Some(""));
        assert_eq!(h.state.transcript[0].kind, TranscriptEntryKind::User);
    }

    #[test]
    fn hydrate_replay_background_task_origin_renders_status() {
        let mut h = SessionEventHandler::new();
        let renderer = SessionReplayRenderer::new();
        let mut task = BackgroundTaskInfo::new(
            "t1",
            BackgroundTaskKind::Process,
            BackgroundTaskStatus::Failed,
        );
        task.detached = Some(true);
        let records = vec![ReplayRecord::Message(ContextMessage {
            role: MessageRole::User,
            content: vec![ContentPart::Text("".to_owned())],
            tool_calls: Vec::new(),
            tool_call_id: None,
            origin: Some(PromptOrigin::Task {
                task_id: "t1".to_owned(),
            }),
            is_error: false,
        })];
        renderer.hydrate_replay(&mut h.state, Some(&agent(records, vec![task])));
        assert_eq!(h.state.transcript.len(), 1);
        assert_eq!(h.state.transcript[0].kind, TranscriptEntryKind::Status);
        assert!(
            h.state.transcript[0]
                .content
                .contains("bash task failed in background")
        );
    }
}
