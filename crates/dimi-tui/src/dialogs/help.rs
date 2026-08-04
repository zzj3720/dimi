//! Modal `/help` panel — port of
//! `apps/dimi/src/tui/components/dialogs/help-panel.ts`
//! (`HelpPanelComponent`).
//!
//! Lists keyboard shortcuts and slash commands (with aliases + descriptions)
//! in colour-coded sections; scrolls with ↑/↓/PgUp/PgDn and closes on
//! Esc / Enter / q.

use crate::component::Component;
use crate::keys::{decode_kitty_printable, matches_key};
use crate::theme::{ColorToken, current_theme};
use crate::wrap::truncate_to_width;

/// One keyboard shortcut row.
#[derive(Debug, Clone)]
pub struct KeyboardShortcut {
    pub keys: String,
    pub description: String,
}

/// One slash command row.
#[derive(Debug, Clone)]
pub struct HelpPanelCommand {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
}

/// `DEFAULT_KEYBOARD_SHORTCUTS` — static list, kept in sync with the global
/// editor bindings.
pub const DEFAULT_KEYBOARD_SHORTCUTS: [(&str, &str); 11] = [
    ("Shift-Tab", "Toggle plan mode"),
    ("Ctrl-G", "Edit in external editor ($VISUAL / $EDITOR)"),
    ("Ctrl-O", "Cycle tool summary / tool cards / full output"),
    ("Ctrl-T", "Expand / collapse the todo list (when truncated)"),
    ("Ctrl-S", "Steer — inject a follow-up during streaming"),
    ("Shift-Enter / Ctrl-J", "Insert newline"),
    ("Ctrl-C", "Interrupt stream / clear input"),
    ("Ctrl-D", "Exit (on empty input)"),
    ("Esc", "Close dialogs / interrupt streaming"),
    ("↑ / ↓", "Browse input history"),
    (
        "Enter",
        "Submit (steers mid-turn by default; Settings → Busy input can make it queue)",
    ),
];

/// Action surfaced via [`HelpPanelComponent::take_action`] (mirrors
/// `onClose`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpPanelAction {
    Close,
}

/// `HelpPanelOptions`.
#[derive(Debug, Clone)]
pub struct HelpPanelOptions {
    pub commands: Vec<HelpPanelCommand>,
    pub shortcuts: Option<Vec<KeyboardShortcut>>,
    /// Terminal height — used to decide whether to show the hint tail.
    pub max_visible: Option<usize>,
}

/// `HelpPanelComponent`.
pub struct HelpPanelComponent {
    opts: HelpPanelOptions,
    scroll_top: usize,
    action: Option<HelpPanelAction>,
}

impl HelpPanelComponent {
    pub fn new(opts: HelpPanelOptions) -> Self {
        HelpPanelComponent {
            opts,
            scroll_top: 0,
            action: None,
        }
    }

    /// Host polls after `handle_input`.
    pub fn take_action(&mut self) -> Option<HelpPanelAction> {
        self.action.take()
    }

    fn get_slash_display_group(name: &str) -> usize {
        if name.starts_with("skill:") { 1 } else { 0 }
    }
}

impl Component for HelpPanelComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let theme = current_theme();
        let accent = |s: &str| theme.fg(ColorToken::Primary, s);
        let dim = |s: &str| theme.fg(ColorToken::TextDim, s);
        let muted = |s: &str| theme.fg(ColorToken::TextMuted, s);
        let kbd_color = |s: &str| theme.fg(ColorToken::Warning, s);
        let slash_color = |s: &str| theme.fg(ColorToken::Primary, s);

        let shortcuts = self.opts.shortcuts.clone().unwrap_or_else(|| {
            DEFAULT_KEYBOARD_SHORTCUTS
                .iter()
                .map(|(keys, description)| KeyboardShortcut {
                    keys: (*keys).to_owned(),
                    description: (*description).to_owned(),
                })
                .collect()
        });
        let kbd_width = shortcuts
            .iter()
            .map(|s| s.keys.len())
            .max()
            .unwrap_or(0)
            .max(8);

        let mut sorted_cmds = self.opts.commands.clone();
        sorted_cmds.sort_by(|a, b| {
            Self::get_slash_display_group(&a.name)
                .cmp(&Self::get_slash_display_group(&b.name))
                .then_with(|| a.name.cmp(&b.name))
        });
        let cmd_labels: Vec<String> = sorted_cmds
            .iter()
            .map(|c| {
                let aliases = if c.aliases.is_empty() {
                    String::new()
                } else {
                    format!(
                        " ({})",
                        c.aliases
                            .iter()
                            .map(|a| format!("/{a}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                format!("/{}{aliases}", c.name)
            })
            .collect();
        let cmd_width = cmd_labels
            .iter()
            .map(|l| l.len())
            .max()
            .unwrap_or(0)
            .max(12);

        let mut lines: Vec<String> = vec![
            accent(&"─".repeat(width)),
            format!(
                "{}{}",
                theme.bold_fg(ColorToken::Primary, " help "),
                muted("· Esc / Enter / q to cancel · ↑↓ scroll")
            ),
            String::new(),
            format!(
                "  {}",
                dim("Sure, Dimi is ready to help! Just send a message to get started.")
            ),
            String::new(),
            format!("  {}", theme.bold("Keyboard shortcuts")),
        ];
        for s in &shortcuts {
            lines.push(format!(
                "    {}  {}",
                kbd_color(&format!("{:<w$}", s.keys, w = kbd_width)),
                dim(&s.description)
            ));
        }
        lines.push(String::new());
        lines.push(format!("  {}", theme.bold("Slash commands")));
        for (i, cmd) in sorted_cmds.iter().enumerate() {
            let label = cmd_labels
                .get(i)
                .cloned()
                .unwrap_or(format!("/{}", cmd.name));
            lines.push(format!(
                "    {}  {}",
                slash_color(&format!("{label:<w$}", w = cmd_width)),
                dim(&cmd.description)
            ));
        }
        lines.push(String::new());
        lines.push(accent(&"─".repeat(width)));

        // Apply scroll windowing — keep the borders visible.
        let content = &lines[1..lines.len() - 1];
        let max_visible = self.opts.max_visible.unwrap_or(24).max(5);
        if content.len() > max_visible {
            self.scroll_top = self.scroll_top.min(content.len() - max_visible);
            let slice = &content[self.scroll_top..self.scroll_top + max_visible];
            let scroll_info = muted(&format!(
                " showing {}-{} of {}",
                self.scroll_top + 1,
                self.scroll_top + slice.len(),
                content.len()
            ));
            let mut out: Vec<String> = Vec::with_capacity(slice.len() + 3);
            out.push(lines[0].clone());
            out.extend(slice.iter().cloned());
            out.push(scroll_info);
            if let Some(last) = lines.last() {
                out.push(last.clone());
            }
            return out
                .iter()
                .map(|line| truncate_to_width(line, width, "...", false))
                .collect();
        }
        self.scroll_top = 0;
        lines
            .iter()
            .map(|line| truncate_to_width(line, width, "...", false))
            .collect()
    }

    fn handle_input(&mut self, data: &str) {
        let printable = decode_kitty_printable(data).unwrap_or_else(|| data.to_owned());
        if matches_key(data, "escape")
            || matches_key(data, "enter")
            || printable == "q"
            || printable == "Q"
        {
            self.action = Some(HelpPanelAction::Close);
            return;
        }
        if matches_key(data, "up") {
            self.scroll_top = self.scroll_top.saturating_sub(1);
            return;
        }
        if matches_key(data, "down") {
            self.scroll_top += 1; // render clamps
            return;
        }
        if matches_key(data, "pageup") {
            self.scroll_top = self.scroll_top.saturating_sub(10);
            return;
        }
        if matches_key(data, "pagedown") {
            self.scroll_top += 10;
        }
    }

    fn invalidate(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK_COLORS, set_palette};

    fn commands() -> Vec<HelpPanelCommand> {
        vec![
            HelpPanelCommand {
                name: "model".to_owned(),
                aliases: vec!["m".to_owned()],
                description: "Select a model".to_owned(),
            },
            HelpPanelCommand {
                name: "help".to_owned(),
                aliases: Vec::new(),
                description: "Show this panel".to_owned(),
            },
            HelpPanelCommand {
                name: "skill:agent".to_owned(),
                aliases: Vec::new(),
                description: "Run a skill".to_owned(),
            },
        ]
    }

    #[test]
    fn renders_sections_and_commands() {
        set_palette(DARK_COLORS);
        let mut c = HelpPanelComponent::new(HelpPanelOptions {
            commands: commands(),
            shortcuts: None,
            max_visible: None,
        });
        let lines = c.render(100);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("help"), "{joined}");
        assert!(joined.contains("Keyboard shortcuts"), "{joined}");
        assert!(joined.contains("Shift-Tab"), "{joined}");
        assert!(joined.contains("Slash commands"), "{joined}");
        assert!(joined.contains("/model (/m)"), "{joined}");
        assert!(joined.contains("skill:agent"), "{joined}");
        // skill: group sorts after builtin.
        let model_idx = joined.find("/model").unwrap();
        let skill_idx = joined.find("/skill:agent").unwrap();
        assert!(model_idx < skill_idx, "{joined}");
    }

    #[test]
    fn esc_enter_q_close() {
        set_palette(DARK_COLORS);
        let mut c = HelpPanelComponent::new(HelpPanelOptions {
            commands: commands(),
            shortcuts: None,
            max_visible: None,
        });
        c.handle_input("\x1b");
        assert_eq!(c.take_action(), Some(HelpPanelAction::Close));
        c.handle_input("\r");
        assert_eq!(c.take_action(), Some(HelpPanelAction::Close));
        c.handle_input("q");
        assert_eq!(c.take_action(), Some(HelpPanelAction::Close));
    }

    #[test]
    fn scroll_window_with_many_commands() {
        set_palette(DARK_COLORS);
        let many: Vec<HelpPanelCommand> = (0..30)
            .map(|i| HelpPanelCommand {
                name: format!("cmd{i}"),
                aliases: Vec::new(),
                description: format!("description {i}"),
            })
            .collect();
        let mut c = HelpPanelComponent::new(HelpPanelOptions {
            commands: many,
            shortcuts: None,
            max_visible: Some(10),
        });
        let lines = c.render(100);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("showing 1-10 of"), "{joined}");
        c.handle_input("\x1b[6~"); // pagedown
        let lines = c.render(100);
        let joined = crate::ansi::strip_ansi(&lines.join("\n"));
        assert!(joined.contains("showing 11-20 of"), "{joined}");
    }
}
