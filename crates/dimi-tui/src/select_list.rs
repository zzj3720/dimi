//! `SelectList` — a scrollable selection list with descriptions
//! (port of `@dimi-agent/pi-tui` `src/components/select-list.ts`).

use crate::component::Component;
use crate::keys::matches_key;
use crate::width::visible_width;
use crate::wrap::truncate_to_width;

const DEFAULT_PRIMARY_COLUMN_WIDTH: usize = 32;
const PRIMARY_COLUMN_GAP: usize = 2;
const MIN_DESCRIPTION_WIDTH: usize = 10;

fn normalize_to_single_line(text: &str) -> String {
    text.split(['\r', '\n'])
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned()
}

fn clamp(value: usize, min: usize, max: usize) -> usize {
    value.max(min).min(max)
}

/// One selectable item.
#[derive(Debug, Clone)]
pub struct SelectItem {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

impl SelectItem {
    pub fn new(value: &str, label: &str, description: Option<&str>) -> Self {
        SelectItem {
            value: value.to_owned(),
            label: label.to_owned(),
            description: description.map(str::to_owned),
        }
    }
}

/// Theme hooks for the selection list.
pub trait SelectListTheme {
    fn selected_prefix(&self, text: &str) -> String;
    fn selected_text(&self, text: &str) -> String;
    fn description(&self, text: &str) -> String;
    fn scroll_info(&self, text: &str) -> String;
    fn no_match(&self, text: &str) -> String;
}

/// Layout options for the primary column.
#[derive(Debug, Clone, Default)]
pub struct SelectListLayoutOptions {
    pub min_primary_column_width: Option<usize>,
    pub max_primary_column_width: Option<usize>,
}

/// A scrollable selection list.
pub struct SelectList {
    items: Vec<SelectItem>,
    filtered_items: Vec<SelectItem>,
    selected_index: usize,
    max_visible: usize,
    theme: Box<dyn SelectListTheme>,
    layout: SelectListLayoutOptions,
}

impl SelectList {
    pub fn new(
        items: Vec<SelectItem>,
        max_visible: usize,
        theme: Box<dyn SelectListTheme>,
        layout: SelectListLayoutOptions,
    ) -> Self {
        SelectList {
            filtered_items: items.clone(),
            items,
            selected_index: 0,
            max_visible,
            theme,
            layout,
        }
    }

    pub fn set_filter(&mut self, filter: &str) {
        let filter_lower = filter.to_lowercase();
        self.filtered_items = self
            .items
            .iter()
            .filter(|item| item.value.to_lowercase().starts_with(&filter_lower))
            .cloned()
            .collect();
        self.selected_index = 0;
    }

    pub fn set_selected_index(&mut self, index: usize) {
        self.selected_index = clamp(index, 0, self.filtered_items.len().saturating_sub(1));
    }

    pub fn get_selected_item(&self) -> Option<&SelectItem> {
        self.filtered_items.get(self.selected_index)
    }

    fn get_display_value(item: &SelectItem) -> &str {
        if item.label.is_empty() {
            &item.value
        } else {
            &item.label
        }
    }

    fn get_primary_column_bounds(&self) -> (usize, usize) {
        let raw_min = self
            .layout
            .min_primary_column_width
            .or(self.layout.max_primary_column_width)
            .unwrap_or(DEFAULT_PRIMARY_COLUMN_WIDTH);
        let raw_max = self
            .layout
            .max_primary_column_width
            .or(self.layout.min_primary_column_width)
            .unwrap_or(DEFAULT_PRIMARY_COLUMN_WIDTH);
        (raw_min.min(raw_max).max(1), raw_min.max(raw_max).max(1))
    }

    fn get_primary_column_width(&self) -> usize {
        let (min, max) = self.get_primary_column_bounds();
        let widest = self
            .filtered_items
            .iter()
            .map(|item| visible_width(Self::get_display_value(item)) + PRIMARY_COLUMN_GAP)
            .max()
            .unwrap_or(0);
        clamp(widest, min, max)
    }

    fn truncate_primary(&self, item: &SelectItem, max_width: usize) -> String {
        let display_value = Self::get_display_value(item);
        truncate_to_width(display_value, max_width, "", false)
    }

    fn render_item(
        &self,
        item: &SelectItem,
        is_selected: bool,
        width: usize,
        description_single_line: Option<&str>,
        primary_column_width: usize,
    ) -> String {
        let prefix = if is_selected { "→ " } else { "  " };
        let prefix_width = visible_width(prefix);

        if let Some(desc) = description_single_line {
            if width > 40 {
                let effective_primary =
                    (primary_column_width.min(width.saturating_sub(prefix_width + 4))).max(1);
                let max_primary_width =
                    (effective_primary.saturating_sub(PRIMARY_COLUMN_GAP)).max(1);
                let truncated_value = self.truncate_primary(item, max_primary_width);
                let truncated_value_width = visible_width(&truncated_value);
                let spacing =
                    " ".repeat((effective_primary.saturating_sub(truncated_value_width)).max(1));
                let description_start = prefix_width + truncated_value_width + spacing.len();
                let remaining_width = width.saturating_sub(description_start + 2);

                if remaining_width > MIN_DESCRIPTION_WIDTH {
                    let truncated_desc = truncate_to_width(desc, remaining_width, "", false);
                    if is_selected {
                        return self.theme.selected_text(&format!(
                            "{prefix}{truncated_value}{spacing}{truncated_desc}"
                        ));
                    }
                    let desc_text = self
                        .theme
                        .description(&format!("{spacing}{truncated_desc}"));
                    return format!("{prefix}{truncated_value}{desc_text}");
                }
            }
        }

        let max_width = width.saturating_sub(prefix_width + 2);
        let truncated_value = self.truncate_primary(item, max_width);
        if is_selected {
            return self
                .theme
                .selected_text(&format!("{prefix}{truncated_value}"));
        }
        format!("{prefix}{truncated_value}")
    }
}

impl Component for SelectList {
    fn render(&mut self, width: usize) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();

        if self.filtered_items.is_empty() {
            lines.push(self.theme.no_match("  No matching commands"));
            return lines;
        }

        let primary_column_width = self.get_primary_column_width();

        let start_index = {
            let half = self.max_visible / 2;
            let max_start = self.filtered_items.len().saturating_sub(self.max_visible);
            clamp(self.selected_index.saturating_sub(half), 0, max_start)
        };
        let end_index = (start_index + self.max_visible).min(self.filtered_items.len());

        for i in start_index..end_index {
            let Some(item) = self.filtered_items.get(i) else {
                continue;
            };
            let is_selected = i == self.selected_index;
            let desc = item.description.as_deref().map(normalize_to_single_line);
            lines.push(self.render_item(
                item,
                is_selected,
                width,
                desc.as_deref(),
                primary_column_width,
            ));
        }

        if start_index > 0 || end_index < self.filtered_items.len() {
            let scroll_text = format!(
                "  ({}/{})",
                self.selected_index + 1,
                self.filtered_items.len()
            );
            lines.push(self.theme.scroll_info(&truncate_to_width(
                &scroll_text,
                width.saturating_sub(2),
                "",
                false,
            )));
        }

        lines
    }

    fn handle_input(&mut self, data: &str) {
        if matches_key(data, "up") {
            self.selected_index = if self.selected_index == 0 {
                self.filtered_items.len().saturating_sub(1)
            } else {
                self.selected_index - 1
            };
        } else if matches_key(data, "down") {
            self.selected_index = if self.selected_index + 1 >= self.filtered_items.len() {
                0
            } else {
                self.selected_index + 1
            };
        }
        // Enter / Escape are handled by the dialog host (which owns onSelect /
        // onCancel); the list only moves the selection.
    }

    fn invalidate(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestTheme;
    impl SelectListTheme for TestTheme {
        fn selected_prefix(&self, text: &str) -> String {
            text.to_owned()
        }
        fn selected_text(&self, text: &str) -> String {
            format!("[{}]", text)
        }
        fn description(&self, text: &str) -> String {
            format!("<{}>", text)
        }
        fn scroll_info(&self, text: &str) -> String {
            format!("({})", text.trim())
        }
        fn no_match(&self, text: &str) -> String {
            text.to_owned()
        }
    }

    fn items() -> Vec<SelectItem> {
        vec![
            SelectItem::new("a", "Alpha", Some("first item")),
            SelectItem::new("b", "Beta", Some("second item")),
            SelectItem::new("c", "Gamma", None),
        ]
    }

    #[test]
    fn renders_selected_first() {
        let mut list = SelectList::new(items(), 5, Box::new(TestTheme), Default::default());
        let lines = list.render(80);
        assert_eq!(lines.len(), 3);
        assert!(
            lines[0].starts_with("[→ Alpha"),
            "selected row: {}",
            lines[0]
        );
        assert!(lines[1].starts_with("  Beta"), "unselected: {}", lines[1]);
    }

    #[test]
    fn down_moves_selection() {
        let mut list = SelectList::new(items(), 5, Box::new(TestTheme), Default::default());
        list.handle_input("\x1b[B");
        assert_eq!(
            list.get_selected_item().map(|i| i.value.as_str()),
            Some("b")
        );
        list.handle_input("\x1b[B");
        assert_eq!(
            list.get_selected_item().map(|i| i.value.as_str()),
            Some("c")
        );
        // Wrap to top.
        list.handle_input("\x1b[B");
        assert_eq!(
            list.get_selected_item().map(|i| i.value.as_str()),
            Some("a")
        );
        // Up wraps to bottom.
        list.handle_input("\x1b[A");
        assert_eq!(
            list.get_selected_item().map(|i| i.value.as_str()),
            Some("c")
        );
    }

    #[test]
    fn filter_narrows_and_resets() {
        let mut list = SelectList::new(items(), 5, Box::new(TestTheme), Default::default());
        list.handle_input("\x1b[B");
        list.set_filter("a");
        assert_eq!(list.filtered_items.len(), 1);
        assert_eq!(
            list.get_selected_item().map(|i| i.value.as_str()),
            Some("a")
        );
    }

    #[test]
    fn no_match_message() {
        let mut list = SelectList::new(items(), 5, Box::new(TestTheme), Default::default());
        list.set_filter("zzz");
        let lines = list.render(80);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("No matching"));
    }

    #[test]
    fn scroll_indicator_when_many() {
        let many: Vec<SelectItem> = (0..20)
            .map(|i| SelectItem::new(&format!("v{i}"), &format!("Item {i}"), None))
            .collect();
        let mut list = SelectList::new(many, 5, Box::new(TestTheme), Default::default());
        let lines = list.render(80);
        assert_eq!(lines.len(), 6); // 5 visible + scroll info
        assert!(lines[5].contains("(1/20)"), "scroll info: {}", lines[5]);
    }
}
