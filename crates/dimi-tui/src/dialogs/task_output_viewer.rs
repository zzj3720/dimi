//! Task output viewer — full-screen output viewer for a single background
//! task. Port of
//! `apps/dimi/src/tui/components/dialogs/task-output-viewer.ts`
//! (`TaskOutputViewer`). Snapshot view (no live tail).

use crate::component::{Component, Focusable};
use crate::dialogs::task_types::BackgroundTaskInfo;
use crate::keys::{matches_key, printable_char};
use crate::theme::{ColorToken, current_theme};
use crate::width::visible_width;
use crate::wrap::truncate_to_width;

const ELLIPSIS: &str = "…";

/// `TaskOutputViewerProps`.
#[derive(Debug, Clone)]
pub struct TaskOutputViewerProps {
    pub task_id: String,
    pub info: Option<BackgroundTaskInfo>,
    pub output: String,
}

/// Action the host reacts to (mirrors `onClose`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputViewerAction {
    Close,
}

fn pad_to_width(line: &str, width: usize) -> String {
    let w = visible_width(line);
    if w == width {
        return line.to_owned();
    }
    if w > width {
        return truncate_to_width(line, width, ELLIPSIS, false);
    }
    format!("{line}{}", " ".repeat(width - w))
}

fn fit_exactly(line: &str, width: usize) -> String {
    let mut s = line.to_owned();
    if visible_width(&s) > width {
        s = truncate_to_width(&s, width, ELLIPSIS, false);
    }
    pad_to_width(&s, width)
}

/// `TaskOutputViewer`.
pub struct TaskOutputViewer {
    props: TaskOutputViewerProps,
    /// Terminal rows (viewer is full-screen).
    rows: usize,
    /// Output split on `\n`.
    lines: Vec<String>,
    /// Index of the topmost visible line.
    scroll_top: usize,
    focused: bool,
    action: Option<OutputViewerAction>,
}

impl TaskOutputViewer {
    pub fn new(props: TaskOutputViewerProps, rows: usize) -> Self {
        let lines = split_output(&props.output);
        TaskOutputViewer {
            props,
            rows,
            lines,
            scroll_top: 0,
            focused: false,
            action: None,
        }
    }

    /// `setProps` — update the task info/output, following the tail when the
    /// user is parked at the bottom.
    pub fn set_props(&mut self, next: TaskOutputViewerProps) {
        let previous_output = self.props.output.clone();
        let was_at_bottom = self.scroll_top >= self.max_scroll();
        self.props = next;
        if self.props.output != previous_output {
            self.lines = split_output(&self.props.output);
            if was_at_bottom {
                self.scroll_top = self.max_scroll();
            } else {
                self.scroll_top = self.scroll_top.min(self.max_scroll());
            }
        }
    }

    pub fn take_action(&mut self) -> Option<OutputViewerAction> {
        self.action.take()
    }

    fn scroll_by(&mut self, delta: i64) {
        self.scroll_to(self.scroll_top as i64 + delta);
    }

    fn scroll_to(&mut self, target: i64) {
        self.scroll_top = target.clamp(0, self.max_scroll() as i64) as usize;
    }

    fn max_scroll(&self) -> usize {
        self.lines.len().saturating_sub(self.viewable_rows())
    }

    /// Content rows visible inside the body frame: total rows − header −
    /// footer − top border − bottom border.
    fn viewable_rows(&self) -> usize {
        self.rows.saturating_sub(4).max(1)
    }

    fn render_header(&self, width: usize) -> String {
        let theme = current_theme();
        let title = theme.bold_fg(ColorToken::Primary, " Task output ");
        let id = theme.bold_fg(ColorToken::Text, &self.props.task_id);
        let mut segments: Vec<String> = Vec::new();
        if let Some(info) = &self.props.info {
            segments.push(theme.fg(info.status.color(), info.status.label()));
            if info.kind == crate::dialogs::task_types::TaskKind::Process
                && let Some(exit_code) = info.exit_code
            {
                segments.push(theme.fg(ColorToken::TextMuted, &format!("exit {exit_code}")));
            }
            if !info.description.is_empty() {
                segments.push(theme.fg(ColorToken::TextMuted, &info.description));
            }
        }
        let composed = format!(
            "{title}{id}{}",
            if segments.is_empty() {
                String::new()
            } else {
                format!("  {}", segments.join("  "))
            }
        );
        fit_exactly(&composed, width)
    }

    fn render_body(&mut self, width: usize, body_height: usize) -> Vec<String> {
        let theme = current_theme();
        let inner_width = (width.saturating_sub(4)).max(1);
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
        let mut out = vec![top];
        for i in 0..view_rows {
            let line_index = self.scroll_top + i;
            let raw = self.lines.get(line_index).cloned().unwrap_or_default();
            let inner = fit_exactly(&theme.fg(ColorToken::Text, &raw), inner_width);
            out.push(format!(
                "{} {} {}",
                theme.fg(ColorToken::Primary, "│"),
                inner,
                theme.fg(ColorToken::Primary, "│")
            ));
        }
        out.push(bottom);
        out
    }

    fn render_footer(&self, width: usize, body_height: usize) -> String {
        let theme = current_theme();
        let key = |text: &str| theme.bold_fg(ColorToken::Primary, text);
        let dim = |text: &str| theme.fg(ColorToken::TextMuted, text);

        let total = self.lines.len();
        let view_rows = (body_height.saturating_sub(2)).max(1);
        let max_scroll = total.saturating_sub(view_rows);
        let percent = if max_scroll == 0 {
            100
        } else {
            ((self.scroll_top as f64 / max_scroll as f64) * 100.0).round() as i64
        };
        let line_from = self.scroll_top + 1;
        let line_to = total.min(self.scroll_top + view_rows);

        let position = theme.fg(
            ColorToken::TextMuted,
            &format!(" {line_from}-{line_to} / {total} ({percent}%) "),
        );
        let keys = format!(
            "{} {}  {} {}  {} {}  {} {}",
            key("↑↓"),
            dim("line"),
            key("PgUp/PgDn/Ctrl+U/D"),
            dim("page"),
            key("g/G"),
            dim("top/bot"),
            key("Q/Esc"),
            dim("cancel")
        );
        let left = format!(" {keys}");
        let left_w = visible_width(&left);
        let right_w = visible_width(&position);
        if left_w + 2 + right_w <= width {
            return format!("{left}{}{position}", " ".repeat(width - left_w - right_w));
        }
        fit_exactly(&left, width)
    }
}

fn split_output(output: &str) -> Vec<String> {
    if output.is_empty() {
        vec!["[no output captured]".to_owned()]
    } else {
        output.split('\n').map(str::to_owned).collect()
    }
}

impl Component for TaskOutputViewer {
    fn render(&mut self, width: usize) -> Vec<String> {
        let rows = self.rows.max(3);
        let body_height = rows - 2;
        let header = self.render_header(width);
        let body = self.render_body(width, body_height);
        let footer = self.render_footer(width, body_height);
        let mut out = vec![header];
        out.extend(body);
        out.push(footer);
        out
    }

    fn handle_input(&mut self, data: &str) {
        let visible = self.viewable_rows();
        let k = printable_char(data);

        if matches_key(data, "escape") || k == "q" || k == "Q" {
            self.action = Some(OutputViewerAction::Close);
            return;
        }
        if matches_key(data, "up") || k == "k" {
            self.scroll_by(-1);
            return;
        }
        if matches_key(data, "down") || k == "j" {
            self.scroll_by(1);
            return;
        }
        if matches_key(data, "pageup")
            || matches_key(data, "ctrl+u")
            || k == " "
            || data == "\u{0002}"
        // C-b
        {
            self.scroll_by(-(visible as i64 - 1).max(1));
            return;
        }
        if matches_key(data, "pagedown") || matches_key(data, "ctrl+d") || data == "\u{0006}"
        // C-f
        {
            self.scroll_by((visible as i64 - 1).max(1));
            return;
        }
        if matches_key(data, "home") || k == "g" {
            self.scroll_to(0);
            return;
        }
        if matches_key(data, "end") || k == "G" {
            self.scroll_to(self.max_scroll() as i64);
        }
    }

    fn invalidate(&mut self) {}

    fn as_focusable_mut(&mut self) -> Option<&mut dyn Focusable> {
        Some(self)
    }
}

impl Focusable for TaskOutputViewer {
    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs::task_types::{BackgroundTaskInfo, BackgroundTaskStatus, TaskKind};
    use crate::theme::{DARK_COLORS, set_palette};

    fn plain(joined: &str) -> String {
        crate::ansi::strip_ansi(joined)
    }

    fn task() -> BackgroundTaskInfo {
        BackgroundTaskInfo {
            task_id: "task_1".to_owned(),
            status: BackgroundTaskStatus::Running,
            kind: TaskKind::Process,
            description: "npm test".to_owned(),
            command: Some("npm test".to_owned()),
            exit_code: None,
            pid: 1234,
            started_at: 0,
            ended_at: None,
            agent_id: None,
            subagent_type: None,
            question_count: 0,
            tool_call_id: None,
            stop_reason: None,
            detached: true,
        }
    }

    #[test]
    fn renders_full_screen() {
        set_palette(DARK_COLORS);
        let mut v = TaskOutputViewer::new(
            TaskOutputViewerProps {
                task_id: "task_1".to_owned(),
                info: Some(task()),
                output: "line1\nline2\nline3".to_owned(),
            },
            10,
        );
        let lines = v.render(100);
        assert_eq!(lines.len(), 10);
        let joined = plain(&lines.join("\n"));
        assert!(joined.contains("Task output"), "{joined}");
        assert!(joined.contains("task_1"), "{joined}");
        assert!(joined.contains("running"), "{joined}");
        assert!(joined.contains("npm test"), "{joined}");
        assert!(joined.contains("line1"), "{joined}");
        assert!(joined.contains("line2"), "{joined}");
        assert!(joined.contains("line3"), "{joined}");
        assert!(joined.contains("1-3 / 3 (100%)"), "{joined}");
    }

    #[test]
    fn scrolling_and_close() {
        set_palette(DARK_COLORS);
        let output: String = (0..20).map(|i| format!("line{i}\n")).collect();
        let mut v = TaskOutputViewer::new(
            TaskOutputViewerProps {
                task_id: "task_2".to_owned(),
                info: None,
                output,
            },
            10,
        );
        v.handle_input("\x1b[B"); // down
        assert_eq!(v.scroll_top, 1);
        v.handle_input("G"); // end
        assert_eq!(v.scroll_top, v.max_scroll());
        v.handle_input("q");
        assert_eq!(v.take_action(), Some(OutputViewerAction::Close));
    }

    #[test]
    fn empty_output_placeholder() {
        set_palette(DARK_COLORS);
        let mut v = TaskOutputViewer::new(
            TaskOutputViewerProps {
                task_id: "task_3".to_owned(),
                info: None,
                output: String::new(),
            },
            8,
        );
        assert!(plain(&v.render(60).join("\n")).contains("[no output captured]"));
    }
}
