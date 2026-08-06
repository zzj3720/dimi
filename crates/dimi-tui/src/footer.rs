//! `FooterComponent` — the two-line status bar
//! (port of `apps/dimi/src/tui/components/chrome/footer.ts`, slice 6 scope:
//! mode/model/tasks/cwd/git slots + transient hint + context line; tips
//! rotation and status_line.command land with the app-shell integration).

use crate::component::Component;
use crate::theme::{ColorToken, current_theme};
use crate::width::visible_width;
use crate::wrap::truncate_to_width;

/// The app state the footer renders.
#[derive(Debug, Clone, Default)]
pub struct FooterState {
    pub permission_mode: String, // "default" | "auto" | "yolo"
    pub plan_mode: bool,
    pub swarm_mode: bool,
    pub model: String,
    pub thinking_effort: String, // "off" | "on" | "high" | ...
    pub work_dir: String,
    pub git_branch: Option<String>,
    pub background_bash_task_count: usize,
    pub background_agent_count: usize,
    pub context_usage: usize,
    pub context_tokens: usize,
    pub max_context_tokens: usize,
    pub cache_hit_rate: Option<usize>, // percent
    pub transient_hint: Option<String>,
}

impl FooterState {
    pub fn new() -> Self {
        FooterState::default()
    }
}

/// Default status-line item order (mirrors `DEFAULT_STATUS_LINE_ITEMS`).
const DEFAULT_STATUS_LINE_ITEMS: &[&str] = &["mode", "model", "tasks", "cwd", "git"];

/// Shorten the cwd for display (mirrors `shortenCwd`): home → ~.
fn shorten_cwd(work_dir: &str) -> String {
    shorten_cwd_with_home(work_dir, std::env::var("HOME").ok().as_deref())
}

/// Home-injectable version for tests.
fn shorten_cwd_with_home(work_dir: &str, home: Option<&str>) -> String {
    if work_dir.is_empty() {
        return String::new();
    }
    if let Some(home) = home {
        if let Some(rest) = work_dir.strip_prefix(home) {
            if rest.is_empty() {
                return "~".to_owned();
            }
            return format!("~{rest}");
        }
    }
    work_dir.to_owned()
}

/// Format the context status (mirrors `formatContextStatus`).
fn format_context_status(
    usage: usize,
    tokens: usize,
    max_tokens: usize,
    cache_hit_rate: Option<usize>,
) -> String {
    let usage_percent = if max_tokens > 0 {
        (usage as f64 / max_tokens as f64 * 100.0).round() as usize
    } else {
        0
    };
    let mut s = format!("{usage_percent}% · {tokens}/{max_tokens} tok");
    if let Some(rate) = cache_hit_rate {
        s.push_str(&format!(" · CH {rate}%"));
    }
    s
}

/// Format the git badge (mirrors `formatFooterGitBadge`).
fn format_git_badge(branch: &str) -> String {
    current_theme().fg(ColorToken::TextMuted, &format!("⎇ {branch}"))
}

/// The two-line footer.
pub struct FooterComponent {
    pub state: FooterState,
}

impl FooterComponent {
    pub fn new(state: FooterState) -> Self {
        FooterComponent { state }
    }

    pub fn update(&mut self, state: FooterState) {
        self.state = state;
    }

    /// Build the slot pieces (mode / model / tasks / cwd / git / tips).
    fn build_slots(&self) -> Vec<(String, Vec<String>)> {
        let mut slots: Vec<(String, Vec<String>)> = Vec::new();
        let theme = current_theme();

        // mode
        let mut modes: Vec<String> = Vec::new();
        if self.state.permission_mode == "auto" {
            modes.push(theme.bold_fg(ColorToken::Warning, "auto"));
        }
        if self.state.permission_mode == "yolo" {
            modes.push(theme.bold_fg(ColorToken::Warning, "yolo"));
        }
        if self.state.plan_mode {
            modes.push(theme.bold_fg(ColorToken::Primary, "plan"));
        }
        if self.state.swarm_mode {
            modes.push(theme.bold_fg(ColorToken::Accent, "swarm"));
        }
        if !modes.is_empty() {
            slots.push(("mode".to_owned(), vec![modes.join(" ")]));
        }

        // model
        if !self.state.model.is_empty() {
            let effort = self.state.thinking_effort.as_str();
            let thinking_label = if effort != "off" {
                if effort != "on" && !effort.is_empty() {
                    format!(" thinking: {effort}")
                } else {
                    " thinking".to_owned()
                }
            } else {
                String::new()
            };
            let model_label = format!("{}{}", self.state.model, thinking_label);
            slots.push((
                "model".to_owned(),
                vec![theme.fg(ColorToken::Text, &model_label)],
            ));
        }

        // tasks
        let mut task_badges: Vec<String> = Vec::new();
        if self.state.background_bash_task_count > 0 {
            let noun = if self.state.background_bash_task_count == 1 {
                "task"
            } else {
                "tasks"
            };
            task_badges.push(theme.fg(
                ColorToken::Primary,
                &format!("[{} {noun} running]", self.state.background_bash_task_count),
            ));
        }
        if self.state.background_agent_count > 0 {
            let noun = if self.state.background_agent_count == 1 {
                "agent"
            } else {
                "agents"
            };
            task_badges.push(theme.fg(
                ColorToken::Primary,
                &format!("[{} {noun} running]", self.state.background_agent_count),
            ));
        }
        if !task_badges.is_empty() {
            slots.push(("tasks".to_owned(), task_badges));
        }

        // cwd
        let cwd = shorten_cwd(&self.state.work_dir);
        if !cwd.is_empty() {
            slots.push(("cwd".to_owned(), vec![theme.fg(ColorToken::TextDim, &cwd)]));
        }

        // git
        if let Some(branch) = &self.state.git_branch {
            slots.push(("git".to_owned(), vec![format_git_badge(branch)]));
        }

        slots
    }
}

impl Component for FooterComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let theme = current_theme();

        // ── Line 1: slots composed per status_line.items ──
        let slots = self.build_slots();
        let mut left: Vec<String> = Vec::new();
        for slot_name in DEFAULT_STATUS_LINE_ITEMS {
            if let Some((_, pieces)) = slots.iter().find(|(name, _)| name == slot_name) {
                left.extend(pieces.iter().cloned());
            }
        }
        let left_line = left.join("  ");
        let left_width = visible_width(&left_line);

        let line1 = if left_width <= width {
            left_line
        } else {
            truncate_to_width(&left_line, width, "…", false)
        };

        // ── Line 2: transient hint (bottom-left) + context (right) ──
        let context_text = format_context_status(
            self.state.context_usage,
            self.state.context_tokens,
            self.state.max_context_tokens,
            self.state.cache_hit_rate,
        );
        let context_width = visible_width(&context_text);
        let line2 = if let Some(hint) = &self.state.transient_hint {
            let max_hint_width = width.saturating_sub(context_width + 1);
            let shown_hint = if visible_width(hint) <= max_hint_width {
                hint.clone()
            } else {
                truncate_to_width(hint, max_hint_width, "…", false)
            };
            let hint_width = visible_width(&shown_hint);
            let pad = width.saturating_sub(hint_width + context_width);
            format!(
                "{}{}{}",
                theme.bold_fg(ColorToken::Warning, &shown_hint),
                " ".repeat(pad),
                theme.fg(ColorToken::Text, &context_text)
            )
        } else {
            let left_pad = width.saturating_sub(context_width);
            format!(
                "{}{}",
                " ".repeat(left_pad),
                theme.fg(ColorToken::Text, &context_text)
            )
        };

        vec![
            truncate_to_width(&line1, width, "…", false),
            truncate_to_width(&line2, width, "…", false),
        ]
    }

    fn invalidate(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK_COLORS, set_palette};

    fn state() -> FooterState {
        FooterState {
            permission_mode: "yolo".to_owned(),
            plan_mode: true,
            model: "claude-3-5-sonnet".to_owned(),
            work_dir: "/home/user/proj".to_owned(),
            git_branch: Some("main".to_owned()),
            context_usage: 50,
            context_tokens: 5000,
            max_context_tokens: 200000,
            cache_hit_rate: Some(88),
            ..Default::default()
        }
    }

    #[test]
    fn footer_renders_two_lines() {
        set_palette(DARK_COLORS);
        let mut f = FooterComponent::new(state());
        let lines = Component::render(&mut f, 80);
        assert_eq!(lines.len(), 2);
        let joined = lines.join("\n");
        assert!(joined.contains("yolo"));
        assert!(joined.contains("plan"));
        assert!(joined.contains("claude"));
        assert!(joined.contains("main"));
        assert!(joined.contains("CH 88%"));
        // cwd renders (either shortened or as-is).
        assert!(joined.contains("proj"), "cwd should appear: {joined}");
    }

    #[test]
    fn footer_shows_transient_hint_left() {
        set_palette(DARK_COLORS);
        let mut s = state();
        s.transient_hint = Some("Warning: config degraded".to_owned());
        let mut f = FooterComponent::new(s);
        let lines = Component::render(&mut f, 80);
        // Hint on line 2 (left), context on the right.
        assert!(
            lines[1].contains("Warning: config degraded"),
            "{}",
            lines[1]
        );
        assert!(lines[1].contains("CH 88%"), "{}", lines[1]);
    }

    #[test]
    fn footer_home_shortening() {
        assert_eq!(
            shorten_cwd_with_home("/home/user/proj", Some("/home/user")),
            "~/proj"
        );
        assert_eq!(shorten_cwd_with_home("/home/user", Some("/home/user")), "~");
        assert_eq!(shorten_cwd_with_home("/etc", Some("/home/user")), "/etc");
        assert_eq!(shorten_cwd_with_home("", Some("/home/user")), "");
    }

    #[test]
    fn footer_empty_state() {
        set_palette(DARK_COLORS);
        let mut f = FooterComponent::new(FooterState::default());
        let lines = Component::render(&mut f, 80);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].contains("0%"), "{}", lines[1]);
    }
}
