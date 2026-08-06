//! Markdown theme — the `MarkdownTheme` contract plus the dimi dark/light
//! implementation, ported from `apps/dimi/src/tui/theme/pi-tui-theme.ts` and
//! `packages/pi-tui/test/test-themes.ts`.

use regex::Regex;

use crate::theme::{ColorToken, current_theme, hex_to_rgb};

/// Strips the literal `### `-style hash prefix from heading text before
/// re-styling. The pi-tui renderer emits the prefix for h3+ already wrapped
/// in bold SGR codes; mirror `HEADING_HASH_PREFIX`:
/// `^((?:\x1b\[[0-9;]*m)*)#{1,6}[ \t]+` replaced with `$1`.
fn strip_hash(text: &str) -> String {
    let re = Regex::new(r"^((?:\x1b\[[0-9;]*m)*)#{1,6}[ \t]+").expect("valid heading regex");
    re.replace(text, "$1").into_owned()
}

/// Theme functions for markdown elements — each function takes text and
/// returns text with ANSI styling (mirrors `MarkdownTheme` in pi-tui).
pub trait MarkdownTheme {
    fn heading(&self, text: &str) -> String;
    fn link(&self, text: &str) -> String;
    fn link_url(&self, text: &str) -> String;
    fn code(&self, text: &str) -> String;
    fn code_block(&self, text: &str) -> String;
    fn code_block_border(&self, text: &str) -> String;
    fn quote(&self, text: &str) -> String;
    fn quote_border(&self, text: &str) -> String;
    fn hr(&self, text: &str) -> String;
    fn list_bullet(&self, text: &str) -> String;
    fn bold(&self, text: &str) -> String;
    fn italic(&self, text: &str) -> String;
    fn strikethrough(&self, text: &str) -> String;
    fn underline(&self, text: &str) -> String;
    /// Highlight a code block body. The dimi theme's cli-highlight pass is a
    /// known gap in the Rust port; both transient and full paths passthrough.
    fn highlight_code(&self, code: &str, _lang: Option<&str>) -> Vec<String> {
        code.split('\n').map(str::to_owned).collect()
    }
    /// Prefix applied to each rendered code block line (default: "  ").
    fn code_block_indent(&self) -> &'static str {
        "  "
    }
}

/// The dimi `MarkdownTheme`, backed by the global theme singleton so theme
/// switching takes effect within a single render (like the TS adapter which
/// routes every call through `currentTheme`).
#[derive(Debug, Clone, Default)]
pub struct DimiMarkdownTheme;

impl DimiMarkdownTheme {
    pub fn new() -> Self {
        DimiMarkdownTheme
    }
}

fn theme() -> crate::theme::Theme {
    current_theme()
}

impl MarkdownTheme for DimiMarkdownTheme {
    fn heading(&self, text: &str) -> String {
        // chalk.bold.hex(color('text'))(text) — chain [BOLD, FG_HEX].
        use crate::style::{BOLD, StyleChain};
        let t = theme();
        let (r, g, b) = hex_to_rgb(ColorToken::Text.hex(t.palette()));
        StyleChain::new(vec![BOLD, crate::style::fg_hex(r, g, b)]).apply(&strip_hash(text))
    }

    fn link(&self, text: &str) -> String {
        theme().fg(ColorToken::Primary, text)
    }

    fn link_url(&self, text: &str) -> String {
        theme().fg(ColorToken::TextMuted, text)
    }

    fn code(&self, text: &str) -> String {
        theme().fg(ColorToken::Primary, text)
    }

    fn code_block(&self, text: &str) -> String {
        text.to_owned()
    }

    fn code_block_border(&self, text: &str) -> String {
        theme().fg(ColorToken::TextMuted, text)
    }

    fn quote(&self, text: &str) -> String {
        theme().fg(ColorToken::TextDim, text)
    }

    fn quote_border(&self, text: &str) -> String {
        theme().fg(ColorToken::TextDim, text)
    }

    fn hr(&self, text: &str) -> String {
        theme().fg(ColorToken::Border, text)
    }

    fn list_bullet(&self, text: &str) -> String {
        // Match the assistant-message bullet: unordered `- ` becomes `• `.
        let replaced = if let Some(rest) = text.strip_prefix('-') {
            format!("•{rest}")
        } else {
            text.to_owned()
        };
        theme().fg(ColorToken::Text, &replaced)
    }

    fn bold(&self, text: &str) -> String {
        theme().bold(text)
    }

    fn italic(&self, text: &str) -> String {
        theme().italic(text)
    }

    fn strikethrough(&self, text: &str) -> String {
        theme().strikethrough(text)
    }

    fn underline(&self, text: &str) -> String {
        theme().underline(text)
    }
}

/// Create the dimi markdown theme reading the current palette.
pub fn create_markdown_theme() -> DimiMarkdownTheme {
    DimiMarkdownTheme::new()
}

/// Transient variant — identical in the Rust port because code highlighting
/// is a passthrough for both (the TS transient path skips cli-highlight).
pub fn create_markdown_theme_transient() -> DimiMarkdownTheme {
    DimiMarkdownTheme::new()
}

// ---------------------------------------------------------------------------
// Test theme (aligns with packages/pi-tui/test/test-themes.ts)
// ---------------------------------------------------------------------------

fn ansi(open: &'static str, close: &'static str, text: &str) -> String {
    format!("{open}{text}{close}")
}

/// Default markdown theme used by pi-tui's own tests (16-color ANSI).
#[derive(Debug, Clone, Default)]
pub struct TestMarkdownTheme;

impl MarkdownTheme for TestMarkdownTheme {
    fn heading(&self, text: &str) -> String {
        ansi("\x1b[1m\x1b[36m", "\x1b[39m\x1b[22m", text)
    }
    fn link(&self, text: &str) -> String {
        ansi("\x1b[34m", "\x1b[39m", text)
    }
    fn link_url(&self, text: &str) -> String {
        ansi("\x1b[2m", "\x1b[22m", text)
    }
    fn code(&self, text: &str) -> String {
        ansi("\x1b[33m", "\x1b[39m", text)
    }
    fn code_block(&self, text: &str) -> String {
        ansi("\x1b[32m", "\x1b[39m", text)
    }
    fn code_block_border(&self, text: &str) -> String {
        ansi("\x1b[2m", "\x1b[22m", text)
    }
    fn quote(&self, text: &str) -> String {
        ansi("\x1b[3m", "\x1b[23m", text)
    }
    fn quote_border(&self, text: &str) -> String {
        ansi("\x1b[2m", "\x1b[22m", text)
    }
    fn hr(&self, text: &str) -> String {
        ansi("\x1b[2m", "\x1b[22m", text)
    }
    fn list_bullet(&self, text: &str) -> String {
        ansi("\x1b[36m", "\x1b[39m", text)
    }
    fn bold(&self, text: &str) -> String {
        ansi("\x1b[1m", "\x1b[22m", text)
    }
    fn italic(&self, text: &str) -> String {
        ansi("\x1b[3m", "\x1b[23m", text)
    }
    fn strikethrough(&self, text: &str) -> String {
        ansi("\x1b[9m", "\x1b[29m", text)
    }
    fn underline(&self, text: &str) -> String {
        ansi("\x1b[4m", "\x1b[24m", text)
    }
}

/// Create the pi-tui test theme (for markdown unit tests).
pub fn test_markdown_theme() -> TestMarkdownTheme {
    TestMarkdownTheme
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK_COLORS, set_palette};

    #[test]
    fn strip_hash_prefix() {
        assert_eq!(strip_hash("### Hello"), "Hello");
        assert_eq!(strip_hash("# Hello"), "Hello");
        assert_eq!(strip_hash("Hello"), "Hello");
        // ANSI-prefixed heading prefix (as emitted by the renderer for h3+).
        assert_eq!(
            strip_hash("\x1b[1m\x1b[38;2;224;224;224m### Hello"),
            "\x1b[1m\x1b[38;2;224;224;224mHello"
        );
    }

    #[test]
    fn dimi_heading_bytes() {
        set_palette(DARK_COLORS);
        let t = create_markdown_theme();
        // chalk.bold.hex(color('text'))(text): [BOLD, FG_HEX] chain.
        assert_eq!(
            t.heading("Hello"),
            "\x1b[1m\x1b[38;2;224;224;224mHello\x1b[39m\x1b[22m"
        );
    }

    #[test]
    fn dimi_list_bullet_replaces_dash() {
        set_palette(DARK_COLORS);
        let t = create_markdown_theme();
        assert_eq!(t.list_bullet("- "), "\x1b[38;2;224;224;224m• \x1b[39m");
        assert_eq!(t.list_bullet("1. "), "\x1b[38;2;224;224;224m1. \x1b[39m");
    }
}
