//! `PlanBoxComponent` — renders an ExitPlanMode plan inside a full box
//! border (port of `apps/dimi/src/tui/components/messages/plan-box.ts`).

use crate::component::Component;
use crate::markdown::Markdown;
use crate::markdown_theme::create_markdown_theme;
use crate::width::visible_width;
use crate::wrap::truncate_to_width;

const LEFT_MARGIN: usize = 2;
const SIDE_PADDING: usize = 1;

/// ExitPlanMode plan box with a border and optional status chip.
pub struct PlanBoxComponent {
    markdown: Markdown,
    status: Option<(String, String)>, // (label, colorHex)
    border_hex: String,
    cached_width: Option<usize>,
    cached_lines: Option<Vec<String>>,
}

impl PlanBoxComponent {
    pub fn new(
        plan: &str,
        border_hex: &str,
        _plan_path: Option<&str>,
        status: Option<(String, String)>,
    ) -> Self {
        let mut markdown = Markdown::new(
            plan.trim(),
            0,
            0,
            Box::new(create_markdown_theme()),
            None,
            Default::default(),
        );
        markdown.set_hyperlinks(false);
        PlanBoxComponent {
            markdown,
            status,
            border_hex: border_hex.to_owned(),
            cached_width: None,
            cached_lines: None,
        }
    }

    fn paint(&self, s: &str) -> String {
        let hex = self.border_hex.trim_start_matches('#');
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
        crate::style::StyleChain::new(vec![crate::style::fg_hex(r, g, b)]).apply(s)
    }

    fn build_title(&self, horz_len: usize) -> String {
        let fallback = " plan ";
        let status_suffix = match &self.status {
            Some((label, color_hex)) => {
                let hex = color_hex.trim_start_matches('#');
                let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
                let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
                let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
                format!(
                    " · {}",
                    crate::style::StyleChain::new(vec![crate::style::fg_hex(r, g, b)]).apply(label)
                )
            }
            None => String::new(),
        };
        let fallback_with_status = format!(" plan{status_suffix} ");
        let budget = horz_len.saturating_sub(1);
        let fallback_title = if visible_width(&fallback_with_status) <= budget {
            truncate_to_width(&fallback_with_status, budget, "…", false)
        } else {
            truncate_to_width(fallback, budget, "…", false)
        };
        // plan_path linking is a hyperlink feature; slice 2 renders the
        // fallback title only (the goldens exercise the no-path case).
        let _ = fallback;
        fallback_title
    }
}

impl Component for PlanBoxComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let safe_width = width;
        if safe_width == 0 {
            return vec![String::new()];
        }
        if safe_width < LEFT_MARGIN + 4 {
            let lines = self.markdown.render(safe_width.max(1));
            return lines
                .iter()
                .map(|l| truncate_to_width(l, safe_width, "…", false))
                .collect();
        }
        if let (Some(cached_width), Some(cached_lines)) = (&self.cached_width, &self.cached_lines) {
            if *cached_width == width {
                return cached_lines.clone();
            }
        }

        let horz_len = (safe_width.saturating_sub(LEFT_MARGIN + 2)).max(2);
        let content_width = (horz_len.saturating_sub(SIDE_PADDING * 2)).max(1);

        let indent = " ".repeat(LEFT_MARGIN);
        let title = self.build_title(horz_len);
        let trailing_dash_len = horz_len.saturating_sub(visible_width(&title));
        let top = format!(
            "{indent}{}{}{}{}",
            self.paint("┌"),
            self.paint(&title),
            self.paint(&"─".repeat(trailing_dash_len)),
            self.paint("┐")
        );
        let bottom = format!(
            "{indent}{}",
            self.paint(&format!("└{}┘", "─".repeat(horz_len)))
        );

        let raw_lines = self.markdown.render(content_width);
        let mut lines = vec![top];
        for raw in raw_lines {
            let pad = content_width.saturating_sub(visible_width(&raw));
            lines.push(format!(
                "{indent}{} {raw}{} {}",
                self.paint("│"),
                " ".repeat(pad),
                self.paint("│")
            ));
        }
        lines.push(bottom);

        let fitted: Vec<String> = lines
            .iter()
            .map(|l| truncate_to_width(l, safe_width, "…", false))
            .collect();
        self.cached_width = Some(width);
        self.cached_lines = Some(fitted.clone());
        fitted
    }

    fn invalidate(&mut self) {
        self.cached_width = None;
        self.cached_lines = None;
        self.markdown.invalidate();
    }
}
