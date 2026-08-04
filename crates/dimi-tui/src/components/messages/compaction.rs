//! `CompactionComponent` — renders a compaction block in the transcript
//! (port of `apps/dimi/src/tui/components/dialogs/compaction.ts`).

use crate::component::Component;
use crate::components::messages::STATUS_BULLET;
use crate::components::spacer::Spacer;
use crate::components::text::Text;
use crate::theme::{ColorToken, current_theme};

/// Compaction block lifecycle:
/// - constructed → blinking white bullet + "Compacting context..." +
///   optional instruction
/// - `mark_done` → solid green bullet + "Compaction complete (X → Y tokens)"
/// - `mark_canceled` → solid warning bullet + "Compaction cancelled"
pub struct CompactionComponent {
    instruction: Option<String>,
    blink_on: bool,
    done: bool,
    canceled: bool,
    tokens_before: Option<i64>,
    tokens_after: Option<i64>,
    summary: Option<String>,
    expanded: bool,
    header_text: Text,
    instruction_text: Option<Text>,
    summary_text: Option<Text>,
}

impl CompactionComponent {
    pub fn new(instruction: Option<&str>) -> Self {
        let header_text = Text::new(
            &build_header(HeaderState {
                tokens_before: None,
                tokens_after: None,
                summary: None,
                done: false,
                canceled: false,
                blink_on: true,
                expanded: false,
            }),
            0,
            0,
        );
        let instruction_text =
            instruction.map(|i| Text::new(&current_theme().dim(&format!("  {i}")), 0, 0));
        CompactionComponent {
            instruction: instruction.map(str::to_owned),
            blink_on: true,
            done: false,
            canceled: false,
            tokens_before: None,
            tokens_after: None,
            summary: None,
            expanded: false,
            header_text,
            instruction_text,
            summary_text: None,
        }
    }

    pub fn mark_done(
        &mut self,
        tokens_before: Option<i64>,
        tokens_after: Option<i64>,
        summary: Option<&str>,
    ) {
        if self.done || self.canceled {
            return;
        }
        self.done = true;
        self.tokens_before = tokens_before;
        self.tokens_after = tokens_after;
        self.summary = summary.map(str::to_owned);
        self.header_text.set_text(&build_header(HeaderState {
            tokens_before: self.tokens_before,
            tokens_after: self.tokens_after,
            summary: self.summary.as_deref(),
            done: true,
            canceled: false,
            blink_on: false,
            expanded: self.expanded,
        }));
        if self.expanded {
            self.add_summary_child();
        }
    }

    pub fn mark_canceled(&mut self) {
        if self.done || self.canceled {
            return;
        }
        self.canceled = true;
        self.header_text.set_text(&build_header(HeaderState {
            tokens_before: None,
            tokens_after: None,
            summary: None,
            done: false,
            canceled: true,
            blink_on: false,
            expanded: false,
        }));
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        if self.expanded == expanded {
            return;
        }
        self.expanded = expanded;
        if expanded {
            self.add_summary_child();
        } else {
            self.summary_text = None;
        }
        self.header_text.set_text(&build_header(HeaderState {
            tokens_before: self.tokens_before,
            tokens_after: self.tokens_after,
            summary: self.summary.as_deref(),
            done: self.done,
            canceled: self.canceled,
            blink_on: self.blink_on,
            expanded: self.expanded,
        }));
    }

    fn add_summary_child(&mut self) {
        if self.summary_text.is_some()
            || self.summary.is_none()
            || self.summary.as_deref().is_some_and(str::is_empty)
        {
            return;
        }
        let indented = self
            .summary
            .as_deref()
            .unwrap_or("")
            .split('\n')
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        self.summary_text = Some(Text::new(&current_theme().dim(&indented), 0, 0));
    }

    /// Advance the blink (called on the blink interval by the coordinator).
    pub fn tick_blink(&mut self) {
        if self.done || self.canceled {
            return;
        }
        self.blink_on = !self.blink_on;
        self.header_text.set_text(&build_header(HeaderState {
            tokens_before: None,
            tokens_after: None,
            summary: None,
            done: false,
            canceled: false,
            blink_on: self.blink_on,
            expanded: false,
        }));
    }

    pub fn is_running(&self) -> bool {
        !self.done && !self.canceled
    }
}

/// Header render state.
#[derive(Clone, Copy)]
struct HeaderState<'a> {
    tokens_before: Option<i64>,
    tokens_after: Option<i64>,
    summary: Option<&'a str>,
    done: bool,
    canceled: bool,
    blink_on: bool,
    expanded: bool,
}

fn build_header(state: HeaderState<'_>) -> String {
    let HeaderState {
        tokens_before,
        tokens_after,
        summary,
        done,
        canceled,
        blink_on,
        expanded,
    } = state;
    let theme = current_theme();
    if done {
        let bullet = theme.fg(ColorToken::Success, STATUS_BULLET);
        let label = theme.bold_fg(ColorToken::Success, "Compaction complete");
        let detail = match (tokens_before, tokens_after) {
            (Some(before), Some(after)) => theme.dim(&format!(" ({before} → {after} tokens)")),
            _ => String::new(),
        };
        let shortcut_hint = if summary.is_some_and(|s| !s.is_empty()) {
            theme.dim(&format!(
                " (Ctrl-O to {} compaction summary)",
                if expanded { "hide" } else { "show" }
            ))
        } else {
            String::new()
        };
        format!("{bullet}{label}{detail}{shortcut_hint}")
    } else if canceled {
        let bullet = theme.fg(ColorToken::Warning, STATUS_BULLET);
        let label = theme.bold_fg(ColorToken::Warning, "Compaction cancelled");
        format!("{bullet}{label}")
    } else {
        // Running.
        let bullet = if blink_on {
            theme.fg(ColorToken::Text, STATUS_BULLET)
        } else {
            "  ".to_owned()
        };
        let label = theme.bold_fg(ColorToken::Primary, "Compacting context...");
        format!("{bullet}{label}")
    }
}

impl Component for CompactionComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        lines.extend(Spacer::new(1).render(width));
        lines.extend(self.header_text.render(width));
        if let Some(instruction) = &mut self.instruction_text {
            lines.extend(instruction.render(width));
        }
        if let Some(summary) = &mut self.summary_text {
            lines.extend(summary.render(width));
        }
        lines
    }

    fn invalidate(&mut self) {
        self.header_text.set_text(&build_header(HeaderState {
            tokens_before: self.tokens_before,
            tokens_after: self.tokens_after,
            summary: self.summary.as_deref(),
            done: self.done,
            canceled: self.canceled,
            blink_on: self.blink_on,
            expanded: self.expanded,
        }));
        if let Some(instruction) = &self.instruction {
            if let Some(text) = &mut self.instruction_text {
                text.set_text(&current_theme().dim(&format!("  {instruction}")));
            }
        }
        if let Some(summary) = &self.summary {
            if let Some(text) = &mut self.summary_text {
                let indented = summary
                    .split('\n')
                    .map(|line| format!("  {line}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                text.set_text(&current_theme().dim(&indented));
            }
        }
    }
}
