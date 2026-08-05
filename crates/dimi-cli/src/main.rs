//! dimi-cli binary — the Rust TUI entry (M6, slice 6).
//!
//! Replaces the TS `apps/dimi/src/tui` entry's final shape: parses
//! `--wire` / `--config`, checks the TTY, installs the dark palette, and
//! runs the `DimiApp` coordinator (raw terminal + differential `Tui` +
//! transcript/footer/editor + the engine-backed session backend).
//!
//! Engine wiring lands in slice 6a: normal prompts and `!` bash lines run a
//! real `dimi-engine` `Engine::run_turn` against the provider configured in
//! `config.toml` (`model` / `base_url` / `api_key`), streaming events into
//! the transcript.
//!
//! ```text
//! cargo run -p dimi-cli -- --wire crates/dimi-tui/testdata/sample-wire.jsonl
//! ```

mod activity;
mod app;
mod btw_panel;
mod config;
mod panels;
mod tasks_panel;
mod transcript;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use app::DimiApp;
use config::{default_config_path, load_config};
use dimi_tui::theme::{DARK_COLORS, set_palette};

fn main() -> ExitCode {
    let ParsedArgs {
        wire: cli_wire,
        config: cli_config,
    } = match parse_args() {
        Ok(parsed) => parsed,
        Err(msg) => {
            eprintln!("{msg}");
            print_usage();
            return ExitCode::from(2);
        }
    };

    if !std::io::stdin().is_terminal() {
        eprintln!("dimi-cli requires a TTY (run it interactively)");
        return ExitCode::from(1);
    }

    // CLI wins over the config file for `--wire`; the config file path is
    // `--config` or `~/.dimi/config.toml`.
    let config_path = cli_config.unwrap_or_else(default_config_path);
    let mut config = load_config(&config_path);
    if let Some(wire) = cli_wire {
        config.wire = Some(wire);
    }

    set_palette(DARK_COLORS);

    let mut app = DimiApp::new(config);
    app.run();
    ExitCode::SUCCESS
}

/// Parsed CLI arguments.
struct ParsedArgs {
    wire: Option<String>,
    config: Option<PathBuf>,
}

fn parse_args() -> Result<ParsedArgs, String> {
    let mut wire = None;
    let mut config = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--wire" => {
                wire = args.next();
                if wire.is_none() {
                    return Err("--wire requires a path".to_owned());
                }
            }
            "--config" => {
                config = args.next().map(PathBuf::from);
                if config.is_none() {
                    return Err("--config requires a path".to_owned());
                }
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(ParsedArgs { wire, config })
}

fn print_usage() {
    eprintln!(
        "dimi-cli — the Rust dimi TUI (slice 6)\n\
         \n\
         usage: dimi-cli [--wire <path>] [--config <path>]\n\
         \n\
         options:\n\
         \x20 --wire <path>    cold-rebuild the transcript from a wire.jsonl file\n\
         \x20 --config <path>  config.toml path (default: ~/.dimi/config.toml)\n\
         \x20 -h, --help       show this help"
    );
}
