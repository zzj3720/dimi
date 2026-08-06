//! Shared tab strip renderer for tabbed dialogs (model selector, plugin
//! marketplace, …) — port of
//! `apps/dimi/src/tui/utils/tab-strip.ts` (`renderTabStrip`).
//!
//! The active tab is filled with the brand background, inactive tabs are
//! muted — matching the AskUserQuestion dialog. When the strip is wider than
//! the terminal, it scrolls to keep the active tab visible, framed by `<`/`>`
//! markers.

use crate::style::{BOLD, StyleChain, bg_hex, fg_hex};
use crate::theme::{ColorPalette, hex_to_rgb};
use crate::width::visible_width;

/// Style one tab cell. Active and inactive cells have the same visible width
/// so switching never shifts the layout (`styleTab`).
fn style_tab(label: &str, is_active: bool, colors: &ColorPalette) -> String {
    let cell = format!(" {label} ");
    if is_active {
        // chalk.bgHex(primary).hex(text).bold(cell) — bg opens first, then fg,
        // then bold; closes in reverse.
        let (pr, pg, pb) = hex_to_rgb(colors.primary);
        let (tr, tg, tb) = hex_to_rgb(colors.text);
        StyleChain::new(vec![bg_hex(pr, pg, pb), fg_hex(tr, tg, tb), BOLD]).apply(&cell)
    } else {
        // chalk.hex(textMuted)(cell)
        let (r, g, b) = hex_to_rgb(colors.text_muted);
        StyleChain::single(fg_hex(r, g, b)).apply(&cell)
    }
}

/// `renderTabStrip` — render the strip into exactly one ANSI line.
pub fn render_tab_strip(
    labels: &[&str],
    active_index: usize,
    width: usize,
    colors: &ColorPalette,
) -> String {
    let segments: Vec<String> = labels
        .iter()
        .enumerate()
        .map(|(i, label)| style_tab(label, i == active_index, colors))
        .collect();

    // If everything fits with a leading space, show the whole strip. Account
    // for the single spaces `join(' ')` inserts between tabs.
    let total_segment_width: usize = segments.iter().map(|s| visible_width(s)).sum();
    let full_separator_width = segments.len().saturating_sub(1);
    if 1 + total_segment_width + full_separator_width <= width {
        return format!(" {}", segments.join(" "));
    }

    // Scrolling needed. Find the widest window that contains activeIndex.
    let segment_widths: Vec<usize> = segments.iter().map(|s| visible_width(s)).collect();
    let mut start = active_index;
    let mut end = active_index + 1;
    let mut content_width = segment_widths.get(active_index).copied().unwrap_or(0);

    let fits = |s: usize, e: usize, cw: usize| -> bool {
        let need_left = s > 0;
        let need_right = e < segments.len();
        let frame_width = (if need_left { 2 } else { 1 }) + (if need_right { 2 } else { 0 });
        let separators = e.saturating_sub(s).saturating_sub(1);
        cw + separators + frame_width <= width
    };

    loop {
        let left_w = if start > 0 {
            segment_widths[start - 1]
        } else {
            usize::MAX
        };
        let right_w = if end < segments.len() {
            segment_widths[end]
        } else {
            usize::MAX
        };
        if left_w == usize::MAX && right_w == usize::MAX {
            break;
        }

        if left_w <= right_w {
            if start > 0 && fits(start - 1, end, content_width + left_w) {
                content_width += left_w;
                start -= 1;
            } else if end < segments.len() && fits(start, end + 1, content_width + right_w) {
                content_width += right_w;
                end += 1;
            } else {
                break;
            }
        } else if end < segments.len() && fits(start, end + 1, content_width + right_w) {
            content_width += right_w;
            end += 1;
        } else if start > 0 && fits(start - 1, end, content_width + left_w) {
            content_width += left_w;
            start -= 1;
        } else {
            break;
        }
    }

    let has_left = start > 0;
    let has_right = end < segments.len();
    let (r, g, b) = hex_to_rgb(colors.text_muted);
    let muted = |s: &str| StyleChain::single(fg_hex(r, g, b)).apply(s);
    let mut strip = if has_left {
        muted("< ")
    } else {
        " ".to_owned()
    };
    strip.push_str(&segments[start..end].join(" "));
    if has_right {
        strip.push_str(&muted(" >"));
    }
    strip
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK_COLORS, set_palette};

    #[test]
    fn fits_all_tabs() {
        set_palette(DARK_COLORS);
        let strip = render_tab_strip(&["All", "anthropic", "kimi"], 0, 40, &DARK_COLORS);
        let plain = crate::ansi::strip_ansi(&strip);
        // ' ' + segments.join(' '), where each segment is " {label} " — so a
        // leading space plus double spaces between tabs.
        assert_eq!(plain, "  All   anthropic   kimi ");
    }

    #[test]
    fn active_tab_uses_bg() {
        set_palette(DARK_COLORS);
        let strip = render_tab_strip(&["All", "anthropic"], 0, 40, &DARK_COLORS);
        // Active tab opens with a background colour (48;2).
        assert!(strip.contains("\x1b[48;2;"), "{strip}");
        assert!(strip.contains(" All "), "{strip}");
        // Inactive tab has no background.
        let inactive = strip.split(" All ").nth(1).unwrap_or("");
        assert!(!inactive.contains("\x1b[48;2;"), "{strip}");
    }

    #[test]
    fn scrolls_with_markers() {
        set_palette(DARK_COLORS);
        let labels: Vec<String> = (0..10).map(|i| format!("tab{i}")).collect();
        let labels_ref: Vec<&str> = labels.iter().map(|s| s.as_ref()).collect();
        let strip = render_tab_strip(&labels_ref, 8, 20, &DARK_COLORS);
        let plain = crate::ansi::strip_ansi(&strip);
        assert!(plain.starts_with("< "), "{plain}");
        assert!(plain.ends_with(" >"), "{plain}");
        // Active tab visible.
        assert!(plain.contains(" tab8 "), "{plain}");
    }

    #[test]
    fn visible_width_never_exceeds() {
        set_palette(DARK_COLORS);
        let labels: Vec<String> = (0..8).map(|i| format!("tab{i}")).collect();
        let labels_ref: Vec<&str> = labels.iter().map(|s| s.as_ref()).collect();
        for active in 0..labels.len() {
            let strip = render_tab_strip(&labels_ref, active, 20, &DARK_COLORS);
            assert!(
                visible_width(&strip) <= 20,
                "width {} > 20 for active {active}: {strip}",
                visible_width(&strip)
            );
        }
    }
}
