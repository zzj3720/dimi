//! `TaskListComponent` — the background-tasks panel (a simplified port of the
//! TS `TasksBrowserApp`): a bordered pane listing `task_id / status /
//! description`, shown while the `/tasks` panel is open. The task data comes
//! from the engine backend (`EngineBackend` tracks `task.started` /
//! `task.settled`); with no tracked tasks the panel shows an empty list.
//! Renders nothing (0 lines) while closed.

use dimi_tui::component::Component;
use dimi_tui::theme::{ColorToken, current_theme};
use dimi_tui::wrap::truncate_to_width;

/// One background task row (`BackgroundTaskInfo` distilled for display).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskInfo {
    pub task_id: String,
    pub status: String,
    pub description: Option<String>,
}

impl TaskInfo {
    /// `#[allow(dead_code)]`: a test/construction convenience; the app builds
    /// rows from `BackgroundTaskInfo` directly.
    #[allow(dead_code)]
    pub fn new(task_id: impl Into<String>, status: impl Into<String>) -> Self {
        TaskInfo {
            task_id: task_id.into(),
            status: status.into(),
            description: None,
        }
    }
}

/// The tasks panel. Renders nothing while closed.
pub struct TaskListComponent {
    tasks: Vec<TaskInfo>,
    open: bool,
}

impl TaskListComponent {
    pub fn new() -> Self {
        TaskListComponent {
            tasks: Vec::new(),
            open: false,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    pub fn set_tasks(&mut self, tasks: Vec<TaskInfo>) {
        self.tasks = tasks;
    }

    /// `#[allow(dead_code)]`: read by tests; the app populates via
    /// [`TaskListComponent::set_tasks`].
    #[allow(dead_code)]
    pub fn tasks(&self) -> &[TaskInfo] {
        &self.tasks
    }
}

impl Default for TaskListComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for TaskListComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        if !self.open {
            return Vec::new();
        }
        let theme = current_theme();
        let mut lines: Vec<String> = vec![theme.fg(ColorToken::Border, &"─".repeat(width))];
        lines.push(theme.bold_fg(ColorToken::Primary, "  Tasks"));
        if self.tasks.is_empty() {
            lines.push(theme.fg(ColorToken::TextDim, "  No background tasks."));
        } else {
            for task in &self.tasks {
                let description = task.description.as_deref().unwrap_or("");
                let text = if description.is_empty() {
                    format!("  ○ {} · {}", task.task_id, task.status)
                } else {
                    format!("  ○ {} · {}  {}", task.task_id, task.status, description)
                };
                lines.push(theme.fg(ColorToken::Text, &text));
            }
        }
        lines.push(theme.fg(
            ColorToken::TextDim,
            &truncate_to_width("  Esc close · ↑↓ navigate", width, "…", false),
        ));
        lines
    }

    fn invalidate(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use dimi_tui::ansi::strip_ansi;
    use dimi_tui::theme::{DARK_COLORS, set_palette};

    #[test]
    fn closed_renders_zero_lines() {
        set_palette(DARK_COLORS);
        let mut t = TaskListComponent::new();
        t.set_tasks(vec![TaskInfo::new("t1", "running")]);
        assert!(Component::render(&mut t, 80).is_empty());
    }

    #[test]
    fn open_with_tasks_renders_rows() {
        set_palette(DARK_COLORS);
        let mut t = TaskListComponent::new();
        t.set_tasks(vec![
            TaskInfo::new("task_1", "running"),
            TaskInfo {
                task_id: "task_2".to_owned(),
                status: "completed".to_owned(),
                description: Some("run tests".to_owned()),
            },
        ]);
        t.set_open(true);
        let joined = strip_ansi(&Component::render(&mut t, 80).join("\n"));
        assert!(joined.contains("Tasks"), "{joined}");
        assert!(joined.contains("task_1"), "{joined}");
        assert!(joined.contains("running"), "{joined}");
        assert!(joined.contains("task_2"), "{joined}");
        assert!(joined.contains("completed"), "{joined}");
        assert!(joined.contains("run tests"), "{joined}");
    }

    #[test]
    fn open_without_tasks_renders_empty_message() {
        set_palette(DARK_COLORS);
        let mut t = TaskListComponent::new();
        t.set_open(true);
        let joined = strip_ansi(&Component::render(&mut t, 80).join("\n"));
        assert!(joined.contains("No background tasks"), "{joined}");
    }

    #[test]
    fn closing_hides_the_panel() {
        set_palette(DARK_COLORS);
        let mut t = TaskListComponent::new();
        t.set_tasks(vec![TaskInfo::new("t1", "running")]);
        t.set_open(true);
        assert!(!Component::render(&mut t, 80).is_empty());
        t.set_open(false);
        assert!(Component::render(&mut t, 80).is_empty());
    }
}
