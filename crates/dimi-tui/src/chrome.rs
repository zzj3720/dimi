//! Chrome and pane components — welcome, banner, todo panel, queue pane,
//! and the choice picker (port of
//! `apps/dimi/src/tui/components/chrome/{welcome,banner,todo-panel}.ts`,
//! `panes/queue-pane.ts`, and `dialogs/choice-picker.ts`).

use crate::component::Component;
use crate::fuzzy::fuzzy_filter;
use crate::keys::matches_key;
use crate::paging::{PageView, page_view};
use crate::theme::{ColorToken, current_theme};
use crate::width::visible_width;
use crate::wrap::truncate_to_width;

/// `❯` — selected pointer (`SELECT_POINTER`).
const SELECT_POINTER: &str = "❯";
/// `← current` — currently-active marker (`CURRENT_MARK`).
const CURRENT_MARK: &str = "← current";

// ===========================================================================
// WelcomeComponent
// ===========================================================================

/// App state consumed by the welcome panel.
#[derive(Debug, Clone)]
pub struct WelcomeState {
    pub model: String,
    pub work_dir: String,
    pub session_id: String,
    pub version: String,
    /// Model alias → display name map.
    pub available_models: std::collections::HashMap<String, String>,
    pub mcp_servers_summary: Option<String>,
}

impl WelcomeState {
    pub fn new() -> Self {
        WelcomeState {
            model: String::new(),
            work_dir: String::new(),
            session_id: String::new(),
            version: String::new(),
            available_models: std::collections::HashMap::new(),
            mcp_servers_summary: None,
        }
    }
}

impl Default for WelcomeState {
    fn default() -> Self {
        Self::new()
    }
}

/// Effective model display name (mirrors `effectiveModelAlias` usage).
fn effective_model_name(state: &WelcomeState) -> String {
    if state.model.is_empty() {
        return "not set, run /login".to_owned();
    }
    state
        .available_models
        .get(&state.model)
        .cloned()
        .unwrap_or_else(|| state.model.clone())
}

/// Welcome panel with the Dimi logo box.
pub struct WelcomeComponent {
    pub state: WelcomeState,
}

impl WelcomeComponent {
    pub fn new(state: WelcomeState) -> Self {
        WelcomeComponent { state }
    }
}

impl Component for WelcomeComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let safe_width = width;
        let theme = current_theme();
        let is_logged_out = self.state.model.is_empty();
        let model_value = if is_logged_out {
            theme.fg(ColorToken::Warning, "not set, run /login")
        } else {
            effective_model_name(&self.state)
        };

        if safe_width < 24 {
            let title = theme.bold_hex(ColorToken::Primary, "Welcome to Dimi!");
            let prompt = if is_logged_out {
                theme.fg(ColorToken::Warning, "Run /login to connect a provider.")
            } else {
                theme.fg(ColorToken::TextDim, "Send /help for help information.")
            };
            let model = format!("Model: {model_value}");
            return ["", &title, &prompt, &model]
                .iter()
                .map(|line| truncate_to_width(line, safe_width, "…", false))
                .collect();
        }

        let inner_width = (safe_width.saturating_sub(4)).max(1);
        let logo = ["▐█▛█▛█▌", "▐█████▌"];
        let logo_width = logo.iter().map(|r| visible_width(r)).max().unwrap_or(0);
        let gap = "  ";
        let text_width = (inner_width.saturating_sub(logo_width + gap.len())).max(4);

        let right_row0 = truncate_to_width(
            &theme.bold_hex(ColorToken::Primary, "Welcome to Dimi!"),
            text_width,
            "…",
            false,
        );
        let right_row1 = truncate_to_width(
            &theme.fg(
                ColorToken::TextDim,
                if is_logged_out {
                    "Run /login to connect a provider."
                } else {
                    "Send /help for help information."
                },
            ),
            text_width,
            "…",
            false,
        );

        let label_style = |text: &str| theme.bold_hex(ColorToken::TextDim, text);
        let info_lines = vec![
            format!("{}{}", label_style("Directory: "), self.state.work_dir),
            format!("{}{}", label_style("Session:   "), self.state.session_id),
            format!("{}{}", label_style("Model:     "), model_value),
            format!("{}{}", label_style("Version:   "), self.state.version),
        ];
        let mut content_lines: Vec<String> = vec![
            format!(
                "{}{}{}",
                theme.fg(
                    ColorToken::Primary,
                    &logo[0].to_owned().pad_end_placeholder(logo_width)
                ),
                gap,
                right_row0
            ),
            format!(
                "{}{}{}",
                theme.fg(
                    ColorToken::Primary,
                    &logo[1].to_owned().pad_end_placeholder(logo_width)
                ),
                gap,
                right_row1
            ),
            String::new(),
        ];
        content_lines.extend(info_lines);
        if let Some(mcp) = &self.state.mcp_servers_summary {
            content_lines.push(format!("{}{}", label_style("MCP:       "), mcp));
        }

        let mut lines: Vec<String> = vec![
            String::new(),
            theme.fg(
                ColorToken::Primary,
                &format!("╭{}╮", "─".repeat(safe_width.saturating_sub(2))),
            ),
            format!(
                "{}{}{}",
                theme.fg(ColorToken::Primary, "│"),
                " ".repeat(safe_width.saturating_sub(2)),
                theme.fg(ColorToken::Primary, "│")
            ),
        ];

        for content in content_lines {
            let truncated = truncate_to_width(&content, inner_width, "…", false);
            let vis = visible_width(&truncated);
            let right_pad = inner_width.saturating_sub(vis);
            lines.push(format!(
                "{}{}{}{}{}",
                theme.fg(ColorToken::Primary, "│"),
                "  ",
                truncated,
                " ".repeat(right_pad),
                theme.fg(ColorToken::Primary, "│")
            ));
        }

        lines.push(format!(
            "{}{}{}",
            theme.fg(ColorToken::Primary, "│"),
            " ".repeat(safe_width.saturating_sub(2)),
            theme.fg(ColorToken::Primary, "│")
        ));
        lines.push(theme.fg(
            ColorToken::Primary,
            &format!("╰{}╯", "─".repeat(safe_width.saturating_sub(2))),
        ));
        lines.push(String::new());

        lines
            .iter()
            .map(|line| truncate_to_width(line, safe_width, "…", false))
            .collect()
    }

    fn invalidate(&mut self) {}
}

/// Small helper for padEnd on a string by visible width (logo rows are ASCII).
trait PadEndPlaceholder {
    fn pad_end_placeholder(&self, width: usize) -> String;
}
impl PadEndPlaceholder for String {
    fn pad_end_placeholder(&self, width: usize) -> String {
        let current = visible_width(self);
        if current >= width {
            self.clone()
        } else {
            format!("{self}{}", " ".repeat(width - current))
        }
    }
}

// ===========================================================================
// BannerComponent
// ===========================================================================

/// Banner state.
#[derive(Debug, Clone)]
pub struct BannerState {
    pub tag: Option<String>,
    pub main_text: String,
    pub sub_text: Option<String>,
}

/// A transient banner shown below the welcome panel.
pub struct BannerComponent {
    pub state: BannerState,
}

impl BannerComponent {
    pub fn new(state: BannerState) -> Self {
        BannerComponent { state }
    }
}

impl Component for BannerComponent {
    fn render(&mut self, _width: usize) -> Vec<String> {
        let theme = current_theme();
        let mut lines = Vec::new();
        let mut head = String::new();
        if let Some(tag) = &self.state.tag {
            head.push_str(&theme.bold_fg(ColorToken::Primary, &format!("✦ {tag}")));
            head.push(' ');
        }
        head.push_str(&theme.bold_fg(ColorToken::TextStrong, &self.state.main_text));
        lines.push(head);
        if let Some(sub) = &self.state.sub_text {
            lines.push(format!("  {}", theme.fg(ColorToken::TextDim, sub)));
        }
        lines.push(String::new());
        lines
    }

    fn invalidate(&mut self) {}
}

// ===========================================================================
// TodoPanelComponent
// ===========================================================================

/// Todo status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
}

impl TodoStatus {
    fn label(&self) -> &'static str {
        match self {
            TodoStatus::Done => "done",
            TodoStatus::InProgress => "in progress",
            TodoStatus::Pending => "pending",
        }
    }
}

/// One todo item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoItem {
    pub title: String,
    pub status: TodoStatus,
}

impl TodoItem {
    pub fn new(title: &str, status: TodoStatus) -> Self {
        TodoItem {
            title: title.to_owned(),
            status,
        }
    }
}

const MAX_VISIBLE: usize = 5;

/// Visible todo rows + hidden counts (mirrors `selectVisibleTodos`).
#[derive(Debug, Clone)]
pub struct VisibleTodos {
    pub rows: Vec<TodoItem>,
    pub hidden: usize,
    pub hidden_counts: [usize; 3], // [done, in_progress, pending]
}

/// Pick which todos to render when the list exceeds MAX_VISIBLE.
pub fn select_visible_todos(todos: &[TodoItem]) -> VisibleTodos {
    if todos.len() <= MAX_VISIBLE {
        return VisibleTodos {
            rows: todos.to_vec(),
            hidden: 0,
            hidden_counts: [0, 0, 0],
        };
    }
    let mut in_progress: Vec<usize> = Vec::new();
    let mut pending: Vec<usize> = Vec::new();
    let mut done: Vec<usize> = Vec::new();
    for (i, todo) in todos.iter().enumerate() {
        match todo.status {
            TodoStatus::InProgress => in_progress.push(i),
            TodoStatus::Pending => pending.push(i),
            TodoStatus::Done => done.push(i),
        }
    }

    let mut picked: Vec<usize> = Vec::new();
    for i in in_progress.iter().take(MAX_VISIBLE) {
        picked.push(*i);
    }

    if picked.len() < MAX_VISIBLE {
        let done_candidates: Vec<usize> = done.iter().rev().copied().collect();
        let pending_candidates = pending.clone();

        let remaining = MAX_VISIBLE - picked.len();
        let (done_count, pending_count) = if done_candidates.is_empty() {
            (0, pending_candidates.len().min(remaining))
        } else if pending_candidates.is_empty() {
            (done_candidates.len().min(remaining), 0)
        } else {
            let mut dc = 1;
            let pc = (remaining - 1).min(pending_candidates.len());
            if pc < remaining - 1 {
                dc = done_candidates.len().min(remaining - pc);
            }
            (dc, pc)
        };
        for i in done_candidates.iter().take(done_count) {
            picked.push(*i);
        }
        for i in pending_candidates.iter().take(pending_count) {
            picked.push(*i);
        }
    }

    picked.sort_unstable();
    picked.dedup();

    let mut hidden_counts = [0usize; 3];
    for (i, todo) in todos.iter().enumerate() {
        if !picked.contains(&i) {
            let idx = match todo.status {
                TodoStatus::Done => 0,
                TodoStatus::InProgress => 1,
                TodoStatus::Pending => 2,
            };
            hidden_counts[idx] += 1;
        }
    }

    VisibleTodos {
        rows: picked.iter().map(|i| todos[*i].clone()).collect(),
        hidden: todos.len() - picked.len(),
        hidden_counts,
    }
}

/// Format hidden counts ("1 done · 1 pending").
pub fn format_hidden_counts(counts: [usize; 3]) -> String {
    let mut parts = Vec::new();
    let statuses = [
        TodoStatus::Done,
        TodoStatus::InProgress,
        TodoStatus::Pending,
    ];
    for status in statuses {
        let idx = match status {
            TodoStatus::Done => 0,
            TodoStatus::InProgress => 1,
            TodoStatus::Pending => 2,
        };
        if counts[idx] > 0 {
            parts.push(format!("{} {}", counts[idx], status.label()));
        }
    }
    parts.join(" · ")
}

/// Live-updating TODO list shown before the input area.
pub struct TodoPanelComponent {
    todos: Vec<TodoItem>,
    expanded: bool,
}

impl TodoPanelComponent {
    pub fn new() -> Self {
        TodoPanelComponent {
            todos: Vec::new(),
            expanded: false,
        }
    }

    pub fn set_todos(&mut self, todos: Vec<TodoItem>) {
        self.todos = todos;
    }

    pub fn get_todos(&self) -> &[TodoItem] {
        &self.todos
    }

    pub fn clear(&mut self) {
        self.todos.clear();
        self.expanded = false;
    }

    pub fn is_empty(&self) -> bool {
        self.todos.is_empty()
    }

    pub fn has_overflow(&self) -> bool {
        self.todos.len() > MAX_VISIBLE
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }

    pub fn toggle_expanded(&mut self) {
        self.expanded = !self.expanded;
    }
}

impl Default for TodoPanelComponent {
    fn default() -> Self {
        Self::new()
    }
}

fn render_todo_row(todo: &TodoItem) -> String {
    let theme = current_theme();
    let marker = match todo.status {
        TodoStatus::InProgress => theme.bold_fg(ColorToken::Primary, "●"),
        TodoStatus::Done => theme.fg(ColorToken::Success, "✓"),
        TodoStatus::Pending => theme.fg(ColorToken::TextDim, "○"),
    };
    let title = match todo.status {
        TodoStatus::InProgress => theme.bold_fg(ColorToken::Text, &todo.title),
        TodoStatus::Done => theme.strikethrough_fg(ColorToken::TextDim, &todo.title),
        TodoStatus::Pending => theme.fg(ColorToken::Text, &todo.title),
    };
    format!("  {marker} {title}")
}

impl Component for TodoPanelComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        if self.todos.is_empty() {
            return Vec::new();
        }
        let theme = current_theme();
        let mut lines: Vec<String> = vec![
            theme.fg(ColorToken::Border, &"─".repeat(width)),
            theme.bold_fg(ColorToken::Primary, "  Todo"),
        ];

        if self.expanded {
            for todo in &self.todos {
                lines.push(render_todo_row(todo));
            }
            if self.todos.len() > MAX_VISIBLE {
                lines.push(theme.fg(
                    ColorToken::TextDim,
                    &format!("  all {} items · ctrl+t to collapse", self.todos.len()),
                ));
            }
        } else {
            let visible = select_visible_todos(&self.todos);
            for todo in &visible.rows {
                lines.push(render_todo_row(todo));
            }
            if visible.hidden > 0 {
                let distribution = format_hidden_counts(visible.hidden_counts);
                let suffix = if distribution.is_empty() {
                    String::new()
                } else {
                    format!(" ({distribution})")
                };
                lines.push(theme.fg(
                    ColorToken::TextDim,
                    &format!("  … +{} more{suffix} · ctrl+t to expand", visible.hidden),
                ));
            }
        }

        lines
            .iter()
            .map(|line| truncate_to_width(line, width, "…", false))
            .collect()
    }

    fn invalidate(&mut self) {}
}

// ===========================================================================
// QueuePaneComponent
// ===========================================================================

/// One queued message.
#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub text: String,
    pub mode: Option<&'static str>, // "bash" | "prompt"
}

impl QueuedMessage {
    pub fn new(text: &str, mode: Option<&'static str>) -> Self {
        QueuedMessage {
            text: text.to_owned(),
            mode,
        }
    }
}

/// Queue pane options.
#[derive(Debug, Clone)]
pub struct QueuePaneOptions {
    pub messages: Vec<QueuedMessage>,
    pub is_compacting: bool,
    pub is_streaming: bool,
    pub can_steer_immediately: bool,
    pub enter_steers_by_default: bool,
}

/// The queue pane listing queued messages before the editor.
pub struct QueuePaneComponent {
    messages: Vec<QueuedMessage>,
    hint: Option<String>,
}

impl QueuePaneComponent {
    pub fn new(options: QueuePaneOptions) -> Self {
        let hint = if !options.messages.is_empty() {
            let has_steerable = options.messages.iter().any(|m| m.mode != Some("bash"));
            let can_steer = options.can_steer_immediately && has_steerable;
            let steer_hint = if options.enter_steers_by_default {
                "  ↑ to edit · enter steers · ctrl-s flushes queue"
            } else {
                "  ↑ to edit · ctrl-s to steer immediately"
            };
            if options.is_compacting && !options.is_streaming {
                Some("  ↑ to edit · will send after compaction".to_owned())
            } else if can_steer {
                Some(steer_hint.to_owned())
            } else {
                Some("  ↑ to edit · will send after current task".to_owned())
            }
        } else {
            None
        };
        QueuePaneComponent {
            messages: options.messages,
            hint,
        }
    }
}

impl Component for QueuePaneComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let theme = current_theme();
        let mut lines: Vec<String> = vec![theme.fg(ColorToken::Border, &"─".repeat(width))];

        for item in &self.messages {
            let single_line: String = item.text.split_whitespace().collect::<Vec<_>>().join(" ");
            let prefix = format!("  {SELECT_POINTER} ");
            if item.mode == Some("bash") {
                let prompt = "$ ";
                let available =
                    (width.saturating_sub(visible_width(&prefix) + visible_width(prompt))).max(1);
                let truncated = truncate_to_width(&single_line, available, "…", false);
                lines.push(format!(
                    "{}{}",
                    theme.fg(ColorToken::Accent, &prefix),
                    theme.fg(ColorToken::ShellMode, &format!("{prompt}{truncated}"))
                ));
            } else {
                let available = (width.saturating_sub(visible_width(&prefix))).max(1);
                let truncated = truncate_to_width(&single_line, available, "…", false);
                lines.push(theme.fg(ColorToken::Accent, &format!("{prefix}{truncated}")));
            }
        }

        if let Some(hint) = &self.hint {
            lines.push(theme.fg(
                ColorToken::TextDim,
                &truncate_to_width(hint, width, "…", false),
            ));
        }

        lines
    }

    fn invalidate(&mut self) {}
}

// ===========================================================================
// ChoicePickerComponent
// ===========================================================================

/// One choice option.
#[derive(Debug, Clone)]
pub struct ChoiceOption {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
    pub tone: Option<ColorToken>,
    /// Color token applied to the description while this option is selected;
    /// falls back to `TextMuted` when unset or not selected.
    pub description_tone: Option<ColorToken>,
}

/// Choice picker options.
#[derive(Debug, Clone)]
pub struct ChoicePickerOptions {
    pub title: String,
    pub options: Vec<ChoiceOption>,
    pub current_value: Option<String>,
    pub hint: Option<String>,
    /// Optional formatter for hint lines (`formatHint` in TS).
    pub format_hint: Option<fn(&str) -> String>,
    pub notice: Option<String>,
    pub notice_tone: Option<ColorToken>,
    /// When true, typed characters fuzzy-filter the list and a search line
    /// is shown.
    pub searchable: bool,
    /// Items per page; defaults to 8.
    pub page_size: Option<usize>,
    /// Mirrors `onSessionOnlySelect !== undefined` in TS: when true the
    /// picker advertises and handles Alt+S (session-only select).
    pub has_session_only: bool,
}

/// Action a host should react to after `handle_input` (ported from the TS
/// `onSelect` / `onCancel` / `onSessionOnlySelect` callbacks — the Rust host
/// polls [`ChoicePickerComponent::take_action`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerAction {
    Select,
    Cancel,
    SessionOnly,
}

const DEFAULT_PAGE_SIZE: usize = 8;

/// Wrap a description into lines no wider than `width` (port of the TS
/// `wrapDescription` helper in choice-picker.ts).
fn wrap_description(text: &str, width: usize) -> Vec<String> {
    let max_width = width.max(1);
    let words: Vec<&str> = text
        .split_whitespace()
        .filter(|word| !word.is_empty())
        .collect();
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in words {
        let candidate = if current.is_empty() {
            word.to_owned()
        } else {
            format!("{current} {word}")
        };
        if visible_width(&candidate) <= max_width {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            lines.push(current);
        }
        current = if visible_width(word) <= max_width {
            word.to_owned()
        } else {
            truncate_to_width(word, max_width, "…", false)
        };
    }

    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Searchable list state machine for the picker (port of the TS
/// `SearchableList` — cursor clamping, paging, and query filtering).
struct ChoiceList {
    items: Vec<ChoiceOption>,
    query: String,
    cursor: usize,
    page_size: usize,
    searchable: bool,
}

impl ChoiceList {
    fn new(options: &ChoicePickerOptions) -> Self {
        let cursor = options
            .options
            .iter()
            .position(|o| Some(&o.value) == options.current_value.as_ref())
            .unwrap_or(0);
        ChoiceList {
            items: options.options.clone(),
            query: String::new(),
            cursor,
            page_size: options.page_size.unwrap_or(DEFAULT_PAGE_SIZE),
            searchable: options.searchable,
        }
    }

    fn filtered(&self) -> Vec<ChoiceOption> {
        if self.query.is_empty() {
            self.items.clone()
        } else {
            fuzzy_filter(self.items.clone(), &self.query, |o| {
                format!("{} {}", o.label, o.description.as_deref().unwrap_or(""))
            })
        }
    }

    fn view(&self) -> ChoiceListView {
        let items = self.filtered();
        let total = items.len();
        let selected = if total == 0 {
            0
        } else {
            self.cursor.min(total - 1)
        };
        let page = page_view(total, selected, self.page_size);
        ChoiceListView {
            items,
            page,
            selected_index: selected,
            query: self.query.clone(),
        }
    }

    fn selected(&self) -> Option<ChoiceOption> {
        let items = self.filtered();
        if items.is_empty() {
            return None;
        }
        items.get(self.cursor.min(items.len() - 1)).cloned()
    }

    fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_down(&mut self) {
        let max = self.filtered().len().saturating_sub(1);
        self.cursor = self.cursor.min(max).saturating_add(1).min(max);
    }

    fn page_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(self.page_size);
    }

    fn page_down(&mut self) {
        let max = self.filtered().len().saturating_sub(1);
        self.cursor = self.cursor.saturating_add(self.page_size).min(max);
    }

    /// Clears the active query and resets the cursor. Returns whether a
    /// query was cleared.
    fn clear_query(&mut self) -> bool {
        if self.query.is_empty() {
            return false;
        }
        self.query.clear();
        self.cursor = 0;
        true
    }

    fn handle_key(&mut self, data: &str) -> bool {
        if matches_key(data, "up") {
            self.move_up();
            return true;
        }
        if matches_key(data, "down") {
            self.move_down();
            return true;
        }
        if matches_key(data, "pageup") || matches_key(data, "pageUp") {
            self.page_up();
            return true;
        }
        if matches_key(data, "pagedown") || matches_key(data, "pageDown") {
            self.page_down();
            return true;
        }
        if !self.searchable {
            return false;
        }
        if matches_key(data, "backspace") {
            if !self.query.is_empty() {
                self.query.pop();
                self.cursor = 0;
            }
            return true;
        }
        if let Some(ch) = crate::keys::decode_printable_key(data) {
            self.query.push_str(&ch);
            self.cursor = 0;
            return true;
        }
        false
    }
}

#[derive(Debug, Clone)]
struct ChoiceListView {
    items: Vec<ChoiceOption>,
    page: PageView,
    selected_index: usize,
    query: String,
}

/// Modal single-select list (port of `ChoicePickerComponent`).
pub struct ChoicePickerComponent {
    opts: ChoicePickerOptions,
    list: ChoiceList,
    action: Option<PickerAction>,
}

impl ChoicePickerComponent {
    pub fn new(opts: ChoicePickerOptions) -> Self {
        let list = ChoiceList::new(&opts);
        ChoicePickerComponent {
            opts,
            list,
            action: None,
        }
    }

    pub fn set_searchable(&mut self, searchable: bool) {
        self.opts.searchable = searchable;
    }

    /// The host polls this after `handle_input` to learn what to do
    /// (mirrors the TS `onSelect` / `onCancel` / `onSessionOnlySelect`).
    pub fn take_action(&mut self) -> Option<PickerAction> {
        self.action.take()
    }

    /// The currently selected option's value.
    pub fn get_selected_value(&self) -> Option<String> {
        self.list.selected().map(|o| o.value)
    }

    fn option_label_style(option: &ChoiceOption, selected: bool) -> String {
        let theme = current_theme();
        if option.tone == Some(ColorToken::Error) {
            return if selected {
                theme.bold_fg(ColorToken::Error, &option.label)
            } else {
                theme.fg(ColorToken::Error, &option.label)
            };
        }
        if selected {
            theme.bold_fg(ColorToken::Primary, &option.label)
        } else {
            theme.fg(ColorToken::Text, &option.label)
        }
    }
}

impl Component for ChoicePickerComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let theme = current_theme();
        let searchable = self.opts.searchable;
        let view = self.list.view();
        let options = view.items;

        let mut nav_parts = vec!["↑↓ navigate".to_owned()];
        if view.page.page_count > 1 {
            nav_parts.push("←→ page".to_owned());
        }
        nav_parts.push("Enter select".to_owned());
        if self.opts.has_session_only {
            nav_parts.push("Alt+S session-only".to_owned());
        }
        nav_parts.push("Esc cancel".to_owned());
        let hint = self
            .opts
            .hint
            .clone()
            .unwrap_or_else(|| nav_parts.join(" · "));

        let title_suffix = if searchable && view.query.is_empty() {
            theme.fg(ColorToken::TextMuted, "  (type to search)")
        } else {
            String::new()
        };
        let hint_lines: Vec<String> = hint
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).to_owned())
            .collect();
        let mut lines: Vec<String> = vec![
            theme.fg(ColorToken::Primary, &"─".repeat(width)),
            format!(
                "{}{}",
                theme.bold_fg(ColorToken::Primary, &format!(" {}", self.opts.title)),
                title_suffix
            ),
        ];
        for hint_line in hint_lines {
            lines.push(match self.opts.format_hint {
                Some(fmt) => fmt(&format!(" {hint_line}")),
                None => theme.fg(ColorToken::TextMuted, &format!(" {hint_line}")),
            });
        }
        if let Some(notice) = &self.opts.notice {
            let tone = self.opts.notice_tone.unwrap_or(ColorToken::Success);
            let notice_width = (width.saturating_sub(1)).max(1);
            for notice_line in notice.split('\n') {
                for wrapped in wrap_description(notice_line, notice_width) {
                    lines.push(theme.fg(tone, &format!(" {wrapped}")));
                }
            }
        }
        lines.push(String::new());
        if searchable && !view.query.is_empty() {
            lines.push(format!(
                "{}{}",
                theme.fg(ColorToken::Primary, " Search: "),
                theme.fg(ColorToken::Text, &view.query)
            ));
        }

        if options.is_empty() {
            lines.push(theme.fg(ColorToken::TextMuted, "   No matches"));
        }
        for i in view.page.start..view.page.end {
            let Some(opt) = options.get(i) else {
                continue;
            };
            let is_selected = i == view.selected_index;
            let is_current = Some(&opt.value) == self.opts.current_value.as_ref();
            let pointer = if is_selected { SELECT_POINTER } else { " " };
            let mut line = theme.fg(
                if is_selected {
                    ColorToken::Primary
                } else {
                    ColorToken::TextDim
                },
                &format!("  {pointer} "),
            );
            line.push_str(&Self::option_label_style(opt, is_selected));
            if is_current {
                line.push_str(&format!(" {}", theme.fg(ColorToken::Success, CURRENT_MARK)));
            }
            lines.push(line);
            if let Some(desc) = &opt.description {
                if !desc.is_empty() {
                    let description_width = (width.saturating_sub(4)).max(1);
                    let description_color = if is_selected {
                        opt.description_tone.unwrap_or(ColorToken::TextMuted)
                    } else {
                        ColorToken::TextMuted
                    };
                    for desc_line in wrap_description(desc, description_width) {
                        lines.push(theme.fg(description_color, &format!("    {desc_line}")));
                    }
                }
            }
        }

        lines.push(String::new());
        if view.page.page_count > 1 {
            lines.push(theme.fg(
                ColorToken::TextMuted,
                &format!(" Page {}/{}", view.page.page + 1, view.page.page_count),
            ));
        }
        lines.push(theme.fg(ColorToken::Primary, &"─".repeat(width)));
        lines
            .iter()
            .map(|line| truncate_to_width(line, width, "…", false))
            .collect()
    }

    fn handle_input(&mut self, data: &str) {
        if matches_key(data, "escape") {
            if self.list.clear_query() {
                return;
            }
            self.action = Some(PickerAction::Cancel);
            return;
        }
        if matches_key(data, "alt+s") && self.opts.has_session_only {
            if let Some(chosen) = self.list.selected() {
                self.action = Some(PickerAction::SessionOnly);
                let _ = chosen;
            }
            return;
        }
        // Left/Right page through the list (this picker has no horizontal
        // control).
        if matches_key(data, "left") {
            self.list.page_up();
            return;
        }
        if matches_key(data, "right") {
            self.list.page_down();
            return;
        }
        // Enter always selects. Space selects too — but only when the list is
        // not searchable; in a searchable list a space must reach the query.
        let is_space = matches_key(data, "space")
            || crate::keys::decode_printable_key(data).as_deref() == Some(" ");
        if matches_key(data, "enter") || (is_space && !self.opts.searchable) {
            self.action = Some(PickerAction::Select);
            return;
        }
        self.list.handle_key(data);
    }

    fn invalidate(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK_COLORS, set_palette};
    use serde::Deserialize;
    use std::fs;

    #[derive(Deserialize)]
    struct Fixture {
        name: String,
        #[serde(default)]
        width: usize,
        #[serde(default)]
        lines: Vec<String>,
    }

    fn golden_path() -> String {
        format!(
            "{}/testdata/chrome-golden.jsonl",
            env!("CARGO_MANIFEST_DIR")
        )
    }

    fn build(name: &str) -> Box<dyn Component> {
        match name {
            "welcome_full" => {
                let mut s = WelcomeState::new();
                s.model = "claude-3-5-sonnet".to_owned();
                s.work_dir = "/home/user/proj".to_owned();
                s.session_id = "session_abc".to_owned();
                s.version = "1.0.0".to_owned();
                s.available_models
                    .insert("claude-3-5-sonnet".to_owned(), "Sonnet".to_owned());
                Box::new(WelcomeComponent::new(s))
            }
            "welcome_narrow" => {
                let mut s = WelcomeState::new();
                s.model = "claude-3-5-sonnet".to_owned();
                s.work_dir = "/home/user/proj".to_owned();
                s.session_id = "session_abc".to_owned();
                s.version = "1.0.0".to_owned();
                s.available_models
                    .insert("claude-3-5-sonnet".to_owned(), "Sonnet".to_owned());
                Box::new(WelcomeComponent::new(s))
            }
            "welcome_logged_out" => {
                let mut s = WelcomeState::new();
                s.work_dir = "/home/user/proj".to_owned();
                s.session_id = "session_abc".to_owned();
                s.version = "1.0.0".to_owned();
                Box::new(WelcomeComponent::new(s))
            }
            "banner_full" => Box::new(BannerComponent::new(BannerState {
                tag: Some("tip".to_owned()),
                main_text: "Use /help to see commands".to_owned(),
                sub_text: Some("More details here".to_owned()),
            })),
            "todo_small" => {
                let mut c = TodoPanelComponent::new();
                c.set_todos(vec![
                    TodoItem::new("First task", TodoStatus::InProgress),
                    TodoItem::new("Second task", TodoStatus::Done),
                    TodoItem::new("Third task", TodoStatus::Pending),
                ]);
                Box::new(c)
            }
            "todo_collapsed" => {
                let mut c = TodoPanelComponent::new();
                c.set_todos(todo_seven());
                Box::new(c)
            }
            "todo_expanded" => {
                let mut c = TodoPanelComponent::new();
                c.set_todos(todo_seven());
                c.set_expanded(true);
                Box::new(c)
            }
            "queue_two" => Box::new(QueuePaneComponent::new(QueuePaneOptions {
                messages: vec![
                    QueuedMessage::new("first queued message", None),
                    QueuedMessage::new("!ls -la", Some("bash")),
                ],
                is_compacting: false,
                is_streaming: true,
                can_steer_immediately: false,
                enter_steers_by_default: false,
            })),
            "queue_empty" => Box::new(QueuePaneComponent::new(QueuePaneOptions {
                messages: Vec::new(),
                is_compacting: false,
                is_streaming: false,
                can_steer_immediately: true,
                enter_steers_by_default: false,
            })),
            "choice_picker" => Box::new(ChoicePickerComponent::new(ChoicePickerOptions {
                title: "Choose an option".to_owned(),
                options: vec![
                    ChoiceOption {
                        value: "a".to_owned(),
                        label: "Alpha".to_owned(),
                        description: Some("first choice".to_owned()),
                        tone: None,
                        description_tone: None,
                    },
                    ChoiceOption {
                        value: "b".to_owned(),
                        label: "Beta".to_owned(),
                        description: None,
                        tone: None,
                        description_tone: None,
                    },
                ],
                current_value: None,
                hint: None,
                format_hint: None,
                notice: None,
                notice_tone: None,
                searchable: false,
                page_size: None,
                has_session_only: false,
            })),
            other => panic!("unknown fixture {other}"),
        }
    }

    fn todo_seven() -> Vec<TodoItem> {
        vec![
            TodoItem::new("Task 1", TodoStatus::Pending),
            TodoItem::new("Task 2", TodoStatus::InProgress),
            TodoItem::new("Task 3", TodoStatus::Done),
            TodoItem::new("Task 4", TodoStatus::Pending),
            TodoItem::new("Task 5", TodoStatus::Pending),
            TodoItem::new("Task 6", TodoStatus::Done),
            TodoItem::new("Task 7", TodoStatus::Pending),
        ]
    }

    #[test]
    fn chrome_golden_byte_exact() {
        set_palette(DARK_COLORS);
        let data = fs::read_to_string(golden_path()).expect("golden file");
        let mut passed = 0usize;
        for line in data.lines() {
            let fixture: Fixture = serde_json::from_str(line).expect("fixture json");
            let mut component = build(&fixture.name);
            let rendered = component.render(fixture.width);
            assert_eq!(
                rendered, fixture.lines,
                "fixture {} (width {})",
                fixture.name, fixture.width
            );
            passed += 1;
        }
        eprintln!("chrome golden passed: {passed} fixtures");
    }

    #[test]
    fn select_visible_todos_prefers_in_progress() {
        let todos = todo_seven();
        let visible = select_visible_todos(&todos);
        assert_eq!(visible.rows.len(), 5);
        assert_eq!(visible.hidden, 2);
        // in_progress always included.
        assert!(
            visible
                .rows
                .iter()
                .any(|t| t.status == TodoStatus::InProgress)
        );
    }
}
