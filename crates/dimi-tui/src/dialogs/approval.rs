//! Approval panel + full-screen preview viewer — port of
//! `apps/dimi/src/tui/components/dialogs/approval-panel.ts` and
//! `approval-preview.ts`.
//!
//! This is the UI side of the reverse-rpc approval flow. The reverse-rpc
//! callback wiring (mapping a chosen response back to the SDK) is left to a
//! later slice; here the component surfaces the response via
//! [`ApprovalPanelAction`] which the host polls with `take_action()`.

use crate::code_highlight::{highlight_lines, lang_from_path};
use crate::component::Component;
use crate::diff::{ClusteredDiffOptions, render_diff_lines_clustered};
use crate::keys::{decode_kitty_printable, matches_key};
use crate::theme::{ColorToken, current_theme};
use crate::width::visible_width;
use crate::wrap::{truncate_to_width, wrap_text_with_ansi};

use super::append_wrapped;
use super::input_line::InputLine;

// ===========================================================================
// Data types (reverse-rpc view-layer types)
// ===========================================================================

/// One todo display item.
#[derive(Debug, Clone)]
pub struct TodoDisplayItem {
    pub title: String,
    pub status: String,
}

/// A display block in the approval panel (`DisplayBlock`).
#[derive(Debug, Clone)]
pub enum DisplayBlock {
    Brief {
        text: String,
    },
    Diff {
        path: String,
        old_text: String,
        new_text: String,
        old_start: Option<usize>,
        new_start: Option<usize>,
        is_summary: Option<bool>,
    },
    Shell {
        language: String,
        command: String,
        cwd: Option<String>,
        description: Option<String>,
        danger: Option<String>,
    },
    FileOp {
        operation: String,
        path: String,
        detail: Option<String>,
    },
    FileContent {
        path: String,
        content: String,
        language: Option<String>,
    },
    UrlFetch {
        url: String,
        method: Option<String>,
    },
    Search {
        query: String,
        scope: Option<String>,
    },
    Invocation {
        kind: String,
        name: String,
        description: Option<String>,
    },
    Todo {
        items: Vec<TodoDisplayItem>,
    },
    BackgroundTask {
        task_id: String,
        kind: String,
        status: String,
        description: String,
    },
}

/// One approval choice (`ApprovalPanelChoice`).
#[derive(Debug, Clone)]
pub struct ApprovalChoice {
    pub label: String,
    /// `'approved' | 'approved_for_session' | 'rejected' | 'cancelled'`.
    pub response: String,
    pub selected_label: Option<String>,
    pub requires_feedback: bool,
    pub description: Option<String>,
}

/// `ApprovalPanelData`.
#[derive(Debug, Clone)]
pub struct ApprovalPanelData {
    pub id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub action: String,
    pub description: String,
    pub display: Vec<DisplayBlock>,
    pub choices: Vec<ApprovalChoice>,
}

/// `PendingApproval`.
#[derive(Debug, Clone)]
pub struct PendingApproval {
    pub data: ApprovalPanelData,
}

/// The user's response (`ApprovalPanelResponse.response`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalResponse {
    Approved,
    ApprovedForSession,
    Rejected,
    Cancelled,
}

impl ApprovalResponse {
    fn from_str(s: &str) -> Option<ApprovalResponse> {
        match s {
            "approved" => Some(ApprovalResponse::Approved),
            "approved_for_session" => Some(ApprovalResponse::ApprovedForSession),
            "rejected" => Some(ApprovalResponse::Rejected),
            "cancelled" => Some(ApprovalResponse::Cancelled),
            _ => None,
        }
    }
}

/// `ApprovalPanelResponse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalPanelResponse {
    pub response: ApprovalResponse,
    pub feedback: Option<String>,
    pub selected_label: Option<String>,
}

/// Action surfaced via [`ApprovalPanelComponent::take_action`] (mirrors
/// `onResponse` / `onOpenPreview` / `onToggleToolOutput`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalPanelAction {
    Respond(ApprovalPanelResponse),
    OpenPreview,
    ToggleToolOutput,
}

const DIFF_SUMMARY_MAX_LINES: usize = 10;
const CONTENT_SUMMARY_MAX_LINES: usize = 10;

/// `truncateOneLine` — first line, truncate to `max` chars with an ellipsis.
fn truncate_one_line(text: &str, max: usize) -> String {
    let first_line = text.split('\n').next().unwrap_or("");
    if first_line.chars().count() > max {
        let cut: String = first_line.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        first_line.to_owned()
    }
}

/// `normalizeApprovalText`.
fn normalize_approval_text(text: &str) -> String {
    text.replace("\r\n", "\n").trim().to_owned()
}

/// `isDuplicateBriefBlock`.
fn is_duplicate_brief_block(block: &DisplayBlock, description: &str) -> bool {
    let DisplayBlock::Brief { text } = block else {
        return false;
    };
    if text.trim().is_empty() {
        return false;
    }
    let normalized_description = normalize_approval_text(description);
    if normalized_description.is_empty() {
        return false;
    }
    let normalized_block_text = normalize_approval_text(text);
    if normalized_block_text == normalized_description {
        return true;
    }
    let block_lines: Vec<&str> = normalized_block_text.split('\n').collect();
    if block_lines.len() <= 1 {
        return false;
    }
    normalize_approval_text(&block_lines[1..].join("\n")) == normalized_description
}

/// `headerFor` — the approval panel title for a tool name.
pub fn header_for(tool_name: &str) -> String {
    match tool_name {
        "Bash" => "Run this command?".to_owned(),
        "Write" => "Write this file?".to_owned(),
        "Edit" => "Apply these edits?".to_owned(),
        "TaskStop" => "Stop this task?".to_owned(),
        "ExitPlanMode" => "Ready to build with this plan?".to_owned(),
        other => format!("Approve {other}?"),
    }
}

/// `renderShellDisplayBlock`.
fn render_shell_display_block(block: &DisplayBlock, s: &BlockStyles, width: usize) -> Vec<String> {
    let DisplayBlock::Shell {
        command,
        cwd,
        description,
        danger,
        ..
    } = block
    else {
        return Vec::new();
    };
    let mut lines: Vec<String> = Vec::new();
    if let Some(cwd) = cwd {
        if !cwd.is_empty() {
            lines.push((s.dim)(&format!("cwd: {cwd}")));
        }
    }
    if let Some(danger) = danger {
        lines.push((s.error_bold)(&format!("Dangerous: {danger}")));
    }
    let cmd_lines: Vec<&str> = if command.is_empty() {
        vec![""]
    } else {
        command.split('\n').collect()
    };
    for (idx, cmd_line) in cmd_lines.iter().enumerate() {
        let prefix = if idx == 0 {
            format!("{} ", (s.accent)("$"))
        } else {
            format!("{} ", (s.dim)("·"))
        };
        append_wrapped(
            &mut lines,
            &prefix,
            "  ",
            &(s.strong)(cmd_line),
            width,
            None,
        );
    }
    if let Some(description) = description {
        if !description.is_empty() {
            lines.push(format!("  {}", (s.dim)(description)));
        }
    }
    lines
}

/// `makeBlockStyles` output.
struct BlockStyles {
    strong: fn(&str) -> String,
    dim: fn(&str) -> String,
    accent: fn(&str) -> String,
    gutter: fn(&str) -> String,
    error_bold: fn(&str) -> String,
}

fn make_block_styles() -> BlockStyles {
    BlockStyles {
        strong: |s| current_theme().fg(ColorToken::TextStrong, s),
        dim: |s| current_theme().fg(ColorToken::TextDim, s),
        accent: |s| current_theme().fg(ColorToken::Accent, s),
        gutter: |s| current_theme().fg(ColorToken::DiffGutter, s),
        error_bold: |s| current_theme().bold_fg(ColorToken::Error, s),
    }
}

/// `renderDisplayBlock`.
fn render_display_block(
    block: &DisplayBlock,
    s: &BlockStyles,
    content_width: usize,
) -> Vec<String> {
    match block {
        DisplayBlock::Diff {
            path,
            old_text,
            new_text,
            ..
        } => render_diff_lines_clustered(
            old_text,
            new_text,
            path,
            &ClusteredDiffOptions {
                context_lines: Some(3),
                expand_key_hint: Some("ctrl+e to preview".to_owned()),
                max_lines: Some(DIFF_SUMMARY_MAX_LINES),
                ..Default::default()
            },
        ),
        DisplayBlock::FileContent {
            path,
            content,
            language,
        } => {
            let lang = language.clone().or_else(|| lang_from_path(path));
            let all_lines = highlight_lines(content, lang.as_deref());
            let shown = &all_lines[..all_lines.len().min(CONTENT_SUMMARY_MAX_LINES)];
            let mut lines = vec![(s.strong)(path)];
            for (i, line) in shown.iter().enumerate() {
                lines.push(format!(
                    "{}{}",
                    (s.gutter)(&format!("{:>4}  ", i + 1)),
                    line
                ));
            }
            let remaining = all_lines.len() - shown.len();
            if remaining > 0 {
                let plural = if remaining > 1 { "s" } else { "" };
                lines.push((s.dim)(&format!(
                    "     … {remaining} more line{plural} hidden (ctrl+e to preview)"
                )));
            }
            lines
        }
        DisplayBlock::Shell { .. } => render_shell_display_block(block, s, content_width),
        DisplayBlock::FileOp {
            operation,
            path,
            detail,
        } => {
            let op = (s.accent)(&format!("{operation:<5}"));
            let mut lines = vec![format!("{op} {}", (s.strong)(path))];
            if let Some(detail) = detail {
                if !detail.is_empty() {
                    lines.push((s.dim)(detail));
                }
            }
            lines
        }
        DisplayBlock::UrlFetch { url, method } => {
            let m = method
                .clone()
                .unwrap_or_else(|| "GET".to_owned())
                .to_uppercase();
            let method = (s.accent)(&format!("{m:<5}"));
            vec![format!("{method} {}", (s.strong)(url))]
        }
        DisplayBlock::Search { query, scope } => {
            let mut lines = vec![format!("{} {}", (s.accent)("search"), (s.strong)(query))];
            if let Some(scope) = scope {
                if !scope.is_empty() {
                    lines.push((s.dim)(&format!("scope: {scope}")));
                }
            }
            lines
        }
        DisplayBlock::Invocation {
            kind,
            name,
            description,
        } => {
            let kind = (s.accent)(&format!("{kind:<5}"));
            let mut lines = vec![format!("{kind} {}", (s.strong)(name))];
            if let Some(description) = description {
                if !description.is_empty() {
                    lines.push((s.dim)(&truncate_one_line(description, 200)));
                }
            }
            lines
        }
        DisplayBlock::Brief { text } => {
            if text.is_empty() {
                Vec::new()
            } else {
                text.split('\n')
                    .map(|line| {
                        if line.is_empty() {
                            String::new()
                        } else {
                            (s.strong)(line)
                        }
                    })
                    .collect()
            }
        }
        DisplayBlock::BackgroundTask {
            task_id,
            kind,
            status,
            description,
        } => vec![(s.strong)(&format!(
            "{status} {kind} task {task_id}: {description}"
        ))],
        DisplayBlock::Todo { items } => items
            .iter()
            .map(|item| (s.strong)(&format!("- [{}] {}", item.status, item.title)))
            .collect(),
    }
}

/// `buildNumericHint` — `1/2/3` style numeric shortcut hint.
fn build_numeric_hint(count: usize) -> String {
    if count == 0 {
        return "↵".to_owned();
    }
    (1..=count.min(9))
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("/")
}

// ===========================================================================
// ApprovalPanelComponent
// ===========================================================================

/// `ApprovalPanelComponent`.
pub struct ApprovalPanelComponent {
    request: PendingApproval,
    selected_index: usize,
    feedback_mode: bool,
    feedback_input: InputLine,
    has_toggle_tool_output: bool,
    has_open_preview: bool,
    action: Option<ApprovalPanelAction>,
}

impl ApprovalPanelComponent {
    pub fn new(
        request: PendingApproval,
        has_toggle_tool_output: bool,
        has_open_preview: bool,
    ) -> Self {
        ApprovalPanelComponent {
            request,
            selected_index: 0,
            feedback_mode: false,
            feedback_input: InputLine::new(),
            has_toggle_tool_output,
            has_open_preview,
            action: None,
        }
    }

    /// Host polls after `handle_input`.
    pub fn take_action(&mut self) -> Option<ApprovalPanelAction> {
        self.action.take()
    }

    fn choice_at(&self, index: usize) -> Option<&ApprovalChoice> {
        self.request.data.choices.get(index)
    }

    fn choice_count(&self) -> usize {
        self.request.data.choices.len()
    }

    fn ensure_valid_selection(&mut self) {
        let count = self.choice_count();
        if count == 0 {
            self.selected_index = 0;
            return;
        }
        if self.selected_index >= count {
            self.selected_index = count - 1;
        }
    }

    fn submit(&mut self, index: usize, feedback: Option<String>) {
        let Some(option) = self.choice_at(index) else {
            return;
        };
        let response =
            ApprovalResponse::from_str(&option.response).unwrap_or(ApprovalResponse::Rejected);
        let feedback = feedback.filter(|f| !f.is_empty());
        self.action = Some(ApprovalPanelAction::Respond(ApprovalPanelResponse {
            response,
            feedback,
            selected_label: option.selected_label.clone(),
        }));
    }

    fn select_and_submit(&mut self, index: usize) {
        let Some(option) = self.choice_at(index) else {
            return;
        };
        if option.requires_feedback {
            self.selected_index = index;
            self.feedback_mode = true;
        } else {
            self.submit(index, None);
        }
    }

    fn find_previewable_block(&self) -> bool {
        self.request.data.display.iter().any(|block| {
            matches!(
                block,
                DisplayBlock::Diff { .. } | DisplayBlock::FileContent { .. }
            )
        })
    }

    fn render_inline_feedback_line(&self, width: usize, label_with_num: &str) -> String {
        let theme = current_theme();
        let prefix = format!(
            "{} {}  ",
            theme.bold_fg(ColorToken::Accent, "▶"),
            theme.bold_fg(ColorToken::Accent, label_with_num)
        );
        let input_width = 4usize.max(width.saturating_sub(visible_width(&prefix)) + 2);
        let input_line = self.feedback_input.render(input_width);
        let inline_input = input_line
            .strip_prefix("> ")
            .unwrap_or(&input_line)
            .to_owned();
        format!("{prefix}{inline_input}")
    }
}

impl Component for ApprovalPanelComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let theme = current_theme();
        self.ensure_valid_selection();
        let s = make_block_styles();
        let border_color = |text: &str| theme.fg(ColorToken::BorderFocus, text);
        let border_color_bold = |text: &str| theme.bold_fg(ColorToken::BorderFocus, text);
        let select_color_bold = |text: &str| theme.bold_fg(ColorToken::Accent, text);
        let dim = |text: &str| theme.fg(ColorToken::TextDim, text);
        let strong = |text: &str| theme.fg(ColorToken::TextStrong, text);
        let indent = |text: String| format!("  {text}");

        let data = &self.request.data;
        let title = header_for(&data.tool_name);
        let horizontal_bar = border_color(&"─".repeat(width));
        let mut lines: Vec<String> = vec![
            horizontal_bar.clone(),
            indent(format!(
                "{} {}",
                border_color_bold("▶"),
                border_color_bold(&title)
            )),
        ];

        let deduped_blocks: Vec<&DisplayBlock> = data
            .display
            .iter()
            .filter(|block| !is_duplicate_brief_block(block, &data.description))
            .take(5)
            .collect();
        let has_previewable = deduped_blocks.iter().any(|block| {
            matches!(
                block,
                DisplayBlock::Diff { .. } | DisplayBlock::FileContent { .. }
            )
        });

        if !deduped_blocks.is_empty() {
            lines.push(String::new());
            for block in deduped_blocks {
                let block_lines = render_display_block(block, &s, width.saturating_sub(2).max(1));
                for line in block_lines {
                    lines.push(indent(line));
                }
            }
        } else if !data.description.is_empty() {
            lines.push(String::new());
            for desc_line in data.description.split('\n') {
                lines.push(indent(dim(desc_line)));
            }
        }

        lines.push(String::new());
        for (idx, option) in data.choices.iter().enumerate() {
            let is_selected = idx == self.selected_index;
            let num = idx + 1;
            let label_with_num = format!("{num}. {}", option.label);
            if self.feedback_mode && option.requires_feedback && is_selected {
                lines.push(indent(self.render_inline_feedback_line(
                    width.saturating_sub(2),
                    &label_with_num,
                )));
            } else if is_selected {
                lines.push(indent(format!(
                    "{} {}",
                    select_color_bold("▶"),
                    select_color_bold(&label_with_num)
                )));
            } else {
                lines.push(indent(strong(&format!("  {label_with_num}"))));
            }

            if let Some(description) = &option.description {
                if !(description.is_empty()
                    || (self.feedback_mode && option.requires_feedback && is_selected))
                {
                    for desc_line in
                        wrap_text_with_ansi(description, 20usize.max(width.saturating_sub(7)))
                    {
                        lines.push(indent(format!("     {}", dim(&desc_line))));
                    }
                }
            }
        }

        lines.push(String::new());
        if self.feedback_mode {
            lines.push(indent(dim("Type feedback · ↵ submit.")));
        } else {
            let expand_hint = if has_previewable {
                " · ctrl+e preview"
            } else {
                ""
            };
            lines.push(indent(dim(&format!(
                "↑/↓ select · {} choose · ↵ confirm{expand_hint}",
                build_numeric_hint(data.choices.len())
            ))));
        }
        lines.push(horizontal_bar);

        lines
            .iter()
            .map(|line| truncate_to_width(line, width, "...", false))
            .collect()
    }

    fn handle_input(&mut self, data: &str) {
        if matches_key(data, "escape") || matches_key(data, "ctrl+c") || matches_key(data, "ctrl+d")
        {
            self.action = Some(ApprovalPanelAction::Respond(ApprovalPanelResponse {
                response: ApprovalResponse::Rejected,
                feedback: None,
                selected_label: None,
            }));
            return;
        }

        if matches_key(data, "ctrl+e") {
            if self.find_previewable_block() && self.has_open_preview {
                self.action = Some(ApprovalPanelAction::OpenPreview);
            }
            return;
        }

        if matches_key(data, "ctrl+o") {
            if self.has_toggle_tool_output {
                self.action = Some(ApprovalPanelAction::ToggleToolOutput);
            }
            return;
        }

        if self.feedback_mode {
            let count = self.choice_count();
            if matches_key(data, "up") && count > 0 {
                self.feedback_mode = false;
                self.selected_index = (self.selected_index + count - 1) % count;
                return;
            }
            if matches_key(data, "down") && count > 0 {
                self.feedback_mode = false;
                self.selected_index = (self.selected_index + 1) % count;
                return;
            }
            match self.feedback_input.handle_input(data) {
                super::input_line::InputEvent::Submit => {
                    let value = self.feedback_input.get_value().to_owned();
                    self.submit(self.selected_index, Some(value));
                }
                super::input_line::InputEvent::Escape => {
                    self.feedback_mode = false;
                    self.feedback_input.set_value("");
                }
                super::input_line::InputEvent::None => {}
            }
            return;
        }

        let count = self.choice_count();
        if count == 0 {
            return;
        }
        if matches_key(data, "up") {
            self.selected_index = (self.selected_index + count - 1) % count;
            return;
        }
        if matches_key(data, "down") {
            self.selected_index = (self.selected_index + 1) % count;
            return;
        }
        if matches_key(data, "enter") {
            self.select_and_submit(self.selected_index);
            return;
        }

        let printable = decode_kitty_printable(data).unwrap_or_else(|| data.to_owned());
        if let Ok(n) = printable.parse::<usize>() {
            if n >= 1 && n <= count {
                self.select_and_submit(n - 1);
            }
        }
    }

    fn invalidate(&mut self) {}
}

// ===========================================================================
// ApprovalPreviewViewer
// ===========================================================================

/// Preview block (`DiffDisplayBlock | FileContentDisplayBlock`).
#[derive(Debug, Clone)]
pub enum ApprovalPreviewBlock {
    Diff {
        path: String,
        old_text: String,
        new_text: String,
        old_start: Option<usize>,
        new_start: Option<usize>,
    },
    FileContent {
        path: String,
        content: String,
        language: Option<String>,
    },
}

/// Action surfaced via [`ApprovalPreviewViewer::take_action`] (mirrors
/// `onClose`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalPreviewAction {
    Close,
}

/// `ApprovalPreviewViewer` — full-screen preview of an Edit diff or Write
/// file content. Lines are pre-rendered at construction and only sliced on
/// scroll, so per-frame render cost stays `O(viewport)`.
pub struct ApprovalPreviewViewer {
    block: ApprovalPreviewBlock,
    /// Terminal rows (mirrors `terminal.rows`).
    rows: usize,
    /// Pre-rendered body lines (ANSI-styled, no border / no gutter).
    body_lines: Vec<String>,
    /// Title shown in the header (path + diff stats / "Write" label).
    header_title: String,
    /// Index of the topmost visible line.
    scroll_top: usize,
    action: Option<ApprovalPreviewAction>,
}

impl ApprovalPreviewViewer {
    pub fn new(block: ApprovalPreviewBlock, rows: usize) -> Self {
        let built = build_body(&block);
        ApprovalPreviewViewer {
            block,
            rows,
            body_lines: built.lines,
            header_title: built.title,
            scroll_top: 0,
            action: None,
        }
    }

    /// Host polls after `handle_input`.
    pub fn take_action(&mut self) -> Option<ApprovalPreviewAction> {
        self.action.take()
    }

    fn viewable_rows(&self) -> usize {
        self.rows.saturating_sub(4).max(1)
    }

    fn max_scroll(&self) -> usize {
        self.body_lines.len().saturating_sub(self.viewable_rows())
    }

    fn scroll_by(&mut self, delta: isize) {
        let target = self.scroll_top as isize + delta;
        let max = self.max_scroll() as isize;
        self.scroll_top = target.max(0).min(max) as usize;
    }

    fn render_header(&self, width: usize) -> String {
        let theme = current_theme();
        let title = theme.bold_fg(ColorToken::Primary, " Preview ");
        fit_exactly(&format!("{title}{}", self.header_title), width)
    }

    fn render_body(&mut self, width: usize, body_height: usize) -> Vec<String> {
        let theme = current_theme();
        let inner_width = width.saturating_sub(4).max(1);

        let max = self.max_scroll();
        if self.scroll_top > max {
            self.scroll_top = max;
        }

        let view_rows = body_height.saturating_sub(2);
        let top = theme.fg(
            ColorToken::Primary,
            &format!("┌{}┐", "─".repeat(width.saturating_sub(2))),
        );
        let bottom = theme.fg(
            ColorToken::Primary,
            &format!("└{}┘", "─".repeat(width.saturating_sub(2))),
        );

        let mut out: Vec<String> = vec![top];
        for i in 0..view_rows {
            let line_index = self.scroll_top + i;
            let raw = self.body_lines.get(line_index).cloned().unwrap_or_default();
            out.push(format!(
                "{}{}{}",
                theme.fg(ColorToken::Primary, "│ "),
                fit_exactly(&raw, inner_width),
                theme.fg(ColorToken::Primary, " │")
            ));
        }
        out.push(bottom);
        out
    }

    fn render_footer(&self, width: usize, body_height: usize) -> String {
        let theme = current_theme();
        let key = |text: &str| theme.bold_fg(ColorToken::Primary, text);
        let dim = |text: &str| theme.fg(ColorToken::TextMuted, text);

        let total = self.body_lines.len();
        let view_rows = body_height.saturating_sub(2).max(1);
        let max_scroll = total.saturating_sub(view_rows);
        let percent = if max_scroll == 0 {
            100
        } else {
            ((self.scroll_top as f64 / max_scroll as f64) * 100.0).round() as usize
        };
        let line_from = if total == 0 { 0 } else { self.scroll_top + 1 };
        let line_to = total.min(self.scroll_top + view_rows);

        let position = theme.fg(
            ColorToken::TextMuted,
            &format!(" {line_from}-{line_to} / {total} ({percent}%) "),
        );
        let keys = format!(
            "{} {}  {} {}  {} {}  {} {}",
            key("↑↓"),
            dim("line"),
            key("PgUp/PgDn"),
            dim("page"),
            key("g/G"),
            dim("top/bot"),
            key("Q/Esc/Ctrl+E"),
            dim("cancel"),
        );
        let left = format!(" {keys}");
        let left_w = visible_width(&left);
        let right_w = visible_width(&position);
        if left_w + 2 + right_w <= width {
            return format!("{left}{}{}", " ".repeat(width - left_w - right_w), position);
        }
        fit_exactly(&left, width)
    }
}

impl Component for ApprovalPreviewViewer {
    fn render(&mut self, width: usize) -> Vec<String> {
        let rows = self.rows.max(3);
        let body_height = rows - 2;

        let header = self.render_header(width);
        let body = self.render_body(width, body_height);
        let footer = self.render_footer(width, body_height);

        let mut out = Vec::with_capacity(1 + body.len() + 1);
        out.push(header);
        out.extend(body);
        out.push(footer);
        out
    }

    fn handle_input(&mut self, data: &str) {
        let visible = self.viewable_rows();
        let k = decode_kitty_printable(data);

        if matches_key(data, "escape") || matches_key(data, "ctrl+e") {
            self.action = Some(ApprovalPreviewAction::Close);
            return;
        }
        if k.as_deref() == Some("q") || k.as_deref() == Some("Q") {
            self.action = Some(ApprovalPreviewAction::Close);
            return;
        }
        if matches_key(data, "up") || k.as_deref() == Some("k") {
            self.scroll_by(-1);
            return;
        }
        if matches_key(data, "down") || k.as_deref() == Some("j") {
            self.scroll_by(1);
            return;
        }
        if matches_key(data, "pageup") || k.as_deref() == Some(" ") || data == "\x02" {
            self.scroll_by(-(visible.saturating_sub(1).max(1) as isize));
            return;
        }
        if matches_key(data, "pagedown") || data == "\x06" {
            self.scroll_by(visible.saturating_sub(1).max(1) as isize);
            return;
        }
        if matches_key(data, "home") || k.as_deref() == Some("g") {
            self.scroll_top = 0;
            return;
        }
        if matches_key(data, "end") || k.as_deref() == Some("G") {
            self.scroll_top = self.max_scroll();
        }
    }

    fn invalidate(&mut self) {
        let built = build_body(&self.block);
        self.body_lines = built.lines;
        self.header_title = built.title;
    }
}

/// `fitExactly` — truncate to width if too wide, pad to exactly width.
fn fit_exactly(line: &str, width: usize) -> String {
    let mut s = line.to_owned();
    if visible_width(&s) > width {
        s = truncate_to_width(&s, width, "…", false);
    }
    pad_to_width(&s, width)
}

fn pad_to_width(line: &str, width: usize) -> String {
    let w = visible_width(line);
    if w == width {
        return line.to_owned();
    }
    if w > width {
        return truncate_to_width(line, width, "…", false);
    }
    format!("{line}{}", " ".repeat(width - w))
}

struct BuiltBody {
    lines: Vec<String>,
    title: String,
}

fn build_body(block: &ApprovalPreviewBlock) -> BuiltBody {
    match block {
        ApprovalPreviewBlock::Diff {
            path,
            old_text,
            new_text,
            old_start,
            new_start,
        } => {
            // renderDiffLinesClustered emits a `+N -M path` header on its first
            // line; we pull it out into the viewer chrome so the body is purely
            // scrollable diff content.
            let rendered = render_diff_lines_clustered(
                old_text,
                new_text,
                path,
                &ClusteredDiffOptions {
                    context_lines: Some(3),
                    old_start: Some(old_start.unwrap_or(1)),
                    new_start: Some(new_start.unwrap_or(1)),
                    ..Default::default()
                },
            );
            let mut iter = rendered.into_iter();
            let header = iter.next().unwrap_or_default();
            BuiltBody {
                lines: iter.collect(),
                title: strip_leading_space(&header),
            }
        }
        ApprovalPreviewBlock::FileContent {
            path,
            content,
            language,
        } => {
            let lang = language.clone().or_else(|| lang_from_path(path));
            let highlighted = highlight_lines(content, lang.as_deref());
            let theme = current_theme();
            let lines = highlighted
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    format!(
                        "{}{}",
                        theme.fg(ColorToken::DiffGutter, &format!("{:>4}  ", i + 1)),
                        line
                    )
                })
                .collect();
            let title = theme.fg(ColorToken::TextStrong, path);
            BuiltBody { lines, title }
        }
    }
}

fn strip_leading_space(s: &str) -> String {
    s.trim_start_matches(' ').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(label: &str, response: &str, requires_feedback: bool) -> ApprovalChoice {
        ApprovalChoice {
            label: label.to_owned(),
            response: response.to_owned(),
            selected_label: None,
            requires_feedback,
            description: None,
        }
    }

    fn pending() -> PendingApproval {
        PendingApproval {
            data: ApprovalPanelData {
                id: "approval_1".to_owned(),
                tool_call_id: "tool_1".to_owned(),
                tool_name: "Bash".to_owned(),
                action: "run".to_owned(),
                description: "run a command".to_owned(),
                display: vec![DisplayBlock::Shell {
                    language: "bash".to_owned(),
                    command: "rm -rf /tmp/cache".to_owned(),
                    cwd: Some("/home/user/proj".to_owned()),
                    description: None,
                    danger: Some("recursive delete".to_owned()),
                }],
                choices: vec![
                    choice("Approve once", "approved", false),
                    choice("Reject with feedback", "rejected", true),
                ],
            },
        }
    }

    #[test]
    fn header_for_known_tools() {
        assert_eq!(header_for("Bash"), "Run this command?");
        assert_eq!(header_for("Write"), "Write this file?");
        assert_eq!(header_for("Edit"), "Apply these edits?");
    }

    #[test]
    fn renders_border_title_blocks_choices() {
        let mut c = ApprovalPanelComponent::new(pending(), true, true);
        let lines = c.render(100);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("Run this command?"), "{joined}");
        assert!(joined.contains("Dangerous: recursive delete"), "{joined}");
        assert!(joined.contains("cwd: /home/user/proj"), "{joined}");
        assert!(joined.contains("$ rm -rf /tmp/cache"), "{joined}");
        assert!(joined.contains("1. Approve once"), "{joined}");
        assert!(joined.contains("2. Reject with feedback"), "{joined}");
        assert!(joined.contains("1/2 choose"), "{joined}");
        // has previewable? shell block → no preview hint here.
    }

    #[test]
    fn enter_submits_selected() {
        let mut c = ApprovalPanelComponent::new(pending(), true, true);
        c.handle_input("\r");
        match c.take_action() {
            Some(ApprovalPanelAction::Respond(resp)) => {
                assert_eq!(resp.response, ApprovalResponse::Approved);
            }
            other => panic!("expected respond, got {other:?}"),
        }
    }

    #[test]
    fn down_moves_selection_and_feedback_flow() {
        let mut c = ApprovalPanelComponent::new(pending(), true, true);
        c.handle_input("\x1b[B"); // down to feedback choice
        c.handle_input("\r"); // enter → enters feedback mode
        // Now in feedback mode: type + submit.
        c.handle_input("because it is dangerous");
        c.handle_input("\r");
        match c.take_action() {
            Some(ApprovalPanelAction::Respond(resp)) => {
                assert_eq!(resp.response, ApprovalResponse::Rejected);
                assert_eq!(resp.feedback.as_deref(), Some("because it is dangerous"));
            }
            other => panic!("expected respond, got {other:?}"),
        }
    }

    #[test]
    fn escape_rejects() {
        let mut c = ApprovalPanelComponent::new(pending(), true, true);
        c.handle_input("\x1b");
        match c.take_action() {
            Some(ApprovalPanelAction::Respond(resp)) => {
                assert_eq!(resp.response, ApprovalResponse::Rejected);
            }
            other => panic!("expected reject, got {other:?}"),
        }
    }

    #[test]
    fn numeric_key_selects() {
        let mut c = ApprovalPanelComponent::new(pending(), true, true);
        c.handle_input("2"); // number key
        // choice 2 requires feedback → enters feedback mode.
        c.handle_input("\r"); // enter with empty feedback → submit Rejected, no feedback
        match c.take_action() {
            Some(ApprovalPanelAction::Respond(resp)) => {
                assert_eq!(resp.response, ApprovalResponse::Rejected);
                assert_eq!(resp.feedback, None);
            }
            other => panic!("expected respond, got {other:?}"),
        }
    }

    #[test]
    fn diff_block_renders_counts() {
        let pending = PendingApproval {
            data: ApprovalPanelData {
                id: "a".to_owned(),
                tool_call_id: "t".to_owned(),
                tool_name: "Edit".to_owned(),
                action: "edit".to_owned(),
                description: String::new(),
                display: vec![DisplayBlock::Diff {
                    path: "src/a.ts".to_owned(),
                    old_text: "alpha\nbeta\ngamma".to_owned(),
                    new_text: "alpha\nBETA\ngamma".to_owned(),
                    old_start: None,
                    new_start: None,
                    is_summary: None,
                }],
                choices: vec![choice("Approve", "approved", false)],
            },
        };
        let mut c = ApprovalPanelComponent::new(pending, true, true);
        let lines = c.render(100);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("Apply these edits?"), "{joined}");
        assert!(joined.contains("src/a.ts"), "{joined}");
        assert!(joined.contains("ctrl+e preview"), "{joined}");
    }

    // ── Preview viewer ──

    #[test]
    fn preview_fills_rows() {
        let mut v = ApprovalPreviewViewer::new(
            ApprovalPreviewBlock::Diff {
                path: "src/foo.ts".to_owned(),
                old_text: "alpha\nbeta\ngamma".to_owned(),
                new_text: "alpha\nBETA\ngamma".to_owned(),
                old_start: None,
                new_start: None,
            },
            24,
        );
        let lines = v.render(100);
        assert_eq!(lines.len(), 24);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("src/foo.ts"), "{joined}");
        assert!(joined.contains("beta"), "{joined}");
        assert!(joined.contains("BETA"), "{joined}");
    }

    #[test]
    fn preview_scrolls_and_closes() {
        let content = (1..=60)
            .map(|i| format!("row-{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut v = ApprovalPreviewViewer::new(
            ApprovalPreviewBlock::FileContent {
                path: "src/big.ts".to_owned(),
                content,
                language: None,
            },
            24,
        );
        v.handle_input("\x1b[6~"); // pagedown
        let lines = v.render(100);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("row-"), "{joined}");
        v.handle_input("\x1b");
        assert_eq!(v.take_action(), Some(ApprovalPreviewAction::Close));
    }

    #[test]
    fn duplicate_brief_is_dropped() {
        let brief = DisplayBlock::Brief {
            text: "Update README.md".to_owned(),
        };
        assert!(is_duplicate_brief_block(&brief, "Update README.md"));
        // A brief whose first line is the description is also dropped.
        let multi = DisplayBlock::Brief {
            text: "Title line\nUpdate README.md".to_owned(),
        };
        assert!(is_duplicate_brief_block(&multi, "Update README.md"));
        assert!(!is_duplicate_brief_block(&brief, "something else"));
    }
}
