//! Session picker dialog — port of
//! `apps/dimi/src/tui/components/dialogs/session-picker.ts`
//! (`SessionPickerComponent`).

use unicode_segmentation::UnicodeSegmentation;

use crate::component::Component;
use crate::keys::matches_key;
use crate::searchable_list::SearchableList;
use crate::theme::{ColorToken, current_theme};
use crate::width::visible_width;
use crate::wrap::truncate_to_width;

use super::{CURRENT_MARK, SELECT_POINTER, single_line};

/// One session row (`SessionRow`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub id: String,
    pub title: Option<String>,
    pub last_prompt: Option<String>,
    pub work_dir: String,
    /// File-system mtime in milliseconds (same unit as `Date.now()`).
    pub updated_at: i64,
}

/// Action surfaced to the host via [`SessionPickerComponent::take_action`]
/// (mirrors the `onSelect` / `onCancel` / `onToggleScope` / `onCtrlC` /
/// `onCtrlD` callbacks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionPickerAction {
    Select(SessionRow),
    Cancel,
    ToggleScope(String),
    CtrlC,
    CtrlD,
}

/// `SessionPickerOptions`.
#[derive(Debug, Clone)]
pub struct SessionPickerOptions {
    pub sessions: Vec<SessionRow>,
    pub loading: bool,
    pub current_session_id: String,
    pub scope: SessionScope,
    pub initial_selected_session_id: Option<String>,
    pub page_size: Option<usize>,
    pub max_visible_sessions: Option<usize>,
    pub has_toggle_scope: bool,
    /// Home directory used to alias `~` paths (mirrors `process.env.HOME`).
    pub home: Option<String>,
    /// "Now" timestamp in ms (mirrors `Date.now()`); defaults to system time.
    pub now_ms: Option<i64>,
}

/// `'cwd' | 'all'` scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionScope {
    Cwd,
    All,
}

const ELLIPSIS: &str = "…";

/// Port of `formatRelativeTime`.
pub fn format_relative_time(ts: i64, now_ms: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    let diff_sec = (now_ms - ts).max(0) / 1000;
    if diff_sec < 60 {
        return "just now".to_owned();
    }
    let minutes = diff_sec / 60;
    if minutes < 60 {
        return format!("{minutes}m ago");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    let days = hours / 24;
    format!("{days}d ago")
}

/// Port of `homeAlias` — `~`-alias paths under the user's home directory.
pub fn home_aliased(path: &str, home: &str) -> String {
    if !home.is_empty() && path.starts_with(home) {
        return format!("~{}", &path[home.len()..]);
    }
    path.to_owned()
}

/// Truncates from the LEFT (keeps the tail), prefixing an ellipsis when
/// clipped (port of `truncatePathLeft`).
pub fn truncate_path_left(path: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if visible_width(path) <= max_width {
        return path.to_owned();
    }
    if max_width == 1 {
        return ELLIPSIS.to_owned();
    }
    let segments: Vec<&str> = path.graphemes(true).collect();
    let mut used = 0usize;
    let budget = max_width - 1; // reserve 1 column for ellipsis
    let mut i = segments.len() as isize - 1;
    while i >= 0 {
        let seg = segments[i as usize];
        let w = visible_width(seg);
        if used + w > budget {
            break;
        }
        used += w;
        i -= 1;
    }
    let tail: String = segments[(i + 1) as usize..].concat();
    format!("{ELLIPSIS}{tail}")
}

fn session_search_text(session: &SessionRow) -> String {
    let base = session.title.as_deref().unwrap_or(&session.id).trim();
    if base.is_empty() {
        session.id.clone()
    } else {
        single_line(base)
    }
}

/// `SessionPickerComponent`.
pub struct SessionPickerComponent {
    sessions: Vec<SessionRow>,
    loading: bool,
    current_session_id: String,
    scope: SessionScope,
    has_toggle_scope: bool,
    max_visible_sessions: usize,
    page_size: usize,
    visible_count: usize,
    list: SearchableList<SessionRow>,
    home: String,
    now_ms: i64,
    action: Option<SessionPickerAction>,
}

impl SessionPickerComponent {
    pub fn new(opts: SessionPickerOptions) -> Self {
        let page_size = opts.page_size.unwrap_or(50).max(1);
        let initial_index = match &opts.initial_selected_session_id {
            Some(id) => opts.sessions.iter().position(|s| &s.id == id).unwrap_or(0),
            None => 0,
        };
        let list = SearchableList::new(
            opts.sessions.clone(),
            session_search_text,
            Some(page_size),
            Some(initial_index),
            true,
        );
        let initial_loaded_pages = (initial_index + 1).div_ceil(page_size);
        let visible_count = opts.sessions.len().min(initial_loaded_pages * page_size);
        let home = opts
            .home
            .clone()
            .unwrap_or_else(|| std::env::var("HOME").unwrap_or_default());
        let now_ms = opts.now_ms.unwrap_or_else(now_ms);
        SessionPickerComponent {
            sessions: opts.sessions,
            loading: opts.loading,
            current_session_id: opts.current_session_id,
            scope: opts.scope,
            has_toggle_scope: opts.has_toggle_scope,
            max_visible_sessions: opts.max_visible_sessions.unwrap_or(4),
            page_size,
            visible_count,
            list,
            home,
            now_ms,
            action: None,
        }
    }

    /// Host polls after `handle_input`.
    pub fn take_action(&mut self) -> Option<SessionPickerAction> {
        self.action.take()
    }

    fn filtered_sessions(&self) -> Vec<SessionRow> {
        self.list.view().items
    }

    fn loaded_sessions(&self, sessions: &[SessionRow]) -> Vec<SessionRow> {
        let count = sessions.len().min(self.visible_count);
        sessions[..count].to_vec()
    }

    fn sync_visible_count(&mut self, previous_query: &str) {
        let view = self.list.view();
        if view.query != previous_query {
            self.visible_count = view.items.len().min(self.page_size);
            return;
        }
        let loaded_count = view.items.len().min(self.visible_count);
        if view.selected_index + 1 >= loaded_count && loaded_count < view.items.len() {
            self.visible_count = view.items.len().min(self.visible_count + self.page_size);
        }
    }

    fn render_session_card(
        &self,
        width: usize,
        session: &SessionRow,
        is_selected: bool,
        is_current: bool,
    ) -> Vec<String> {
        let theme = current_theme();
        let pointer = if is_selected { SELECT_POINTER } else { " " };
        let indent = "  ";
        let indent_width = visible_width(indent);
        let title_color = if is_selected {
            ColorToken::Primary
        } else {
            ColorToken::Text
        };
        let title_style = |text: &str| -> String {
            if is_selected {
                theme.bold_fg(title_color, text)
            } else {
                theme.fg(title_color, text)
            }
        };

        let time = format_relative_time(session.updated_at, self.now_ms);
        let badge = if is_current { CURRENT_MARK } else { "" };
        let raw_title = {
            let t = session.title.as_deref().unwrap_or(&session.id).trim();
            if t.is_empty() {
                session.id.clone()
            } else {
                t.to_owned()
            }
        };

        let mut trailing_parts: Vec<&str> = Vec::new();
        if !time.is_empty() {
            trailing_parts.push(&time);
        }
        if !badge.is_empty() {
            trailing_parts.push(badge);
        }
        let trailing_text = if trailing_parts.is_empty() {
            String::new()
        } else {
            format!("  {}", trailing_parts.join("  "))
        };
        let trailing_width = visible_width(&trailing_text);
        let header_prefix_width = visible_width(pointer) + 1; // pointer + space
        let title_budget = 8usize.max(
            width
                .saturating_sub(header_prefix_width)
                .saturating_sub(trailing_width),
        );
        let shown_title =
            truncate_to_width(&single_line(&raw_title), title_budget, ELLIPSIS, false);

        let mut header = theme.fg(
            if is_selected {
                ColorToken::Primary
            } else {
                ColorToken::TextDim
            },
            &format!("{pointer} "),
        );
        header.push_str(&title_style(&shown_title));
        if !time.is_empty() {
            header.push_str(&format!("  {}", theme.fg(ColorToken::TextDim, &time)));
        }
        if !badge.is_empty() {
            header.push_str(&format!("  {}", theme.fg(ColorToken::Success, badge)));
        }
        let mut card: Vec<String> = vec![header];

        let full_id = &session.id;
        let id_width = visible_width(full_id);
        let meta_gap = "   ";
        let meta_gap_width = visible_width(meta_gap);
        let id_line_width = indent_width + id_width;
        let aliased_dir = home_aliased(&session.work_dir, &self.home);
        let dir_width = visible_width(&aliased_dir);

        if id_line_width + meta_gap_width + dir_width <= width {
            card.push(format!(
                "{indent}{}{}{}",
                theme.fg(ColorToken::TextMuted, full_id),
                theme.fg(ColorToken::TextDim, meta_gap),
                theme.fg(ColorToken::TextMuted, &aliased_dir)
            ));
        } else {
            card.push(format!(
                "{indent}{}",
                theme.fg(
                    ColorToken::TextMuted,
                    &truncate_to_width(
                        full_id,
                        id_width.max(width.saturating_sub(indent_width)),
                        ELLIPSIS,
                        false
                    )
                )
            ));
            let dir_budget = 8usize.max(width.saturating_sub(indent_width));
            let dir = truncate_path_left(&aliased_dir, dir_budget);
            card.push(format!("{indent}{}", theme.fg(ColorToken::TextMuted, &dir)));
        }

        let raw_prompt = session.last_prompt.as_deref().map(str::trim);
        if let Some(raw_prompt) = raw_prompt {
            if !raw_prompt.is_empty() {
                let prompt_marker = "› ";
                let prompt_marker_width = visible_width(prompt_marker);
                let prompt_budget = 8usize.max(
                    width
                        .saturating_sub(indent_width)
                        .saturating_sub(prompt_marker_width),
                );
                let prompt_text =
                    truncate_to_width(&single_line(raw_prompt), prompt_budget, ELLIPSIS, false);
                card.push(format!(
                    "{indent}{}",
                    theme.fg(
                        ColorToken::TextDim,
                        &format!("{prompt_marker}{prompt_text}")
                    )
                ));
            }
        }

        card
    }

    /// Port of `renderLines`; `render()` applies the final width clamp.
    fn render_lines(&self, width: usize) -> Vec<String> {
        let theme = current_theme();
        let mut lines: Vec<String> = vec![theme.fg(ColorToken::Primary, &"─".repeat(width))];
        let title = if self.scope == SessionScope::All {
            "All sessions"
        } else {
            "Sessions"
        };
        let scope_hint = if !self.has_toggle_scope {
            None
        } else if self.scope == SessionScope::All {
            Some("Ctrl+A current cwd")
        } else {
            Some("Ctrl+A all")
        };

        if self.loading {
            lines.push(theme.bold_fg(
                ColorToken::Primary,
                &truncate_to_width(title, width, ELLIPSIS, false),
            ));
            lines.push(theme.fg(
                ColorToken::TextMuted,
                &truncate_to_width("Loading sessions...", width, ELLIPSIS, false),
            ));
            lines.push(theme.fg(ColorToken::Primary, &"─".repeat(width)));
            return lines;
        }

        if self.sessions.is_empty() {
            let mut hint_parts: Vec<&str> = Vec::new();
            if let Some(h) = scope_hint {
                hint_parts.push(h);
            }
            hint_parts.push("Esc cancel");
            lines.push(theme.bold_fg(
                ColorToken::Primary,
                &truncate_to_width(title, width, ELLIPSIS, false),
            ));
            lines.push(theme.fg(
                ColorToken::TextMuted,
                &truncate_to_width(&hint_parts.join(" · "), width, ELLIPSIS, false),
            ));
            lines.push(String::new());
            lines.push(theme.fg(
                ColorToken::TextMuted,
                &truncate_to_width("No sessions found.", width, ELLIPSIS, false),
            ));
            lines.push(theme.fg(ColorToken::Primary, &"─".repeat(width)));
            return lines;
        }

        let view = self.list.view();
        let title_suffix = if view.query.is_empty() {
            theme.fg(ColorToken::TextMuted, "  (type to search)")
        } else {
            String::new()
        };
        let mut hint_parts: Vec<&str> = Vec::new();
        if !view.query.is_empty() {
            hint_parts.push("Backspace clear");
        }
        hint_parts.push("↑↓ navigate");
        if let Some(h) = scope_hint {
            hint_parts.push(h);
        }
        hint_parts.push("Enter select");
        hint_parts.push("Esc cancel");

        lines.push(format!(
            "{}{}",
            theme.bold_fg(ColorToken::Primary, title),
            title_suffix
        ));
        lines.push(theme.fg(ColorToken::TextMuted, &hint_parts.join(" · ")));
        lines.push(String::new());

        if !view.query.is_empty() {
            lines.push(format!(
                "{}{}",
                theme.fg(ColorToken::Primary, "Search: "),
                theme.fg(ColorToken::Text, &view.query)
            ));
        }

        let loaded_sessions = self.loaded_sessions(&view.items);
        if loaded_sessions.is_empty() {
            lines.push(theme.fg(
                ColorToken::TextMuted,
                &truncate_to_width("No matches", width, ELLIPSIS, false),
            ));
            lines.push(theme.fg(ColorToken::Primary, &"─".repeat(width)));
            return lines;
        }
        let selected_index = view.selected_index;
        let visible_start = selected_index
            .saturating_sub(self.max_visible_sessions / 2)
            .min(
                loaded_sessions
                    .len()
                    .saturating_sub(self.max_visible_sessions),
            );
        let visible_end = (visible_start + self.max_visible_sessions).min(loaded_sessions.len());
        let visible_sessions = &loaded_sessions[visible_start..visible_end];

        for (vi, session) in visible_sessions.iter().enumerate() {
            let index = visible_start + vi;
            let is_selected = index == selected_index;
            let is_current = session.id == self.current_session_id;
            let card = self.render_session_card(width, session, is_selected, is_current);
            lines.extend(card);
            if vi < visible_sessions.len() - 1 {
                lines.push(String::new());
            }
        }

        let filtered_count = view.items.len();
        if loaded_sessions.len() > visible_sessions.len() || !view.query.is_empty() {
            lines.push(String::new());
            let total_suffix = if !view.query.is_empty() {
                format!(
                    "{} loaded / {filtered_count} matches",
                    loaded_sessions.len()
                )
            } else if loaded_sessions.len() == self.sessions.len() {
                format!("{} sessions", loaded_sessions.len())
            } else {
                format!(
                    "{} loaded / {} sessions",
                    loaded_sessions.len(),
                    self.sessions.len()
                )
            };
            let footer = format!(
                "Showing {}-{} of {total_suffix}",
                visible_start + 1,
                visible_start + visible_sessions.len()
            );
            lines.push(theme.fg(
                ColorToken::TextMuted,
                &truncate_to_width(&footer, width, ELLIPSIS, false),
            ));
        }

        lines.push(theme.fg(ColorToken::Primary, &"─".repeat(width)));
        lines
    }
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Component for SessionPickerComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        self.render_lines(width)
            .iter()
            .map(|line| truncate_to_width(line, width, ELLIPSIS, false))
            .collect()
    }

    fn handle_input(&mut self, data: &str) {
        if matches_key(data, "ctrl+c") {
            self.action = Some(SessionPickerAction::CtrlC);
            return;
        }
        if matches_key(data, "ctrl+d") {
            self.action = Some(SessionPickerAction::CtrlD);
            return;
        }
        if matches_key(data, "ctrl+a") {
            let selected_id = self
                .list
                .selected()
                .map(|s| s.id)
                .unwrap_or_else(|| self.current_session_id.clone());
            if self.has_toggle_scope {
                self.action = Some(SessionPickerAction::ToggleScope(selected_id));
            }
            return;
        }
        if matches_key(data, "escape") {
            if self.list.clear_query() {
                self.visible_count = self.filtered_sessions().len().min(self.page_size);
                return;
            }
            self.action = Some(SessionPickerAction::Cancel);
            return;
        }
        if matches_key(data, "enter") {
            if let Some(session) = self.list.selected() {
                self.action = Some(SessionPickerAction::Select(session));
            }
            return;
        }

        let previous_query = self.list.view().query;
        if self.list.handle_key(data) {
            self.sync_visible_count(&previous_query);
        }
    }

    fn invalidate(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, title: Option<&str>, work_dir: &str, updated_at: i64) -> SessionRow {
        SessionRow {
            id: id.to_owned(),
            title: title.map(str::to_owned),
            last_prompt: None,
            work_dir: work_dir.to_owned(),
            updated_at,
        }
    }

    fn opts() -> SessionPickerOptions {
        let now = 1_700_000_000_000i64;
        SessionPickerOptions {
            sessions: vec![
                row(
                    "session_aaa",
                    Some("Add auth"),
                    "/home/user/proj",
                    now - 5_000,
                ),
                row(
                    "session_bbb",
                    Some("Fix TUI"),
                    "/home/user/other",
                    now - 2 * 3_600_000,
                ),
                row(
                    "session_ccc",
                    None,
                    "/home/user/proj/deep",
                    now - 3 * 86_400_000,
                ),
            ],
            loading: false,
            current_session_id: "session_bbb".to_owned(),
            scope: SessionScope::Cwd,
            initial_selected_session_id: None,
            page_size: None,
            max_visible_sessions: None,
            has_toggle_scope: true,
            home: Some("/home/user".to_owned()),
            now_ms: Some(now),
        }
    }

    #[test]
    fn relative_time_formats() {
        let now = 1_700_000_000_000i64;
        assert_eq!(format_relative_time(now - 5_000, now), "just now");
        assert_eq!(format_relative_time(now - 120_000, now), "2m ago");
        assert_eq!(format_relative_time(now - 7_200_000, now), "2h ago");
        assert_eq!(format_relative_time(now - 3 * 86_400_000, now), "3d ago");
        assert_eq!(format_relative_time(0, now), "");
    }

    #[test]
    fn home_alias_and_left_truncation() {
        assert_eq!(home_aliased("/home/user/proj", "/home/user"), "~/proj");
        assert_eq!(home_aliased("/etc/hosts", "/home/user"), "/etc/hosts");
        // Left truncation keeps the longest tail that fits (budget = max-1).
        let t = truncate_path_left("/home/user/very/long/path", 14);
        assert_eq!(t, "…ery/long/path");
        assert!(visible_width(&t) <= 14);
        // Fits → unchanged.
        assert_eq!(truncate_path_left("/tmp", 80), "/tmp");
    }

    #[test]
    fn renders_header_and_session_cards() {
        let mut c = SessionPickerComponent::new(opts());
        let lines = c.render(100);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("Sessions"), "{joined}");
        assert!(joined.contains("(type to search)"), "{joined}");
        assert!(joined.contains("Add auth"), "{joined}");
        assert!(joined.contains("just now"), "{joined}");
        assert!(joined.contains("session_bbb"), "{joined}");
        assert!(joined.contains("← current"), "{joined}");
        assert!(joined.contains("Ctrl+A all"), "{joined}");
    }

    #[test]
    fn enter_selects_and_escape_cancels() {
        let mut c = SessionPickerComponent::new(opts());
        c.handle_input("\r");
        match c.take_action() {
            Some(SessionPickerAction::Select(s)) => assert_eq!(s.id, "session_aaa"),
            other => panic!("expected select, got {other:?}"),
        }
        c.handle_input("\x1b");
        assert_eq!(c.take_action(), Some(SessionPickerAction::Cancel));
    }

    #[test]
    fn ctrl_a_toggles_scope() {
        let mut c = SessionPickerComponent::new(opts());
        c.handle_input("\x01"); // ctrl+a
        match c.take_action() {
            Some(SessionPickerAction::ToggleScope(id)) => assert_eq!(id, "session_aaa"),
            other => panic!("expected toggle scope, got {other:?}"),
        }
    }

    #[test]
    fn search_filters_and_esc_clears_query_first() {
        let mut c = SessionPickerComponent::new(opts());
        for ch in "auth".chars() {
            c.handle_input(&ch.to_string());
        }
        let lines = c.render(100);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("Search: auth"), "{joined}");
        assert!(joined.contains("1 loaded / 1 matches"), "{joined}");
        // Esc clears query first.
        c.handle_input("\x1b");
        assert!(c.take_action().is_none());
        c.handle_input("\x1b");
        assert_eq!(c.take_action(), Some(SessionPickerAction::Cancel));
    }
}
