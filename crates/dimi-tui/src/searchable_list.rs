//! Generic cursor + fuzzy-search + paging state machine shared by list
//! pickers — port of `apps/dimi/src/tui/utils/searchable-list.ts`.
//!
//! The component owns presentation and the keys that carry component-specific
//! meaning — Enter (submit), Esc (cancel), and ←/→ (paging in one picker, a
//! thinking toggle in another). This unit owns the keys that behave
//! identically everywhere: ↑/↓, PgUp/PgDn, and search editing.

use crate::fuzzy::fuzzy_filter;
use crate::keys::{decode_printable_key, matches_key};
use crate::paging::{PageView, page_view};

const DEFAULT_PAGE_SIZE: usize = 8;

/// A searchable, pageable list of `T` with a cursor (port of `SearchableList`).
pub struct SearchableList<T> {
    items: Vec<T>,
    to_search_text: Box<dyn Fn(&T) -> String>,
    page_size: usize,
    searchable: bool,
    query: String,
    cursor: usize,
}

/// Snapshot of the filtered list + paging for rendering (port of
/// `SearchableListView`).
#[derive(Debug, Clone)]
pub struct SearchableView<T> {
    /// Items after the active query filter.
    pub items: Vec<T>,
    /// Page math for the current cursor over `items`.
    pub page: PageView,
    /// Cursor clamped into the current `items` range.
    pub selected_index: usize,
    pub query: String,
}

impl<T> SearchableList<T> {
    /// `to_search_text` mirrors `toSearchText` (text a list item is
    /// fuzzy-matched against).
    pub fn new(
        items: Vec<T>,
        to_search_text: impl Fn(&T) -> String + 'static,
        page_size: Option<usize>,
        initial_index: Option<usize>,
        searchable: bool,
    ) -> Self {
        SearchableList {
            items,
            to_search_text: Box::new(to_search_text),
            page_size: page_size.unwrap_or(DEFAULT_PAGE_SIZE),
            searchable,
            query: String::new(),
            cursor: initial_index.unwrap_or(0),
        }
    }

    /// Items after the active query filter.
    pub fn filtered(&self) -> Vec<T>
    where
        T: Clone,
    {
        if self.query.is_empty() {
            self.items.clone()
        } else {
            fuzzy_filter(self.items.clone(), &self.query, &self.to_search_text)
        }
    }

    /// The item under the cursor, clamped into the filtered range.
    pub fn selected(&self) -> Option<T>
    where
        T: Clone,
    {
        let items = self.filtered();
        if items.is_empty() {
            return None;
        }
        items.get(self.cursor.min(items.len() - 1)).cloned()
    }

    /// Current filtered view for rendering.
    pub fn view(&self) -> SearchableView<T>
    where
        T: Clone,
    {
        let items = self.filtered();
        let total = items.len();
        let selected_index = if total == 0 {
            0
        } else {
            self.cursor.min(total - 1)
        };
        SearchableView {
            page: page_view(total, self.cursor, self.page_size),
            items,
            selected_index,
            query: self.query.clone(),
        }
    }

    pub fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_down(&mut self)
    where
        T: Clone,
    {
        let max = self.filtered().len().saturating_sub(1);
        self.cursor = self.cursor.min(max).saturating_add(1).min(max);
    }

    pub fn page_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(self.page_size);
    }

    pub fn page_down(&mut self)
    where
        T: Clone,
    {
        let max = self.filtered().len().saturating_sub(1);
        self.cursor = self.cursor.saturating_add(self.page_size).min(max);
    }

    /// Clears the active query and resets the cursor. Returns whether a query
    /// was cleared.
    pub fn clear_query(&mut self) -> bool {
        if self.query.is_empty() {
            return false;
        }
        self.query.clear();
        self.cursor = 0;
        true
    }

    /// Handles the keys every picker shares: ↑/↓, PgUp/PgDn, and — when
    /// searchable — Backspace and printable characters. Returns true when the
    /// key was consumed. Enter, Esc, and ←/→ are intentionally left to the
    /// component.
    pub fn handle_key(&mut self, data: &str) -> bool
    where
        T: Clone,
    {
        if matches_key(data, "up") {
            self.move_up();
            return true;
        }
        if matches_key(data, "down") {
            self.move_down();
            return true;
        }
        if matches_key(data, "pageup") || matches_key(data, "pageUp") {
            self.page_up();
            return true;
        }
        if matches_key(data, "pagedown") || matches_key(data, "pageDown") {
            self.page_down();
            return true;
        }
        if !self.searchable {
            return false;
        }
        if matches_key(data, "backspace") {
            if !self.query.is_empty() {
                self.query.pop();
                self.cursor = 0;
            }
            return true;
        }
        // `printableChar(data)` in the TS = `decodeKittyPrintable(data) ?? data`:
        // a bare single printable character (letter/digit/space/punct) also
        // reaches the query, not just CSI-u kitty sequences.
        if let Some(ch) = decode_printable_key(data) {
            self.query.push_str(&ch);
            self.cursor = 0;
            return true;
        }
        let is_single_printable = data.chars().count() == 1
            && data.chars().next().is_some_and(|c| {
                let code = c as u32;
                code >= 0x20 && code != 0x7f
            });
        if is_single_printable {
            self.query.push_str(data);
            self.cursor = 0;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> SearchableList<&'static str> {
        SearchableList::new(
            vec!["alpha", "beta", "gamma"],
            |s| (*s).to_owned(),
            None,
            Some(1),
            true,
        )
    }

    #[test]
    fn view_reports_selected_index() {
        let l = list();
        let v = l.view();
        assert_eq!(v.selected_index, 1);
        assert_eq!(v.items.len(), 3);
    }

    #[test]
    fn up_down_clamp() {
        let mut l = list();
        l.move_up();
        assert_eq!(l.view().selected_index, 0);
        l.move_up(); // clamps at top
        assert_eq!(l.view().selected_index, 0);
        l.move_down();
        assert_eq!(l.view().selected_index, 1);
    }

    #[test]
    fn query_filters_and_resets_cursor() {
        let mut l = list();
        l.handle_key("\x1b[B");
        l.handle_key("a");
        l.handle_key("l"); // "al" → only alpha matches
        assert_eq!(l.view().items, vec!["alpha"]);
        assert_eq!(l.view().selected_index, 0);
        assert!(l.clear_query());
        assert!(!l.clear_query());
        assert_eq!(l.view().items.len(), 3);
    }

    #[test]
    fn page_up_down() {
        let items: Vec<String> = (0..20).map(|i| format!("item{i}")).collect();
        let mut l = SearchableList::new(items, |s| s.clone(), Some(8), None, false);
        l.move_down();
        l.move_down();
        assert_eq!(l.view().selected_index, 2);
        l.page_down();
        assert_eq!(l.view().selected_index, 10);
        l.page_up();
        assert_eq!(l.view().selected_index, 2);
    }

    #[test]
    fn selected_clamped_when_cursor_overruns() {
        let mut l = list();
        l.move_down();
        l.move_down();
        l.move_down();
        assert_eq!(l.selected(), Some("gamma"));
        l.move_down();
        assert_eq!(l.selected(), Some("gamma"));
    }
}
