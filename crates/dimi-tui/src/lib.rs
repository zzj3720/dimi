//! `dimi-tui` — Rust terminal UI for Dimi.
//!
//! M5 scope of the rustify plan (see `PLAN.md`): port the TS TUI
//! (`apps/dimi/src/tui`) and its rendering library (`@dimi-agent/pi-tui`)
//! to Rust. The differential rendering core is a contract-aligned port of
//! pi-tui's `TUI` class so every migrated component can be diffed against the
//! TS TUI frame-by-frame (component golden tests + real-terminal snapshot
//! diffs).
//!
//! Slice map (each slice has its own DoD):
//! 1. **rendering core** (this slice): width/ANSI handling, component +
//!    container, theme tokens, differential `Tui`, terminal abstraction, demo.
//! 2. message rendering: markdown + transcript components over `dimi-store`.
//! 3. editor: `CustomEditor` equivalent + full key handling.
//! 4. interactive controllers: session-event routing, streaming UI, chrome.
//! 5. dialogs / reverse-rpc + slash-command dispatch.
//! 6. integration: session management, theme switching, engine wiring,
//!    replace the `apps/dimi` TUI entry.

pub mod ansi;
pub mod chrome;
pub mod code_highlight;
pub mod commands;
pub mod component;
pub mod components;
pub mod container;
pub mod controllers;
pub mod custom_editor;
pub mod dialogs;
pub mod diff;
pub mod editor;
pub mod footer;
pub mod fuzzy;
pub mod image;
pub mod keys;
pub mod loader;
pub mod markdown;
pub mod markdown_theme;
pub mod paging;
pub mod process_terminal;
pub mod queue_controller;
pub mod searchable_list;
pub mod select_list;
pub mod session;
pub mod slice8_golden;
pub mod stdin_buffer;
pub mod style;
pub mod tab_strip;
pub mod terminal;
pub mod theme;
pub mod tui;
pub mod width;
pub mod wire_transcript;
pub mod working_tips;
pub mod wrap;
