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
    match parse_args() {
        Ok(Action::Help) => {
            print_usage();
            ExitCode::SUCCESS
        }
        Ok(Action::Version) => {
            println!("{}", version());
            ExitCode::SUCCESS
        }
        Ok(Action::Run(parsed)) => run(parsed),
        Err(msg) => {
            eprintln!("{msg}");
            print_usage();
            ExitCode::from(2)
        }
    }
}

/// The version string printed by `--version`. A bare version number, mirroring
/// the TS CLI's `dimi --version` output (`apps/dimi`'s commander `-V, --version`).
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Run the interactive TUI. Requires a real terminal (no pipes or redirection).
fn run(
    ParsedArgs {
        wire: cli_wire,
        config: cli_config,
    }: ParsedArgs,
) -> ExitCode {
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
#[derive(Debug)]
struct ParsedArgs {
    wire: Option<String>,
    config: Option<PathBuf>,
}

/// Top-level action resolved from the CLI arguments. `--help`/`--version` are
/// resolved to their own actions so `main` can handle them before the TTY
/// check (version/help must work in a piped, non-interactive invocation).
#[derive(Debug)]
enum Action {
    Run(ParsedArgs),
    Help,
    Version,
}

fn parse_args() -> Result<Action, String> {
    parse_args_from(std::env::args().skip(1))
}

fn parse_args_from<I>(args: I) -> Result<Action, String>
where
    I: IntoIterator<Item = String>,
{
    let mut wire = None;
    let mut config = None;
    let mut args = args.into_iter();
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
            "--help" | "-h" => return Ok(Action::Help),
            "--version" | "-V" => return Ok(Action::Version),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Action::Run(ParsedArgs { wire, config }))
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
         \x20 -h, --help       show this help\n\
         \x20 -V, --version    print the version and exit"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the CLI parser over an explicit argv (bypasses `std::env::args`).
    fn run(args: &[&str]) -> Result<Action, String> {
        parse_args_from(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_args_runs_with_defaults() {
        match run(&[]).unwrap() {
            Action::Run(ParsedArgs { wire, config }) => {
                assert!(wire.is_none());
                assert!(config.is_none());
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn version_flags_resolve_to_version_action() {
        assert!(matches!(run(&["--version"]).unwrap(), Action::Version));
        assert!(matches!(run(&["-V"]).unwrap(), Action::Version));
    }

    #[test]
    fn help_flags_resolve_to_help_action() {
        assert!(matches!(run(&["--help"]).unwrap(), Action::Help));
        assert!(matches!(run(&["-h"]).unwrap(), Action::Help));
    }

    #[test]
    fn version_wins_when_combined_with_other_flags() {
        assert!(matches!(
            run(&["--wire", "w.jsonl", "--version"]).unwrap(),
            Action::Version
        ));
    }

    #[test]
    fn wire_and_config_are_carried() {
        match run(&["--wire", "w.jsonl", "--config", "c.toml"]).unwrap() {
            Action::Run(ParsedArgs { wire, config }) => {
                assert_eq!(wire.as_deref(), Some("w.jsonl"));
                assert_eq!(config.as_deref(), Some(PathBuf::from("c.toml").as_path()));
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn value_flags_require_an_argument() {
        assert_eq!(run(&["--wire"]).unwrap_err(), "--wire requires a path");
        assert_eq!(run(&["--config"]).unwrap_err(), "--config requires a path");
    }

    #[test]
    fn unknown_argument_is_rejected() {
        assert_eq!(run(&["-S"]).unwrap_err(), "unknown argument: -S");
        assert_eq!(run(&["-p", "hello"]).unwrap_err(), "unknown argument: -p");
    }

    #[test]
    fn version_string_matches_crate_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
