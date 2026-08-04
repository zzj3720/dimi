//! Chalk-equivalent styled string builder.
//!
//! Byte-aligned with `chalk` (v5) at level 3: a chained style applies a list
//! of SGR open/close pairs. Applying a chain to text that already contains
//! close sequences re-opens each style after its own close (chalk's nested
//! style mechanism), which is what produces the exact byte sequences the TS
//! TUI emits.

/// One SGR attribute: open + close sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub open: &'static str,
    pub close: &'static str,
}

/// True-color foreground: `ESC[38;2;R;G;Bm` / `ESC[39m`.
///
/// The open sequence is built at runtime and leaked once; the number of
/// distinct colors is bounded by the palette size, so the leak is negligible.
pub fn fg_hex(r: u8, g: u8, b: u8) -> Style {
    Style {
        open: Box::leak(format!("\x1b[38;2;{r};{g};{b}m").into_boxed_str()),
        close: "\x1b[39m",
    }
}

/// True-color background: `ESC[48;2;R;G;Bm` / `ESC[49m`.
pub fn bg_hex(r: u8, g: u8, b: u8) -> Style {
    Style {
        open: Box::leak(format!("\x1b[48;2;{r};{g};{b}m").into_boxed_str()),
        close: "\x1b[49m",
    }
}

pub const BOLD: Style = Style {
    open: "\x1b[1m",
    close: "\x1b[22m",
};
pub const DIM: Style = Style {
    open: "\x1b[2m",
    close: "\x1b[22m",
};
pub const ITALIC: Style = Style {
    open: "\x1b[3m",
    close: "\x1b[23m",
};
pub const UNDERLINE: Style = Style {
    open: "\x1b[4m",
    close: "\x1b[24m",
};
pub const STRIKETHROUGH: Style = Style {
    open: "\x1b[9m",
    close: "\x1b[29m",
};

/// A chain of styles applied in order (chalk `chalk.style1.style2(...)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleChain {
    styles: Vec<Style>,
}

impl StyleChain {
    pub fn new(styles: Vec<Style>) -> Self {
        StyleChain { styles }
    }

    pub fn single(style: Style) -> Self {
        StyleChain {
            styles: vec![style],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }

    /// Apply the chain to `text` with chalk's re-open semantics.
    ///
    /// chalk applies styles per line: text is split on `\n` and each line is
    /// styled independently (open + transformed + close), then rejoined. For
    /// each style in chain order, every occurrence of the style's close
    /// sequence inside the line is replaced by `close + open` (re-opening the
    /// style after its own close).
    pub fn apply(&self, text: &str) -> String {
        if self.styles.is_empty() {
            return text.to_owned();
        }
        let mut out = String::with_capacity(text.len() + 16);
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&self.apply_line(line));
        }
        out
    }

    fn apply_line(&self, line: &str) -> String {
        let mut transformed = line.to_owned();
        for style in &self.styles {
            if !style.close.is_empty() && transformed.contains(style.close) {
                let reopen = format!("{}{}", style.close, style.open);
                transformed = transformed.replace(style.close, &reopen);
            }
        }
        let mut out = String::with_capacity(transformed.len() + 16);
        for style in &self.styles {
            out.push_str(style.open);
        }
        out.push_str(&transformed);
        for style in self.styles.iter().rev() {
            out.push_str(style.close);
        }
        out
    }

    /// The concatenated open sequences (used for style-prefix extraction with
    /// the NUL sentinel technique in the markdown renderer).
    pub fn opens(&self) -> String {
        let mut out = String::new();
        for style in &self.styles {
            out.push_str(style.open);
        }
        out
    }
}

impl From<Style> for StyleChain {
    fn from(style: Style) -> Self {
        StyleChain::single(style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(r: u8, g: u8, b: u8) -> Style {
        fg_hex(r, g, b)
    }

    #[test]
    fn single_fg() {
        assert_eq!(
            StyleChain::new(vec![hex(79, 168, 255)]).apply("x"),
            "\x1b[38;2;79;168;255mx\x1b[39m"
        );
    }

    #[test]
    fn fg_then_bold() {
        assert_eq!(
            StyleChain::new(vec![hex(79, 168, 255), BOLD]).apply("x"),
            "\x1b[38;2;79;168;255m\x1b[1mx\x1b[22m\x1b[39m"
        );
    }

    #[test]
    fn bold_then_fg() {
        assert_eq!(
            StyleChain::new(vec![BOLD, hex(224, 224, 224)]).apply("x"),
            "\x1b[1m\x1b[38;2;224;224;224mx\x1b[39m\x1b[22m"
        );
    }

    #[test]
    fn reopen_bold_inside_input() {
        assert_eq!(
            StyleChain::new(vec![BOLD, hex(224, 224, 224)]).apply("\x1b[1mx\x1b[22m"),
            "\x1b[1m\x1b[38;2;224;224;224m\x1b[1mx\x1b[22m\x1b[1m\x1b[39m\x1b[22m"
        );
    }

    #[test]
    fn reopen_fg_inside_input() {
        assert_eq!(
            StyleChain::new(vec![BOLD, hex(224, 224, 224)]).apply("a\x1b[39mb"),
            "\x1b[1m\x1b[38;2;224;224;224ma\x1b[39m\x1b[38;2;224;224;224mb\x1b[39m\x1b[22m"
        );
    }

    #[test]
    fn reverse_chain_reopen() {
        assert_eq!(
            StyleChain::new(vec![hex(224, 224, 224), BOLD]).apply("\x1b[1mx\x1b[22m"),
            "\x1b[38;2;224;224;224m\x1b[1m\x1b[1mx\x1b[22m\x1b[1m\x1b[22m\x1b[39m"
        );
    }

    #[test]
    fn single_style_open_close() {
        assert_eq!(BOLD.open, "\x1b[1m");
        assert_eq!(BOLD.close, "\x1b[22m");
        assert_eq!(DIM.open, "\x1b[2m");
        assert_eq!(DIM.close, "\x1b[22m");
        assert_eq!(ITALIC.open, "\x1b[3m");
        assert_eq!(ITALIC.close, "\x1b[23m");
        assert_eq!(UNDERLINE.open, "\x1b[4m");
        assert_eq!(UNDERLINE.close, "\x1b[24m");
        assert_eq!(STRIKETHROUGH.open, "\x1b[9m");
        assert_eq!(STRIKETHROUGH.close, "\x1b[29m");
    }

    #[test]
    fn rgb_open_sequences() {
        assert_eq!(fg_hex(79, 168, 255).open, "\x1b[38;2;79;168;255m");
        assert_eq!(bg_hex(90, 90, 90).open, "\x1b[48;2;90;90;90m");
        assert_eq!(bg_hex(90, 90, 90).close, "\x1b[49m");
    }

    #[test]
    fn opens_concat() {
        let chain = StyleChain::new(vec![BOLD, hex(224, 224, 224)]);
        assert_eq!(chain.opens(), "\x1b[1m\x1b[38;2;224;224;224m");
    }
}
