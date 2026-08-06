//! `CustomEditor` — the dimi application editor: pi-tui Editor plus the
//! app-level keybindings, `>` / `!` prompt symbol, argument hints, and the
//! full box border (port of
//! `apps/dimi/src/tui/components/editor/custom-editor.ts`, slice 6 scope:
//! key dispatch + prompt symbol + side borders; autocomplete / mentions /
//! paste-image land with the dialogs slice).

use crate::component::Component;
use crate::editor::Editor;
use crate::keys::{is_key_release, matches_key};
use crate::theme::{ColorToken, current_theme};
use crate::width::visible_width;

/// Kitty CSI-u caps-lock normalization — rewrite `ctrl+<LETTER>` with caps
/// lock into `ctrl+<letter>` without caps lock (mirrors `normalizeCapsLockedCtrl`).
pub fn normalize_caps_locked_ctrl(data: &str) -> String {
    // ^\x1b\[(\d+);(\d+)((?::\d+)*)u$
    let rest = match data.strip_prefix("\x1b[") {
        Some(r) => r,
        None => return data.to_owned(),
    };
    let rest = match rest.strip_suffix('u') {
        Some(r) => r,
        None => return data.to_owned(),
    };
    let (main, tail) = match rest.split_once(';') {
        Some((m, t)) => (m, t),
        None => return data.to_owned(),
    };
    if !tail.chars().all(|c| c.is_ascii_digit() || c == ':') {
        return data.to_owned();
    }
    let Ok(codepoint) = main.parse::<u32>() else {
        return data.to_owned();
    };
    // `tail` = "<modifier>[:<event>]"; keep the `:event` suffix for the output.
    let (mod_str, event_tail) = match tail.split_once(':') {
        Some((m, e)) => (m, format!(":{e}")),
        None => (tail, String::new()),
    };
    let modifier_plus_1: u32 = mod_str.parse().unwrap_or(0);
    let modifier = modifier_plus_1.saturating_sub(1);
    const CAPS_LOCK_BIT: u32 = 64;
    const CTRL_BIT: u32 = 4;
    const SHIFT_BIT: u32 = 1;
    if modifier & CAPS_LOCK_BIT == 0 {
        return data.to_owned();
    }
    if modifier & CTRL_BIT == 0 {
        return data.to_owned();
    }
    if modifier & SHIFT_BIT != 0 {
        return data.to_owned();
    }
    if !(65..=90).contains(&codepoint) {
        return data.to_owned();
    }
    let lowered = codepoint + 32;
    let stripped = (modifier & !CAPS_LOCK_BIT) + 1;
    format!("\x1b[{lowered};{stripped}{event_tail}u")
}

/// The editor border color (dimi `createEditorTheme` → borderColor).
pub fn editor_border_color(text: &str) -> String {
    current_theme().fg(ColorToken::Border, text)
}

/// Inject a `> ` / `! ` prompt symbol into the first content line
/// (mirrors `injectPromptSymbol`).
pub fn inject_prompt_symbol(
    line: &str,
    symbol: &str,
    paint: Option<&dyn Fn(&str) -> String>,
) -> Option<String> {
    if line.len() < 4 {
        return None;
    }
    for c in line.chars().take(4) {
        if c != ' ' {
            return None;
        }
    }
    let rendered = match paint {
        Some(p) => p(symbol),
        None => symbol.to_owned(),
    };
    Some(format!("  {rendered} {}", &line[4..]))
}

/// Wrap pi-tui editor output in a full box with `│` side bars and `╭╮╰╯`
/// corners (mirrors `wrapWithSideBorders`).
pub fn wrap_with_side_borders(
    lines: Vec<String>,
    paint: &dyn Fn(&str) -> String,
    connected_above: bool,
    label: Option<&str>,
) -> Vec<String> {
    let mut seen_top = false;
    lines
        .into_iter()
        .map(|line| {
            let plain = crate::ansi::strip_ansi(&line);
            if plain.starts_with('─') {
                let is_top = !seen_top;
                let left_corner = if seen_top {
                    "╰"
                } else if connected_above {
                    "├"
                } else {
                    "╭"
                };
                let right_corner = if seen_top {
                    "╯"
                } else if connected_above {
                    "┤"
                } else {
                    "╮"
                };
                seen_top = true;
                let plain_chars: Vec<char> = plain.chars().collect();
                if plain_chars.len() == 1 {
                    return paint(left_corner);
                }
                let middle: String = plain_chars[1..plain_chars.len() - 1].iter().collect();
                if is_top {
                    if let Some(label) = label {
                        if middle.chars().all(|c| c == '─') {
                            let label_width = visible_width(label);
                            if label_width <= middle.len() {
                                return format!(
                                    "{}{}{}{}",
                                    paint(left_corner),
                                    label,
                                    paint(&"─".repeat(middle.len() - label_width)),
                                    paint(right_corner)
                                );
                            }
                        }
                    }
                }
                paint(&format!("{left_corner}{middle}{right_corner}"))
            } else if line.is_empty() {
                line
            } else {
                let first_ch = line.chars().next().unwrap_or(' ');
                let last_ch = line.chars().last().unwrap_or(' ');
                let head = if first_ch == ' ' {
                    paint("│")
                } else {
                    first_ch.to_string()
                };
                let tail = if line.len() > 1 && last_ch == ' ' {
                    paint("│")
                } else {
                    last_ch.to_string()
                };
                if line.len() == 1 {
                    return head;
                }
                let middle = &line[first_ch.len_utf8()..line.len() - last_ch.len_utf8()];
                format!("{head}{middle}{tail}")
            }
        })
        .collect()
}

/// App-level editor callbacks.
#[allow(clippy::derivable_impls)]
pub struct CustomEditorCallbacks {
    pub on_escape: Option<Box<dyn FnMut()>>,
    pub on_ctrl_c: Option<Box<dyn FnMut()>>,
    pub on_ctrl_d: Option<Box<dyn FnMut()>>,
    pub on_ctrl_o: Option<Box<dyn FnMut()>>,
    pub on_ctrl_s: Option<Box<dyn FnMut()>>,
    pub on_undo: Option<Box<dyn FnMut()>>,
    pub on_shift_tab: Option<Box<dyn FnMut()>>,
}

#[allow(clippy::derivable_impls)]
impl Default for CustomEditorCallbacks {
    fn default() -> Self {
        CustomEditorCallbacks {
            on_escape: None,
            on_ctrl_c: None,
            on_ctrl_d: None,
            on_ctrl_o: None,
            on_ctrl_s: None,
            on_undo: None,
            on_shift_tab: None,
        }
    }
}

/// The dimi application editor.
pub struct CustomEditor {
    inner: Editor,
    pub input_mode: InputMode,
    callbacks: CustomEditorCallbacks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Prompt,
    Bash,
}

impl CustomEditor {
    pub fn new(options: crate::editor::EditorOptions, callbacks: CustomEditorCallbacks) -> Self {
        CustomEditor {
            inner: Editor::new(options),
            input_mode: InputMode::Prompt,
            callbacks,
        }
    }

    pub fn inner_mut(&mut self) -> &mut Editor {
        &mut self.inner
    }

    pub fn inner(&self) -> &Editor {
        &self.inner
    }

    pub fn set_input_mode(&mut self, mode: InputMode) {
        self.input_mode = mode;
    }
}

impl Component for CustomEditor {
    fn render(&mut self, width: usize) -> Vec<String> {
        let lines = self.inner.render(width);
        if lines.len() < 3 {
            return lines;
        }
        let is_bash = self.input_mode == InputMode::Bash;
        let symbol = if is_bash { "!" } else { ">" };
        let paint: Option<&dyn Fn(&str) -> String> = if is_bash {
            Some(&editor_border_color)
        } else {
            None
        };
        let mut lines = lines;
        let first_content_idx = 1;
        if let Some(first) = lines.get_mut(first_content_idx) {
            if let Some(with_prompt) = inject_prompt_symbol(first, symbol, paint) {
                *first = with_prompt;
            }
        }
        let label = if is_bash {
            Some(current_theme().bold_fg(ColorToken::ShellMode, "! shell mode "))
        } else {
            None
        };
        wrap_with_side_borders(lines, &editor_border_color, false, label.as_deref())
    }

    fn handle_input(&mut self, data: &str) {
        let normalized = normalize_caps_locked_ctrl(data);
        if is_key_release(&normalized) {
            return;
        }

        // App-level keybindings.
        if matches_key(&normalized, "ctrl+d") && self.inner.get_text().is_empty() {
            if let Some(cb) = &mut self.callbacks.on_ctrl_d {
                cb();
            }
            return;
        }
        if matches_key(&normalized, "ctrl+c") {
            if let Some(cb) = &mut self.callbacks.on_ctrl_c {
                cb();
            }
            return;
        }
        if matches_key(&normalized, "ctrl+o") {
            if let Some(cb) = &mut self.callbacks.on_ctrl_o {
                cb();
            }
            return;
        }
        if matches_key(&normalized, "ctrl+s") {
            if let Some(cb) = &mut self.callbacks.on_ctrl_s {
                cb();
            }
            return;
        }
        if matches_key(&normalized, "ctrl+-") {
            if let Some(cb) = &mut self.callbacks.on_undo {
                cb();
            }
            return;
        }
        if matches_key(&normalized, "shift+tab") {
            if let Some(cb) = &mut self.callbacks.on_shift_tab {
                cb();
            }
            return;
        }

        // Exit bash mode on empty backspace/escape.
        if self.input_mode == InputMode::Bash
            && self.inner.get_text().is_empty()
            && (matches_key(&normalized, "escape") || matches_key(&normalized, "backspace"))
        {
            self.input_mode = InputMode::Prompt;
            return;
        }

        if matches_key(&normalized, "escape") {
            if let Some(cb) = &mut self.callbacks.on_escape {
                cb();
            }
            return;
        }

        // Enter bash mode: `!` at the start of an empty prompt.
        let is_bang = normalized == "!"
            || crate::keys::decode_printable_key(&normalized).as_deref() == Some("!");
        if self.input_mode == InputMode::Prompt && self.inner.get_text().is_empty() && is_bang {
            self.input_mode = InputMode::Bash;
            return;
        }

        self.inner.handle_input(&normalized);
    }

    fn invalidate(&mut self) {
        self.inner.invalidate();
    }

    fn as_focusable_mut(&mut self) -> Option<&mut dyn crate::component::Focusable> {
        Some(&mut self.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_lock_ctrl_normalized() {
        // ctrl+d with caps lock: codepoint 68 ('D'), modifier ctrl|caps_lock + 1.
        let out = normalize_caps_locked_ctrl("\x1b[68;69u");
        assert_eq!(out, "\x1b[100;5u");
        // Non-ctrl caps-lock left alone.
        assert_eq!(normalize_caps_locked_ctrl("\x1b[68;66u"), "\x1b[68;66u");
        // ctrl+shift+caps left alone.
        assert_eq!(normalize_caps_locked_ctrl("\x1b[68;70u"), "\x1b[68;70u");
    }

    #[test]
    fn prompt_symbol_injected() {
        let line = "    hello";
        let out = inject_prompt_symbol(line, ">", None).unwrap();
        assert_eq!(out, "  > hello");
    }

    #[test]
    fn prompt_symbol_requires_padding() {
        assert_eq!(inject_prompt_symbol("hello", ">", None), None);
    }

    #[test]
    fn side_borders_wrap() {
        let lines = vec!["────".to_owned(), " hi ".to_owned(), "────".to_owned()];
        let out = wrap_with_side_borders(lines, &|s| format!("[{s}]"), false, None);
        assert_eq!(out[0], "[╭──╮]");
        // TS replaces the leading/trailing spaces with │ (line.slice(1,-1)).
        assert_eq!(out[1], "[│]hi[│]");
        assert_eq!(out[2], "[╰──╯]");
    }

    #[test]
    fn bash_mode_prompt() {
        let mut e = CustomEditor::new(
            crate::editor::EditorOptions { padding_x: 4 },
            CustomEditorCallbacks::default(),
        );
        // Type `!` at empty prompt → bash mode (the `!` is NOT in the buffer).
        e.handle_input("!");
        assert_eq!(e.input_mode, InputMode::Bash);
        assert_eq!(e.inner.get_text(), "");
        // Escape exits bash mode.
        e.handle_input("\x1b");
        assert_eq!(e.input_mode, InputMode::Prompt);
    }
}
