//! Pure paging math shared by list pickers — port of
//! `apps/dimi/src/tui/utils/paging.ts`.

/// Page geometry derived from a cursor position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageView {
    /// Zero-based index of the page containing `selected_index`.
    pub page: usize,
    /// Total number of pages; always at least 1, even for an empty list.
    pub page_count: usize,
    /// Inclusive slice start of the current page.
    pub start: usize,
    /// Exclusive slice end of the current page (clamped to `total`).
    pub end: usize,
}

/// Compute the page containing `selected_index` over `total` items.
pub fn page_view(total: usize, selected_index: usize, page_size: usize) -> PageView {
    let size = page_size.max(1);
    let page_count = total.div_ceil(size).max(1);
    let safe_index = if total == 0 {
        0
    } else {
        selected_index.min(total - 1)
    };
    let page = (safe_index / size).min(page_count - 1);
    let start = page * size;
    let end = (start + size).min(total);
    PageView {
        page,
        page_count,
        start,
        end,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_list_one_page() {
        let v = page_view(0, 0, 8);
        assert_eq!(v.page, 0);
        assert_eq!(v.page_count, 1);
        assert_eq!(v.start, 0);
        assert_eq!(v.end, 0);
    }

    #[test]
    fn single_page() {
        let v = page_view(3, 1, 8);
        assert_eq!(v.page, 0);
        assert_eq!(v.page_count, 1);
        assert_eq!((v.start, v.end), (0, 3));
    }

    #[test]
    fn second_page() {
        let v = page_view(20, 9, 8);
        assert_eq!(v.page, 1);
        assert_eq!(v.page_count, 3);
        assert_eq!((v.start, v.end), (8, 16));
    }

    #[test]
    fn last_page_clamped_end() {
        let v = page_view(20, 19, 8);
        assert_eq!(v.page, 2);
        assert_eq!(v.page_count, 3);
        assert_eq!((v.start, v.end), (16, 20));
    }

    #[test]
    fn out_of_range_index_clamped() {
        let v = page_view(5, 99, 8);
        assert_eq!(v.page, 0);
        assert_eq!(v.end, 5);
    }

    #[test]
    fn page_size_at_least_one() {
        let v = page_view(3, 2, 0);
        assert_eq!(v.page_count, 3);
        assert_eq!(v.page, 2);
    }
}
