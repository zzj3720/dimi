//! Color palette — the semantic `ColorPalette` from
//! `apps/dimi/src/tui/theme/colors.ts`, ported verbatim.
//!
//! This is the single source of truth for color tokens in the Rust TUI.
//! Mirror discipline: when a token is added/renamed/removed in the TS
//! `ColorPalette`, the same change must land here (and in the docs/schema
//! mirrors per the apps/dimi AGENTS.md hard rule).

/// Semantic color tokens consumed by every UI component.
pub struct ColorPalette {
    // ── Brand ──
    /// Dominant interactive/brand colour: links & inline code, the selected
    /// item in nearly every dialog, the focused editor border, plan/"running"
    /// badges, spinners.
    pub primary: &'static str,
    /// Secondary highlight: approval "▶" prefix, device-code box, image
    /// placeholder, BTW / queue panes, custom-registry import.
    pub accent: &'static str,

    // ── Text ──
    /// Default body text.
    pub text: &'static str,
    /// Emphasised / bold text.
    pub text_strong: &'static str,
    /// Secondary, dimmed text.
    pub text_dim: &'static str,
    /// Faintest text.
    pub text_muted: &'static str,

    // ── Surface ──
    /// Borders: pane & editor borders, markdown horizontal rule.
    pub border: &'static str,
    /// Focus / attention border — currently only the approval panel.
    pub border_focus: &'static str,

    // ── State ──
    /// Success: ✓ marks, "enabled", completed states.
    pub success: &'static str,
    /// Warning: auto/yolo badges, stale markers, plan-mode hint.
    pub warning: &'static str,
    /// Error: error messages, failed tool output.
    pub error: &'static str,

    // ── Diff ──
    /// Added lines.
    pub diff_added: &'static str,
    /// Removed lines.
    pub diff_removed: &'static str,
    /// Added lines — intra-line changed words (bold).
    pub diff_added_strong: &'static str,
    /// Removed lines — intra-line changed words (bold).
    pub diff_removed_strong: &'static str,
    /// Line-number gutter (also approval panel/preview).
    pub diff_gutter: &'static str,
    /// Meta / hunk headers.
    pub diff_meta: &'static str,

    // ── Roles ──
    /// User message: bullet & text, skill-activation name.
    pub role_user: &'static str,

    // ── Shell mode ──
    /// Shell mode (`!`): the `!` prompt symbol, bash-mode editor border, and
    /// the echoed `$ command` line.
    pub shell_mode: &'static str,
}

pub const DARK_COLORS: ColorPalette = ColorPalette {
    primary: "#4FA8FF",
    accent: "#5BC0BE",

    text: "#E0E0E0",
    text_strong: "#F5F5F5",
    text_dim: "#888888",
    text_muted: "#6B6B6B",

    border: "#5A5A5A",
    border_focus: "#E8A838",

    success: "#4EC87E",
    warning: "#E8A838",
    error: "#E85454",

    diff_added: "#4EC87E",
    diff_removed: "#E85454",
    diff_added_strong: "#7AD99B",
    diff_removed_strong: "#F08585",
    diff_gutter: "#6B6B6B",
    diff_meta: "#888888",

    role_user: "#FFCB6B",
    shell_mode: "#BD93F9",
};

pub const LIGHT_COLORS: ColorPalette = ColorPalette {
    primary: "#1565C0",
    accent: "#00838F",

    text: "#1A1A1A",
    text_strong: "#1A1A1A",
    text_dim: "#454545",
    text_muted: "#5F5F5F",

    border: "#737373",
    border_focus: "#92660A",

    success: "#1B7F3B",
    warning: "#92660A",
    error: "#B3261E",

    diff_added: "#1B7F3B",
    diff_removed: "#B3261E",
    diff_added_strong: "#0D5C2B",
    diff_removed_strong: "#8E1D16",
    diff_gutter: "#5F5F5F",
    diff_meta: "#454545",

    role_user: "#7A4E00",
    shell_mode: "#5E35B1",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_palette_has_all_tokens_filled() {
        let c = &DARK_COLORS;
        for hex in [
            c.primary,
            c.accent,
            c.text,
            c.text_strong,
            c.text_dim,
            c.text_muted,
            c.border,
            c.border_focus,
            c.success,
            c.warning,
            c.error,
            c.diff_added,
            c.diff_removed,
            c.diff_added_strong,
            c.diff_removed_strong,
            c.diff_gutter,
            c.diff_meta,
            c.role_user,
            c.shell_mode,
        ] {
            assert!(hex.starts_with('#'), "token must be a hex color: {hex}");
            assert_eq!(hex.len(), 7, "token must be #RRGGBB: {hex}");
        }
    }

    #[test]
    fn light_palette_has_all_tokens_filled() {
        let c = &LIGHT_COLORS;
        for hex in [
            c.primary,
            c.accent,
            c.text,
            c.text_strong,
            c.text_dim,
            c.text_muted,
            c.border,
            c.border_focus,
            c.success,
            c.warning,
            c.error,
            c.diff_added,
            c.diff_removed,
            c.diff_added_strong,
            c.diff_removed_strong,
            c.diff_gutter,
            c.diff_meta,
            c.role_user,
            c.shell_mode,
        ] {
            assert!(hex.starts_with('#'), "token must be a hex color: {hex}");
            assert_eq!(hex.len(), 7, "token must be #RRGGBB: {hex}");
        }
    }
}
