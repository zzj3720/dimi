//! Transcript message components — user / assistant / thinking / status /
//! notice / compaction, ported from `apps/dimi/src/tui/components/messages/*`
//! and `components/dialogs/compaction.ts`.

pub mod assistant_message;
pub mod compaction;
pub mod golden_tests;
pub mod plan_box;
pub mod shell_execution;
pub mod status_message;
pub mod thinking;
pub mod tool_call;
pub mod tool_golden_tests;
pub mod tool_renderers;
pub mod user_message;

/// `● ` — the shared transcript status bullet (`STATUS_BULLET`).
pub const STATUS_BULLET: &str = "● ";
/// `✨ ` — the user message bullet (`USER_MESSAGE_BULLET`).
pub const USER_MESSAGE_BULLET: &str = "✨ ";
/// `✓ ` / `✗ ` — success / failure marks.
pub const SUCCESS_MARK: &str = "✓ ";
pub const FAILURE_MARK: &str = "✗ ";
/// `  ` — continuation indent for two-cell leading markers (`MESSAGE_INDENT`).
pub const MESSAGE_INDENT: &str = "  ";
/// Preview caps shared by thinking / tool results / shell snippets.
pub const RESULT_PREVIEW_LINES: usize = 3;
pub const THINKING_PREVIEW_LINES: usize = 2;
pub const COMMAND_PREVIEW_LINES: usize = 10;
/// Braille spinner frames (live thinking).
pub const BRAILLE_SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
