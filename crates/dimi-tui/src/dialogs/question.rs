//! Structured question dialog — port of
//! `apps/dimi/src/tui/components/dialogs/question-dialog.ts`
//! (`QuestionDialogComponent`).
//!
//! Each question collects an answer locally (single-select, multi-select with
//! ✓ checkboxes, or a free-text "Other" row), and a final Submit tab reviews
//! everything before the answers are emitted upstream. The reverse-rpc wiring
//! is left to a later slice; the component surfaces answers / cancel via
//! [`QuestionAction`] which the host polls with `take_action()`.

use std::collections::HashSet;

use crate::component::Component;
use crate::keys::{decode_kitty_printable, matches_key};
use crate::theme::{ColorToken, current_theme};
use crate::width::visible_width;
use crate::wrap::truncate_to_width;

use super::append_wrapped;
use super::input_line::{InputEvent, InputLine};

const NUMBER_KEYS: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
const MAX_BODY_LINES: usize = 12;
const DEFAULT_OTHER_LABEL: &str = "Other";
const NOT_ANSWERED_LABEL: &str = "Not answered";
const REVIEW_TITLE: &str = "Review your answer before submit";
const SUBMIT_PROMPT: &str = "Ready to submit your answers?";
const UNANSWERED_WARNING: &str = "Some questions are still unanswered.";
const SUBMIT_ACTIONS: [&str; 2] = ["Submit", "Cancel"];

/// One preset option.
#[derive(Debug, Clone)]
pub struct QuestionOption {
    pub label: String,
    pub description: Option<String>,
}

/// One question (`QuestionPanelItem`).
#[derive(Debug, Clone)]
pub struct QuestionPanelItem {
    pub question: String,
    pub header: Option<String>,
    pub body: Option<String>,
    pub multi_select: bool,
    pub other_label: Option<String>,
    pub other_description: Option<String>,
    pub options: Vec<QuestionOption>,
}

/// `QuestionPanelData`.
#[derive(Debug, Clone)]
pub struct QuestionPanelData {
    pub id: String,
    pub tool_call_id: String,
    pub questions: Vec<QuestionPanelItem>,
}

/// `PendingQuestion`.
#[derive(Debug, Clone)]
pub struct PendingQuestion {
    pub data: QuestionPanelData,
}

/// Action surfaced via [`QuestionDialogComponent::take_action`] (mirrors
/// `onAnswer` / `onToggleToolOutput`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestionAction {
    /// Non-empty answer set for submit (`answers` in question order).
    Answer {
        answers: Vec<String>,
        method: Option<String>,
    },
    /// User cancelled (esc / ctrl+c / ctrl+d / "Cancel" submit action) —
    /// mirrors `onAnswer({ answers: [] })`.
    Cancel,
    ToggleToolOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OptionKind {
    Preset,
    Other,
}

#[derive(Debug, Clone)]
struct DisplayOption {
    label: String,
    description: Option<String>,
    kind: OptionKind,
}

/// `QuestionDialogComponent`.
pub struct QuestionDialogComponent {
    request: PendingQuestion,
    max_visible_options: usize,
    has_toggle_tool_output: bool,
    current_tab: usize,
    submit_action_idx: usize,
    editing_other: bool,
    review_message: Option<String>,
    last_answer_method: Option<String>,
    cursors: Vec<usize>,
    single_selections: Vec<Option<usize>>,
    multi_selections: Vec<HashSet<usize>>,
    other_drafts: Vec<String>,
    committed_other_values: Vec<Option<String>>,
    answers: Vec<Option<String>>,
    other_input: InputLine,
    action: Option<QuestionAction>,
}

impl QuestionDialogComponent {
    pub fn new(
        request: PendingQuestion,
        max_visible_options: Option<usize>,
        has_toggle_tool_output: bool,
    ) -> Self {
        let total = request.data.questions.len();
        QuestionDialogComponent {
            request,
            max_visible_options: max_visible_options.unwrap_or(6),
            has_toggle_tool_output,
            current_tab: 0,
            submit_action_idx: 0,
            editing_other: false,
            review_message: None,
            last_answer_method: None,
            cursors: vec![0; total],
            single_selections: vec![None; total],
            multi_selections: (0..total).map(|_| HashSet::new()).collect(),
            other_drafts: vec![String::new(); total],
            committed_other_values: vec![None; total],
            answers: vec![None; total],
            other_input: InputLine::new(),
            action: None,
        }
    }

    /// Host polls after `handle_input`.
    pub fn take_action(&mut self) -> Option<QuestionAction> {
        self.action.take()
    }

    // ── Tabs / navigation ─────────────────────────────────────────────

    fn total_tabs(&self) -> usize {
        self.request.data.questions.len() + 1
    }

    fn submit_tab_index(&self) -> usize {
        self.request.data.questions.len()
    }

    fn is_submit_tab(&self) -> bool {
        self.current_tab == self.submit_tab_index()
    }

    fn is_editing_other(&self) -> bool {
        self.editing_other && !self.is_submit_tab()
    }

    fn current_question_index(&self) -> Option<usize> {
        if self.is_submit_tab() {
            None
        } else {
            Some(self.current_tab)
        }
    }

    fn current_cursor(&self) -> usize {
        match self.current_question_index() {
            Some(idx) => self.cursors.get(idx).copied().unwrap_or(0),
            None => 0,
        }
    }

    fn display_options(&self, question_idx: usize) -> Vec<DisplayOption> {
        let Some(question) = self.request.data.questions.get(question_idx) else {
            return Vec::new();
        };
        let mut out: Vec<DisplayOption> = question
            .options
            .iter()
            .map(|option| DisplayOption {
                label: option.label.clone(),
                description: option.description.clone(),
                kind: OptionKind::Preset,
            })
            .collect();
        let other_label = question
            .other_label
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_OTHER_LABEL);
        let other_description = question
            .other_description
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        out.push(DisplayOption {
            label: other_label.to_owned(),
            description: other_description,
            kind: OptionKind::Other,
        });
        out
    }

    fn other_option_index(&self, question_idx: usize) -> usize {
        self.request
            .data
            .questions
            .get(question_idx)
            .map(|q| q.options.len())
            .unwrap_or(0)
    }

    fn is_other_option(&self, question_idx: usize, option_idx: usize) -> bool {
        option_idx == self.other_option_index(question_idx)
    }

    fn goto_tab(&mut self, target: usize) {
        let total = self.total_tabs();
        if total == 0 {
            return;
        }
        let wrapped = ((target % total) + total) % total;
        if wrapped == self.current_tab {
            return;
        }
        self.current_tab = wrapped;
        self.editing_other = false;
        self.review_message = None;
        if self.is_submit_tab() {
            self.submit_action_idx = 0;
        }
    }

    fn move_question_cursor(&mut self, delta: isize) {
        let Some(question_idx) = self.current_question_index() else {
            return;
        };
        let total = self.display_options(question_idx).len();
        if total == 0 {
            return;
        }
        let cursor = self.current_cursor() as isize;
        let next = ((cursor + delta) % total as isize + total as isize) % total as isize;
        if let Some(slot) = self.cursors.get_mut(question_idx) {
            *slot = next as usize;
        }
        self.review_message = None;
    }

    // ── Answer mutation ───────────────────────────────────────────────

    fn activate_question_option(&mut self, option_idx: usize, method: &str) {
        let Some(question_idx) = self.current_question_index() else {
            return;
        };
        let Some(question) = self.request.data.questions.get(question_idx) else {
            return;
        };
        if let Some(slot) = self.cursors.get_mut(question_idx) {
            *slot = option_idx;
        }
        self.editing_other = false;
        self.review_message = None;

        if self.is_other_option(question_idx, option_idx) {
            self.enter_other_input(question_idx);
            return;
        }

        if question.multi_select {
            let Some(set) = self.multi_selections.get_mut(question_idx) else {
                return;
            };
            if set.contains(&option_idx) {
                set.remove(&option_idx);
            } else {
                set.insert(option_idx);
            }
            self.last_answer_method = Some(method.to_owned());
            self.update_answer(question_idx);
            return;
        }

        self.single_selections[question_idx] = Some(option_idx);
        self.committed_other_values[question_idx] = None;
        self.last_answer_method = Some(method.to_owned());
        self.update_answer(question_idx);
        self.advance_after_single_select(question_idx);
    }

    fn enter_other_input(&mut self, question_idx: usize) {
        let other_idx = self.other_option_index(question_idx);
        if let Some(slot) = self.cursors.get_mut(question_idx) {
            *slot = other_idx;
        }
        self.editing_other = true;
        let draft = self.other_draft_value(question_idx);
        self.other_input.set_value(&draft);
        self.review_message = None;
    }

    fn commit_other_input(&mut self, raw_value: Option<String>, method: &str) {
        let Some(question_idx) = self.current_question_index() else {
            return;
        };
        let multi_select = self
            .request
            .data
            .questions
            .get(question_idx)
            .map(|q| q.multi_select)
            .unwrap_or(false);
        let value = raw_value
            .unwrap_or_else(|| self.other_input.get_value().to_owned())
            .trim()
            .to_owned();
        if value.is_empty() {
            return;
        }
        self.other_input.set_value(&value);
        self.other_drafts[question_idx] = value.clone();
        self.committed_other_values[question_idx] = Some(value);

        let other_idx = self.other_option_index(question_idx);
        if multi_select {
            if let Some(set) = self.multi_selections.get_mut(question_idx) {
                set.insert(other_idx);
            }
        } else {
            self.single_selections[question_idx] = Some(other_idx);
        }

        self.last_answer_method = Some(method.to_owned());
        self.update_answer(question_idx);
        self.editing_other = false;
        self.review_message = None;

        if !multi_select {
            self.advance_after_single_select(question_idx);
        }
    }

    fn advance_after_single_select(&mut self, question_idx: usize) {
        let next = self.find_next_unanswered_after(question_idx);
        self.current_tab = next.unwrap_or_else(|| self.submit_tab_index());
        self.review_message = None;
        if self.is_submit_tab() {
            self.submit_action_idx = 0;
        }
    }

    fn find_next_unanswered_after(&self, from_idx: usize) -> Option<usize> {
        let total = self.request.data.questions.len();
        (from_idx + 1..total).find(|&idx| !self.is_answered(idx))
    }

    fn update_answer(&mut self, question_idx: usize) {
        let Some(question) = self.request.data.questions.get(question_idx) else {
            return;
        };
        let multi_select = question.multi_select;
        let option_labels: Vec<String> = question.options.iter().map(|o| o.label.clone()).collect();
        let other_idx = self.other_option_index(question_idx);

        if multi_select {
            let mut labels: Vec<String> = Vec::new();
            let set = self
                .multi_selections
                .get(question_idx)
                .cloned()
                .unwrap_or_default();
            for (i, label) in option_labels.iter().enumerate() {
                if !set.contains(&i) {
                    continue;
                }
                if !label.is_empty() {
                    labels.push(label.clone());
                }
            }
            let other_text = self.committed_other_values[question_idx].clone();
            if set.contains(&other_idx) {
                if let Some(text) = other_text {
                    if !text.is_empty() {
                        labels.push(text);
                    }
                }
            }
            if let Some(slot) = self.answers.get_mut(question_idx) {
                *slot = if labels.is_empty() {
                    None
                } else {
                    Some(labels.join(", "))
                };
            }
            return;
        }

        let selection = self.single_selections[question_idx];
        let Some(selection) = selection else {
            if let Some(slot) = self.answers.get_mut(question_idx) {
                *slot = None;
            }
            return;
        };

        if self.is_other_option(question_idx, selection) {
            let other_text = self.committed_other_values[question_idx].clone();
            if let Some(slot) = self.answers.get_mut(question_idx) {
                *slot = other_text.filter(|t| !t.is_empty());
            }
            return;
        }

        let label = option_labels.get(selection).cloned().unwrap_or_default();
        if let Some(slot) = self.answers.get_mut(question_idx) {
            *slot = if label.is_empty() { None } else { Some(label) };
        }
    }

    fn execute_submit_action(&mut self, action_idx: usize, method: &str) {
        if action_idx == 1 {
            self.action = Some(QuestionAction::Cancel);
            return;
        }
        self.review_message = None;
        self.emit_answers(method);
    }

    fn emit_answers(&mut self, method: &str) {
        let out: Vec<String> = self
            .answers
            .iter()
            .filter_map(|a| a.clone().filter(|s| !s.is_empty()))
            .collect();
        let method = self
            .last_answer_method
            .clone()
            .unwrap_or_else(|| method.to_owned());
        self.action = Some(QuestionAction::Answer {
            answers: out,
            method: Some(method),
        });
    }

    // ── Input routing ─────────────────────────────────────────────────

    fn handle_other_input(&mut self, data: &str) {
        let Some(question_idx) = self.current_question_index() else {
            return;
        };
        if matches_key(data, "tab") {
            self.sync_other_draft(question_idx);
            self.editing_other = false;
            self.goto_tab(self.current_tab + 1);
            return;
        }
        if matches_key(data, "up") {
            self.sync_other_draft(question_idx);
            self.editing_other = false;
            self.move_question_cursor(-1);
            return;
        }
        if matches_key(data, "down") {
            self.sync_other_draft(question_idx);
            self.editing_other = false;
            self.move_question_cursor(1);
            return;
        }
        match self.other_input.handle_input(data) {
            InputEvent::Submit => {
                self.commit_other_input(None, "enter");
            }
            // Escape has no onEscape on the TS other input → no-op.
            InputEvent::Escape | InputEvent::None => {}
        }
        self.sync_other_draft(question_idx);
        self.review_message = None;
    }

    fn handle_submit_input(&mut self, data: &str) {
        if matches_key(data, "up") {
            self.submit_action_idx =
                (self.submit_action_idx + SUBMIT_ACTIONS.len() - 1) % SUBMIT_ACTIONS.len();
            self.review_message = None;
            return;
        }
        if matches_key(data, "down") {
            self.submit_action_idx = (self.submit_action_idx + 1) % SUBMIT_ACTIONS.len();
            self.review_message = None;
            return;
        }
        if matches_key(data, "left") {
            self.goto_tab(self.current_tab.saturating_sub(1));
            return;
        }
        if matches_key(data, "right") || matches_key(data, "tab") {
            self.goto_tab(self.current_tab + 1);
            return;
        }
        if matches_key(data, "enter") {
            self.execute_submit_action(self.submit_action_idx, "enter");
            return;
        }
        let printable = decode_kitty_printable(data).unwrap_or_else(|| data.to_owned());
        if printable == "1" {
            self.submit_action_idx = 0;
            self.execute_submit_action(0, "number_key");
            return;
        }
        if printable == "2" {
            self.submit_action_idx = 1;
            self.execute_submit_action(1, "number_key");
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────

    fn other_draft_value(&self, question_idx: usize) -> String {
        self.other_drafts
            .get(question_idx)
            .cloned()
            .filter(|s| !s.is_empty())
            .or_else(|| self.committed_other_values[question_idx].clone())
            .unwrap_or_default()
    }

    fn sync_other_draft(&mut self, question_idx: usize) {
        if let Some(slot) = self.other_drafts.get_mut(question_idx) {
            *slot = self.other_input.get_value().to_owned();
        }
    }

    fn is_answered(&self, question_idx: usize) -> bool {
        self.answers
            .get(question_idx)
            .map(|a| a.as_deref().is_some_and(|s| !s.is_empty()))
            .unwrap_or(false)
    }

    fn has_unanswered_questions(&self) -> bool {
        (0..self.request.data.questions.len()).any(|i| !self.is_answered(i))
    }

    // ── Rendering ─────────────────────────────────────────────────────

    fn render_question_tab(&mut self, width: usize) -> Vec<String> {
        let theme = current_theme();
        let Some(question_idx) = self.current_question_index() else {
            return self.render_submit_tab(width);
        };
        let Some(question) = self.request.data.questions.get(question_idx) else {
            return Vec::new();
        };
        let question = question.clone();

        let accent = |s: &str| theme.fg(ColorToken::Primary, s);
        let dim = |s: &str| theme.fg(ColorToken::TextDim, s);
        let success = |s: &str| theme.fg(ColorToken::Success, s);

        let render_width = width.max(1);
        let mut lines: Vec<String> = vec![
            accent(&"─".repeat(render_width)),
            theme.bold_fg(ColorToken::Primary, " question"),
            String::new(),
        ];
        self.push_tabs(&mut lines);
        lines.push(String::new());

        append_wrapped(
            &mut lines,
            " ? ",
            "   ",
            &question.question,
            render_width,
            Some(&accent),
        );
        if self.is_editing_other() {
            lines.push(dim("   Type your answer, then press Enter to save."));
        }

        if let Some(body) = &question.body {
            let body = body.trim();
            if !body.is_empty() {
                lines.push(String::new());
                let body_lines: Vec<&str> = body.split('\n').collect();
                let visible_body_lines = &body_lines[..body_lines.len().min(MAX_BODY_LINES)];
                for body_line in visible_body_lines {
                    append_wrapped(
                        &mut lines,
                        "   ",
                        "   ",
                        body_line,
                        render_width,
                        Some(&dim),
                    );
                }
                if body_lines.len() > visible_body_lines.len() {
                    lines.push(dim(&format!(
                        "   ... {} more lines",
                        body_lines.len() - visible_body_lines.len()
                    )));
                }
            }
        }

        lines.push(String::new());

        let options = self.display_options(question_idx);
        let cursor = self.current_cursor();
        let visible_start = self.compute_visible_start(cursor, options.len());
        let visible_end = (visible_start + self.max_visible_options).min(options.len());
        let multi_set = self.multi_selections[question_idx].clone();
        let single_selection = self.single_selections[question_idx];

        for i in visible_start..visible_end {
            let Some(option) = options.get(i) else {
                continue;
            };
            let num = i + 1;
            let is_cursor = i == cursor;
            let is_other = option.kind == OptionKind::Other;
            let is_selected = if question.multi_select {
                multi_set.contains(&i)
            } else {
                single_selection == Some(i)
            };

            if self.is_editing_other() && is_cursor && is_other {
                lines.push(self.render_editing_other_line(
                    render_width,
                    question_idx,
                    option,
                    num,
                    is_selected,
                ));
                continue;
            }

            let label = self.render_option_label(question_idx, option, is_cursor);

            let tone: Box<dyn Fn(&str) -> String>;
            let prefix: String;
            if question.multi_select {
                let checked = if is_selected { "✓" } else { " " };
                prefix = format!("  [{checked}] ");
                if is_selected && is_cursor {
                    tone = Box::new(|s| theme.bold_fg(ColorToken::Success, s));
                } else if is_selected {
                    tone = Box::new(success);
                } else if is_cursor {
                    tone = Box::new(accent);
                } else {
                    tone = Box::new(dim);
                }
            } else if is_selected && self.is_answered(question_idx) {
                prefix = if is_cursor {
                    format!("  → [{num}] ")
                } else {
                    format!("    [{num}] ")
                };
                if is_cursor {
                    tone = Box::new(|s| theme.bold_fg(ColorToken::Success, s));
                } else {
                    tone = Box::new(success);
                }
            } else if is_cursor {
                prefix = format!("  → [{num}] ");
                tone = Box::new(accent);
            } else {
                prefix = format!("    [{num}] ");
                tone = Box::new(dim);
            }
            let continuation = " ".repeat(visible_width(&prefix));
            append_wrapped(
                &mut lines,
                &prefix,
                &continuation,
                &label,
                render_width,
                Some(&*tone),
            );

            if let Some(description) = &option.description {
                if !(description.is_empty() || (self.is_editing_other() && is_cursor && is_other)) {
                    append_wrapped(
                        &mut lines,
                        "        ",
                        "        ",
                        description,
                        render_width,
                        Some(&dim),
                    );
                }
            }
        }

        if visible_end < options.len() || visible_start > 0 {
            lines.push(dim(&format!(
                "   showing {}-{} of {}",
                visible_start + 1,
                visible_end,
                options.len()
            )));
        }

        lines.push(String::new());
        lines.push(self.build_question_hint(&dim, question_idx));
        lines.push(accent(&"─".repeat(render_width)));

        lines
            .iter()
            .map(|line| truncate_to_width(line, width, "...", false))
            .collect()
    }

    fn render_submit_tab(&mut self, width: usize) -> Vec<String> {
        let theme = current_theme();
        let accent = |s: &str| theme.fg(ColorToken::Primary, s);
        let dim = |s: &str| theme.fg(ColorToken::TextDim, s);
        let text = |s: &str| theme.fg(ColorToken::Text, s);
        let warning = |s: &str| theme.fg(ColorToken::Warning, s);

        let render_width = width.max(1);
        let mut lines: Vec<String> = vec![
            accent(&"─".repeat(render_width)),
            theme.bold_fg(ColorToken::Primary, " question"),
            String::new(),
        ];
        self.push_tabs(&mut lines);
        lines.push(String::new());
        lines.push(theme.bold_fg(ColorToken::Text, &format!(" {REVIEW_TITLE}")));
        let review_warning = self.review_message.clone().or_else(|| {
            if self.has_unanswered_questions() {
                Some(UNANSWERED_WARNING.to_owned())
            } else {
                None
            }
        });
        if let Some(w) = review_warning {
            lines.push(warning(&format!("  {w}")));
        }
        lines.push(String::new());

        for (i, question) in self.request.data.questions.iter().enumerate() {
            let answer = self.answers[i].clone();
            append_wrapped(
                &mut lines,
                &format!("  {}  ", dim("Q")),
                "       ",
                &question.question,
                render_width,
                None,
            );
            if let Some(answer) = answer {
                if !answer.is_empty() {
                    append_wrapped(
                        &mut lines,
                        &format!("  {}  ", accent("→")),
                        "       ",
                        &text(&answer),
                        render_width,
                        None,
                    );
                } else {
                    lines.push(format!("  {}  {}", dim("→"), dim(NOT_ANSWERED_LABEL)));
                }
            } else {
                lines.push(format!("  {}  {}", dim("→"), dim(NOT_ANSWERED_LABEL)));
            }
        }

        lines.push(String::new());
        lines.push(text(&format!(" {SUBMIT_PROMPT}")));
        lines.push(String::new());

        for (i, label) in SUBMIT_ACTIONS.iter().enumerate() {
            let num = i + 1;
            if i == self.submit_action_idx {
                lines.push(accent(&format!("  → [{num}] {label}")));
            } else {
                lines.push(dim(&format!("    [{num}] {label}")));
            }
        }

        lines.push(String::new());
        lines.push(self.build_submit_hint(&dim));
        lines.push(accent(&"─".repeat(render_width)));

        lines
            .iter()
            .map(|line| truncate_to_width(line, width, "...", false))
            .collect()
    }

    fn push_tabs(&self, lines: &mut Vec<String>) {
        let theme = current_theme();
        let dim = |s: &str| theme.fg(ColorToken::TextDim, s);
        let active = |s: &str| theme.bg(ColorToken::Primary, &theme.bold_fg(ColorToken::Text, s));

        let mut tabs: Vec<String> = Vec::new();
        for (i, question) in self.request.data.questions.iter().enumerate() {
            let fallback = format!("Q{}", i + 1);
            let label = question
                .header
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(&fallback);
            if i == self.current_tab {
                tabs.push(active(&format!(" {label} ")));
            } else if self.is_answered(i) {
                tabs.push(theme.fg(ColorToken::Success, &format!("(✓) {label}")));
            } else {
                tabs.push(dim(&format!("(○) {label}")));
            }
        }
        let submit_label = "Submit";
        if self.is_submit_tab() {
            tabs.push(active(&format!(" {submit_label} ")));
        } else {
            tabs.push(dim(&format!(" {submit_label} ")));
        }

        lines.push(format!(" {}", tabs.join("  ")));
    }

    fn build_question_hint(&self, dim: &dyn Fn(&str) -> String, question_idx: usize) -> String {
        if self.is_editing_other() {
            let mut parts: Vec<&str> = vec!["type answer", "↵ save"];
            if self.total_tabs() > 1 {
                parts.push("tab switch");
            }
            parts.push("esc cancel");
            return dim(&format!("  {}", parts.join("  ")));
        }

        let option_count = self
            .display_options(question_idx)
            .len()
            .min(NUMBER_KEYS.len());
        let number_hint = if option_count <= 1 {
            "1".to_owned()
        } else {
            format!("1-{option_count}")
        };
        let Some(question) = self.request.data.questions.get(question_idx) else {
            return dim("  esc cancel");
        };
        let verb = if question.multi_select {
            "toggle"
        } else {
            "choose"
        };
        let mut parts: Vec<String> =
            vec!["↑↓ select".to_owned(), format!("{number_hint} / ↵ {verb}")];
        if self.total_tabs() > 1 {
            parts.push("←/→/tab switch".to_owned());
        }
        parts.push("esc cancel".to_owned());
        dim(&format!("  {}", parts.join("  ")))
    }

    fn build_submit_hint(&self, dim: &dyn Fn(&str) -> String) -> String {
        let mut parts: Vec<&str> = vec!["↑↓ select", "1/2 choose", "↵ confirm"];
        if self.total_tabs() > 1 {
            parts.push("←/→/tab switch");
        }
        parts.push("esc cancel");
        dim(&format!("  {}", parts.join("  ")))
    }

    fn compute_visible_start(&self, cursor: usize, total: usize) -> usize {
        if total <= self.max_visible_options {
            return 0;
        }
        let half = self.max_visible_options / 2;
        let max = total.saturating_sub(self.max_visible_options);
        cursor.saturating_sub(half).min(max)
    }

    fn render_option_label(
        &self,
        question_idx: usize,
        option: &DisplayOption,
        is_cursor: bool,
    ) -> String {
        if option.kind != OptionKind::Other {
            return option.label.clone();
        }
        let value = self.other_draft_value(question_idx);
        if self.is_editing_other() && is_cursor {
            return format!("{}: {}█", option.label, value);
        }
        if !value.is_empty() {
            return format!("{}: {value}", option.label);
        }
        option.label.clone()
    }

    fn render_editing_other_line(
        &self,
        width: usize,
        question_idx: usize,
        option: &DisplayOption,
        num: usize,
        is_selected: bool,
    ) -> String {
        let theme = current_theme();
        let Some(question) = self.request.data.questions.get(question_idx) else {
            return option.label.clone();
        };

        let prefix: String = if question.multi_select {
            let checked = if is_selected { "✓" } else { " " };
            let body = format!("  [{checked}] {}: ", option.label);
            if is_selected {
                theme.bold_fg(ColorToken::Success, &body)
            } else {
                theme.fg(ColorToken::Primary, &body)
            }
        } else {
            let body = format!("  → [{num}] {}: ", option.label);
            if is_selected && self.is_answered(question_idx) {
                theme.bold_fg(ColorToken::Success, &body)
            } else {
                theme.fg(ColorToken::Primary, &body)
            }
        };

        let input_width = 4usize.max(width.saturating_sub(visible_width(&prefix)) + 2);
        let input_line = self.other_input.render(input_width);
        let inline_input = input_line
            .strip_prefix("> ")
            .unwrap_or(&input_line)
            .to_owned();
        format!("{prefix}{inline_input}")
    }
}

impl Component for QuestionDialogComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        if self.is_submit_tab() {
            self.render_submit_tab(width)
        } else {
            self.render_question_tab(width)
        }
    }

    fn handle_input(&mut self, data: &str) {
        if matches_key(data, "escape") || matches_key(data, "ctrl+c") || matches_key(data, "ctrl+d")
        {
            self.action = Some(QuestionAction::Cancel);
            return;
        }

        if matches_key(data, "ctrl+o") {
            if self.has_toggle_tool_output {
                self.action = Some(QuestionAction::ToggleToolOutput);
            }
            return;
        }

        if self.is_editing_other() {
            self.handle_other_input(data);
            return;
        }

        if self.is_submit_tab() {
            self.handle_submit_input(data);
            return;
        }

        let Some(question_idx) = self.current_question_index() else {
            return;
        };
        let Some(question) = self.request.data.questions.get(question_idx) else {
            return;
        };

        let option_count = self.display_options(question_idx).len();
        if option_count == 0 {
            return;
        }

        if matches_key(data, "up") {
            self.move_question_cursor(-1);
            return;
        }
        if matches_key(data, "down") {
            self.move_question_cursor(1);
            return;
        }
        if matches_key(data, "left") {
            self.goto_tab(self.current_tab.saturating_sub(1));
            return;
        }
        if matches_key(data, "right") || matches_key(data, "tab") {
            self.goto_tab(self.current_tab + 1);
            return;
        }
        if matches_key(data, "enter") {
            self.activate_question_option(self.current_cursor(), "enter");
            return;
        }

        let printable = decode_kitty_printable(data).unwrap_or_else(|| data.to_owned());
        if let Some(num_idx) = NUMBER_KEYS.iter().position(|k| *k == printable) {
            if num_idx < option_count {
                self.cursors[question_idx] = num_idx;
                self.activate_question_option(num_idx, "number_key");
            }
            return;
        }

        let is_space = printable == " " || matches_key(data, "space");
        if is_space && question.multi_select {
            self.activate_question_option(self.current_cursor(), "space");
        }
    }

    fn invalidate(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question(multi: bool) -> QuestionPanelItem {
        QuestionPanelItem {
            question: "Pick an option".to_owned(),
            header: Some("Q1".to_owned()),
            body: Some("Please choose one of the following.".to_owned()),
            multi_select: multi,
            other_label: None,
            other_description: None,
            options: vec![
                QuestionOption {
                    label: "Alpha".to_owned(),
                    description: Some("first choice".to_owned()),
                },
                QuestionOption {
                    label: "Beta".to_owned(),
                    description: None,
                },
            ],
        }
    }

    fn pending() -> PendingQuestion {
        PendingQuestion {
            data: QuestionPanelData {
                id: "q_1".to_owned(),
                tool_call_id: "tool_1".to_owned(),
                questions: vec![question(false)],
            },
        }
    }

    #[test]
    fn renders_question_with_options() {
        let mut c = QuestionDialogComponent::new(pending(), None, true);
        let lines = c.render(80);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("question"), "{joined}");
        assert!(joined.contains("Pick an option"), "{joined}");
        assert!(joined.contains("Please choose one"), "{joined}");
        assert!(joined.contains("[1] Alpha"), "{joined}");
        assert!(joined.contains("first choice"), "{joined}");
        assert!(joined.contains("[2] Beta"), "{joined}");
        assert!(joined.contains("Other"), "{joined}");
        assert!(joined.contains("↑↓ select"), "{joined}");
    }

    #[test]
    fn enter_selects_and_advances_to_submit() {
        let mut c = QuestionDialogComponent::new(pending(), None, true);
        c.handle_input("\r"); // select Alpha → single-select advances to submit tab
        assert!(c.is_submit_tab());
        let lines = c.render(80);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("Review your answer"), "{joined}");
        assert!(joined.contains("Alpha"), "{joined}");
        assert!(joined.contains("Ready to submit"), "{joined}");
        assert!(joined.contains("[1] Submit"), "{joined}");
        assert!(joined.contains("[2] Cancel"), "{joined}");
    }

    #[test]
    fn submit_emits_answer() {
        let mut c = QuestionDialogComponent::new(pending(), None, true);
        c.handle_input("\r"); // answer Alpha
        c.handle_input("\r"); // enter on Submit
        match c.take_action() {
            Some(QuestionAction::Answer { answers, method }) => {
                assert_eq!(answers, vec!["Alpha".to_owned()]);
                assert_eq!(method.as_deref(), Some("enter"));
            }
            other => panic!("expected answer, got {other:?}"),
        }
    }

    #[test]
    fn number_key_selects() {
        let mut c = QuestionDialogComponent::new(pending(), None, true);
        c.handle_input("2"); // number key selects Beta
        assert!(c.is_submit_tab());
        let lines = c.render(80);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("Beta"), "{joined}");
    }

    #[test]
    fn escape_cancels() {
        let mut c = QuestionDialogComponent::new(pending(), None, true);
        c.handle_input("\x1b");
        assert_eq!(c.take_action(), Some(QuestionAction::Cancel));
    }

    #[test]
    fn multi_select_toggles_with_space() {
        let mut c = QuestionDialogComponent::new(
            PendingQuestion {
                data: QuestionPanelData {
                    id: "q".to_owned(),
                    tool_call_id: "t".to_owned(),
                    questions: vec![question(true)],
                },
            },
            None,
            true,
        );
        c.handle_input(" "); // toggle Alpha on
        c.handle_input("\x1b[B"); // down to Beta
        c.handle_input(" "); // toggle Beta on
        let lines = c.render(80);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("[✓] Alpha"), "{joined}");
        assert!(joined.contains("[✓] Beta"), "{joined}");
        // Space again on Beta toggles off.
        c.handle_input(" ");
        let lines = c.render(80);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("[ ] Beta"), "{joined}");
    }

    #[test]
    fn other_input_flow() {
        let mut c = QuestionDialogComponent::new(pending(), None, true);
        // Cursor starts on Alpha; move to Other (last option).
        c.handle_input("\x1b[B");
        c.handle_input("\x1b[B");
        c.handle_input("\r"); // enter Other → editing mode
        c.handle_input("custom");
        c.handle_input("\r"); // commit
        assert!(c.is_submit_tab());
        let lines = c.render(80);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("custom"), "{joined}");
    }
}
