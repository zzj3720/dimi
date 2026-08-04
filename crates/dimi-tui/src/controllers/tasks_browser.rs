//! Tasks browser controller — the state-machine part of
//! `apps/dimi/src/tui/controllers/tasks-browser.ts`.
//!
//! Owns the task list state (filter all/active, selection, tail output,
//! flash messages, output viewer) and the pure transitions
//! (`handleSelect`, `handleToggleFilter`, `handleStop`, `loadTail`,
//! `handleOpenOutput`, …). The `TasksBrowserApp` component, the `/tasks`
//! polling timers, and the async `session.listBackgroundTasks` /
//! `stopBackgroundTask` / `getBackgroundTaskOutput` calls are
//! `// TODO(legacy)`.

use crate::controllers::{BackgroundTaskInfo, BackgroundTaskKind, BackgroundTaskStatus};

/// `TasksFilter` — the `all` / `active` toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TasksFilter {
    All,
    Active,
}

impl TasksFilter {
    pub fn toggled(&self) -> TasksFilter {
        match self {
            TasksFilter::All => TasksFilter::Active,
            TasksFilter::Active => TasksFilter::All,
        }
    }
}

/// Output-viewer sub-state (`TasksBrowserState.viewer`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskViewerState {
    pub task_id: String,
    pub output: String,
    pub refresh_id: u64,
}

/// The tasks-browser state (`TasksBrowserState`), minus the component/timers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TasksBrowserState {
    pub open: bool,
    pub filter: TasksFilter,
    pub selected_task_id: Option<String>,
    pub tail_output: Option<String>,
    pub tail_loading: bool,
    pub tail_request_id: u64,
    pub flash_message: Option<String>,
    pub viewer: Option<TaskViewerState>,
}

impl Default for TasksBrowserState {
    fn default() -> Self {
        TasksBrowserState {
            open: false,
            filter: TasksFilter::All,
            selected_task_id: None,
            tail_output: None,
            tail_loading: false,
            tail_request_id: 0,
            flash_message: None,
            viewer: None,
        }
    }
}

/// Result of [`TasksBrowserController::show`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShowOutcome {
    /// The browser is already open.
    AlreadyOpen,
    /// No active session to list tasks from.
    NoSession,
    /// Opened; the selected task (if any) has a tail load pending.
    Opened { selected: Option<String> },
}

/// The tasks browser controller (port of `TasksBrowserController`).
#[derive(Debug, Default)]
pub struct TasksBrowserController {
    pub state: TasksBrowserState,
}

impl TasksBrowserController {
    pub fn new() -> Self {
        TasksBrowserController::default()
    }

    /// `pickInitialSelection` — prefer a `running` task, else the first
    /// candidate (filtered to active when the filter is `active`).
    pub fn pick_initial_selection(
        &self,
        tasks: &[BackgroundTaskInfo],
        filter: TasksFilter,
    ) -> Option<String> {
        let candidates: Vec<&BackgroundTaskInfo> = match filter {
            TasksFilter::All => tasks.iter().collect(),
            TasksFilter::Active => tasks.iter().filter(|t| !t.status.is_terminal()).collect(),
        };
        if candidates.is_empty() {
            return None;
        }
        candidates
            .iter()
            .find(|t| t.status == BackgroundTaskStatus::Running)
            .map(|t| t.task_id.clone())
            .or_else(|| Some(candidates[0].task_id.clone()))
    }

    /// `show`.
    pub fn show(&mut self, tasks: &[BackgroundTaskInfo], has_session: bool) -> ShowOutcome {
        if self.state.open {
            return ShowOutcome::AlreadyOpen;
        }
        if !has_session {
            // TODO(legacy): host.showError('No active session.')
            self.state.flash_message = Some("No active session.".to_owned());
            return ShowOutcome::NoSession;
        }
        // TODO(legacy): async session.listBackgroundTasks({ activeOnly: false })
        let filter = TasksFilter::All;
        let selected = self.pick_initial_selection(tasks, filter);
        self.state.open = true;
        self.state.filter = filter;
        self.state.selected_task_id = selected.clone();
        self.state.tail_output = None;
        self.state.tail_loading = false;
        self.state.tail_request_id = 0;
        self.state.flash_message = None;
        self.state.viewer = None;
        // TODO(legacy): create TasksBrowserApp + mount; start 1s poll timer
        if let Some(selected) = &selected {
            self.load_tail(selected, has_session);
        }
        ShowOutcome::Opened { selected }
    }

    /// `close`.
    pub fn close(&mut self) {
        if !self.state.open {
            return;
        }
        if self.state.viewer.is_some() {
            self.close_output_viewer();
        }
        // TODO(legacy): clear pollTimer / flashTimer; restore saved children
        self.state.open = false;
        self.state.viewer = None;
    }

    /// `repaint` — the prop push to `TasksBrowserApp` is legacy; the state is
    /// already up to date.
    pub fn repaint(&mut self) {
        // TODO(legacy): browser.component.setProps({ tasks, filter, … })
    }

    /// `handleSelect`.
    pub fn handle_select(&mut self, task_id: &str) {
        if !self.state.open {
            return;
        }
        if self.state.selected_task_id.as_deref() == Some(task_id) {
            return;
        }
        self.state.selected_task_id = Some(task_id.to_owned());
        self.state.tail_output = None;
        self.state.tail_loading = true;
        self.repaint();
        self.load_tail(task_id, true);
    }

    /// `handleToggleFilter`.
    pub fn handle_toggle_filter(&mut self) {
        if !self.state.open {
            return;
        }
        self.state.filter = self.state.filter.toggled();
        self.repaint();
    }

    /// `handleRefresh` — flash "Refreshing…" and kick a (legacy) refresh.
    pub fn handle_refresh(&mut self) {
        if !self.state.open {
            return;
        }
        self.flash("Refreshing…", 600);
        // TODO(legacy): void this.refresh() (async listBackgroundTasks → pushProps)
    }

    /// `handleStop` — flash the stopping state; the actual
    /// `session.stopBackgroundTask` is legacy. Success/failure land via
    /// [`TasksBrowserController::handle_stop_result`].
    pub fn handle_stop(&mut self, task_id: &str, has_session: bool) {
        if !self.state.open {
            return;
        }
        if !has_session {
            self.flash("No active session.", 2500);
            return;
        }
        self.flash(&format!("Stopping {task_id}…"), 1500);
        // TODO(legacy): async session.stopBackgroundTask(taskId) → refresh
    }

    /// Surface a stop failure (`Stop failed: …`).
    pub fn handle_stop_failure(&mut self, message: &str) {
        if !self.state.open {
            return;
        }
        self.flash(&format!("Stop failed: {message}"), 2500);
    }

    /// `handleOpenOutput` — the fetch is legacy; the host reports the output
    /// via [`TasksBrowserController::open_output_result`].
    pub fn handle_open_output(&mut self, task_id: &str, has_session: bool) {
        if !self.state.open {
            return;
        }
        if self.state.viewer.is_some() {
            return;
        }
        if !has_session {
            self.flash("No active session.", 2500);
            return;
        }
        // TODO(legacy): async session.getBackgroundTaskOutput(taskId)
        let _ = task_id;
    }

    /// Complete `handleOpenOutput` with the fetched output (or an error).
    pub fn open_output_result(&mut self, task_id: &str, output: Result<String, String>) {
        if !self.state.open {
            return;
        }
        match output {
            Ok(output) => {
                self.state.viewer = Some(TaskViewerState {
                    task_id: task_id.to_owned(),
                    output,
                    refresh_id: 0,
                });
                // TODO(legacy): create TaskOutputViewer + mount; start 1s poll
            }
            Err(message) => {
                self.flash(&format!("Cannot open output: {message}"), 2500);
            }
        }
    }

    /// `loadTail` — bumps the request id; the fetch is legacy.
    pub fn load_tail(&mut self, task_id: &str, has_session: bool) {
        if !self.state.open {
            return;
        }
        if !has_session {
            self.state.tail_loading = false;
            self.repaint();
            return;
        }
        self.state.tail_request_id += 1;
        // TODO(legacy): async session.getBackgroundTaskOutput(taskId, { tail: 4000 })
        let _ = task_id;
    }

    /// Settle a tail fetch. `output` is `None` on error (→ empty output).
    pub fn load_tail_result(&mut self, task_id: &str, request_id: u64, output: Option<String>) {
        if !self.state.open {
            return;
        }
        if self.state.tail_request_id != request_id {
            return;
        }
        if self.state.selected_task_id.as_deref() != Some(task_id) {
            return;
        }
        self.state.tail_output = Some(output.unwrap_or_default());
        self.state.tail_loading = false;
        self.repaint();
    }

    /// `flash`.
    pub fn flash(&mut self, message: &str, duration_ms: u64) {
        if !self.state.open {
            return;
        }
        // TODO(legacy): clearTimeout(flashTimer)
        self.state.flash_message = Some(message.to_owned());
        // TODO(legacy): flashTimer = setTimeout(() => { clearFlash() }, duration_ms)
        let _ = duration_ms;
    }

    /// The timer-expiry half of `flash` — clears the message.
    pub fn clear_flash(&mut self) {
        if !self.state.open {
            return;
        }
        self.state.flash_message = None;
        self.repaint();
    }

    /// `closeOutputViewer`.
    pub fn close_output_viewer(&mut self) {
        if self.state.viewer.is_none() {
            return;
        }
        // TODO(legacy): clearInterval(viewer.pollTimer); restore saved children
        self.state.viewer = None;
    }
}

/// `shouldShowBackgroundTaskTranscript`-independent terminal check helper.
pub fn is_terminal_status(status: BackgroundTaskStatus) -> bool {
    status.is_terminal()
}

/// Group helper used by the browser: count active (non-terminal) tasks.
pub fn count_active_tasks(tasks: &[BackgroundTaskInfo]) -> (usize, usize) {
    let mut bash = 0usize;
    let mut agents = 0usize;
    for task in tasks {
        if task.status.is_terminal() {
            continue;
        }
        if task.kind == BackgroundTaskKind::Agent {
            agents += 1;
        } else {
            bash += 1;
        }
    }
    (bash, agents)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, status: BackgroundTaskStatus) -> BackgroundTaskInfo {
        BackgroundTaskInfo::new(id, BackgroundTaskKind::Process, status)
    }

    #[test]
    fn pick_initial_selection_prefers_running() {
        let c = TasksBrowserController::new();
        let tasks = vec![
            task("a", BackgroundTaskStatus::Completed),
            task("b", BackgroundTaskStatus::Running),
            task("c", BackgroundTaskStatus::Failed),
        ];
        assert_eq!(
            c.pick_initial_selection(&tasks, TasksFilter::All)
                .as_deref(),
            Some("b")
        );
        // Active filter drops terminal tasks.
        assert_eq!(
            c.pick_initial_selection(&tasks, TasksFilter::Active)
                .as_deref(),
            Some("b")
        );
    }

    #[test]
    fn pick_initial_selection_falls_back_to_first() {
        let c = TasksBrowserController::new();
        let tasks = vec![
            task("a", BackgroundTaskStatus::Running),
            task("z", BackgroundTaskStatus::Completed),
        ];
        assert_eq!(
            c.pick_initial_selection(&tasks, TasksFilter::All)
                .as_deref(),
            Some("a")
        );
        assert_eq!(c.pick_initial_selection(&[], TasksFilter::All), None);
    }

    #[test]
    fn show_requires_session_and_opens() {
        let mut c = TasksBrowserController::new();
        let tasks = vec![task("a", BackgroundTaskStatus::Running)];
        assert_eq!(c.show(&tasks, false), ShowOutcome::NoSession);
        assert!(!c.state.open);

        assert_eq!(
            c.show(&tasks, true),
            ShowOutcome::Opened {
                selected: Some("a".to_owned())
            }
        );
        assert!(c.state.open);
        // loadTail bumped the request id but leaves tail_loading false until
        // the async fetch settles.
        assert!(!c.state.tail_loading);
        assert_eq!(c.state.tail_request_id, 1);

        // Already open.
        assert_eq!(c.show(&tasks, true), ShowOutcome::AlreadyOpen);
    }

    #[test]
    fn select_toggles_tail_state() {
        let mut c = TasksBrowserController::new();
        c.show(&[task("a", BackgroundTaskStatus::Running)], true);
        c.state.tail_loading = false;

        c.handle_select("a");
        // Same selection → no change.
        assert!(!c.state.tail_loading);

        c.handle_select("b");
        assert_eq!(c.state.selected_task_id.as_deref(), Some("b"));
        assert!(c.state.tail_loading);
        assert!(c.state.tail_output.is_none());
    }

    #[test]
    fn filter_toggle_switches_all_active() {
        let mut c = TasksBrowserController::new();
        c.show(&[], true);
        assert_eq!(c.state.filter, TasksFilter::All);
        c.handle_toggle_filter();
        assert_eq!(c.state.filter, TasksFilter::Active);
        c.handle_toggle_filter();
        assert_eq!(c.state.filter, TasksFilter::All);
    }

    #[test]
    fn tail_request_id_guards_stale_results() {
        let mut c = TasksBrowserController::new();
        c.show(&[task("a", BackgroundTaskStatus::Running)], true);
        let req = c.state.tail_request_id;
        // A stale (older) request settles as a no-op.
        c.load_tail_result("a", req.saturating_sub(1), Some("stale".to_owned()));
        assert!(!c.state.tail_loading); // request already consumed by show? see below

        // Fresh request lands.
        c.load_tail("a", true);
        let req = c.state.tail_request_id;
        c.load_tail_result("a", req, Some("fresh output".to_owned()));
        assert_eq!(c.state.tail_output.as_deref(), Some("fresh output"));
        assert!(!c.state.tail_loading);
    }

    #[test]
    fn stop_and_stop_failure_flash() {
        let mut c = TasksBrowserController::new();
        c.show(&[], true);
        c.handle_stop("t1", true);
        assert_eq!(c.state.flash_message.as_deref(), Some("Stopping t1…"));
        c.handle_stop("t1", false);
        assert_eq!(c.state.flash_message.as_deref(), Some("No active session."));
        c.handle_stop_failure("boom");
        assert_eq!(c.state.flash_message.as_deref(), Some("Stop failed: boom"));
        c.clear_flash();
        assert!(c.state.flash_message.is_none());
    }

    #[test]
    fn open_output_viewer_and_close() {
        let mut c = TasksBrowserController::new();
        c.show(&[], true);
        c.open_output_result("t1", Ok("out".to_owned()));
        let viewer = c.state.viewer.as_ref().unwrap();
        assert_eq!(viewer.task_id, "t1");
        assert_eq!(viewer.output, "out");

        // Can't open a second viewer while one is open.
        c.handle_open_output("t2", true);
        assert_eq!(c.state.viewer.as_ref().unwrap().task_id, "t1");

        c.close_output_viewer();
        assert!(c.state.viewer.is_none());
    }

    #[test]
    fn open_output_error_flashes() {
        let mut c = TasksBrowserController::new();
        c.show(&[], true);
        c.open_output_result("t1", Err("denied".to_owned()));
        assert!(c.state.viewer.is_none());
        assert_eq!(
            c.state.flash_message.as_deref(),
            Some("Cannot open output: denied")
        );
    }

    #[test]
    fn count_active_tasks_ignores_terminal() {
        let tasks = vec![
            task("a", BackgroundTaskStatus::Running),
            task("b", BackgroundTaskStatus::Completed),
        ];
        assert_eq!(count_active_tasks(&tasks), (1, 0));
    }
}
