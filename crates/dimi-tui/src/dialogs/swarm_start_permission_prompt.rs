//! Swarm start-permission prompt — configures [`StartPermissionPromptComponent`]
//! with swarm-specific options. Port of
//! `apps/dimi/src/tui/components/dialogs/swarm-start-permission-prompt.ts`
//! (`SwarmStartPermissionPromptComponent`).

use super::start_permission_prompt::{
    StartPermissionOption, StartPermissionPromptComponent, StartPermissionPromptOptions,
};

/// `SwarmStartPermissionChoice`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwarmStartPermissionChoice {
    Auto,
    Yolo,
    Manual,
}

impl SwarmStartPermissionChoice {
    pub fn as_str(&self) -> &'static str {
        match self {
            SwarmStartPermissionChoice::Auto => "auto",
            SwarmStartPermissionChoice::Yolo => "yolo",
            SwarmStartPermissionChoice::Manual => "manual",
        }
    }
}

/// `NOTICE_LINES` from swarm-start-permission-prompt.ts.
pub const SWARM_START_NOTICE_LINES: [&str; 3] = [
    "Manual mode asks you before Dimi runs commands, edits files, or takes other risky actions.",
    "Manual mode can block swarm work while agents are running.",
    "You can go back without losing your command.",
];

/// `SwarmStartPermissionPromptComponent`.
pub fn swarm_start_permission_prompt_component()
-> StartPermissionPromptComponent<SwarmStartPermissionChoice> {
    StartPermissionPromptComponent::new(StartPermissionPromptOptions {
        title: "Start a swarm task with approvals on?".to_owned(),
        notice_lines: SWARM_START_NOTICE_LINES.iter().map(|s| (*s).to_owned()).collect(),
        options: vec![
            StartPermissionOption {
                value: SwarmStartPermissionChoice::Auto,
                label: "Switch to Auto and start".to_owned(),
                description:
                    "Best for swarm tasks. Tools are approved automatically, and questions are skipped."
                        .to_owned(),
            },
            StartPermissionOption {
                value: SwarmStartPermissionChoice::Yolo,
                label: "Switch to YOLO and start".to_owned(),
                description:
                    "Tools and plan changes are approved automatically. Dimi may still ask you questions."
                        .to_owned(),
            },
            StartPermissionOption {
                value: SwarmStartPermissionChoice::Manual,
                label: "Start in Manual".to_owned(),
                description:
                    "Keep approvals on. Dimi may stop and wait for you during the swarm task."
                        .to_owned(),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Component;
    use crate::dialogs::start_permission_prompt::StartPermissionAction;
    use crate::theme::{DARK_COLORS, set_palette};

    fn plain(joined: &str) -> String {
        crate::ansi::strip_ansi(joined)
    }

    #[test]
    fn swarm_prompt_renders_notices_and_options() {
        set_palette(DARK_COLORS);
        let mut c = swarm_start_permission_prompt_component();
        let lines = c.render(100);
        let joined = plain(&lines.join("\n"));
        assert!(
            joined.contains("Start a swarm task with approvals on?"),
            "{joined}"
        );
        assert!(
            joined.contains("Manual mode can block swarm work"),
            "{joined}"
        );
        assert!(joined.contains("Switch to Auto and start"), "{joined}");
        assert!(joined.contains("Start in Manual"), "{joined}");
        assert!(
            joined.contains("You can go back without losing your command."),
            "{joined}"
        );
    }

    #[test]
    fn swarm_select_action() {
        set_palette(DARK_COLORS);
        let mut c = swarm_start_permission_prompt_component();
        c.handle_input("\r");
        assert_eq!(
            c.take_action(),
            Some(StartPermissionAction::Select(
                SwarmStartPermissionChoice::Auto
            ))
        );
    }
}
