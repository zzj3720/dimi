//! Color palette — the semantic `ColorPalette` from
//! `apps/dimi/src/tui/theme/colors.ts`, ported verbatim.
//!
//! This is the single source of truth for color tokens in the Rust TUI.
//! Mirror discipline: when a token is added/renamed/removed in the TS
//! `ColorPalette`, the same change must land here (and in the docs/schema
//! mirrors per the apps/dimi AGENTS.md hard rule).

/// Semantic color tokens consumed by every UI component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Semantic color token — mirrors `ColorToken` (`keyof ColorPalette`) in the
/// TS theme. Used to look up a hex value from a palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorToken {
    Primary,
    Accent,
    Text,
    TextStrong,
    TextDim,
    TextMuted,
    Border,
    BorderFocus,
    Success,
    Warning,
    Error,
    DiffAdded,
    DiffRemoved,
    DiffAddedStrong,
    DiffRemovedStrong,
    DiffGutter,
    DiffMeta,
    RoleUser,
    ShellMode,
}

impl ColorToken {
    pub fn hex(&self, palette: &ColorPalette) -> &'static str {
        match self {
            ColorToken::Primary => palette.primary,
            ColorToken::Accent => palette.accent,
            ColorToken::Text => palette.text,
            ColorToken::TextStrong => palette.text_strong,
            ColorToken::TextDim => palette.text_dim,
            ColorToken::TextMuted => palette.text_muted,
            ColorToken::Border => palette.border,
            ColorToken::BorderFocus => palette.border_focus,
            ColorToken::Success => palette.success,
            ColorToken::Warning => palette.warning,
            ColorToken::Error => palette.error,
            ColorToken::DiffAdded => palette.diff_added,
            ColorToken::DiffRemoved => palette.diff_removed,
            ColorToken::DiffAddedStrong => palette.diff_added_strong,
            ColorToken::DiffRemovedStrong => palette.diff_removed_strong,
            ColorToken::DiffGutter => palette.diff_gutter,
            ColorToken::DiffMeta => palette.diff_meta,
            ColorToken::RoleUser => palette.role_user,
            ColorToken::ShellMode => palette.shell_mode,
        }
    }
}

/// Parse a `#RRGGBB` hex color into `(r, g, b)`.
pub(crate) fn hex_to_rgb(hex: &str) -> (u8, u8, u8) {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    (r, g, b)
}

/// Theme — the `currentTheme` singleton equivalent. Holds the active palette;
/// style methods return ANSI-styled strings (byte-aligned with the TS
/// `Theme` class which delegates to chalk).
#[derive(Debug, Clone)]
pub struct Theme {
    palette: ColorPalette,
}

impl Theme {
    pub fn new(palette: ColorPalette) -> Self {
        Theme { palette }
    }

    pub fn palette(&self) -> &ColorPalette {
        &self.palette
    }

    pub fn set_palette(&mut self, palette: ColorPalette) {
        self.palette = palette;
    }

    /// The hex color string for a token.
    pub fn color(&self, token: ColorToken) -> String {
        token.hex(&self.palette).to_owned()
    }

    fn fg_style(&self, token: ColorToken) -> crate::style::Style {
        let (r, g, b) = hex_to_rgb(token.hex(&self.palette));
        crate::style::fg_hex(r, g, b)
    }

    fn bg_style(&self, token: ColorToken) -> crate::style::Style {
        let (r, g, b) = hex_to_rgb(token.hex(&self.palette));
        crate::style::bg_hex(r, g, b)
    }

    /// `chalk.hex(token)(text)` — foreground only.
    pub fn fg(&self, token: ColorToken, text: &str) -> String {
        use crate::style::StyleChain;
        StyleChain::single(self.fg_style(token)).apply(text)
    }

    /// `chalk.hex(token).bold(text)`.
    pub fn bold_fg(&self, token: ColorToken, text: &str) -> String {
        use crate::style::{BOLD, StyleChain};
        StyleChain::new(vec![self.fg_style(token), BOLD]).apply(text)
    }

    /// `chalk.bold.hex(token)(text)` — bold opens *before* the colour.
    ///
    /// Some TS components call `chalk.bold.hex(...)` directly instead of
    /// `currentTheme.boldFg(...)`; chalk applies styles in chain order, so
    /// this variant emits `\x1b[1m` before the `38;2` colour and closes in
    /// reverse. Used by `welcome.ts`, `diff-preview.ts`, and the markdown
    /// heading theme — do not "normalise" it back to [`bold_fg`].
    pub fn bold_hex(&self, token: ColorToken, text: &str) -> String {
        use crate::style::{BOLD, StyleChain};
        StyleChain::new(vec![BOLD, self.fg_style(token)]).apply(text)
    }

    /// `chalk.hex(token).dim(text)`.
    pub fn dim_fg(&self, token: ColorToken, text: &str) -> String {
        use crate::style::{DIM, StyleChain};
        StyleChain::new(vec![self.fg_style(token), DIM]).apply(text)
    }

    /// `chalk.hex(token).italic(text)`.
    pub fn italic_fg(&self, token: ColorToken, text: &str) -> String {
        use crate::style::{ITALIC, StyleChain};
        StyleChain::new(vec![self.fg_style(token), ITALIC]).apply(text)
    }

    /// `chalk.hex(token).underline(text)`.
    pub fn underline_fg(&self, token: ColorToken, text: &str) -> String {
        use crate::style::{StyleChain, UNDERLINE};
        StyleChain::new(vec![self.fg_style(token), UNDERLINE]).apply(text)
    }

    /// `chalk.hex(token).strikethrough(text)`.
    pub fn strikethrough_fg(&self, token: ColorToken, text: &str) -> String {
        use crate::style::{STRIKETHROUGH, StyleChain};
        StyleChain::new(vec![self.fg_style(token), STRIKETHROUGH]).apply(text)
    }

    /// `chalk.bgHex(token)(text)`.
    pub fn bg(&self, token: ColorToken, text: &str) -> String {
        use crate::style::StyleChain;
        StyleChain::single(self.bg_style(token)).apply(text)
    }

    /// `chalk.bold(text)`.
    pub fn bold(&self, text: &str) -> String {
        use crate::style::StyleChain;
        StyleChain::single(crate::style::BOLD).apply(text)
    }

    /// `chalk.dim(text)`.
    pub fn dim(&self, text: &str) -> String {
        use crate::style::StyleChain;
        StyleChain::single(crate::style::DIM).apply(text)
    }

    /// `chalk.italic(text)`.
    pub fn italic(&self, text: &str) -> String {
        use crate::style::StyleChain;
        StyleChain::single(crate::style::ITALIC).apply(text)
    }

    /// `chalk.underline(text)`.
    pub fn underline(&self, text: &str) -> String {
        use crate::style::StyleChain;
        StyleChain::single(crate::style::UNDERLINE).apply(text)
    }

    /// `chalk.strikethrough(text)`.
    pub fn strikethrough(&self, text: &str) -> String {
        use crate::style::StyleChain;
        StyleChain::single(crate::style::STRIKETHROUGH).apply(text)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::new(DARK_COLORS)
    }
}

thread_local! {
    /// Global theme singleton — mirrors `currentTheme` in the TS TUI.
    /// Initialised with the dark palette; switch via [`set_palette`].
    static CURRENT_THEME: std::cell::RefCell<Theme> = std::cell::RefCell::new(Theme::default());
}

/// Read the current theme (snapshot copy).
pub fn current_theme() -> Theme {
    CURRENT_THEME.with(|t| t.borrow().clone())
}

/// Replace the global palette (theme switch).
pub fn set_palette(palette: ColorPalette) {
    CURRENT_THEME.with(|t| t.borrow_mut().set_palette(palette));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_hex_matches_palette() {
        assert_eq!(ColorToken::Primary.hex(&DARK_COLORS), "#4FA8FF");
        assert_eq!(ColorToken::RoleUser.hex(&DARK_COLORS), "#FFCB6B");
        assert_eq!(ColorToken::ShellMode.hex(&DARK_COLORS), "#BD93F9");
        assert_eq!(ColorToken::Text.hex(&DARK_COLORS), "#E0E0E0");
        assert_eq!(ColorToken::TextDim.hex(&DARK_COLORS), "#888888");
        assert_eq!(ColorToken::TextMuted.hex(&DARK_COLORS), "#6B6B6B");
        assert_eq!(ColorToken::Border.hex(&DARK_COLORS), "#5A5A5A");
    }

    #[test]
    fn theme_fg_bytes() {
        let theme = Theme::new(DARK_COLORS);
        // chalk.hex('#E0E0E0')('x')
        assert_eq!(
            theme.fg(ColorToken::Text, "x"),
            "\x1b[38;2;224;224;224mx\x1b[39m"
        );
        // chalk.hex('#FFCB6B').bold('x')
        assert_eq!(
            theme.bold_fg(ColorToken::RoleUser, "x"),
            "\x1b[38;2;255;203;107m\x1b[1mx\x1b[22m\x1b[39m"
        );
        // chalk.bold.hex('#4FA8FF')('x') — bold opens before the colour.
        assert_eq!(
            theme.bold_hex(ColorToken::Primary, "x"),
            "\x1b[1m\x1b[38;2;79;168;255mx\x1b[39m\x1b[22m"
        );
        // chalk.dim('x')
        assert_eq!(theme.dim("x"), "\x1b[2mx\x1b[22m");
        // chalk.hex('#E85454').italic('x')
        assert_eq!(
            theme.italic_fg(ColorToken::Error, "x"),
            "\x1b[38;2;232;84;84m\x1b[3mx\x1b[23m\x1b[39m"
        );
    }

    #[test]
    fn singleton_defaults_dark() {
        let t = current_theme();
        assert_eq!(t.color(ColorToken::Primary), "#4FA8FF");
        set_palette(LIGHT_COLORS);
        let t = current_theme();
        assert_eq!(t.color(ColorToken::Primary), "#1565C0");
        set_palette(DARK_COLORS);
    }

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
