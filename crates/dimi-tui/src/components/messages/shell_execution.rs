//! `ShellExecutionComponent` — Bash command preview + result output
//! (port of `apps/dimi/src/tui/components/messages/shell-execution.ts`).

use crate::component::Component;
use crate::components::messages::tool_renderers::{ToolResultData, TruncatedOutputComponent};
use crate::components::text::Text;
use crate::theme::{ColorToken, current_theme};

/// Shell execution block options.
pub struct ShellExecutionOptions {
    pub command: Option<String>,
    pub result: Option<ToolResultData>,
    pub expanded: bool,
    pub show_command: bool,
    /// Max command lines to render; `None` = no cap (expanded view).
    pub command_preview_lines: Option<usize>,
    pub result_preview_lines: Option<usize>,
    pub tail_output: bool,
    pub expand_hint: bool,
}

/// Renders a `$ command` preview (optional) and the result output below it.
pub struct ShellExecutionComponent {
    children: Vec<Box<dyn Component>>,
}

impl ShellExecutionComponent {
    pub fn new(options: ShellExecutionOptions) -> Self {
        let mut children: Vec<Box<dyn Component>> = Vec::new();
        if options.show_command {
            add_command_preview(
                &mut children,
                options.command.as_deref().unwrap_or(""),
                options.command_preview_lines,
            );
        }
        if let Some(result) = options.result {
            children.push(Box::new(TruncatedOutputComponent::new(
                &result.output,
                options.expanded,
                result.is_error,
                options.result_preview_lines,
                None,
                Some(options.expand_hint),
                Some(options.tail_output),
                ColorToken::TextMuted,
            )));
        }
        ShellExecutionComponent { children }
    }
}

fn add_command_preview(
    children: &mut Vec<Box<dyn Component>>,
    command: &str,
    preview_lines: Option<usize>,
) {
    if command.is_empty() {
        return;
    }
    let all_lines: Vec<&str> = command.split('\n').collect();
    let lines: Vec<&str> = match preview_lines {
        Some(cap) => all_lines.iter().take(cap).copied().collect(),
        None => all_lines,
    };
    for (i, line) in lines.iter().enumerate() {
        // `$` prompt uses the shell-mode hue; the command body uses textDim.
        let text = if i == 0 {
            format!(
                "{}{}",
                current_theme().fg(ColorToken::ShellMode, "$ "),
                current_theme().dim(line)
            )
        } else {
            format!("  {}", current_theme().dim(line))
        };
        children.push(Box::new(Text::new(&text, 2, 0)));
    }
}

impl Component for ShellExecutionComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        for child in &mut self.children {
            lines.extend(child.render(width));
        }
        lines
    }

    fn invalidate(&mut self) {
        for child in &mut self.children {
            child.invalidate();
        }
    }
}
