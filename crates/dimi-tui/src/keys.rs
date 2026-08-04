//! Keyboard input handling — port of `@dimi-agent/pi-tui` `src/keys.ts`.
//!
//! Supports legacy terminal sequences and the Kitty keyboard protocol
//! (CSI-u). `matches_key(data, key_id)` checks whether raw input matches a
//! key identifier like `"ctrl+c"`, `"escape"`, `"shift+tab"`; `parse_key`
//! returns the identifier for recognized input; `decode_printable_key`
//! extracts the printable character from CSI-u / modifyOtherKeys sequences
//! (the printable-key decoding discipline from apps/dimi AGENTS.md).

use std::cell::Cell;

thread_local! {
    /// Kitty keyboard protocol state. Thread-local so parallel tests never
    /// interfere; production uses a single event-loop thread (the terminal and
    /// key matching run on the same thread), matching the TS module global.
    static KITTY_PROTOCOL_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// Set the global Kitty keyboard protocol state (called by the terminal after
/// detecting protocol support).
pub fn set_kitty_protocol_active(active: bool) {
    KITTY_PROTOCOL_ACTIVE.set(active);
}

/// Query whether the Kitty keyboard protocol is currently active.
pub fn is_kitty_protocol_active() -> bool {
    KITTY_PROTOCOL_ACTIVE.get()
}

// =============================================================================
// Constants
// =============================================================================

const MODIFIERS_SHIFT: u32 = 1;
const MODIFIERS_ALT: u32 = 2;
const MODIFIERS_CTRL: u32 = 4;
const MODIFIERS_SUPER: u32 = 8;
const LOCK_MASK: u32 = 64 + 128; // Caps Lock + Num Lock

const CODEPOINTS_ESCAPE: i64 = 27;
const CODEPOINTS_TAB: i64 = 9;
const CODEPOINTS_ENTER: i64 = 13;
const CODEPOINTS_SPACE: i64 = 32;
const CODEPOINTS_BACKSPACE: i64 = 127;
const CODEPOINTS_KP_ENTER: i64 = 57414;

const ARROW_UP: i64 = -1;
const ARROW_DOWN: i64 = -2;
const ARROW_RIGHT: i64 = -3;
const ARROW_LEFT: i64 = -4;

const FUNC_DELETE: i64 = -10;
const FUNC_INSERT: i64 = -11;
const FUNC_PAGE_UP: i64 = -12;
const FUNC_PAGE_DOWN: i64 = -13;
const FUNC_HOME: i64 = -14;
const FUNC_END: i64 = -15;

/// Kitty functional key equivalents (numpad → plain keys).
fn normalize_kitty_functional_codepoint(codepoint: i64) -> i64 {
    match codepoint {
        57399 => 48, // KP_0
        57400 => 49, // KP_1
        57401 => 50, // KP_2
        57402 => 51, // KP_3
        57403 => 52, // KP_4
        57404 => 53, // KP_5
        57405 => 54, // KP_6
        57406 => 55, // KP_7
        57407 => 56, // KP_8
        57408 => 57, // KP_9
        57409 => 46, // KP_DECIMAL
        57410 => 47, // KP_DIVIDE
        57411 => 42, // KP_MULTIPLY
        57412 => 45, // KP_SUBTRACT
        57413 => 43, // KP_ADD
        57415 => 61, // KP_EQUAL
        57416 => 44, // KP_SEPARATOR
        57417 => ARROW_LEFT,
        57418 => ARROW_RIGHT,
        57419 => ARROW_UP,
        57420 => ARROW_DOWN,
        57421 => FUNC_PAGE_UP,
        57422 => FUNC_PAGE_DOWN,
        57423 => FUNC_HOME,
        57424 => FUNC_END,
        57425 => FUNC_INSERT,
        57426 => FUNC_DELETE,
        other => other,
    }
}

/// Shifted uppercase letters normalize to lowercase for identity matching.
fn normalize_shifted_letter_identity_codepoint(codepoint: i64, modifier: u32) -> i64 {
    let effective_modifier = modifier & !LOCK_MASK;
    if (effective_modifier & MODIFIERS_SHIFT) != 0 && (65..=90).contains(&codepoint) {
        codepoint + 32
    } else {
        codepoint
    }
}

const SYMBOL_KEYS: &[char] = &[
    '`', '-', '=', '[', ']', '\\', ';', '\'', ',', '.', '/', '!', '@', '#', '$', '%', '^', '&',
    '*', '(', ')', '_', '+', '|', '~', '{', '}', ':', '<', '>', '?',
];

fn is_symbol_key(c: char) -> bool {
    SYMBOL_KEYS.contains(&c)
}

// =============================================================================
// Kitty Protocol Parsing
// =============================================================================

/// Event types from the Kitty keyboard protocol (flag 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventType {
    Press,
    Repeat,
    Release,
}

#[derive(Debug, Clone, Copy)]
struct ParsedKittySequence {
    codepoint: i64,
    shifted_key: Option<i64>,
    base_layout_key: Option<i64>,
    modifier: u32,
    #[allow(dead_code)]
    event_type: KeyEventType,
}

#[derive(Debug, Clone, Copy)]
struct ParsedModifyOtherKeysSequence {
    codepoint: i64,
    modifier: u32,
}

fn parse_event_type(event_type_str: Option<&str>) -> KeyEventType {
    let Some(s) = event_type_str else {
        return KeyEventType::Press;
    };
    match s.parse::<u32>().unwrap_or(0) {
        2 => KeyEventType::Repeat,
        3 => KeyEventType::Release,
        _ => KeyEventType::Press,
    }
}

/// Parse a Kitty CSI-u sequence:
/// `ESC[<codepoint>u`, `ESC[<codepoint>;<mod>u`,
/// `ESC[<codepoint>;<mod>:<event>u`, `ESC[<codepoint>:<shifted>;<mod>u`,
/// `ESC[<codepoint>:<shifted>:<base>;<mod>u`, `ESC[<codepoint>::<base>;<mod>u`.
fn parse_kitty_sequence(data: &str) -> Option<ParsedKittySequence> {
    // ^\x1b\[(\d+)(?::(\d*))?(?::(\d+))?(?:;(\d+))?(?::(\d+))?u$
    let rest = data.strip_prefix("\x1b[")?;
    let rest = rest.strip_suffix('u')?;
    // Split on ';' first (modifier + event), then ':' within the first part.
    let (main, tail) = match rest.split_once(';') {
        Some((m, t)) => (m, Some(t)),
        None => (rest, None),
    };
    let mut main_parts = main.split(':');
    let codepoint = main_parts.next()?.parse::<i64>().ok()?;
    let shifted_key = match main_parts.next() {
        Some(s) if !s.is_empty() => s.parse::<i64>().ok(),
        _ => None,
    };
    let base_layout_key = match main_parts.next() {
        Some(s) => s.parse::<i64>().ok(),
        None => None,
    };
    let (mod_value, event_type) = match tail {
        Some(t) => {
            let mut parts = t.split(':');
            let mod_value = parts
                .next()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(1);
            let event_type = parse_event_type(parts.next());
            (mod_value, event_type)
        }
        None => (1, KeyEventType::Press),
    };
    Some(ParsedKittySequence {
        codepoint,
        shifted_key,
        base_layout_key,
        modifier: mod_value.saturating_sub(1),
        event_type,
    })
}

/// Arrow keys with modifier: `ESC[1;<mod>A/B/C/D` or `ESC[1;<mod>:<event>A/B/C/D`.
fn parse_kitty_arrow(data: &str) -> Option<ParsedKittySequence> {
    let rest = data.strip_prefix("\x1b[1;")?;
    let last = rest.chars().last()?;
    if !matches!(last, 'A' | 'B' | 'C' | 'D') {
        return None;
    }
    let body = &rest[..rest.len() - 1];
    let (mod_str, event_str) = match body.split_once(':') {
        Some((m, e)) => (m, Some(e)),
        None => (body, None),
    };
    let mod_value = mod_str.parse::<u32>().ok()?;
    let event_type = parse_event_type(event_str);
    let codepoint = match last {
        'A' => ARROW_UP,
        'B' => ARROW_DOWN,
        'C' => ARROW_RIGHT,
        _ => ARROW_LEFT,
    };
    Some(ParsedKittySequence {
        codepoint,
        shifted_key: None,
        base_layout_key: None,
        modifier: mod_value.saturating_sub(1),
        event_type,
    })
}

/// Functional keys: `ESC[<num>~` or `ESC[<num>;<mod>~` or
/// `ESC[<num>;<mod>:<event>~`.
fn parse_kitty_functional(data: &str) -> Option<ParsedKittySequence> {
    let rest = data.strip_prefix("\x1b[")?;
    let rest = rest.strip_suffix('~')?;
    let (num_str, tail) = match rest.split_once(';') {
        Some((n, t)) => (n, Some(t)),
        None => (rest, None),
    };
    let key_num = num_str.parse::<i64>().ok()?;
    let func_codepoint = match key_num {
        2 => Some(FUNC_INSERT),
        3 => Some(FUNC_DELETE),
        5 => Some(FUNC_PAGE_UP),
        6 => Some(FUNC_PAGE_DOWN),
        7 => Some(FUNC_HOME),
        8 => Some(FUNC_END),
        _ => None,
    }?;
    let (mod_value, event_type) = match tail {
        Some(t) => {
            let mut parts = t.split(':');
            let mod_value = parts
                .next()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(1);
            let event_type = parse_event_type(parts.next());
            (mod_value, event_type)
        }
        None => (1, KeyEventType::Press),
    };
    Some(ParsedKittySequence {
        codepoint: func_codepoint,
        shifted_key: None,
        base_layout_key: None,
        modifier: mod_value.saturating_sub(1),
        event_type,
    })
}

/// Home/End with modifier: `ESC[1;<mod>H/F` or `ESC[1;<mod>:<event>H/F`.
fn parse_kitty_home_end(data: &str) -> Option<ParsedKittySequence> {
    let rest = data.strip_prefix("\x1b[1;")?;
    let last = rest.chars().last()?;
    if !matches!(last, 'H' | 'F') {
        return None;
    }
    let body = &rest[..rest.len() - 1];
    let (mod_str, event_str) = match body.split_once(':') {
        Some((m, e)) => (m, Some(e)),
        None => (body, None),
    };
    let mod_value = mod_str.parse::<u32>().ok()?;
    let event_type = parse_event_type(event_str);
    Some(ParsedKittySequence {
        codepoint: if last == 'H' { FUNC_HOME } else { FUNC_END },
        shifted_key: None,
        base_layout_key: None,
        modifier: mod_value.saturating_sub(1),
        event_type,
    })
}

fn parse_kitty_any(data: &str) -> Option<ParsedKittySequence> {
    parse_kitty_sequence(data)
        .or_else(|| parse_kitty_arrow(data))
        .or_else(|| parse_kitty_functional(data))
        .or_else(|| parse_kitty_home_end(data))
}

/// Parse xterm modifyOtherKeys: `ESC[27;<mod>;<keycode>~`.
fn parse_modify_other_keys_sequence(data: &str) -> Option<ParsedModifyOtherKeysSequence> {
    let rest = data.strip_prefix("\x1b[27;")?;
    let rest = rest.strip_suffix('~')?;
    let (mod_str, code_str) = rest.split_once(';')?;
    let mod_value = mod_str.parse::<u32>().ok()?;
    let codepoint = code_str.parse::<i64>().ok()?;
    Some(ParsedModifyOtherKeysSequence {
        codepoint,
        modifier: mod_value.saturating_sub(1),
    })
}

fn matches_kitty_sequence(data: &str, expected_codepoint: i64, expected_modifier: u32) -> bool {
    let Some(parsed) = parse_kitty_any(data) else {
        return false;
    };
    let actual_mod = parsed.modifier & !LOCK_MASK;
    let expected_mod = expected_modifier & !LOCK_MASK;
    if actual_mod != expected_mod {
        return false;
    }
    let normalized_codepoint = normalize_shifted_letter_identity_codepoint(
        normalize_kitty_functional_codepoint(parsed.codepoint),
        parsed.modifier,
    );
    let normalized_expected = normalize_shifted_letter_identity_codepoint(
        normalize_kitty_functional_codepoint(expected_codepoint),
        expected_modifier,
    );
    if normalized_codepoint == normalized_expected {
        return true;
    }
    // Alternate match via base layout key (non-Latin layouts), but only when
    // the codepoint is not a recognized Latin letter or symbol (avoids false
    // matches on remapped layouts like Dvorak/Colemak).
    if let Some(base) = parsed.base_layout_key {
        if base == expected_codepoint {
            let cp = normalized_codepoint;
            let is_latin_letter = (97..=122).contains(&cp);
            let is_known_symbol = char::from_u32(cp as u32).is_some_and(is_symbol_key);
            if !is_latin_letter && !is_known_symbol {
                return true;
            }
        }
    }
    false
}

fn matches_modify_other_keys(data: &str, expected_keycode: i64, expected_modifier: u32) -> bool {
    let Some(parsed) = parse_modify_other_keys_sequence(data) else {
        return false;
    };
    parsed.codepoint == expected_keycode && parsed.modifier == expected_modifier
}

fn matches_printable_modify_other_keys(
    data: &str,
    expected_keycode: i64,
    expected_modifier: u32,
) -> bool {
    if expected_modifier == 0 {
        return false;
    }
    let Some(parsed) = parse_modify_other_keys_sequence(data) else {
        return false;
    };
    if parsed.modifier != expected_modifier {
        return false;
    }
    normalize_shifted_letter_identity_codepoint(parsed.codepoint, parsed.modifier)
        == normalize_shifted_letter_identity_codepoint(expected_keycode, expected_modifier)
}

fn is_windows_terminal_session() -> bool {
    let get = |k: &str| std::env::var(k).map(|v| !v.is_empty()).unwrap_or(false);
    is_windows_terminal_session_with(
        get("WT_SESSION"),
        get("SSH_CONNECTION") || get("SSH_CLIENT") || get("SSH_TTY"),
    )
}

/// Environment-snapshot version for tests (the real one reads process env).
fn is_windows_terminal_session_with(wt_session: bool, ssh: bool) -> bool {
    wt_session && !ssh
}

fn matches_raw_backspace(data: &str, expected_modifier: u32) -> bool {
    if data == "\x7f" {
        return expected_modifier == 0;
    }
    if data != "\x08" {
        return false;
    }
    if is_windows_terminal_session() {
        expected_modifier == MODIFIERS_CTRL
    } else {
        expected_modifier == 0
    }
}

// =============================================================================
// Legacy sequences
// =============================================================================

fn matches_legacy_sequence(data: &str, sequences: &[&str]) -> bool {
    sequences.contains(&data)
}

const LEGACY_KEY_SEQUENCES: &[(&str, &[&str])] = &[
    ("up", &["\x1b[A", "\x1bOA"]),
    ("down", &["\x1b[B", "\x1bOB"]),
    ("right", &["\x1b[C", "\x1bOC"]),
    ("left", &["\x1b[D", "\x1bOD"]),
    ("home", &["\x1b[H", "\x1bOH", "\x1b[1~", "\x1b[7~"]),
    ("end", &["\x1b[F", "\x1bOF", "\x1b[4~", "\x1b[8~"]),
    ("insert", &["\x1b[2~"]),
    ("delete", &["\x1b[3~"]),
    ("pageUp", &["\x1b[5~", "\x1b[[5~"]),
    ("pageDown", &["\x1b[6~", "\x1b[[6~"]),
    ("clear", &["\x1b[E", "\x1bOE"]),
    ("f1", &["\x1bOP", "\x1b[11~", "\x1b[[A"]),
    ("f2", &["\x1bOQ", "\x1b[12~", "\x1b[[B"]),
    ("f3", &["\x1bOR", "\x1b[13~", "\x1b[[C"]),
    ("f4", &["\x1bOS", "\x1b[14~", "\x1b[[D"]),
    ("f5", &["\x1b[15~", "\x1b[[E"]),
    ("f6", &["\x1b[17~"]),
    ("f7", &["\x1b[18~"]),
    ("f8", &["\x1b[19~"]),
    ("f9", &["\x1b[20~"]),
    ("f10", &["\x1b[21~"]),
    ("f11", &["\x1b[23~"]),
    ("f12", &["\x1b[24~"]),
];

const LEGACY_SHIFT_SEQUENCES: &[(&str, &[&str])] = &[
    ("up", &["\x1b[a"]),
    ("down", &["\x1b[b"]),
    ("right", &["\x1b[c"]),
    ("left", &["\x1b[d"]),
    ("clear", &["\x1b[e"]),
    ("insert", &["\x1b[2$"]),
    ("delete", &["\x1b[3$"]),
    ("pageUp", &["\x1b[5$"]),
    ("pageDown", &["\x1b[6$"]),
    ("home", &["\x1b[7$"]),
    ("end", &["\x1b[8$"]),
];

const LEGACY_CTRL_SEQUENCES: &[(&str, &[&str])] = &[
    ("up", &["\x1bOa"]),
    ("down", &["\x1bOb"]),
    ("right", &["\x1bOc"]),
    ("left", &["\x1bOd"]),
    ("clear", &["\x1bOe"]),
    ("insert", &["\x1b[2^"]),
    ("delete", &["\x1b[3^"]),
    ("pageUp", &["\x1b[5^"]),
    ("pageDown", &["\x1b[6^"]),
    ("home", &["\x1b[7^"]),
    ("end", &["\x1b[8^"]),
];

fn legacy_sequences_for(name: &str) -> &'static [&'static str] {
    LEGACY_KEY_SEQUENCES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, seqs)| *seqs)
        .unwrap_or(&[])
}

fn legacy_modifier_sequences_for(name: &str, modifier: u32) -> &'static [&'static str] {
    let table: &[(&str, &[&str])] = if modifier == MODIFIERS_SHIFT {
        LEGACY_SHIFT_SEQUENCES
    } else if modifier == MODIFIERS_CTRL {
        LEGACY_CTRL_SEQUENCES
    } else {
        return &[];
    };
    table
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, seqs)| *seqs)
        .unwrap_or(&[])
}

fn matches_legacy_modifier_sequence(data: &str, key: &str, modifier: u32) -> bool {
    matches_legacy_sequence(data, legacy_modifier_sequences_for(key, modifier))
}

const LEGACY_SEQUENCE_KEY_IDS: &[(&str, &str)] = &[
    ("\x1bOA", "up"),
    ("\x1bOB", "down"),
    ("\x1bOC", "right"),
    ("\x1bOD", "left"),
    ("\x1bOH", "home"),
    ("\x1bOF", "end"),
    ("\x1b[E", "clear"),
    ("\x1bOE", "clear"),
    ("\x1bOe", "ctrl+clear"),
    ("\x1b[e", "shift+clear"),
    ("\x1b[2~", "insert"),
    ("\x1b[2$", "shift+insert"),
    ("\x1b[2^", "ctrl+insert"),
    ("\x1b[3$", "shift+delete"),
    ("\x1b[3^", "ctrl+delete"),
    ("\x1b[[5~", "pageUp"),
    ("\x1b[[6~", "pageDown"),
    ("\x1b[a", "shift+up"),
    ("\x1b[b", "shift+down"),
    ("\x1b[c", "shift+right"),
    ("\x1b[d", "shift+left"),
    ("\x1bOa", "ctrl+up"),
    ("\x1bOb", "ctrl+down"),
    ("\x1bOc", "ctrl+right"),
    ("\x1bOd", "ctrl+left"),
    ("\x1b[5$", "shift+pageUp"),
    ("\x1b[6$", "shift+pageDown"),
    ("\x1b[7$", "shift+home"),
    ("\x1b[8$", "shift+end"),
    ("\x1b[5^", "ctrl+pageUp"),
    ("\x1b[6^", "ctrl+pageDown"),
    ("\x1b[7^", "ctrl+home"),
    ("\x1b[8^", "ctrl+end"),
    ("\x1bOP", "f1"),
    ("\x1bOQ", "f2"),
    ("\x1bOR", "f3"),
    ("\x1bOS", "f4"),
    ("\x1b[11~", "f1"),
    ("\x1b[12~", "f2"),
    ("\x1b[13~", "f3"),
    ("\x1b[14~", "f4"),
    ("\x1b[[A", "f1"),
    ("\x1b[[B", "f2"),
    ("\x1b[[C", "f3"),
    ("\x1b[[D", "f4"),
    ("\x1b[[E", "f5"),
    ("\x1b[15~", "f5"),
    ("\x1b[17~", "f6"),
    ("\x1b[18~", "f7"),
    ("\x1b[19~", "f8"),
    ("\x1b[20~", "f9"),
    ("\x1b[21~", "f10"),
    ("\x1b[23~", "f11"),
    ("\x1b[24~", "f12"),
    ("\x1bb", "alt+left"),
    ("\x1bf", "alt+right"),
    ("\x1bp", "alt+up"),
    ("\x1bn", "alt+down"),
];

/// Raw control character for a key: `code & 0x1f`.
fn raw_ctrl_char(key: &str) -> Option<String> {
    let lower = key.to_lowercase();
    let char = lower.chars().next()?;
    let code = char as u32;
    if (97..=122).contains(&code) || matches!(char, '[' | '\\' | ']' | '_') {
        return Some(char::from_u32(code & 0x1f)?.to_string());
    }
    // '-' maps to the same physical key as '_'.
    if char == '-' {
        return Some(char::from_u32(31)?.to_string());
    }
    None
}

// =============================================================================
// Key ID parsing
// =============================================================================

struct ParsedKeyId {
    key: String,
    ctrl: bool,
    shift: bool,
    alt: bool,
    super_modifier: bool,
}

fn parse_key_id(key_id: &str) -> Option<ParsedKeyId> {
    let lower = key_id.to_lowercase();
    let parts: Vec<&str> = lower.split('+').collect();
    let key = parts.last()?.to_owned();
    if key.is_empty() {
        return None;
    }
    Some(ParsedKeyId {
        key: key.to_owned(),
        ctrl: parts.contains(&"ctrl"),
        shift: parts.contains(&"shift"),
        alt: parts.contains(&"alt"),
        super_modifier: parts.contains(&"super"),
    })
}

// =============================================================================
// Key matching
// =============================================================================

/// Check if raw input `data` matches a key identifier like `"ctrl+c"`,
/// `"escape"`, `"shift+tab"` (the `Key` helper names are plain strings).
pub fn matches_key(data: &str, key_id: &str) -> bool {
    let Some(parsed) = parse_key_id(key_id) else {
        return false;
    };
    let mut modifier = 0u32;
    if parsed.shift {
        modifier |= MODIFIERS_SHIFT;
    }
    if parsed.alt {
        modifier |= MODIFIERS_ALT;
    }
    if parsed.ctrl {
        modifier |= MODIFIERS_CTRL;
    }
    if parsed.super_modifier {
        modifier |= MODIFIERS_SUPER;
    }
    let key: &str = &parsed.key;

    if let Some(special) = match_special_key(data, key, modifier) {
        return special;
    }

    // Single letter/digit/symbol keys.
    if key.len() == 1 {
        let c = key.chars().next().unwrap();
        let is_letter = c.is_ascii_lowercase();
        let is_digit = c.is_ascii_digit();
        if !is_letter && !is_digit && !is_symbol_key(c) {
            return false;
        }
        let codepoint = c as i64;
        let raw_ctrl = raw_ctrl_char(key);

        if modifier == MODIFIERS_CTRL + MODIFIERS_ALT && !is_kitty_protocol_active() {
            if let Some(rc) = &raw_ctrl {
                if data == format!("\x1b{rc}") {
                    return true;
                }
            }
        }
        if modifier == MODIFIERS_ALT
            && !is_kitty_protocol_active()
            && (is_letter || is_digit)
            && data == format!("\x1b{key}")
        {
            return true;
        }
        if modifier == MODIFIERS_CTRL {
            if let Some(rc) = &raw_ctrl {
                if data == rc {
                    return true;
                }
            }
            return matches_kitty_sequence(data, codepoint, MODIFIERS_CTRL)
                || matches_printable_modify_other_keys(data, codepoint, MODIFIERS_CTRL);
        }
        if modifier == MODIFIERS_SHIFT + MODIFIERS_CTRL {
            return matches_kitty_sequence(data, codepoint, MODIFIERS_SHIFT + MODIFIERS_CTRL)
                || matches_printable_modify_other_keys(
                    data,
                    codepoint,
                    MODIFIERS_SHIFT + MODIFIERS_CTRL,
                );
        }
        if modifier == MODIFIERS_SHIFT {
            if is_letter && data == c.to_ascii_uppercase().to_string() {
                return true;
            }
            return matches_kitty_sequence(data, codepoint, MODIFIERS_SHIFT)
                || matches_printable_modify_other_keys(data, codepoint, MODIFIERS_SHIFT);
        }
        if modifier != 0 {
            return matches_kitty_sequence(data, codepoint, modifier)
                || matches_printable_modify_other_keys(data, codepoint, modifier);
        }
        // Plain: raw char or Kitty sequence (needed for release events).
        return data == key || matches_kitty_sequence(data, codepoint, 0);
    }

    false
}

/// Match the special keys (escape/space/tab/enter/backspace/arrows/function
/// keys/clear). Returns `Some(result)` when the key was handled, `None` to
/// fall through to the printable-key path.
fn match_special_key(data: &str, key: &str, modifier: u32) -> Option<bool> {
    match key {
        "escape" | "esc" => {
            if modifier != 0 {
                return Some(false);
            }
            Some(
                data == "\x1b"
                    || matches_kitty_sequence(data, CODEPOINTS_ESCAPE, 0)
                    || matches_modify_other_keys(data, CODEPOINTS_ESCAPE, 0),
            )
        }
        "space" => {
            if !is_kitty_protocol_active() {
                if modifier == MODIFIERS_CTRL && data == "\x00" {
                    return Some(true);
                }
                if modifier == MODIFIERS_ALT && data == "\x1b " {
                    return Some(true);
                }
            }
            if modifier == 0 {
                Some(
                    data == " "
                        || matches_kitty_sequence(data, CODEPOINTS_SPACE, 0)
                        || matches_modify_other_keys(data, CODEPOINTS_SPACE, 0),
                )
            } else {
                Some(
                    matches_kitty_sequence(data, CODEPOINTS_SPACE, modifier)
                        || matches_modify_other_keys(data, CODEPOINTS_SPACE, modifier),
                )
            }
        }
        "tab" => {
            if modifier == MODIFIERS_SHIFT {
                Some(
                    data == "\x1b[Z"
                        || matches_kitty_sequence(data, CODEPOINTS_TAB, MODIFIERS_SHIFT)
                        || matches_modify_other_keys(data, CODEPOINTS_TAB, MODIFIERS_SHIFT),
                )
            } else if modifier == 0 {
                Some(data == "\t" || matches_kitty_sequence(data, CODEPOINTS_TAB, 0))
            } else {
                Some(
                    matches_kitty_sequence(data, CODEPOINTS_TAB, modifier)
                        || matches_modify_other_keys(data, CODEPOINTS_TAB, modifier),
                )
            }
        }
        "enter" | "return" => {
            if modifier == MODIFIERS_SHIFT {
                if matches_kitty_sequence(data, CODEPOINTS_ENTER, MODIFIERS_SHIFT)
                    || matches_kitty_sequence(data, CODEPOINTS_KP_ENTER, MODIFIERS_SHIFT)
                {
                    return Some(true);
                }
                if matches_modify_other_keys(data, CODEPOINTS_ENTER, MODIFIERS_SHIFT) {
                    return Some(true);
                }
                if is_kitty_protocol_active() {
                    return Some(data == "\x1b\r" || data == "\n");
                }
                return Some(false);
            }
            if modifier == MODIFIERS_ALT {
                if matches_kitty_sequence(data, CODEPOINTS_ENTER, MODIFIERS_ALT)
                    || matches_kitty_sequence(data, CODEPOINTS_KP_ENTER, MODIFIERS_ALT)
                {
                    return Some(true);
                }
                if matches_modify_other_keys(data, CODEPOINTS_ENTER, MODIFIERS_ALT) {
                    return Some(true);
                }
                if !is_kitty_protocol_active() && data == "\x1b\r" {
                    return Some(true);
                }
                return Some(false);
            }
            if modifier == 0 {
                return Some(
                    data == "\r"
                        || (!is_kitty_protocol_active() && data == "\n")
                        || data == "\x1bOM"
                        || matches_kitty_sequence(data, CODEPOINTS_ENTER, 0)
                        || matches_kitty_sequence(data, CODEPOINTS_KP_ENTER, 0)
                        || matches_modify_other_keys(data, CODEPOINTS_ENTER, 0),
                );
            }
            Some(
                matches_kitty_sequence(data, CODEPOINTS_ENTER, modifier)
                    || matches_kitty_sequence(data, CODEPOINTS_KP_ENTER, modifier)
                    || matches_modify_other_keys(data, CODEPOINTS_ENTER, modifier),
            )
        }
        "backspace" => {
            if modifier == 0 {
                Some(
                    matches_raw_backspace(data, 0)
                        || matches_kitty_sequence(data, CODEPOINTS_BACKSPACE, 0)
                        || matches_modify_other_keys(data, CODEPOINTS_BACKSPACE, 0),
                )
            } else {
                Some(
                    matches_raw_backspace(data, modifier)
                        || matches_kitty_sequence(data, CODEPOINTS_BACKSPACE, modifier)
                        || matches_modify_other_keys(data, CODEPOINTS_BACKSPACE, modifier),
                )
            }
        }
        "insert" | "delete" | "home" | "end" | "pageup" | "pagedown" | "up" | "down" | "left"
        | "right" => {
            let functional = match key {
                "insert" => FUNC_INSERT,
                "delete" => FUNC_DELETE,
                "home" => FUNC_HOME,
                "end" => FUNC_END,
                "pageup" => FUNC_PAGE_UP,
                "pagedown" => FUNC_PAGE_DOWN,
                "up" => ARROW_UP,
                "down" => ARROW_DOWN,
                "left" => ARROW_LEFT,
                "right" => ARROW_RIGHT,
                _ => unreachable!(),
            };
            let legacy_name = match key {
                "up" => "up",
                "down" => "down",
                "left" => "left",
                "right" => "right",
                "home" => "home",
                "end" => "end",
                "insert" => "insert",
                "delete" => "delete",
                "pageup" => "pageUp",
                "pagedown" => "pageDown",
                _ => key,
            };

            // Alt+arrows have bespoke legacy forms.
            if (key == "up" || key == "down" || key == "left" || key == "right")
                && modifier == MODIFIERS_ALT
            {
                let legacy_alt = match key {
                    "up" => "\x1bp",
                    "down" => "\x1bn",
                    "left" => "\x1bb",
                    "right" => "\x1bf",
                    _ => unreachable!(),
                };
                let extra = match key {
                    "left" => {
                        data == "\x1b[1;3D" || (!is_kitty_protocol_active() && data == "\x1bB")
                    }
                    "right" => {
                        data == "\x1b[1;3C" || (!is_kitty_protocol_active() && data == "\x1bF")
                    }
                    _ => false,
                };
                return Some(
                    data == legacy_alt
                        || extra
                        || matches_kitty_sequence(data, functional, MODIFIERS_ALT),
                );
            }
            // Ctrl+left/right accept `\x1b[1;5D`/`\x1b[1;5C`.
            if (key == "left" || key == "right") && modifier == MODIFIERS_CTRL {
                let csi = match key {
                    "left" => data == "\x1b[1;5D",
                    "right" => data == "\x1b[1;5C",
                    _ => false,
                };
                return Some(
                    csi || matches_legacy_modifier_sequence(data, legacy_name, MODIFIERS_CTRL)
                        || matches_kitty_sequence(data, functional, MODIFIERS_CTRL),
                );
            }

            if modifier == 0 {
                return Some(
                    matches_legacy_sequence(data, legacy_sequences_for(legacy_name))
                        || matches_kitty_sequence(data, functional, 0),
                );
            }
            if matches_legacy_modifier_sequence(data, legacy_name, modifier) {
                return Some(true);
            }
            Some(matches_kitty_sequence(data, functional, modifier))
        }
        "clear" => {
            if modifier == 0 {
                Some(
                    matches_legacy_sequence(data, legacy_sequences_for("clear"))
                        || matches_kitty_sequence(data, -5, 0),
                )
            } else {
                Some(
                    matches_legacy_modifier_sequence(data, "clear", modifier)
                        || matches_kitty_sequence(data, -5, modifier),
                )
            }
        }
        "f1" | "f2" | "f3" | "f4" | "f5" | "f6" | "f7" | "f8" | "f9" | "f10" | "f11" | "f12" => {
            if modifier != 0 {
                return Some(false);
            }
            Some(matches_legacy_sequence(data, legacy_sequences_for(key)))
        }
        _ => None,
    }
}

// =============================================================================
// Key parsing
// =============================================================================

fn format_key_name_with_modifiers(key_name: &str, modifier: u32) -> Option<String> {
    let mut mods: Vec<&str> = Vec::new();
    let effective_mod = modifier & !LOCK_MASK;
    let supported = MODIFIERS_SHIFT | MODIFIERS_CTRL | MODIFIERS_ALT | MODIFIERS_SUPER;
    if (effective_mod & !supported) != 0 {
        return None;
    }
    if effective_mod & MODIFIERS_SHIFT != 0 {
        mods.push("shift");
    }
    if effective_mod & MODIFIERS_CTRL != 0 {
        mods.push("ctrl");
    }
    if effective_mod & MODIFIERS_ALT != 0 {
        mods.push("alt");
    }
    if effective_mod & MODIFIERS_SUPER != 0 {
        mods.push("super");
    }
    if mods.is_empty() {
        Some(key_name.to_owned())
    } else {
        Some(format!("{}+{}", mods.join("+"), key_name))
    }
}

fn key_name_for_codepoint(effective_codepoint: i64) -> Option<String> {
    if effective_codepoint == CODEPOINTS_ESCAPE {
        Some("escape".to_owned())
    } else if effective_codepoint == CODEPOINTS_TAB {
        Some("tab".to_owned())
    } else if effective_codepoint == CODEPOINTS_ENTER || effective_codepoint == CODEPOINTS_KP_ENTER
    {
        Some("enter".to_owned())
    } else if effective_codepoint == CODEPOINTS_SPACE {
        Some("space".to_owned())
    } else if effective_codepoint == CODEPOINTS_BACKSPACE {
        Some("backspace".to_owned())
    } else if effective_codepoint == FUNC_DELETE {
        Some("delete".to_owned())
    } else if effective_codepoint == FUNC_INSERT {
        Some("insert".to_owned())
    } else if effective_codepoint == FUNC_HOME {
        Some("home".to_owned())
    } else if effective_codepoint == FUNC_END {
        Some("end".to_owned())
    } else if effective_codepoint == FUNC_PAGE_UP {
        Some("pageUp".to_owned())
    } else if effective_codepoint == FUNC_PAGE_DOWN {
        Some("pageDown".to_owned())
    } else if effective_codepoint == ARROW_UP {
        Some("up".to_owned())
    } else if effective_codepoint == ARROW_DOWN {
        Some("down".to_owned())
    } else if effective_codepoint == ARROW_LEFT {
        Some("left".to_owned())
    } else if effective_codepoint == ARROW_RIGHT {
        Some("right".to_owned())
    } else if (48..=57).contains(&effective_codepoint) {
        char::from_u32(effective_codepoint as u32).map(|c| c.to_string())
    } else if (97..=122).contains(&effective_codepoint) {
        char::from_u32(effective_codepoint as u32).map(|c| c.to_string())
    } else if let Some(c) = char::from_u32(effective_codepoint as u32) {
        if is_symbol_key(c) {
            Some(c.to_string())
        } else {
            None
        }
    } else {
        None
    }
}

fn format_parsed_key(
    codepoint: i64,
    modifier: u32,
    base_layout_key: Option<i64>,
) -> Option<String> {
    let normalized_codepoint = normalize_kitty_functional_codepoint(codepoint);
    let identity_codepoint =
        normalize_shifted_letter_identity_codepoint(normalized_codepoint, modifier);

    let is_latin_letter = (97..=122).contains(&identity_codepoint);
    let is_digit = (48..=57).contains(&identity_codepoint);
    let is_known_symbol = char::from_u32(identity_codepoint as u32).is_some_and(is_symbol_key);
    let effective_codepoint = if is_latin_letter || is_digit || is_known_symbol {
        identity_codepoint
    } else {
        base_layout_key.unwrap_or(identity_codepoint)
    };

    let key_name = key_name_for_codepoint(effective_codepoint)?;
    format_key_name_with_modifiers(&key_name, modifier)
}

/// Parse input data and return the key identifier if recognized (e.g.
/// `"ctrl+c"`, `"escape"`).
pub fn parse_key(data: &str) -> Option<String> {
    if let Some(kitty) = parse_kitty_any(data) {
        return format_parsed_key(kitty.codepoint, kitty.modifier, kitty.base_layout_key);
    }
    if let Some(mok) = parse_modify_other_keys_sequence(data) {
        return format_parsed_key(mok.codepoint, mok.modifier, None);
    }

    // Mode-aware legacy sequences.
    if is_kitty_protocol_active() && (data == "\x1b\r" || data == "\n") {
        return Some("shift+enter".to_owned());
    }

    if let Some((_, key_id)) = LEGACY_SEQUENCE_KEY_IDS.iter().find(|(seq, _)| *seq == data) {
        return Some((*key_id).to_owned());
    }

    if data == "\x1b" {
        return Some("escape".to_owned());
    }
    if data == "\x1c" {
        return Some("ctrl+\\".to_owned());
    }
    if data == "\x1d" {
        return Some("ctrl+]".to_owned());
    }
    if data == "\x1f" {
        return Some("ctrl+-".to_owned());
    }
    if data == "\x1b\x1b" {
        return Some("ctrl+alt+[".to_owned());
    }
    if data == "\x1b\x1c" {
        return Some("ctrl+alt+\\".to_owned());
    }
    if data == "\x1b\x1d" {
        return Some("ctrl+alt+]".to_owned());
    }
    if data == "\x1b\x1f" {
        return Some("ctrl+alt+-".to_owned());
    }
    if data == "\t" {
        return Some("tab".to_owned());
    }
    if data == "\r" || (!is_kitty_protocol_active() && data == "\n") || data == "\x1bOM" {
        return Some("enter".to_owned());
    }
    if data == "\x00" {
        return Some("ctrl+space".to_owned());
    }
    if data == " " {
        return Some("space".to_owned());
    }
    if data == "\x7f" {
        return Some("backspace".to_owned());
    }
    if data == "\x08" {
        return Some(if is_windows_terminal_session() {
            "ctrl+backspace".to_owned()
        } else {
            "backspace".to_owned()
        });
    }
    if data == "\x1b[Z" {
        return Some("shift+tab".to_owned());
    }
    if !is_kitty_protocol_active() && data == "\x1b\r" {
        return Some("alt+enter".to_owned());
    }
    if !is_kitty_protocol_active() && data == "\x1b " {
        return Some("alt+space".to_owned());
    }
    if data == "\x1b\x7f" || data == "\x1b\u{8}" {
        return Some("alt+backspace".to_owned());
    }
    if !is_kitty_protocol_active() && data == "\x1bB" {
        return Some("alt+left".to_owned());
    }
    if !is_kitty_protocol_active() && data == "\x1bF" {
        return Some("alt+right".to_owned());
    }
    if !is_kitty_protocol_active() && data.len() == 2 && data.starts_with('\x1b') {
        let code = data.chars().nth(1).map(|c| c as u32).unwrap_or(0);
        if (1..=26).contains(&code) {
            let letter = char::from_u32(code + 96).unwrap_or('?');
            return Some(format!("ctrl+alt+{letter}"));
        }
        if (97..=122).contains(&code) || (48..=57).contains(&code) {
            let c = char::from_u32(code).unwrap_or('?');
            return Some(format!("alt+{c}"));
        }
    }
    if data == "\x1b[A" {
        return Some("up".to_owned());
    }
    if data == "\x1b[B" {
        return Some("down".to_owned());
    }
    if data == "\x1b[C" {
        return Some("right".to_owned());
    }
    if data == "\x1b[D" {
        return Some("left".to_owned());
    }
    if data == "\x1b[H" || data == "\x1bOH" {
        return Some("home".to_owned());
    }
    if data == "\x1b[F" || data == "\x1bOF" {
        return Some("end".to_owned());
    }
    if data == "\x1b[3~" {
        return Some("delete".to_owned());
    }
    if data == "\x1b[5~" {
        return Some("pageUp".to_owned());
    }
    if data == "\x1b[6~" {
        return Some("pageDown".to_owned());
    }

    // Raw Ctrl+letter / printable char.
    if data.len() == 1 {
        let code = data.chars().next().map(|c| c as u32).unwrap_or(0);
        if (1..=26).contains(&code) {
            let letter = char::from_u32(code + 96).unwrap_or('?');
            return Some(format!("ctrl+{letter}"));
        }
        if (32..=126).contains(&code) {
            return Some(data.to_owned());
        }
    }
    None
}

// =============================================================================
// Release / repeat detection
// =============================================================================

/// True when `data` is a Kitty key-release event (flag 2 active).
pub fn is_key_release(data: &str) -> bool {
    // Bracketed paste content must not be treated as key release (e.g.
    // bluetooth MAC addresses like "90:62:3F:A5").
    if data.contains("\x1b[200~") {
        return false;
    }
    [":3u", ":3~", ":3A", ":3B", ":3C", ":3D", ":3H", ":3F"]
        .iter()
        .any(|s| data.contains(s))
}

/// True when `data` is a Kitty key-repeat event (flag 2 active).
pub fn is_key_repeat(data: &str) -> bool {
    if data.contains("\x1b[200~") {
        return false;
    }
    [":2u", ":2~", ":2A", ":2B", ":2C", ":2D", ":2H", ":2F"]
        .iter()
        .any(|s| data.contains(s))
}

// =============================================================================
// Printable decoding (apps/dimi printable-key discipline)
// =============================================================================

const KITTY_PRINTABLE_ALLOWED_MODIFIERS: u32 = MODIFIERS_SHIFT | LOCK_MASK;

/// Decode a Kitty CSI-u sequence into a printable character, if applicable.
/// Accepts plain or Shift-modified keys only; prefers the shifted keycode.
pub fn decode_kitty_printable(data: &str) -> Option<String> {
    let parsed = parse_kitty_sequence(data)?;
    // Only the codepoint form (no arrow/functional forms) is printable.
    if parsed.codepoint < 0 {
        return None;
    }
    let modifier = parsed.modifier;
    if (modifier & !KITTY_PRINTABLE_ALLOWED_MODIFIERS) != 0 {
        return None;
    }
    if modifier & (MODIFIERS_ALT | MODIFIERS_CTRL) != 0 {
        return None;
    }
    let mut effective_codepoint = parsed.codepoint;
    if modifier & MODIFIERS_SHIFT != 0 {
        if let Some(shifted) = parsed.shifted_key {
            effective_codepoint = shifted;
        }
    }
    effective_codepoint = normalize_kitty_functional_codepoint(effective_codepoint);
    if effective_codepoint < 32 {
        return None;
    }
    char::from_u32(effective_codepoint as u32).map(|c| c.to_string())
}

fn decode_modify_other_keys_printable(data: &str) -> Option<String> {
    let parsed = parse_modify_other_keys_sequence(data)?;
    let modifier = parsed.modifier & !LOCK_MASK;
    if (modifier & !MODIFIERS_SHIFT) != 0 {
        return None;
    }
    if parsed.codepoint < 32 {
        return None;
    }
    char::from_u32(parsed.codepoint as u32).map(|c| c.to_string())
}

/// Decode raw input into a printable character, if applicable. Use this for
/// printable-key comparisons (the apps/dimi printable-key discipline).
pub fn decode_printable_key(data: &str) -> Option<String> {
    decode_kitty_printable(data).or_else(|| decode_modify_other_keys_printable(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Kitty CSI-u ──────────────────────────────────────────────────────
    #[test]
    fn kitty_ctrl_letter() {
        set_kitty_protocol_active(true);
        assert!(matches_key("\x1b[99;5u", "ctrl+c"));
        assert!(matches_key("\x1b[100;5u", "ctrl+d"));
        assert!(matches_key("\x1b[122;5u", "ctrl+z"));
        assert!(!matches_key("\x1b[1089::99;5u", "ctrl+d"));
        assert!(!matches_key("\x1b[1089::99;5u", "ctrl+shift+c"));
        set_kitty_protocol_active(false);
    }

    #[test]
    fn kitty_cyrillic_base_layout() {
        set_kitty_protocol_active(true);
        assert!(matches_key("\x1b[1089::99;5u", "ctrl+c"));
        assert!(matches_key("\x1b[1092::100;5u", "ctrl+d"));
        assert!(matches_key("\x1b[1093::122;5u", "ctrl+z"));
        assert!(matches_key("\x1b[1093::122;6u", "ctrl+shift+z"));
        set_kitty_protocol_active(false);
    }

    #[test]
    fn kitty_modifier_combos() {
        set_kitty_protocol_active(true);
        assert!(matches_key("\x1b[107;9u", "super+k"));
        assert!(matches_key("\x1b[13;9u", "super+enter"));
        assert!(matches_key("\x1b[107;13u", "ctrl+super+k"));
        assert!(matches_key("\x1b[107;14u", "ctrl+shift+super+k"));
        assert!(!matches_key("\x1b[107;13u", "super+k"));
        assert_eq!(parse_key("\x1b[107;9u").as_deref(), Some("super+k"));
        assert_eq!(parse_key("\x1b[13;9u").as_deref(), Some("super+enter"));
        assert_eq!(parse_key("\x1b[107;13u").as_deref(), Some("ctrl+super+k"));
        assert_eq!(
            parse_key("\x1b[107;14u").as_deref(),
            Some("shift+ctrl+super+k")
        );
        set_kitty_protocol_active(false);
    }

    #[test]
    fn kitty_digits_and_keypad() {
        set_kitty_protocol_active(true);
        assert!(matches_key("\x1b[49u", "1"));
        assert!(matches_key("\x1b[49;5u", "ctrl+1"));
        assert!(!matches_key("\x1b[49;5u", "ctrl+2"));
        assert_eq!(parse_key("\x1b[49u").as_deref(), Some("1"));
        assert_eq!(parse_key("\x1b[49;5u").as_deref(), Some("ctrl+1"));
        // Numpad → plain keys.
        assert!(matches_key("\x1b[57400u", "1"));
        assert!(matches_key("\x1b[57410u", "/"));
        assert!(matches_key("\x1b[57417u", "left"));
        assert!(matches_key("\x1b[57426u", "delete"));
        assert_eq!(parse_key("\x1b[57399u").as_deref(), Some("0"));
        assert_eq!(parse_key("\x1b[57409u").as_deref(), Some("."));
        assert_eq!(parse_key("\x1b[57413u").as_deref(), Some("+"));
        assert_eq!(parse_key("\x1b[57417u").as_deref(), Some("left"));
        assert_eq!(parse_key("\x1b[57421u").as_deref(), Some("pageUp"));
        assert_eq!(parse_key("\x1b[57424u").as_deref(), Some("end"));
        set_kitty_protocol_active(false);
    }

    #[test]
    fn kitty_shifted_and_event_formats() {
        set_kitty_protocol_active(true);
        // Shifted key format: codepoint:shifted:base;modifier.
        assert!(matches_key("\x1b[99:67:99;2u", "shift+c"));
        // Event type format.
        assert!(matches_key("\x1b[1089::99;5:3u", "ctrl+c"));
        // Full format: Cyrillic с + base c, ctrl+shift, repeat.
        assert!(matches_key("\x1b[1089:1057:99;6:2u", "ctrl+shift+c"));
        // Dvorak: codepoint authoritative.
        assert!(matches_key("\x1b[107::118;5u", "ctrl+k"));
        assert!(!matches_key("\x1b[107::118;5u", "ctrl+v"));
        assert!(matches_key("\x1b[47::91;5u", "ctrl+/"));
        assert!(!matches_key("\x1b[47::91;5u", "ctrl+["));
        set_kitty_protocol_active(false);
    }

    #[test]
    fn kitty_arrows_functional() {
        set_kitty_protocol_active(true);
        assert!(matches_key("\x1b[1;2A", "shift+up"));
        assert!(matches_key("\x1b[1;5D", "ctrl+left"));
        assert!(matches_key("\x1b[3;2~", "shift+delete"));
        assert!(matches_key("\x1b[7;5~", "ctrl+home"));
        assert!(matches_key("\x1b[1;3C", "alt+right"));
        assert!(matches_key("\x1b[1;2H", "shift+home"));
        set_kitty_protocol_active(false);
    }

    // ── modifyOtherKeys ──────────────────────────────────────────────────
    #[test]
    fn modify_other_keys_basic() {
        set_kitty_protocol_active(false);
        assert!(matches_key("\x1b[27;5;99~", "ctrl+c"));
        assert!(matches_key("\x1b[27;5;100~", "ctrl+d"));
        assert!(matches_key("\x1b[27;5;122~", "ctrl+z"));
        assert_eq!(parse_key("\x1b[27;5;99~").as_deref(), Some("ctrl+c"));
        assert!(matches_key("\x1b[27;5;13~", "ctrl+enter"));
        assert!(matches_key("\x1b[27;2;13~", "shift+enter"));
        assert!(matches_key("\x1b[27;3;13~", "alt+enter"));
        assert_eq!(parse_key("\x1b[27;2;13~").as_deref(), Some("shift+enter"));
        assert!(matches_key("\x1b[27;2;9~", "shift+tab"));
        assert!(matches_key("\x1b[27;5;9~", "ctrl+tab"));
        assert!(matches_key("\x1b[27;1;127~", "backspace"));
        assert!(matches_key("\x1b[27;5;127~", "ctrl+backspace"));
        assert!(matches_key("\x1b[27;1;27~", "escape"));
        assert!(matches_key("\x1b[27;1;32~", "space"));
        assert!(matches_key("\x1b[27;5;32~", "ctrl+space"));
        assert_eq!(parse_key("\x1b[27;1;32~").as_deref(), Some("space"));
    }

    // ── Legacy ───────────────────────────────────────────────────────────
    #[test]
    fn legacy_ctrl_letters() {
        set_kitty_protocol_active(false);
        assert!(matches_key("\x03", "ctrl+c"));
        assert!(matches_key("\x04", "ctrl+d"));
        assert!(matches_key("\x1a", "ctrl+z"));
        assert_eq!(parse_key("\x03").as_deref(), Some("ctrl+c"));
    }

    #[test]
    fn legacy_special_keys() {
        set_kitty_protocol_active(false);
        assert!(matches_key("\x1b", "escape"));
        assert!(matches_key("\t", "tab"));
        assert!(matches_key("\r", "enter"));
        assert!(matches_key("\n", "enter"));
        assert!(matches_key(" ", "space"));
        assert!(matches_key("\x7f", "backspace"));
        assert!(matches_key("\x1b[Z", "shift+tab"));
        assert_eq!(parse_key("\x1b").as_deref(), Some("escape"));
        assert_eq!(parse_key("\t").as_deref(), Some("tab"));
        assert_eq!(parse_key("\x00").as_deref(), Some("ctrl+space"));
    }

    #[test]
    fn legacy_arrows_and_function_keys() {
        set_kitty_protocol_active(false);
        assert!(matches_key("\x1b[A", "up"));
        assert!(matches_key("\x1b[B", "down"));
        assert!(matches_key("\x1b[C", "right"));
        assert!(matches_key("\x1b[D", "left"));
        assert!(matches_key("\x1bOA", "up"));
        assert!(matches_key("\x1b[H", "home"));
        assert!(matches_key("\x1bOH", "home"));
        assert!(matches_key("\x1b[F", "end"));
        assert!(matches_key("\x1b[3~", "delete"));
        assert!(matches_key("\x1b[5~", "pageUp"));
        assert!(matches_key("\x1bOP", "f1"));
        assert!(matches_key("\x1b[15~", "f5"));
        assert_eq!(parse_key("\x1b[A").as_deref(), Some("up"));
        assert_eq!(parse_key("\x1b[5~").as_deref(), Some("pageUp"));
    }

    #[test]
    fn legacy_alt_arrows() {
        set_kitty_protocol_active(false);
        assert!(matches_key("\x1bb", "alt+left"));
        assert!(matches_key("\x1bf", "alt+right"));
        assert!(matches_key("\x1bp", "alt+up"));
        assert!(matches_key("\x1bn", "alt+down"));
        assert!(matches_key("\x1bB", "alt+left")); // rxvt-style when kitty inactive
        assert!(matches_key("\x1bF", "alt+right"));
        set_kitty_protocol_active(true);
        assert!(!matches_key("\x1bB", "alt+left"));
        set_kitty_protocol_active(false);
    }

    #[test]
    fn legacy_alt_letters_and_ctrl_alt() {
        set_kitty_protocol_active(false);
        assert!(matches_key("\x1ba", "alt+a"));
        assert!(matches_key("\x1b1", "alt+1"));
        // ctrl+alt+x → ESC + ctrl char.
        assert!(matches_key("\x1b\x18", "ctrl+alt+x"));
        assert_eq!(parse_key("\x1ba").as_deref(), Some("alt+a"));
    }

    #[test]
    fn legacy_linefeed_kitty_shift_enter() {
        set_kitty_protocol_active(false);
        assert!(matches_key("\n", "enter"));
        set_kitty_protocol_active(true);
        // When kitty active, \n is the Ghostty shift+enter mapping.
        assert!(!matches_key("\n", "enter"));
        assert!(matches_key("\n", "shift+enter"));
        assert!(matches_key("\x1b\r", "shift+enter"));
        set_kitty_protocol_active(false);
    }

    #[test]
    fn legacy_shift_sequences() {
        set_kitty_protocol_active(false);
        assert!(matches_key("\x1b[a", "shift+up"));
        assert!(matches_key("\x1b[2$", "shift+insert"));
        assert!(matches_key("\x1b[3$", "shift+delete"));
        assert!(matches_key("\x1b[5$", "shift+pageUp"));
        assert!(matches_key("\x1b[7$", "shift+home"));
        assert!(matches_key("\x1b[8$", "shift+end"));
        assert!(matches_key("\x1bOa", "ctrl+up"));
        assert!(matches_key("\x1b[2^", "ctrl+insert"));
        assert!(matches_key("\x1b[7^", "ctrl+home"));
    }

    #[test]
    fn legacy_raw_backspace() {
        // Raw 0x08 outside Windows Terminal (no WT_SESSION) is plain
        // backspace; the env-snapshot helper drives the platform branch.
        assert!(!is_windows_terminal_session_with(false, false));
        assert!(is_windows_terminal_session_with(true, false));
        assert!(!is_windows_terminal_session_with(true, true));
        assert!(matches_raw_backspace("\x08", 0));
        assert!(matches_raw_backspace("\x7f", 0));
    }

    #[test]
    fn legacy_ctrl_symbols() {
        set_kitty_protocol_active(false);
        // Ctrl+[ = ESC, Ctrl+\ = 0x1c, Ctrl+] = 0x1d, Ctrl+_ = 0x1f.
        assert!(matches_key("\x1b", "ctrl+["));
        assert!(matches_key("\x1c", "ctrl+\\"));
        assert!(matches_key("\x1d", "ctrl+]"));
        assert!(matches_key("\x1f", "ctrl+-"));
        assert_eq!(parse_key("\x1c").as_deref(), Some("ctrl+\\"));
        assert_eq!(parse_key("\x1d").as_deref(), Some("ctrl+]"));
        assert_eq!(parse_key("\x1f").as_deref(), Some("ctrl+-"));
    }

    // ── printable decoding ───────────────────────────────────────────────
    #[test]
    fn decode_kitty_printable_basic() {
        set_kitty_protocol_active(true);
        assert_eq!(decode_printable_key("\x1b[97u").as_deref(), Some("a"));
        assert_eq!(decode_printable_key("\x1b[65u").as_deref(), Some("A"));
        assert_eq!(decode_printable_key("\x1b[49u").as_deref(), Some("1"));
        assert_eq!(decode_printable_key("\x1b[33u").as_deref(), Some("!"));
        // Control chars rejected.
        assert_eq!(decode_printable_key("\x1b[3u"), None);
        // Ctrl/Alt rejected.
        assert_eq!(decode_printable_key("\x1b[97;5u"), None);
        assert_eq!(decode_printable_key("\x1b[97;3u"), None);
        set_kitty_protocol_active(false);
    }

    #[test]
    fn decode_printable_prefers_shifted() {
        set_kitty_protocol_active(true);
        // Shift held: prefer the shifted keycode (65 = 'A').
        assert_eq!(
            decode_printable_key("\x1b[97:65:97;2u").as_deref(),
            Some("A")
        );
        set_kitty_protocol_active(false);
    }

    #[test]
    fn decode_modify_other_keys_printable() {
        set_kitty_protocol_active(false);
        assert_eq!(decode_printable_key("\x1b[27;5;99~"), None); // ctrl+c not printable
        assert_eq!(decode_printable_key("\x1b[27;1;99~").as_deref(), Some("c"));
        assert_eq!(decode_printable_key("\x1b[27;2;99~").as_deref(), Some("c"));
    }

    #[test]
    fn parse_key_shifted_uppercase() {
        set_kitty_protocol_active(true);
        // Shifted uppercase CSI-u letters parse as shift+letter.
        assert_eq!(parse_key("\x1b[65;2u").as_deref(), Some("shift+a"));
        set_kitty_protocol_active(false);
    }

    #[test]
    fn parse_key_unsupported_modifiers_ignored() {
        set_kitty_protocol_active(true);
        assert_eq!(parse_key("\x1b[97;17u"), None);
        set_kitty_protocol_active(false);
    }

    // ── release / repeat ─────────────────────────────────────────────────
    #[test]
    fn release_and_repeat_detection() {
        assert!(is_key_release("\x1b[97;1:3u"));
        assert!(!is_key_release("\x1b[97;1:2u"));
        assert!(is_key_repeat("\x1b[97;1:2u"));
        assert!(!is_key_repeat("\x1b[97;1:3u"));
        // Bracketed paste must not match release/repeat patterns.
        assert!(!is_key_release("\x1b[200~90:62:3F:A5\x1b[201~"));
        assert!(!is_key_repeat("\x1b[200~90:62:2F:A5\x1b[201~"));
    }
}
