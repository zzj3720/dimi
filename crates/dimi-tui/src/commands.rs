//! Slash-command system — parsing, resolution, and dispatch
//! (port of `apps/dimi/src/tui/commands/{parse,resolve,dispatch}.ts`,
//! slice 6 scope: parse + resolve + the dispatch routing skeleton; the
//! builtin command bodies and skill/plugin activation land with the
//! app-shell integration).

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

/// A slash command declaration.
#[derive(Debug, Clone)]
pub struct SlashCommand {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub availability: SlashCommandAvailability,
}

impl SlashCommand {
    pub fn new(name: &str, description: &str) -> Self {
        SlashCommand {
            name: name.to_owned(),
            aliases: Vec::new(),
            description: description.to_owned(),
            availability: SlashCommandAvailability::Always,
        }
    }
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

/// Resolve a slash input to an intent (mirrors `resolveSlashCommandInput`).
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
        if let Some(reason) = busy_reason {
            if command.availability == SlashCommandAvailability::IdleOnly {
                return SlashCommandIntent::Blocked {
                    command_name: parsed.name,
                    reason,
                };
            }
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
}
