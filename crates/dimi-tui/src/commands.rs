//! Slash-command system — parsing, resolution, dispatch, and the builtin
//! command registry + argument completion
//! (port of `apps/dimi/src/tui/commands/{parse,resolve,dispatch}.ts` plus
//! `complete-args.ts`, `registry.ts`, and `experimental-flags.ts`).
//!
//! Slice 6 scope: parse + resolve + the dispatch routing skeleton. This
//! extension adds the static builtin registry (`BUILTIN_SLASH_COMMANDS`),
//! `resolve_builtin_slash_input` (the TS-accurate resolution path backed by
//! the registry), and the leading-token / directory-path argument
//! completers. The builtin command bodies and skill/plugin activation land
//! with the app-shell integration.

/// A parsed slash command: `/name args`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSlashInput {
    pub name: String,
    pub args: String,
}

/// Parse `/name args` input. Returns `None` for non-slash input, empty
/// commands, or file paths (`/usr/local/bin`) — but allows namespaced plugin
/// commands whose name contains `/` after a `:` (e.g. `plugin:frontend/component`).
pub fn parse_slash_input(input: &str) -> Option<ParsedSlashInput> {
    if !input.starts_with('/') {
        return None;
    }
    let trimmed = input[1..].trim();
    if trimmed.is_empty() {
        return None;
    }
    let (name, args) = match trimmed.find(' ') {
        Some(idx) => (&trimmed[..idx], trimmed[idx + 1..].trim()),
        None => (trimmed, ""),
    };
    if name.contains('/') && !name.contains(':') {
        return None;
    }
    Some(ParsedSlashInput {
        name: name.to_owned(),
        args: args.to_owned(),
    })
}

/// Busy reason for an idle-only command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommandBusyReason {
    Streaming,
    Compacting,
}

/// Command availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommandAvailability {
    Always,
    IdleOnly,
}

/// An autocomplete menu item (mirrors `AutocompleteItem` from pi-tui).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutocompleteItem {
    /// The value inserted into the input when the item is chosen.
    pub value: String,
    /// The label rendered in the menu.
    pub label: String,
    /// Optional description rendered next to the label.
    pub description: Option<String>,
}

/// A completable token (subcommand or flag) for a slash command's argument
/// position (mirrors `ArgCompletionSpec` from `complete-args.ts`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgCompletionSpec {
    /// The token inserted on completion, e.g. `pause` or `resume`.
    pub value: String,
    /// Short description shown in the autocomplete menu.
    pub description: String,
}

/// Argument-completion callback type (mirrors `completeArgs`): given the text
/// typed after `/<command> `, return suggestions or `None`.
pub type CompleteArgsFn = fn(&str) -> Option<Vec<AutocompleteItem>>;

/// A slash command declaration.
#[derive(Debug, Clone)]
pub struct SlashCommand {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub availability: SlashCommandAvailability,
    /// Sort priority, higher first (mirrors `priority`, default 0).
    pub priority: i64,
    /// Argument hint shown in the command palette (mirrors `argumentHint`).
    pub argument_hint: Option<String>,
    /// Experimental flag gating this command (mirrors `experimentalFlag`).
    pub experimental_flag: Option<String>,
    /// Function-valued availability applied to the argument string
    /// (mirrors `availability?: (args) => SlashCommandAvailability`).
    pub availability_fn: Option<fn(&str) -> SlashCommandAvailability>,
    /// Argument autocompletion (mirrors `completeArgs`).
    pub complete_args: Option<CompleteArgsFn>,
}

impl SlashCommand {
    pub fn new(name: &str, description: &str) -> Self {
        SlashCommand {
            name: name.to_owned(),
            aliases: Vec::new(),
            description: description.to_owned(),
            availability: SlashCommandAvailability::Always,
            priority: 0,
            argument_hint: None,
            experimental_flag: None,
            availability_fn: None,
            complete_args: None,
        }
    }
}

/// Resolve a command's availability for a given argument string (mirrors
/// `resolveSlashCommandAvailability`).
///
/// TS semantics: `command.availability ?? 'idle-only'`, then a function-valued
/// availability is applied to `args`. NOTE on the Rust field default:
/// `SlashCommand::new` defaults `availability` to `Always` for slice-6 backward
/// compat, which intentionally diverges from TS's "unset → idle-only". The
/// builtin registry encodes the TS default by setting `availability: IdleOnly`
/// on commands whose TS entry omits it, so for registry commands this function
/// matches TS exactly. Hand-built commands that want TS semantics should set
/// `availability: IdleOnly` explicitly (or provide `availability_fn`).
pub fn resolve_availability(command: &SlashCommand, args: &str) -> SlashCommandAvailability {
    match command.availability_fn {
        Some(f) => f(args),
        None => command.availability,
    }
}

/// Generic leading-token completer for slash-command arguments (port of
/// `completeLeadingArg`).
///
/// pi-tui passes `argumentPrefix` = everything typed after `/<command> `. We
/// only complete the *first* token: once the user has typed a space after it
/// (moved on to an objective, a flag value, etc.) we return `None` so
/// completion never clobbers free text. Matching is a case-insensitive prefix
/// match on `value`. A sole remaining match that equals the prefix is
/// suppressed (e.g. `status` after typing `status`) so Enter submits the
/// command instead of a no-op completion.
pub fn complete_leading_arg(
    specs: &[ArgCompletionSpec],
    argument_prefix: &str,
) -> Option<Vec<AutocompleteItem>> {
    if argument_prefix.contains(' ') {
        return None;
    }
    let lower = argument_prefix.to_lowercase();
    let items: Vec<AutocompleteItem> = specs
        .iter()
        .filter(|spec| spec.value.to_lowercase().starts_with(&lower))
        .map(|spec| AutocompleteItem {
            value: spec.value.clone(),
            label: spec.value.clone(),
            description: Some(spec.description.clone()),
        })
        .collect();
    let only = items.first();
    if items.len() == 1 && only.is_some_and(|item| item.value.to_lowercase() == lower) {
        return None;
    }
    if items.is_empty() { None } else { Some(items) }
}

/// Resolution input.
#[derive(Debug, Clone)]
pub struct ResolveSlashCommandInput {
    pub input: String,
    pub skill_command_map: std::collections::HashMap<String, String>,
    pub plugin_command_map: std::collections::HashSet<String>,
    pub is_streaming: bool,
    pub is_compacting: bool,
}

/// The resolved intent for a slash input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommandIntent {
    NotCommand,
    Builtin {
        name: String,
        args: String,
    },
    Skill {
        command_name: String,
        skill_name: String,
        args: String,
    },
    PluginCommand {
        command_name: String,
        plugin_id: String,
        args: String,
    },
    Message {
        input: String,
    },
    Blocked {
        command_name: String,
        reason: SlashCommandBusyReason,
    },
    Invalid {
        command_name: String,
    },
}

/// Find a command by name or alias.
pub fn find_command<'a>(commands: &'a [SlashCommand], name: &str) -> Option<&'a SlashCommand> {
    commands
        .iter()
        .find(|c| c.name == name || c.aliases.iter().any(|a| a == name))
}

/// Busy reason when streaming/compacting.
pub fn slash_command_busy_reason(
    is_streaming: bool,
    is_compacting: bool,
) -> Option<SlashCommandBusyReason> {
    if is_streaming {
        Some(SlashCommandBusyReason::Streaming)
    } else if is_compacting {
        Some(SlashCommandBusyReason::Compacting)
    } else {
        None
    }
}

/// Busy message (mirrors `slashBusyMessage`).
pub fn slash_busy_message(command_name: &str, reason: SlashCommandBusyReason) -> String {
    match reason {
        SlashCommandBusyReason::Streaming => {
            format!("Cannot /{command_name} while streaming — press Esc or Ctrl-C first.")
        }
        SlashCommandBusyReason::Compacting => {
            format!(
                "Cannot /{command_name} while compacting — wait for compaction to finish first."
            )
        }
    }
}

/// Resolve a slash input to an intent against an explicit command list
/// (slice-6 variant of `resolveSlashCommandInput`).
///
/// Kept intact for backward compat; note it uses the raw `availability` field
/// (not the TS-default-aware [`resolve_availability`]) and takes an explicit
/// command list. Prefer [`resolve_builtin_slash_input`], the TS-accurate path
/// backed by the static builtin registry.
pub fn resolve_slash_command_input(
    options: &ResolveSlashCommandInput,
    commands: &[SlashCommand],
) -> SlashCommandIntent {
    let Some(parsed) = parse_slash_input(&options.input) else {
        return SlashCommandIntent::NotCommand;
    };

    // Builtin commands win first.
    if let Some(command) = find_command(commands, &parsed.name) {
        let busy_reason = slash_command_busy_reason(options.is_streaming, options.is_compacting);
        if let Some(reason) = busy_reason
            && command.availability == SlashCommandAvailability::IdleOnly
        {
            return SlashCommandIntent::Blocked {
                command_name: parsed.name,
                reason,
            };
        }
        return SlashCommandIntent::Builtin {
            name: command.name.clone(),
            args: parsed.args,
        };
    }

    // Skill commands.
    let skill_name = options
        .skill_command_map
        .get(&parsed.name)
        .or_else(|| {
            options
                .skill_command_map
                .get(&format!("skill:{}", parsed.name))
        })
        .cloned();
    if let Some(skill_name) = skill_name {
        let busy_reason = slash_command_busy_reason(options.is_streaming, options.is_compacting);
        if let Some(reason) = busy_reason {
            return SlashCommandIntent::Blocked {
                command_name: parsed.name,
                reason,
            };
        }
        return SlashCommandIntent::Skill {
            command_name: parsed.name,
            skill_name,
            args: parsed.args.trim().to_owned(),
        };
    }

    // Plugin commands (`plugin:command` names).
    if options.plugin_command_map.contains(&parsed.name) {
        let busy_reason = slash_command_busy_reason(options.is_streaming, options.is_compacting);
        if let Some(reason) = busy_reason {
            return SlashCommandIntent::Blocked {
                command_name: parsed.name,
                reason,
            };
        }
        let separator = parsed.name.find(':').unwrap_or(parsed.name.len());
        let plugin_id = parsed.name[..separator].to_owned();
        let command_name = if separator < parsed.name.len() {
            parsed.name[separator + 1..].to_owned()
        } else {
            String::new()
        };
        return SlashCommandIntent::PluginCommand {
            command_name,
            plugin_id,
            args: parsed.args.trim().to_owned(),
        };
    }

    SlashCommandIntent::Message {
        input: options.input.clone(),
    }
}

/// Resolve a slash input against the builtin registry (mirrors `resolve.ts` /
/// `resolveSlashCommandInput` exactly): parse → `find_builtin_slash_command`
/// → experimental-flag gate (stub: always enabled) → busy check against the
/// *resolved* availability (fn-aware, idle-only default) → builtin; else
/// skill map; else plugin map; else message.
///
/// This is the TS-accurate path backed by the static `BUILTIN_SLASH_COMMANDS`
/// registry; [`resolve_slash_command_input`] takes an explicit command list
/// and is kept only for slice-6 backward compat.
pub fn resolve_builtin_slash_input(options: &ResolveSlashCommandInput) -> SlashCommandIntent {
    let Some(parsed) = parse_slash_input(&options.input) else {
        return SlashCommandIntent::NotCommand;
    };

    if let Some(command) = find_builtin_slash_command(&parsed.name)
        && is_experimental_flag_enabled(command.experimental_flag.as_deref())
    {
        let busy_reason = slash_command_busy_reason(options.is_streaming, options.is_compacting);
        if let Some(reason) = busy_reason
            && resolve_availability(command, &parsed.args) == SlashCommandAvailability::IdleOnly
        {
            return SlashCommandIntent::Blocked {
                command_name: parsed.name.clone(),
                reason,
            };
        }
        return SlashCommandIntent::Builtin {
            name: command.name.clone(),
            args: parsed.args.clone(),
        };
    }

    resolve_fallthrough(options, &parsed)
}

/// Skill → plugin → message fallthrough shared by the resolution paths
/// (identical tail to `resolveSlashCommandInput`).
fn resolve_fallthrough(
    options: &ResolveSlashCommandInput,
    parsed: &ParsedSlashInput,
) -> SlashCommandIntent {
    let skill_name = options
        .skill_command_map
        .get(&parsed.name)
        .or_else(|| {
            options
                .skill_command_map
                .get(&format!("skill:{}", parsed.name))
        })
        .cloned();
    if let Some(skill_name) = skill_name {
        let busy_reason = slash_command_busy_reason(options.is_streaming, options.is_compacting);
        if let Some(reason) = busy_reason {
            return SlashCommandIntent::Blocked {
                command_name: parsed.name.clone(),
                reason,
            };
        }
        return SlashCommandIntent::Skill {
            command_name: parsed.name.clone(),
            skill_name,
            args: parsed.args.trim().to_owned(),
        };
    }

    if options.plugin_command_map.contains(&parsed.name) {
        let busy_reason = slash_command_busy_reason(options.is_streaming, options.is_compacting);
        if let Some(reason) = busy_reason {
            return SlashCommandIntent::Blocked {
                command_name: parsed.name.clone(),
                reason,
            };
        }
        let separator = parsed.name.find(':').unwrap_or(parsed.name.len());
        let plugin_id = parsed.name[..separator].to_owned();
        let command_name = if separator < parsed.name.len() {
            parsed.name[separator + 1..].to_owned()
        } else {
            String::new()
        };
        return SlashCommandIntent::PluginCommand {
            command_name,
            plugin_id,
            args: parsed.args.trim().to_owned(),
        };
    }

    SlashCommandIntent::Message {
        input: options.input.clone(),
    }
}

/// Whether an experimental flag is enabled (port of `isExperimentalFlagEnabled`).
///
/// TODO(legacy): real flag state comes from SDK config fetched over RPC at
/// startup (`setExperimentalFeatures` in the TS app). The Rust crate has no
/// SDK snapshot yet, so the stub keeps every command enabled — matching the TS
/// contract that an absent flag is always enabled, and treating unknown flag
/// ids as enabled until the snapshot is wired up.
pub fn is_experimental_flag_enabled(_flag: Option<&str>) -> bool {
    true
}

/// Availability for `/plan`: only `clear` (case-insensitive, trimmed) is
/// idle-only; everything else is always.
fn plan_availability(args: &str) -> SlashCommandAvailability {
    if args.trim().to_lowercase() == "clear" {
        SlashCommandAvailability::IdleOnly
    } else {
        SlashCommandAvailability::Always
    }
}

/// Availability for `/swarm`: an empty/`on`/`off` argument is a configuration
/// toggle (always); anything else starts a new turn (idle-only).
fn swarm_availability(args: &str) -> SlashCommandAvailability {
    let sub = args.trim().to_lowercase();
    if sub.is_empty() || sub == "on" || sub == "off" {
        SlashCommandAvailability::Always
    } else {
        SlashCommandAvailability::IdleOnly
    }
}

fn default_command() -> SlashCommand {
    SlashCommand::new("", "")
}

/// The static builtin command registry (port of `BUILTIN_SLASH_COMMANDS`).
///
/// Names, aliases, descriptions, priorities, argument hints, and availability
/// match `registry.ts` byte-for-byte. Commands whose TS entry omits
/// `availability` (`new`, `sessions`, `compact`, `init`, `fork`) are encoded
/// here as `IdleOnly` — the TS default — so `resolve_availability` matches TS
/// exactly for registry commands. Lazily built behind a `OnceLock` because
/// `SlashCommand` owns `String`s and cannot be a `const` initializer.
static BUILTIN_SLASH_COMMANDS: std::sync::OnceLock<Vec<SlashCommand>> = std::sync::OnceLock::new();

fn builtin_commands() -> Vec<SlashCommand> {
    vec![
        SlashCommand {
            name: "yolo".into(),
            aliases: vec!["yes".into()],
            description: "Toggle YOLO mode: auto-approve tool actions, but the agent may still ask questions.".into(),
            priority: 101,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "auto".into(),
            aliases: Vec::new(),
            description: "Toggle Auto mode: fully autonomous, agent decides everything without asking.".into(),
            priority: 99,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "permission".into(),
            aliases: Vec::new(),
            description: "Select permission mode".into(),
            priority: 100,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "settings".into(),
            aliases: vec!["config".into()],
            description: "Open TUI settings".into(),
            priority: 100,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "plan".into(),
            aliases: Vec::new(),
            description: "Toggle plan mode".into(),
            priority: 100,
            availability_fn: Some(plan_availability),
            ..default_command()
        },
        SlashCommand {
            name: "swarm".into(),
            aliases: Vec::new(),
            description: "Toggle swarm mode or run one task in swarm mode".into(),
            priority: 100,
            argument_hint: Some("[on|off] | <task>".into()),
            availability_fn: Some(swarm_availability),
            complete_args: Some(swarm_argument_completions),
            ..default_command()
        },
        SlashCommand {
            name: "model".into(),
            aliases: Vec::new(),
            description: "Switch LLM model".into(),
            priority: 100,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "secondary_model".into(),
            aliases: Vec::new(),
            description: "Configure the secondary model for subagents".into(),
            priority: 90,
            availability: SlashCommandAvailability::Always,
            experimental_flag: Some("secondary-model".into()),
            ..default_command()
        },
        SlashCommand {
            name: "effort".into(),
            aliases: vec!["thinking".into()],
            description: "Switch thinking effort".into(),
            priority: 95,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "provider".into(),
            aliases: vec!["providers".into()],
            description: "Connect, refresh, or manage AI providers".into(),
            priority: 95,
            argument_hint: Some("[provider|add|import|remove|refresh]".into()),
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "btw".into(),
            aliases: Vec::new(),
            description: "Ask a forked side agent a question".into(),
            priority: 90,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "help".into(),
            aliases: vec!["h".into(), "?".into()],
            description: "Show available commands and shortcuts".into(),
            priority: 80,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "new".into(),
            aliases: vec!["clear".into()],
            description: "Start a fresh session in the current workspace".into(),
            priority: 80,
            // TS omits availability → idle-only default.
            availability: SlashCommandAvailability::IdleOnly,
            ..default_command()
        },
        SlashCommand {
            name: "sessions".into(),
            aliases: vec!["resume".into()],
            description: "Browse and resume sessions".into(),
            priority: 80,
            availability: SlashCommandAvailability::IdleOnly,
            ..default_command()
        },
        SlashCommand {
            name: "tasks".into(),
            aliases: vec!["task".into()],
            description: "Browse background tasks".into(),
            priority: 80,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "mcp".into(),
            aliases: Vec::new(),
            description: "Show MCP server status".into(),
            priority: 60,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "plugins".into(),
            aliases: Vec::new(),
            description: "Manage plugins".into(),
            priority: 60,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "add-dir".into(),
            aliases: Vec::new(),
            description: "Add or list an additional workspace directory".into(),
            priority: 60,
            availability: SlashCommandAvailability::Always,
            argument_hint: Some("[list] | <path>".into()),
            complete_args: Some(add_dir_argument_completions),
            ..default_command()
        },
        SlashCommand {
            name: "experiments".into(),
            aliases: vec!["experimental".into()],
            description: "Manage experimental features".into(),
            priority: 60,
            availability: SlashCommandAvailability::IdleOnly,
            ..default_command()
        },
        SlashCommand {
            name: "reload".into(),
            aliases: Vec::new(),
            description: "Reload session and apply config.toml settings plus tui.toml UI preferences".into(),
            priority: 60,
            availability: SlashCommandAvailability::IdleOnly,
            ..default_command()
        },
        SlashCommand {
            name: "reload-tui".into(),
            aliases: Vec::new(),
            description: "Reload only tui.toml UI preferences".into(),
            priority: 60,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "compact".into(),
            aliases: Vec::new(),
            description: "Compact the conversation context".into(),
            priority: 80,
            argument_hint: Some("<instruction>".into()),
            availability: SlashCommandAvailability::IdleOnly,
            ..default_command()
        },
        SlashCommand {
            name: "init".into(),
            aliases: Vec::new(),
            description: "Analyze the codebase and generate AGENTS.md".into(),
            // TS omits priority (→ 0) and availability (→ idle-only).
            availability: SlashCommandAvailability::IdleOnly,
            ..default_command()
        },
        SlashCommand {
            name: "fork".into(),
            aliases: Vec::new(),
            description: "Fork the current session".into(),
            priority: 80,
            availability: SlashCommandAvailability::IdleOnly,
            ..default_command()
        },
        SlashCommand {
            name: "title".into(),
            aliases: vec!["rename".into()],
            description: "Set or show session title".into(),
            priority: 60,
            argument_hint: Some("<title>".into()),
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "usage".into(),
            aliases: Vec::new(),
            description: "Show session tokens + context window + plan quotas".into(),
            priority: 60,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "status".into(),
            aliases: Vec::new(),
            description: "Show current session and runtime status".into(),
            priority: 60,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "feedback".into(),
            aliases: Vec::new(),
            description: "Send feedback to make Dimi better".into(),
            priority: 60,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "undo".into(),
            aliases: Vec::new(),
            description: "Withdraw the last prompt from the transcript".into(),
            priority: 80,
            availability: SlashCommandAvailability::IdleOnly,
            ..default_command()
        },
        SlashCommand {
            name: "editor".into(),
            aliases: Vec::new(),
            description: "Set the external editor for Ctrl-G".into(),
            priority: 60,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "theme".into(),
            aliases: Vec::new(),
            description: "Set the terminal UI theme".into(),
            priority: 60,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "logout".into(),
            aliases: vec!["disconnect".into()],
            description: "Log out of a configured provider".into(),
            priority: 40,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "login".into(),
            aliases: Vec::new(),
            description: "Connect an AI provider with an account or API key".into(),
            priority: 40,
            argument_hint: Some("[provider]".into()),
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "export-md".into(),
            aliases: vec!["export".into()],
            description: "Export current session as a Markdown file".into(),
            priority: 40,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "export-debug-zip".into(),
            aliases: Vec::new(),
            description: "Export current session as a debug ZIP archive".into(),
            priority: 40,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "copy".into(),
            aliases: Vec::new(),
            description: "Copy the last assistant message to the clipboard".into(),
            priority: 40,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "web".into(),
            aliases: Vec::new(),
            description: "Open the current session in the Web UI by starting a new server".into(),
            priority: 40,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "exit".into(),
            aliases: vec!["quit".into(), "q".into()],
            description: "Exit the application".into(),
            priority: 20,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
        SlashCommand {
            name: "version".into(),
            aliases: Vec::new(),
            description: "Show version information".into(),
            priority: 20,
            availability: SlashCommandAvailability::Always,
            ..default_command()
        },
    ]
}

/// All builtin slash commands (lazily initialized, read-only).
pub fn builtin_slash_commands() -> &'static [SlashCommand] {
    BUILTIN_SLASH_COMMANDS.get_or_init(builtin_commands)
}

/// Find a builtin command by name or alias (mirrors `findBuiltInSlashCommand`).
pub fn find_builtin_slash_command(name: &str) -> Option<&'static SlashCommand> {
    builtin_slash_commands()
        .iter()
        .find(|c| c.name == name || c.aliases.iter().any(|a| a == name))
}

/// Sort commands by descending priority, then ascending name (mirrors
/// `sortSlashCommands`). TS uses `localeCompare`; Rust byte/char `Ord` is a
/// close-enough equivalent — the registry is all ASCII, where byte order
/// equals code-point order.
pub fn sort_slash_commands(mut commands: Vec<SlashCommand>) -> Vec<SlashCommand> {
    commands.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.name.cmp(&b.name))
    });
    commands
}

/// Argument autocompletion for `/swarm` (subcommands `on`/`off`).
pub fn swarm_argument_completions(prefix: &str) -> Option<Vec<AutocompleteItem>> {
    complete_leading_arg(
        &[
            ArgCompletionSpec {
                value: "on".into(),
                description: "Turn swarm mode on".into(),
            },
            ArgCompletionSpec {
                value: "off".into(),
                description: "Turn swarm mode off".into(),
            },
        ],
        prefix,
    )
}

/// Argument autocompletion for `/add-dir`: path-like prefixes complete
/// directories, anything else completes the `list` subcommand.
pub fn add_dir_argument_completions(prefix: &str) -> Option<Vec<AutocompleteItem>> {
    if is_path_like_add_dir_argument(prefix) {
        return complete_add_dir_path(prefix, &system_home_dir());
    }
    complete_leading_arg(
        &[ArgCompletionSpec {
            value: "list".into(),
            description: "Show configured additional workspace directories".into(),
        }],
        prefix,
    )
}

/// Best-effort home directory (TS uses `os.homedir()`, which reads `$HOME` on
/// Unix). Tests inject the home via [`complete_add_dir_path`].
fn system_home_dir() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/"))
}

fn is_path_like_add_dir_argument(argument_prefix: &str) -> bool {
    argument_prefix == "."
        || argument_prefix == ".."
        || argument_prefix.starts_with("./")
        || argument_prefix.starts_with("../")
        || argument_prefix.starts_with('/')
        || argument_prefix.starts_with('~')
}

/// Complete a directory path argument (port of `completeAddDirPath`).
/// `home` is injectable so tests don't depend on the real `$HOME`.
fn complete_add_dir_path(
    argument_prefix: &str,
    home: &std::path::Path,
) -> Option<Vec<AutocompleteItem>> {
    let normalized_prefix = if argument_prefix == "~" {
        "~/"
    } else {
        argument_prefix
    };
    let expanded_prefix = expand_home_prefix(normalized_prefix, home);
    let parent_input =
        get_directory_completion_parent_input(normalized_prefix, &expanded_prefix, home);
    let partial_name = if normalized_prefix.ends_with('/') {
        String::new()
    } else {
        posix_basename(&expanded_prefix).to_owned()
    };
    let partial_name_lower = partial_name.to_lowercase();
    let parent_dir = resolve_directory_completion_parent(&parent_input, home);

    let entries = match std::fs::read_dir(&parent_dir) {
        Ok(entries) => entries,
        Err(_) => return None,
    };

    let mut items = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "." || name == ".." || name.starts_with('.') {
            continue;
        }
        if !partial_name_lower.is_empty() && !name.to_lowercase().starts_with(&partial_name_lower) {
            continue;
        }
        let absolute_path = parent_dir.join(&name);
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !is_directory_path(&absolute_path, &file_type) {
            continue;
        }
        let value =
            format_directory_completion_value(normalized_prefix, &parent_input, &name, home);
        items.push(AutocompleteItem {
            value,
            label: format!("{name}/"),
            description: Some(absolute_path.to_string_lossy().into_owned()),
        });
    }

    if items.is_empty() { None } else { Some(items) }
}

fn expand_home_prefix(argument_prefix: &str, home: &std::path::Path) -> String {
    if argument_prefix == "~" {
        return home.to_string_lossy().into_owned();
    }
    if let Some(rest) = argument_prefix.strip_prefix("~/") {
        return home.join(rest).to_string_lossy().into_owned();
    }
    argument_prefix.to_owned()
}

fn get_directory_completion_parent_input(
    argument_prefix: &str,
    expanded_prefix: &str,
    home: &std::path::Path,
) -> String {
    if argument_prefix == "/" {
        return "/".to_owned();
    }
    if argument_prefix == "~/" {
        return home.to_string_lossy().into_owned();
    }
    if argument_prefix.ends_with('/') {
        return expanded_prefix[..expanded_prefix.len() - 1].to_owned();
    }
    posix_dirname(expanded_prefix).to_owned()
}

fn resolve_directory_completion_parent(
    parent_input: &str,
    home: &std::path::Path,
) -> std::path::PathBuf {
    if parent_input == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = parent_input.strip_prefix("~/") {
        return home.join(rest);
    }
    // pathe's `resolve` is lexical (cwd-relative, resolves `.`/`..` without
    // touching the filesystem) — `std::path::absolute` matches that.
    std::path::absolute(parent_input).unwrap_or_else(|_| std::path::PathBuf::from(parent_input))
}

fn is_directory_path(path: &std::path::Path, file_type: &std::fs::FileType) -> bool {
    if file_type.is_dir() {
        return true;
    }
    if !file_type.is_symlink() {
        return false;
    }
    // Follow the symlink to see whether it points at a directory.
    std::fs::metadata(path).is_ok_and(|m| m.is_dir())
}

fn format_directory_completion_value(
    argument_prefix: &str,
    parent_input: &str,
    entry_name: &str,
    home: &std::path::Path,
) -> String {
    if argument_prefix.starts_with("~/") {
        // `~` + path of `parent_input` relative to the home dir + `/{name}/`.
        let parent = std::path::Path::new(parent_input);
        let abs_parent = std::path::absolute(parent).unwrap_or_else(|_| parent.to_path_buf());
        let rel = relative_path(home, &abs_parent);
        let rel = rel.to_string_lossy();
        let mut value = String::from("~");
        if !rel.is_empty() {
            value.push('/');
            value.push_str(&rel);
        }
        value.push('/');
        value.push_str(entry_name);
        value.push('/');
        value
    } else {
        // TS has two identical branches here (`startsWith('/')` and the rest);
        // both produce `join(parentInput, entryName) + "/"`.
        let mut value = std::path::Path::new(parent_input)
            .join(entry_name)
            .to_string_lossy()
            .into_owned();
        value.push('/');
        value
    }
}

/// Lexical relative path from `from` to `to` (both absolute), matching pathe's
/// `relative` after both sides are resolved.
fn relative_path(from: &std::path::Path, to: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let from: Vec<Component> = from.components().collect();
    let to: Vec<Component> = to.components().collect();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(a, b)| a.as_os_str() == b.as_os_str())
        .count();
    let mut result = std::path::PathBuf::new();
    for _ in common..from.len() {
        result.push("..");
    }
    for component in &to[common..] {
        result.push(component.as_os_str());
    }
    result
}

/// POSIX `basename` (mirrors pathe): the last non-empty slash-separated
/// segment, or `""` (so `basename(".") == "."` and `basename("/") == ""`).
fn posix_basename(path: &str) -> &str {
    for segment in path.rsplit('/') {
        if !segment.is_empty() {
            return segment;
        }
    }
    ""
}

/// POSIX `dirname` (mirrors pathe): strip trailing slashes, drop the final
/// segment; `"/"` for root-ish input, `"."` for relative input without a
/// separator (so `dirname("..") == ".."` and `dirname(".") == "."`).
fn posix_dirname(path: &str) -> &str {
    if path.is_empty() {
        return ".";
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        // All slashes (e.g. `/` or `//`).
        return "/";
    }
    match trimmed.rfind('/') {
        Some(0) => "/",
        Some(i) => &trimmed[..i],
        None => ".",
    }
}

/// Dispatch entry point (mirrors `dispatchInput`): slash input goes through
/// resolution, everything else is a normal message.
pub fn dispatch_input(
    commands: &[SlashCommand],
    skill_command_map: &std::collections::HashMap<String, String>,
    plugin_command_map: &std::collections::HashSet<String>,
    is_streaming: bool,
    is_compacting: bool,
    text: &str,
) -> DispatchAction {
    let options = ResolveSlashCommandInput {
        input: text.to_owned(),
        skill_command_map: skill_command_map.clone(),
        plugin_command_map: plugin_command_map.clone(),
        is_streaming,
        is_compacting,
    };
    let intent = resolve_slash_command_input(&options, commands);
    match intent {
        SlashCommandIntent::NotCommand => DispatchAction::SendNormal(text.to_owned()),
        SlashCommandIntent::Message { input } => DispatchAction::SendNormal(input),
        SlashCommandIntent::Builtin { name, args } => DispatchAction::RunBuiltin { name, args },
        SlashCommandIntent::Skill {
            skill_name, args, ..
        } => DispatchAction::RunSkill { skill_name, args },
        SlashCommandIntent::PluginCommand {
            plugin_id,
            command_name,
            args,
        } => DispatchAction::RunPluginCommand {
            plugin_id,
            command_name,
            args,
        },
        SlashCommandIntent::Blocked {
            command_name,
            reason,
        } => DispatchAction::ShowError(slash_busy_message(&command_name, reason)),
        SlashCommandIntent::Invalid { command_name } => {
            DispatchAction::ShowError(format!("Invalid slash command: /{command_name}"))
        }
    }
}

/// The host action produced by dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchAction {
    SendNormal(String),
    RunBuiltin {
        name: String,
        args: String,
    },
    RunSkill {
        skill_name: String,
        args: String,
    },
    RunPluginCommand {
        plugin_id: String,
        command_name: String,
        args: String,
    },
    ShowError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commands() -> Vec<SlashCommand> {
        let mut c = SlashCommand::new("help", "Show help");
        c.aliases = vec!["?".to_owned()];
        vec![
            c,
            SlashCommand::new("clear", "Clear the conversation"),
            SlashCommand::new("compact", "Compress the conversation"),
        ]
    }

    fn opts(
        input: &str,
        skills: &[(&str, &str)],
        plugins: &[&str],
        streaming: bool,
        compacting: bool,
    ) -> ResolveSlashCommandInput {
        ResolveSlashCommandInput {
            input: input.to_owned(),
            skill_command_map: skills
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            plugin_command_map: plugins.iter().map(|s| s.to_string()).collect(),
            is_streaming: streaming,
            is_compacting: compacting,
        }
    }

    #[test]
    fn parse_basic() {
        let p = parse_slash_input("/help").unwrap();
        assert_eq!(p.name, "help");
        assert_eq!(p.args, "");
        let p = parse_slash_input("/clear --keep-todos").unwrap();
        assert_eq!(p.name, "clear");
        assert_eq!(p.args, "--keep-todos");
    }

    #[test]
    fn parse_rejects_paths_and_plain() {
        assert!(parse_slash_input("hello").is_none());
        assert!(parse_slash_input("/").is_none());
        // File path rejected.
        assert!(parse_slash_input("/usr/local/bin").is_none());
        // Namespaced plugin command allowed.
        let p = parse_slash_input("/plugin:frontend/component").unwrap();
        assert_eq!(p.name, "plugin:frontend/component");
    }

    #[test]
    fn resolve_builtin_with_alias() {
        let cmds = commands();
        let i = resolve_slash_command_input(&opts("/help", &[], &[], false, false), &cmds);
        assert!(matches!(i, SlashCommandIntent::Builtin { name, .. } if name == "help"));
        let i = resolve_slash_command_input(&opts("/?", &[], &[], false, false), &cmds);
        assert!(matches!(i, SlashCommandIntent::Builtin { name, .. } if name == "help"));
    }

    #[test]
    fn resolve_skill_and_plugin() {
        let cmds = commands();
        let i = resolve_slash_command_input(
            &opts("/review", &[("review", "review-skill")], &[], false, false),
            &cmds,
        );
        assert!(matches!(
            i,
            SlashCommandIntent::Skill { skill_name, .. } if skill_name == "review-skill"
        ));
        let i = resolve_slash_command_input(
            &opts("/mcp:deploy", &[], &["mcp:deploy"], false, false),
            &cmds,
        );
        assert!(matches!(
            i,
            SlashCommandIntent::PluginCommand { plugin_id, command_name, .. }
                if plugin_id == "mcp" && command_name == "deploy"
        ));
    }

    #[test]
    fn unknown_falls_through_to_message() {
        let cmds = commands();
        let i = resolve_slash_command_input(&opts("/nope", &[], &[], false, false), &cmds);
        assert!(matches!(i, SlashCommandIntent::Message { .. }));
    }

    #[test]
    fn idle_only_blocked_while_streaming() {
        let mut compact = SlashCommand::new("compact", "Compress");
        compact.availability = SlashCommandAvailability::IdleOnly;
        let cmds = vec![compact];
        let i = resolve_slash_command_input(&opts("/compact", &[], &[], true, false), &cmds);
        assert!(matches!(
            i,
            SlashCommandIntent::Blocked {
                reason: SlashCommandBusyReason::Streaming,
                ..
            }
        ));
        let i = resolve_slash_command_input(&opts("/compact", &[], &[], false, true), &cmds);
        assert!(matches!(
            i,
            SlashCommandIntent::Blocked {
                reason: SlashCommandBusyReason::Compacting,
                ..
            }
        ));
    }

    #[test]
    fn dispatch_routes() {
        let cmds = commands();
        let skills: std::collections::HashMap<String, String> =
            [("review".to_owned(), "review-skill".to_owned())].into();
        let plugins: std::collections::HashSet<String> = ["mcp:deploy".to_owned()].into();
        let a = dispatch_input(&cmds, &skills, &plugins, false, false, "/help");
        assert!(matches!(a, DispatchAction::RunBuiltin { name, .. } if name == "help"));
        let a = dispatch_input(&cmds, &skills, &plugins, false, false, "plain message");
        assert!(matches!(a, DispatchAction::SendNormal(t) if t == "plain message"));
        let a = dispatch_input(&cmds, &skills, &plugins, false, false, "/review");
        assert!(
            matches!(a, DispatchAction::RunSkill { skill_name, .. } if skill_name == "review-skill")
        );
        let a = dispatch_input(&cmds, &skills, &plugins, false, false, "/mcp:deploy");
        assert!(
            matches!(a, DispatchAction::RunPluginCommand { plugin_id, .. } if plugin_id == "mcp")
        );
    }

    #[test]
    fn complete_leading_arg_basic() {
        let specs = [
            ArgCompletionSpec {
                value: "on".into(),
                description: "Turn swarm mode on".into(),
            },
            ArgCompletionSpec {
                value: "off".into(),
                description: "Turn swarm mode off".into(),
            },
            ArgCompletionSpec {
                value: "status".into(),
                description: "Show status".into(),
            },
        ];
        // A space in the prefix means the user moved past the first token → no completion.
        assert!(complete_leading_arg(&specs, "on something").is_none());
        assert!(complete_leading_arg(&specs, "on ").is_none());
        // Case-insensitive prefix match.
        let items = complete_leading_arg(&specs, "O").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].value, "on");
        assert_eq!(items[0].label, "on");
        assert_eq!(items[0].description.as_deref(), Some("Turn swarm mode on"));
        let items = complete_leading_arg(&specs, "STAT").unwrap();
        assert_eq!(items[0].value, "status");
        // No match → None.
        assert!(complete_leading_arg(&specs, "zzz").is_none());
        // Empty prefix offers everything.
        let items = complete_leading_arg(&specs, "").unwrap();
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn complete_leading_arg_sole_match_suppressed() {
        let single = [ArgCompletionSpec {
            value: "status".into(),
            description: "Show status".into(),
        }];
        // Sole remaining match equal to the prefix is suppressed.
        assert!(complete_leading_arg(&single, "status").is_none());
        assert!(complete_leading_arg(&single, "STATUS").is_none());
        // A partial prefix still completes (this is what distinguishes a no-op
        // completion from a real one).
        assert_eq!(
            complete_leading_arg(&single, "sta").unwrap()[0].value,
            "status"
        );
        // With several specs, a full-token prefix that still matches others is kept.
        let specs = [
            ArgCompletionSpec {
                value: "status".into(),
                description: "Show status".into(),
            },
            ArgCompletionSpec {
                value: "stat".into(),
                description: "Stat something".into(),
            },
        ];
        assert_eq!(complete_leading_arg(&specs, "stat").unwrap().len(), 2);
        assert!(complete_leading_arg(&specs, "status").is_none());
    }

    #[test]
    fn resolve_availability_static_and_default() {
        let help = find_builtin_slash_command("help").unwrap();
        assert_eq!(
            resolve_availability(help, ""),
            SlashCommandAvailability::Always
        );
        let undo = find_builtin_slash_command("undo").unwrap();
        assert_eq!(
            resolve_availability(undo, ""),
            SlashCommandAvailability::IdleOnly
        );
        // Commands whose TS entry omits availability default to idle-only.
        for name in ["new", "sessions", "compact", "init", "fork"] {
            let command = find_builtin_slash_command(name).unwrap();
            assert_eq!(
                resolve_availability(command, "anything"),
                SlashCommandAvailability::IdleOnly,
                "/{name} should default to idle-only"
            );
        }
    }

    #[test]
    fn resolve_availability_fn_based() {
        let plan = find_builtin_slash_command("plan").unwrap();
        assert_eq!(
            resolve_availability(plan, ""),
            SlashCommandAvailability::Always
        );
        assert_eq!(
            resolve_availability(plan, "   "),
            SlashCommandAvailability::Always
        );
        assert_eq!(
            resolve_availability(plan, "clear"),
            SlashCommandAvailability::IdleOnly
        );
        assert_eq!(
            resolve_availability(plan, "  CLEAR  "),
            SlashCommandAvailability::IdleOnly
        );
        assert_eq!(
            resolve_availability(plan, "clear the deck"),
            SlashCommandAvailability::Always
        );

        let swarm = find_builtin_slash_command("swarm").unwrap();
        assert_eq!(
            resolve_availability(swarm, ""),
            SlashCommandAvailability::Always
        );
        assert_eq!(
            resolve_availability(swarm, "on"),
            SlashCommandAvailability::Always
        );
        assert_eq!(
            resolve_availability(swarm, "  off "),
            SlashCommandAvailability::Always
        );
        assert_eq!(
            resolve_availability(swarm, "ON"),
            SlashCommandAvailability::Always
        );
        assert_eq!(
            resolve_availability(swarm, "summarize the repo"),
            SlashCommandAvailability::IdleOnly
        );
    }

    #[test]
    fn registry_integrity() {
        let commands = builtin_slash_commands();
        assert!(commands.len() >= 39, "expected the full builtin registry");
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for c in commands {
            assert!(
                seen.insert(c.name.clone()),
                "duplicate command name: /{}",
                c.name
            );
            for alias in &c.aliases {
                assert!(
                    !seen.contains(alias),
                    "alias /{alias} collides with another command's name or alias"
                );
                seen.insert(alias.clone());
            }
            assert!(
                !c.description.is_empty(),
                "empty description for /{}",
                c.name
            );
        }
    }

    #[test]
    fn find_builtin_by_alias() {
        let cases = [
            ("?", "help"),
            ("h", "help"),
            ("resume", "sessions"),
            ("thinking", "effort"),
            ("config", "settings"),
            ("clear", "new"),
            ("export", "export-md"),
            ("q", "exit"),
        ];
        for (alias, name) in cases {
            let found = find_builtin_slash_command(alias).expect(alias);
            assert_eq!(found.name, name, "alias {alias} should resolve to /{name}");
        }
        assert!(find_builtin_slash_command("nonexistent").is_none());
    }

    #[test]
    fn sort_slash_commands_order() {
        let commands = vec![
            SlashCommand {
                name: "zeta".into(),
                priority: 100,
                ..SlashCommand::new("", "")
            },
            SlashCommand {
                name: "alpha".into(),
                priority: 100,
                ..SlashCommand::new("", "")
            },
            SlashCommand {
                name: "mid".into(),
                priority: 50,
                ..SlashCommand::new("", "")
            },
            SlashCommand {
                name: "low".into(),
                priority: 0,
                ..SlashCommand::new("", "")
            },
        ];
        let sorted = sort_slash_commands(commands);
        let names: Vec<&str> = sorted.iter().map(|c| c.name.as_str()).collect();
        // Priority descending; ties broken by ascending name.
        assert_eq!(names, vec!["alpha", "zeta", "mid", "low"]);
    }

    #[test]
    fn resolve_builtin_input_variants() {
        let i = resolve_builtin_slash_input(&opts("/help", &[], &[], false, false));
        assert!(matches!(i, SlashCommandIntent::Builtin { name, .. } if name == "help"));
        // Experimental-flag stub: gated command still resolves as builtin.
        let i = resolve_builtin_slash_input(&opts("/secondary_model", &[], &[], false, false));
        assert!(matches!(i, SlashCommandIntent::Builtin { name, .. } if name == "secondary_model"));
        // Skill.
        let i = resolve_builtin_slash_input(&opts(
            "/review",
            &[("review", "review-skill")],
            &[],
            false,
            false,
        ));
        assert!(
            matches!(i, SlashCommandIntent::Skill { skill_name, .. } if skill_name == "review-skill")
        );
        // Plugin command (builtin `mcp` does not shadow `mcp:deploy`).
        let i =
            resolve_builtin_slash_input(&opts("/mcp:deploy", &[], &["mcp:deploy"], false, false));
        assert!(
            matches!(i, SlashCommandIntent::PluginCommand { plugin_id, command_name, .. } if plugin_id == "mcp" && command_name == "deploy")
        );
        // Unknown → message.
        let i = resolve_builtin_slash_input(&opts("/nope", &[], &[], false, false));
        assert!(matches!(i, SlashCommandIntent::Message { .. }));
        // Non-slash → not a command.
        let i = resolve_builtin_slash_input(&opts("hello", &[], &[], false, false));
        assert!(matches!(i, SlashCommandIntent::NotCommand));
    }

    #[test]
    fn resolve_builtin_blocked_while_streaming() {
        // Idle-only /undo blocked while streaming.
        let i = resolve_builtin_slash_input(&opts("/undo", &[], &[], true, false));
        assert!(matches!(
            i,
            SlashCommandIntent::Blocked {
                reason: SlashCommandBusyReason::Streaming,
                ..
            }
        ));
        // Idle-only /new blocked while compacting.
        let i = resolve_builtin_slash_input(&opts("/new", &[], &[], false, true));
        assert!(matches!(
            i,
            SlashCommandIntent::Blocked {
                reason: SlashCommandBusyReason::Compacting,
                ..
            }
        ));
        // Always commands are not blocked while streaming.
        let i = resolve_builtin_slash_input(&opts("/help", &[], &[], true, false));
        assert!(matches!(i, SlashCommandIntent::Builtin { name, .. } if name == "help"));
        // /swarm <task> is idle-only; /swarm on is always.
        let i = resolve_builtin_slash_input(&opts("/swarm summarize", &[], &[], true, false));
        assert!(matches!(i, SlashCommandIntent::Blocked { .. }));
        let i = resolve_builtin_slash_input(&opts("/swarm on", &[], &[], true, false));
        assert!(matches!(i, SlashCommandIntent::Builtin { name, .. } if name == "swarm"));
        // /plan clear is idle-only; /plan is always.
        let i = resolve_builtin_slash_input(&opts("/plan clear", &[], &[], true, false));
        assert!(matches!(i, SlashCommandIntent::Blocked { .. }));
        let i = resolve_builtin_slash_input(&opts("/plan", &[], &[], true, false));
        assert!(matches!(i, SlashCommandIntent::Builtin { name, .. } if name == "plan"));
    }

    #[test]
    fn swarm_argument_completions_basic() {
        assert_eq!(swarm_argument_completions("o").unwrap()[0].value, "on");
        assert!(swarm_argument_completions("on").is_none());
        assert!(swarm_argument_completions("zzz").is_none());
    }

    #[test]
    fn add_dir_argument_completions_dirs_only() {
        let base =
            std::env::temp_dir().join(format!("dimi_tui_add_dir_test_{}", std::process::id()));
        let home = base.join("home");
        std::fs::remove_dir_all(&base).ok();
        std::fs::create_dir_all(home.join("proj_a")).unwrap();
        std::fs::create_dir_all(home.join("proj_b")).unwrap();
        std::fs::create_dir_all(home.join(".hidden_dir")).unwrap();
        std::fs::write(home.join("notes.txt"), "x").unwrap();
        std::fs::write(home.join("proj_c.txt"), "x").unwrap();

        // `~/` lists directories only: files and hidden entries skipped, values
        // and labels carry a trailing slash, descriptions point at real dirs.
        let items = complete_add_dir_path("~/", &home).unwrap();
        let mut values: Vec<String> = items.iter().map(|i| i.value.clone()).collect();
        values.sort();
        assert_eq!(
            values,
            vec!["~/proj_a/".to_string(), "~/proj_b/".to_string()]
        );
        for item in &items {
            assert!(
                item.value.ends_with('/'),
                "value should end with '/': {}",
                item.value
            );
            assert!(
                item.label.ends_with('/'),
                "label should end with '/': {}",
                item.label
            );
            let desc = item.description.as_deref().expect("description present");
            assert!(
                std::path::Path::new(desc).is_dir(),
                "description should be a dir: {desc}"
            );
        }

        // Prefix filtering is case-insensitive and (unlike completeLeadingArg)
        // does not suppress a sole remaining match.
        let items = complete_add_dir_path("~/PROJ", &home).unwrap();
        let values: Vec<String> = items.iter().map(|i| i.value.clone()).collect();
        assert!(values.contains(&"~/proj_a/".to_string()));
        assert!(values.contains(&"~/proj_b/".to_string()));

        // A non-matching prefix yields None.
        assert!(complete_add_dir_path("~/zzz", &home).is_none());

        // Nested path completion: `~/proj_a/` lists inside the subdirectory.
        std::fs::create_dir_all(home.join("proj_a/inner")).unwrap();
        std::fs::write(home.join("proj_a/file.txt"), "x").unwrap();
        let items = complete_add_dir_path("~/proj_a/", &home).unwrap();
        let values: Vec<String> = items.iter().map(|i| i.value.clone()).collect();
        assert_eq!(values, vec!["~/proj_a/inner/".to_string()]);

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn add_dir_argument_completions_absolute_and_dots() {
        let base =
            std::env::temp_dir().join(format!("dimi_tui_add_dir_abs_test_{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        std::fs::create_dir_all(base.join("sub")).unwrap();
        std::fs::write(base.join("file.txt"), "x").unwrap();

        // Absolute prefix: value = resolved absolute path + trailing slash.
        let prefix = format!("{}/", base.to_string_lossy());
        let items =
            complete_add_dir_path(&prefix, std::path::Path::new("/nonexistent_home")).unwrap();
        assert_eq!(items[0].value, format!("{}/sub/", base.to_string_lossy()));

        // A bare `.` is path-like but completes nothing (non-hidden entries can
        // never match the `.` partial after hidden files are skipped).
        assert!(complete_add_dir_path(".", std::path::Path::new("/nonexistent_home")).is_none());

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn add_dir_argument_completions_falls_back_to_leading_arg() {
        // Non-path-like prefixes complete the `list` subcommand.
        let items = add_dir_argument_completions("l").unwrap();
        assert_eq!(items[0].value, "list");
        // Sole remaining match is suppressed, like completeLeadingArg.
        assert!(add_dir_argument_completions("list").is_none());
        assert!(add_dir_argument_completions("li x").is_none());
    }
}
