//! `Markdown` component — renders markdown to ANSI-styled terminal lines.
//!
//! Byte-aligned port of `@dimi-agent/pi-tui` `src/components/markdown.ts`
//! (858 lines) using `comrak` for CommonMark/GFM parsing. The renderer walks
//! comrak's AST with the same token semantics as marked's token stream: block
//! spacing rules, heading style contexts, inline style-prefix re-opening, list
//! marker normalization, blockquote borders, and width-aware table layout.

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Options as ComrakOptions, parse_document};

use crate::component::Component;
use crate::markdown_theme::MarkdownTheme;
use crate::width::visible_width;
use crate::wrap::wrap_text_with_ansi;

/// Text style function (color / background).
pub type StyleFn = dyn Fn(&str) -> String;

/// Default text styling applied to all markdown text unless overridden by
/// markdown formatting (mirrors `DefaultTextStyle`).
#[derive(Default)]
pub struct DefaultTextStyle {
    /// Foreground color function.
    pub color: Option<Box<StyleFn>>,
    /// Background color function.
    pub bg_color: Option<Box<StyleFn>>,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub underline: bool,
}

/// Renderer options (mirrors `MarkdownOptions`).
#[derive(Debug, Clone, Default)]
pub struct MarkdownOptions {
    /// Preserve source list markers instead of normalizing them.
    pub preserve_ordered_list_markers: bool,
    /// Preserve source backslash escapes instead of normalizing escaped
    /// punctuation.
    pub preserve_backslash_escapes: bool,
}

/// Inline style application mode — the `applyText` closure of the TS
/// `InlineStyleContext`.
#[derive(Debug, Clone, Copy)]
enum ApplyFn {
    /// `applyDefaultStyle` — base text style (color/decorations).
    Default,
    /// Heading style: h1 = heading(bold(underline(x))), h2+ = heading(bold(x)).
    Heading { level: usize },
    /// Identity (blockquote inline context).
    Identity,
}

/// The `InlineStyleContext` — how inline tokens are styled and the prefix
/// re-applied after styled children.
struct InlineCtx {
    apply: ApplyFn,
    prefix: String,
}

/// Markdown component (port of pi-tui `Markdown`).
pub struct Markdown {
    text: String,
    padding_x: usize,
    padding_y: usize,
    theme: Box<dyn MarkdownTheme>,
    default_text_style: Option<DefaultTextStyle>,
    options: MarkdownOptions,
    hyperlinks: bool,
    /// Source text of the most recent render, kept for sourcepos-based marker
    /// preservation (mirrors reading `item.raw` in the TS renderer).
    last_source: Option<String>,
    cached_text: Option<String>,
    cached_width: Option<usize>,
    cached_lines: Option<Vec<String>>,
}

impl Markdown {
    pub fn new(
        text: &str,
        padding_x: usize,
        padding_y: usize,
        theme: Box<dyn MarkdownTheme>,
        default_text_style: Option<DefaultTextStyle>,
        options: MarkdownOptions,
    ) -> Self {
        Markdown {
            text: text.to_owned(),
            padding_x,
            padding_y,
            theme,
            default_text_style,
            options,
            hyperlinks: false,
            last_source: None,
            cached_text: None,
            cached_width: None,
            cached_lines: None,
        }
    }

    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_owned();
        self.invalidate();
    }

    /// Enable/disable OSC 8 hyperlink emission. Defaults to false (matching
    /// the TS `getCapabilities()` fallback for unknown terminals).
    pub fn set_hyperlinks(&mut self, enabled: bool) {
        self.hyperlinks = enabled;
        self.invalidate();
    }

    fn comrak_options() -> ComrakOptions<'static> {
        let mut options = ComrakOptions::default();
        options.extension.strikethrough = true;
        options.extension.table = true;
        options.extension.tasklist = true;
        options.extension.autolink = true;
        options
    }

    /// Apply the default text style (color/decorations) — background is NOT
    /// applied here; it is applied at the padding stage.
    fn apply_default_style(&self, text: &str) -> String {
        let Some(style) = &self.default_text_style else {
            return text.to_owned();
        };
        let mut styled = text.to_owned();
        if let Some(color) = &style.color {
            styled = color(&styled);
        }
        if style.bold {
            styled = self.theme.bold(&styled);
        }
        if style.italic {
            styled = self.theme.italic(&styled);
        }
        if style.strikethrough {
            styled = self.theme.strikethrough(&styled);
        }
        if style.underline {
            styled = self.theme.underline(&styled);
        }
        styled
    }

    /// Style-prefix via the NUL sentinel technique (`getStylePrefix`).
    fn style_prefix(&self, apply: &ApplyFn) -> String {
        const SENTINEL: char = '\u{0}';
        let styled = match apply {
            ApplyFn::Default => self.apply_default_style(&SENTINEL.to_string()),
            ApplyFn::Heading { level } => self.apply_heading_fn(*level, &SENTINEL.to_string()),
            ApplyFn::Identity => SENTINEL.to_string(),
        };
        match styled.find(SENTINEL) {
            Some(idx) => styled[..idx].to_owned(),
            None => String::new(),
        }
    }

    fn apply_heading_fn(&self, level: usize, text: &str) -> String {
        if level == 1 {
            self.theme
                .heading(&self.theme.bold(&self.theme.underline(text)))
        } else {
            self.theme.heading(&self.theme.bold(text))
        }
    }

    fn default_ctx(&self) -> InlineCtx {
        let apply = ApplyFn::Default;
        let prefix = self.style_prefix(&apply);
        InlineCtx { apply, prefix }
    }

    /// `renderToken` — block-level rendering. `next_type` mirrors
    /// `nextTokenType` ("", "space", "list", "paragraph", ...).
    fn render_block<'a>(
        &self,
        node: &'a AstNode<'a>,
        width: usize,
        next_type: &str,
        ctx: &InlineCtx,
    ) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        match &node.data.borrow().value {
            NodeValue::Heading(h) => {
                let level = h.level as usize;
                let heading_prefix = format!("{} ", "#".repeat(level));
                let heading_apply = ApplyFn::Heading { level };
                let heading_ctx = InlineCtx {
                    apply: heading_apply,
                    prefix: self.style_prefix(&heading_apply),
                };
                let heading_text = self.render_inline_children(node, &heading_ctx);
                let styled_heading = if level >= 3 {
                    let styled_prefix = self.apply_heading_fn(level, &heading_prefix);
                    format!("{styled_prefix}{heading_text}")
                } else {
                    heading_text
                };
                lines.push(styled_heading);
                if !next_type.is_empty() && next_type != "space" {
                    lines.push(String::new());
                }
            }
            NodeValue::Paragraph => {
                let paragraph_text = self.render_inline_children(node, ctx);
                lines.push(paragraph_text);
                if !next_type.is_empty() && next_type != "list" && next_type != "space" {
                    lines.push(String::new());
                }
            }
            NodeValue::Text(_) => {
                lines.push(self.render_inline_children(node, ctx));
            }
            NodeValue::CodeBlock(cb) => {
                let indent = self.theme.code_block_indent().to_owned();
                let lang = cb.info.trim();
                lines.push(self.theme.code_block_border(&format!("```{lang}")));
                // Strip the single trailing newline (the line terminator before
                // the closing fence) — marked's token.text has none.
                let literal = cb.literal.strip_suffix('\n').unwrap_or(&cb.literal);
                let highlighted = self
                    .theme
                    .highlight_code(literal, (!lang.is_empty()).then_some(lang));
                for hl_line in highlighted {
                    lines.push(format!("{indent}{hl_line}"));
                }
                lines.push(self.theme.code_block_border("```"));
                if !next_type.is_empty() && next_type != "space" {
                    lines.push(String::new());
                }
            }
            NodeValue::List(list) => {
                let list_lines = self.render_list(node, list, 0, width, ctx);
                lines.extend(list_lines);
            }
            NodeValue::Table(table) => {
                let table_lines = self.render_table(node, table, width, next_type, ctx);
                lines.extend(table_lines);
            }
            NodeValue::BlockQuote => {
                let quote_prefix =
                    self.style_prefix_for(&|t| self.theme.quote(&self.theme.italic(t)));
                let quote_content_width = width.saturating_sub(2).max(1);

                let quote_ctx = InlineCtx {
                    apply: ApplyFn::Identity,
                    prefix: quote_prefix.clone(),
                };
                let mut rendered_quote_lines: Vec<String> = Vec::new();
                let children: Vec<&AstNode> = node.children().collect();
                for (i, child) in children.iter().enumerate() {
                    let next_type = if i + 1 < children.len() {
                        node_type_str(children[i + 1])
                    } else {
                        ""
                    };
                    let child_lines =
                        self.render_block(child, quote_content_width, next_type, &quote_ctx);
                    rendered_quote_lines.extend(child_lines);
                }

                // Avoid rendering an extra empty quote line before spacing.
                while rendered_quote_lines.last().is_some_and(|l| l.is_empty()) {
                    rendered_quote_lines.pop();
                }

                for quote_line in rendered_quote_lines {
                    let styled_line = if quote_prefix.is_empty() {
                        self.theme.quote(&self.theme.italic(&quote_line))
                    } else {
                        // Re-apply the quote style prefix after every SGR reset.
                        let line_with_reapplied =
                            quote_line.replace("\x1b[0m", &format!("\x1b[0m{quote_prefix}"));
                        self.theme.quote(&self.theme.italic(&line_with_reapplied))
                    };
                    let wrapped = wrap_text_with_ansi(&styled_line, quote_content_width);
                    for wrapped_line in wrapped {
                        lines.push(format!("{}{}", self.theme.quote_border("│ "), wrapped_line));
                    }
                }
                if !next_type.is_empty() && next_type != "space" {
                    lines.push(String::new());
                }
            }
            NodeValue::ThematicBreak => {
                let count = width.min(80);
                lines.push(self.theme.hr(&"─".repeat(count)));
                if !next_type.is_empty() && next_type != "space" {
                    lines.push(String::new());
                }
            }
            NodeValue::HtmlBlock(html) => {
                lines.push(self.apply_default_style(html.literal.trim()));
            }
            NodeValue::Item(_) | NodeValue::Document => {
                // Rendered by their parents; treat as plain inline text fallback.
                lines.push(self.render_inline_children(node, ctx));
            }
            other => {
                // Handle any other node types as plain text.
                lines.push(node_text_fallback(other, ctx, self));
            }
        }
        lines
    }

    fn style_prefix_for(&self, apply: &impl Fn(&str) -> String) -> String {
        const SENTINEL: char = '\u{0}';
        let styled = apply(&SENTINEL.to_string());
        match styled.find(SENTINEL) {
            Some(idx) => styled[..idx].to_owned(),
            None => String::new(),
        }
    }

    /// `renderInlineTokens` — inline rendering of a node's children.
    fn render_inline_children<'a>(&self, node: &'a AstNode<'a>, ctx: &InlineCtx) -> String {
        let children: Vec<&AstNode> = node.children().collect();
        self.render_inline_nodes(&children, ctx)
    }

    fn render_inline_nodes<'a>(&self, nodes: &[&'a AstNode<'a>], ctx: &InlineCtx) -> String {
        let mut result = String::new();
        let apply_text_with_newlines = |apply: &ApplyFn, text: &str| -> String {
            let segments: Vec<&str> = text.split('\n').collect();
            let mut out = String::new();
            for (i, segment) in segments.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                out.push_str(&self.apply_fn(apply, segment));
            }
            out
        };

        for node in nodes {
            match &node.data.borrow().value {
                NodeValue::Text(t) => {
                    // Text tokens in list items can have nested tokens; comrak
                    // keeps plain text as Text nodes, so apply directly.
                    result.push_str(&apply_text_with_newlines(&ctx.apply, t.as_ref()));
                }
                NodeValue::Strong => {
                    let inner = self.render_inline_children(node, ctx);
                    result.push_str(&self.theme.bold(&inner));
                    result.push_str(&ctx.prefix);
                }
                NodeValue::Emph => {
                    let inner = self.render_inline_children(node, ctx);
                    result.push_str(&self.theme.italic(&inner));
                    result.push_str(&ctx.prefix);
                }
                NodeValue::Code(c) => {
                    result.push_str(&self.theme.code(&c.literal));
                    result.push_str(&ctx.prefix);
                }
                NodeValue::Link(link) => {
                    let link_text = self.render_inline_children(node, ctx);
                    let styled_link = self.theme.link(&self.theme.underline(&link_text));
                    if self.hyperlinks {
                        let href = &link.url;
                        result.push_str(&self.hyperlink(&styled_link, href));
                        result.push_str(&ctx.prefix);
                    } else {
                        let href_for_comparison =
                            if let Some(stripped) = link.url.strip_prefix("mailto:") {
                                stripped.to_owned()
                            } else {
                                link.url.clone()
                            };
                        // Compare the raw (unstyled) link text against href.
                        let raw_text = self.plain_inline_text(node);
                        if raw_text == link.url || raw_text == href_for_comparison {
                            result.push_str(&styled_link);
                        } else {
                            let url_part = self.theme.link_url(&format!(" ({})", link.url));
                            result.push_str(&styled_link);
                            result.push_str(&url_part);
                        }
                        result.push_str(&ctx.prefix);
                    }
                }
                NodeValue::LineBreak => {
                    result.push('\n');
                }
                NodeValue::SoftBreak => {
                    result.push('\n');
                }
                NodeValue::Strikethrough => {
                    let inner = self.render_inline_children(node, ctx);
                    result.push_str(&self.theme.strikethrough(&inner));
                    result.push_str(&ctx.prefix);
                }
                NodeValue::HtmlInline(html) => {
                    result.push_str(&apply_text_with_newlines(&ctx.apply, html.as_ref()));
                }
                NodeValue::Image(_) => {
                    // pi-tui renders image tokens as their alt text.
                    result.push_str(&apply_text_with_newlines(
                        &ctx.apply,
                        &self.plain_inline_text(node),
                    ));
                }
                NodeValue::Paragraph => {
                    // Nested paragraph tokens (rare) contain inline tokens.
                    let children: Vec<&AstNode> = node.children().collect();
                    result.push_str(&self.render_inline_nodes(&children, ctx));
                }
                other => {
                    result.push_str(&apply_text_with_newlines(
                        &ctx.apply,
                        &node_text_fallback(other, ctx, self),
                    ));
                }
            }
        }

        // Strip a trailing style prefix (the styled child already re-applies it).
        while !ctx.prefix.is_empty() && result.ends_with(&ctx.prefix) {
            result.truncate(result.len() - ctx.prefix.len());
        }
        result
    }

    fn apply_fn(&self, apply: &ApplyFn, text: &str) -> String {
        match apply {
            ApplyFn::Default => self.apply_default_style(text),
            ApplyFn::Heading { level } => self.apply_heading_fn(*level, text),
            ApplyFn::Identity => text.to_owned(),
        }
    }

    fn hyperlink(&self, text: &str, url: &str) -> String {
        format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
    }

    /// Raw unstyled text of an inline node (marked's `token.text`).
    fn plain_inline_text<'a>(&self, node: &'a AstNode<'a>) -> String {
        let mut out = String::new();
        for child in node.children() {
            match &child.data.borrow().value {
                NodeValue::Text(t) => out.push_str(t.as_ref()),
                NodeValue::Code(c) => out.push_str(&c.literal),
                _ => out.push_str(&self.plain_inline_text(child)),
            }
        }
        out
    }

    /// `getOrderedListMarker` — preserve the source marker when
    /// `preserve_ordered_list_markers` is set. comrak does not expose the raw
    /// marker, so the item's source line is read from the most recent render
    /// source (the same line the TS `item.raw` starts with).
    fn ordered_list_marker(&self, index: usize, start: u64, item_line: usize) -> String {
        if self.options.preserve_ordered_list_markers {
            if let Some(src) = &self.last_source {
                if let Some(line_text) = src.lines().nth(item_line.saturating_sub(1)) {
                    let re = regex::Regex::new(r"^(?: {0,3})(\d{1,9}[.)])[ \t]+")
                        .expect("valid marker regex");
                    if let Some(caps) = re.captures(line_text) {
                        return format!("{} ", &caps[1]);
                    }
                }
            }
        }
        format!("{}. ", start + index as u64)
    }

    fn unordered_list_marker(&self) -> String {
        "- ".to_owned()
    }

    /// `renderList` — lists with nesting.
    fn render_list<'a>(
        &self,
        list_node: &'a AstNode<'a>,
        list: &comrak::nodes::NodeList,
        depth: usize,
        width: usize,
        ctx: &InlineCtx,
    ) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let indent = "    ".repeat(depth);
        let start_number = list.start;

        let items: Vec<&AstNode> = list_node.children().collect();
        for (i, item_node) in items.iter().enumerate() {
            let is_last_item = i == items.len() - 1;
            let task_checked = match &item_node.data.borrow().value {
                NodeValue::TaskItem(t) => t.symbol.is_some_and(|c| c == 'x'),
                _ => false,
            };
            let is_task = matches!(item_node.data.borrow().value, NodeValue::TaskItem(_));
            let bullet = if list.list_type == comrak::nodes::ListType::Ordered {
                let item_line = item_node.data.borrow().sourcepos.start.line;
                self.ordered_list_marker(i, start_number as u64, item_line)
            } else {
                self.unordered_list_marker()
            };
            let task_marker = if is_task {
                if task_checked { "[x] " } else { "[ ] " }
            } else {
                ""
            };
            let marker = format!("{bullet}{task_marker}");
            let first_prefix = format!("{indent}{}", self.theme.list_bullet(&marker));
            let continuation_prefix = format!("{indent}{}", " ".repeat(visible_width(&marker)));
            let item_width = width.saturating_sub(visible_width(&first_prefix)).max(1);
            let mut rendered_any_line = false;

            let item_children: Vec<&AstNode> = item_node.children().collect();
            for item_node_child in item_children {
                if matches!(item_node_child.data.borrow().value, NodeValue::List(_)) {
                    let nested = item_node_child.data.borrow().clone();
                    let nested_list = match &nested.value {
                        NodeValue::List(l) => *l,
                        _ => unreachable!(),
                    };
                    let nested_lines =
                        self.render_list(item_node_child, &nested_list, depth + 1, width, ctx);
                    lines.extend(nested_lines);
                    rendered_any_line = true;
                    continue;
                }
                if matches!(item_node_child.data.borrow().value, NodeValue::Item(_)) {
                    // Deeply nested structure (comrak nests item lists inside items).
                    continue;
                }

                let item_lines = self.render_block(item_node_child, item_width, "", ctx);
                for line in item_lines {
                    for wrapped_line in wrap_text_with_ansi(&line, item_width) {
                        let line_prefix = if rendered_any_line {
                            continuation_prefix.clone()
                        } else {
                            first_prefix.clone()
                        };
                        lines.push(format!("{line_prefix}{wrapped_line}"));
                        rendered_any_line = true;
                    }
                }
            }

            if !rendered_any_line {
                lines.push(first_prefix);
            }

            if !list.tight && !is_last_item {
                lines.push(String::new());
            }
        }
        lines
    }

    /// Longest visible width of a whitespace-separated word.
    fn longest_word_width(&self, text: &str, max_width: Option<usize>) -> usize {
        let mut longest = 0usize;
        for word in text.split_whitespace() {
            longest = longest.max(visible_width(word));
        }
        match max_width {
            Some(m) => longest.min(m),
            None => longest,
        }
    }

    fn render_table<'a>(
        &self,
        table_node: &'a AstNode<'a>,
        _table: &comrak::nodes::NodeTable,
        available_width: usize,
        next_type: &str,
        ctx: &InlineCtx,
    ) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        // Children are TableRow(bool) nodes; the first is the header.
        let row_nodes: Vec<&AstNode> = table_node.children().collect();
        let header_cell_nodes: Vec<&AstNode> = row_nodes
            .first()
            .map(|r| r.children().collect())
            .unwrap_or_default();
        let num_cols = header_cell_nodes.len();
        if num_cols == 0 {
            return lines;
        }
        let body_row_nodes: Vec<&AstNode> = row_nodes.iter().skip(1).copied().collect();

        // Border overhead: "│ " + (n-1) * " │ " + " │" = 3n + 1.
        let border_overhead = 3 * num_cols + 1;
        let available_for_cells = available_width.saturating_sub(border_overhead);
        if available_for_cells < num_cols {
            // Too narrow — fall back to raw markdown. comrak has no raw text
            // for the table; render nothing but keep spacing parity.
            if !next_type.is_empty() && next_type != "space" {
                lines.push(String::new());
            }
            return lines;
        }

        const MAX_UNBROKEN_WORD_WIDTH: usize = 30;

        // Render header cells.
        let header_texts: Vec<String> = header_cell_nodes
            .iter()
            .map(|cell| self.render_table_cell(cell, ctx))
            .collect();
        let mut natural_widths: Vec<usize> =
            header_texts.iter().map(|t| visible_width(t)).collect();
        let mut min_word_widths: Vec<usize> = header_texts
            .iter()
            .map(|t| {
                self.longest_word_width(t, Some(MAX_UNBROKEN_WORD_WIDTH))
                    .max(1)
            })
            .collect();

        // Render all row cells.
        let mut row_texts: Vec<Vec<String>> = Vec::new();
        for row in &body_row_nodes {
            let cells: Vec<&AstNode> = row.children().collect();
            let rendered: Vec<String> = cells
                .iter()
                .map(|cell| self.render_table_cell(cell, ctx))
                .collect();
            for (i, cell_text) in rendered.iter().enumerate() {
                if let Some(nw) = natural_widths.get_mut(i) {
                    *nw = (*nw).max(visible_width(cell_text));
                }
                if let Some(mw) = min_word_widths.get_mut(i) {
                    *mw = (*mw).max(
                        self.longest_word_width(cell_text, Some(MAX_UNBROKEN_WORD_WIDTH))
                            .max(1),
                    );
                }
            }
            row_texts.push(rendered);
        }

        let mut min_column_widths = min_word_widths.clone();
        let mut min_cells_width: usize = min_column_widths.iter().sum();
        if min_cells_width > available_for_cells {
            min_column_widths = vec![1; num_cols];
            let remaining = available_for_cells - num_cols;
            if remaining > 0 {
                let total_weight: usize = min_word_widths.iter().map(|w| w.saturating_sub(1)).sum();
                let growth: Vec<usize> = min_word_widths
                    .iter()
                    .map(|w| {
                        let weight = w.saturating_sub(1);
                        if total_weight > 0 {
                            // Math.floor((weight / totalWeight) * remaining) — TS parity.
                            ((weight as f64 / total_weight as f64) * remaining as f64).floor()
                                as usize
                        } else {
                            0
                        }
                    })
                    .collect();
                for (i, g) in growth.iter().enumerate() {
                    min_column_widths[i] += g;
                }
                let allocated: usize = growth.iter().sum();
                let mut leftover = remaining - allocated;
                let mut i = 0;
                while leftover > 0 && i < num_cols {
                    min_column_widths[i] += 1;
                    leftover -= 1;
                    i += 1;
                }
            }
            min_cells_width = min_column_widths.iter().sum();
        }

        // Column widths that fit within the available width.
        let total_natural_width: usize = natural_widths.iter().sum::<usize>() + border_overhead;
        let column_widths: Vec<usize> = if total_natural_width <= available_width {
            natural_widths
                .iter()
                .enumerate()
                .map(|(i, w)| (*w).max(min_column_widths[i]))
                .collect()
        } else {
            let total_grow_potential: usize = natural_widths
                .iter()
                .enumerate()
                .map(|(i, w)| w.saturating_sub(min_column_widths[i]))
                .sum();
            let extra_width = available_for_cells.saturating_sub(min_cells_width);
            let mut widths: Vec<usize> = min_column_widths
                .iter()
                .enumerate()
                .map(|(i, min_w)| {
                    let natural_w = natural_widths[i];
                    let min_delta = natural_w.saturating_sub(*min_w);
                    let grow = if total_grow_potential > 0 {
                        // Math.floor((minWidthDelta / totalGrowPotential) * extraWidth).
                        ((min_delta as f64 / total_grow_potential as f64) * extra_width as f64)
                            .floor() as usize
                    } else {
                        0
                    };
                    min_w + grow
                })
                .collect();
            // Distribute rounding remainder.
            let allocated: usize = widths.iter().sum();
            let mut remaining = available_for_cells.saturating_sub(allocated);
            while remaining > 0 {
                let mut grew = false;
                let mut i = 0;
                while i < num_cols && remaining > 0 {
                    if widths[i] < natural_widths[i] {
                        widths[i] += 1;
                        remaining -= 1;
                        grew = true;
                    }
                    i += 1;
                }
                if !grew {
                    break;
                }
            }
            widths
        };

        // Top border.
        let top_cells: Vec<String> = column_widths.iter().map(|w| "─".repeat(*w)).collect();
        lines.push(format!("┌─{}─┐", top_cells.join("─┬─")));

        // Header with wrapping.
        let header_cell_lines: Vec<Vec<String>> = header_texts
            .iter()
            .enumerate()
            .map(|(i, t)| self.wrap_cell_text(t, column_widths[i]))
            .collect();
        let header_line_count = header_cell_lines.iter().map(|c| c.len()).max().unwrap_or(0);
        for line_idx in 0..header_line_count {
            let row_parts: Vec<String> = header_cell_lines
                .iter()
                .enumerate()
                .map(|(col_idx, cell_lines)| {
                    let text = cell_lines.get(line_idx).cloned().unwrap_or_default();
                    let pad = column_widths[col_idx].saturating_sub(visible_width(&text));
                    self.theme.bold(&format!("{text}{}", " ".repeat(pad)))
                })
                .collect();
            lines.push(format!("│ {} │", row_parts.join(" │ ")));
        }

        // Separator.
        let separator_cells: Vec<String> = column_widths.iter().map(|w| "─".repeat(*w)).collect();
        let separator_line = format!("├─{}─┤", separator_cells.join("─┼─"));
        lines.push(separator_line.clone());

        // Rows with wrapping.
        for (row_index, cells) in row_texts.iter().enumerate() {
            let row_cell_lines: Vec<Vec<String>> = cells
                .iter()
                .enumerate()
                .map(|(i, text)| self.wrap_cell_text(text, column_widths[i]))
                .collect();
            let row_line_count = row_cell_lines.iter().map(|c| c.len()).max().unwrap_or(0);
            for line_idx in 0..row_line_count {
                let row_parts: Vec<String> = row_cell_lines
                    .iter()
                    .enumerate()
                    .map(|(col_idx, cell_lines)| {
                        let text = cell_lines.get(line_idx).cloned().unwrap_or_default();
                        let pad = column_widths[col_idx].saturating_sub(visible_width(&text));
                        format!("{text}{}", " ".repeat(pad))
                    })
                    .collect();
                lines.push(format!("│ {} │", row_parts.join(" │ ")));
            }
            if row_index < row_texts.len() - 1 {
                lines.push(separator_line.clone());
            }
        }

        // Bottom border.
        let bottom_cells: Vec<String> = column_widths.iter().map(|w| "─".repeat(*w)).collect();
        lines.push(format!("└─{}─┘", bottom_cells.join("─┴─")));

        if !next_type.is_empty() && next_type != "space" {
            lines.push(String::new());
        }
        lines
    }

    fn render_table_cell<'a>(&self, cell: &'a AstNode<'a>, ctx: &InlineCtx) -> String {
        let children: Vec<&AstNode> = cell.children().collect();
        self.render_inline_nodes(&children, ctx)
    }

    fn wrap_cell_text(&self, text: &str, max_width: usize) -> Vec<String> {
        wrap_text_with_ansi(text, max_width.max(1))
    }
}

/// `node_type_str` — the next-token type string for spacing decisions.
fn node_type_str<'a>(node: &'a AstNode<'a>) -> &'static str {
    match &node.data.borrow().value {
        NodeValue::Paragraph => "paragraph",
        NodeValue::Heading(_) => "heading",
        NodeValue::CodeBlock(_) => "code",
        NodeValue::List(_) => "list",
        NodeValue::Table(_) => "table",
        NodeValue::BlockQuote => "blockquote",
        NodeValue::ThematicBreak => "hr",
        NodeValue::HtmlBlock(_) => "html",
        NodeValue::Text(_) => "text",
        _ => "",
    }
}

/// Fallback: render a node's literal as plain text.
fn node_text_fallback(node_value: &NodeValue, ctx: &InlineCtx, md: &Markdown) -> String {
    let text = match node_value {
        NodeValue::Code(c) => c.literal.clone(),
        _ => String::new(),
    };
    if text.is_empty() {
        String::new()
    } else {
        md.apply_fn(&ctx.apply, &text)
    }
}

/// Trim partial closing fences from the last code block so streamed code
/// blocks do not shrink/flicker (mirrors `trimPartialClosingFences`).
fn trim_partial_closing_fences<'a>(nodes: &[&'a AstNode<'a>]) {
    // Find the last code block in this list, descending into lists/blockquotes.
    if let Some(last) = nodes.last() {
        let value = last.data.borrow().value.clone();
        match value {
            NodeValue::List(list) => {
                if let Some(item) = last.children().last() {
                    let item_children: Vec<&AstNode> = item.children().collect();
                    trim_partial_closing_fences(&item_children);
                }
                let _ = list;
            }
            NodeValue::BlockQuote => {
                let children: Vec<&AstNode> = last.children().collect();
                trim_partial_closing_fences(&children);
            }
            NodeValue::CodeBlock(cb) if cb.fenced => {
                let marker = (cb.fence_char as char).to_string();
                let _fence_len = cb.fence_length;
                if let Some(last_line) = cb.literal.rsplit('\n').next() {
                    let trimmed = last_line.trim_end_matches('\r');
                    if !trimmed.is_empty()
                        && trimmed.chars().count() < marker.chars().count()
                        && trimmed.chars().all(|c| c == marker.chars().next().unwrap())
                    {
                        // Trim the partial closing line from the literal.
                        if let NodeValue::CodeBlock(cb_mut) = &mut last.data.borrow_mut().value {
                            let literal = cb_mut.literal.clone();
                            if literal.ends_with(trimmed) {
                                let new_len = literal.len() - trimmed.len();
                                let mut new_literal = literal[..new_len].to_owned();
                                while new_literal.ends_with('\n') {
                                    new_literal.pop();
                                }
                                cb_mut.literal = new_literal;
                            }
                        }
                        let _ = _fence_len;
                    }
                }
            }
            _ => {}
        }
    }
}

impl Component for Markdown {
    fn render(&mut self, width: usize) -> Vec<String> {
        if let (Some(cached_text), Some(cached_width), Some(cached_lines)) =
            (&self.cached_text, self.cached_width, &self.cached_lines)
        {
            if cached_text == &self.text && cached_width == width {
                return cached_lines.clone();
            }
        }

        // Content width (subtract horizontal padding).
        let content_width = (width.saturating_sub(self.padding_x * 2)).max(1);

        // Don't render anything if there's no actual text.
        if self.text.trim().is_empty() {
            let result: Vec<String> = Vec::new();
            self.cached_text = Some(self.text.clone());
            self.cached_width = Some(width);
            self.cached_lines = Some(result.clone());
            return result;
        }

        // Replace tabs with 3 spaces.
        let normalized_text = self.text.replace('\t', "   ");
        self.last_source = Some(normalized_text.clone());

        // Parse markdown.
        let arena = comrak::Arena::new();
        let root = parse_document(&arena, &normalized_text, &Self::comrak_options());

        // Trim streamed partial closing fences so code blocks do not
        // shrink/flicker when the final fence character arrives.
        let children: Vec<&AstNode> = root.children().collect();
        trim_partial_closing_fences(&children);

        let default_ctx = self.default_ctx();

        // Render blocks with spacing.
        let mut rendered_lines: Vec<String> = Vec::new();
        for (i, child) in children.iter().enumerate() {
            if i > 0 && has_blank_gap(children[i - 1], child) {
                rendered_lines.push(String::new());
            }
            let next_type = if i + 1 < children.len() {
                if has_blank_gap(child, children[i + 1]) {
                    "space"
                } else {
                    node_type_str(children[i + 1])
                }
            } else {
                ""
            };
            let token_lines = self.render_block(child, content_width, next_type, &default_ctx);
            rendered_lines.extend(token_lines);
        }

        // Wrap lines (NO padding, NO background yet).
        let mut wrapped_lines: Vec<String> = Vec::new();
        for line in rendered_lines {
            for wrapped_line in wrap_text_with_ansi(&line, content_width) {
                wrapped_lines.push(wrapped_line);
            }
        }

        // Add margins and background to each wrapped line.
        let left_margin = " ".repeat(self.padding_x);
        let right_margin = " ".repeat(self.padding_x);
        let bg_fn = self
            .default_text_style
            .as_ref()
            .and_then(|s| s.bg_color.as_ref());
        let mut content_lines: Vec<String> = Vec::new();
        for line in wrapped_lines {
            let line_with_margins = format!("{left_margin}{line}{right_margin}");
            if let Some(bg) = bg_fn {
                content_lines.push(crate::wrap::apply_background_to_line(
                    &line_with_margins,
                    width,
                    |t| bg(t),
                ));
            } else {
                let visible_len = visible_width(&line_with_margins);
                let padding_needed = width.saturating_sub(visible_len);
                content_lines.push(format!("{line_with_margins}{}", " ".repeat(padding_needed)));
            }
        }

        // Top/bottom padding (empty lines).
        let empty_line = " ".repeat(width);
        let mut result: Vec<String> = Vec::new();
        for _ in 0..self.padding_y {
            if let Some(bg) = bg_fn {
                result.push(crate::wrap::apply_background_to_line(
                    &empty_line,
                    width,
                    |t| bg(t),
                ));
            } else {
                result.push(empty_line.clone());
            }
        }
        result.extend(content_lines);
        for _ in 0..self.padding_y {
            if let Some(bg) = bg_fn {
                result.push(crate::wrap::apply_background_to_line(
                    &empty_line,
                    width,
                    |t| bg(t),
                ));
            } else {
                result.push(empty_line.clone());
            }
        }

        self.cached_text = Some(self.text.clone());
        self.cached_width = Some(width);
        self.cached_lines = Some(result.clone());
        if result.is_empty() {
            vec![String::new()]
        } else {
            result
        }
    }

    fn invalidate(&mut self) {
        self.cached_text = None;
        self.cached_width = None;
        self.cached_lines = None;
    }
}

/// True when there is at least one blank line between two sibling blocks
/// (mirrors marked emitting a `space` token for the gap).
fn has_blank_gap<'a>(prev: &'a AstNode<'a>, next: &'a AstNode<'a>) -> bool {
    let prev_end = prev.data.borrow().sourcepos.end.line;
    let next_start = next.data.borrow().sourcepos.start.line;
    next_start.saturating_sub(prev_end) > 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown_theme::create_markdown_theme;
    use crate::theme::ColorToken;
    use crate::theme::{DARK_COLORS, set_palette};
    use serde::Deserialize;
    use std::fs;

    #[derive(Deserialize)]
    struct Fixture {
        name: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        width: usize,
        #[serde(default)]
        lines: Vec<String>,
    }

    fn golden_path() -> String {
        format!(
            "{}/testdata/markdown-golden.jsonl",
            env!("CARGO_MANIFEST_DIR")
        )
    }

    #[test]
    fn markdown_golden_byte_exact() {
        set_palette(DARK_COLORS);
        let data = fs::read_to_string(golden_path()).expect("golden file");
        let mut passed = 0usize;
        for line in data.lines() {
            let fixture: Fixture = serde_json::from_str(line).expect("fixture json");
            if fixture.name == "chalk" {
                continue;
            }
            let mut md = if fixture.name == "default_style_italic" {
                // Thinking-style default: gray italic (chalk.hex('#888888').italic).
                let style = DefaultTextStyle {
                    color: Some(Box::new(move |t| {
                        // chalk.hex('#888888').italic(t) — chain [FG_HEX, ITALIC].
                        crate::style::StyleChain::new(vec![
                            crate::style::fg_hex(0x88, 0x88, 0x88),
                            crate::style::ITALIC,
                        ])
                        .apply(t)
                    })),
                    bg_color: None,
                    bold: false,
                    italic: false,
                    strikethrough: false,
                    underline: false,
                };
                Markdown::new(
                    &fixture.text,
                    0,
                    0,
                    Box::new(create_markdown_theme()),
                    Some(style),
                    MarkdownOptions::default(),
                )
            } else {
                Markdown::new(
                    &fixture.text,
                    0,
                    0,
                    Box::new(create_markdown_theme()),
                    None,
                    MarkdownOptions::default(),
                )
            };
            let rendered = md.render(fixture.width);
            assert_eq!(
                rendered, fixture.lines,
                "fixture {} (width {})",
                fixture.name, fixture.width
            );
            passed += 1;
        }
        eprintln!("markdown golden passed: {passed} fixtures");
    }

    #[test]
    fn default_style_italic_chain_matches_capture() {
        // chalk.hex('#888888').italic(x) — chain [FG_HEX, ITALIC].
        let s = crate::style::StyleChain::new(vec![
            crate::style::fg_hex(0x88, 0x88, 0x88),
            crate::style::ITALIC,
        ])
        .apply("x");
        assert_eq!(s, "\x1b[38;2;136;136;136m\x1b[3mx\x1b[23m\x1b[39m");
    }

    #[test]
    fn style_chain_applies_per_line() {
        // chalk styles each line independently.
        let s = crate::style::StyleChain::new(vec![crate::style::ITALIC]).apply("a\nb");
        assert_eq!(s, "\x1b[3ma\x1b[23m\n\x1b[3mb\x1b[23m");
        let s = crate::style::StyleChain::new(vec![
            crate::style::fg_hex(0x88, 0x88, 0x88),
            crate::style::ITALIC,
        ])
        .apply("a\nb");
        assert_eq!(
            s,
            "\x1b[38;2;136;136;136m\x1b[3ma\x1b[23m\x1b[39m\n\x1b[38;2;136;136;136m\x1b[3mb\x1b[23m\x1b[39m"
        );
    }

    #[test]
    fn link_fallback_url_shown() {
        set_palette(DARK_COLORS);
        let mut md = Markdown::new(
            "See [example](https://example.com) for details",
            0,
            0,
            Box::new(create_markdown_theme()),
            None,
            MarkdownOptions::default(),
        );
        let lines = md.render(80);
        let joined = lines.join("\n");
        assert!(
            joined.contains("https://example.com"),
            "url should be in parens: {joined}"
        );
        assert!(joined.contains("\x1b[38;2;79;168;255m\x1b[4mexample\x1b[24m\x1b[39m"));
        assert!(joined.contains("\x1b[38;2;107;107;107m (https://example.com)\x1b[39m"));
    }

    #[test]
    fn theme_token_sanity() {
        let theme = crate::theme::current_theme();
        assert_eq!(theme.color(ColorToken::Primary), "#4FA8FF");
    }
}
