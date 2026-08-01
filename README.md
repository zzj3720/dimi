# Dimi

[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE) · [Documentation](docs/en/guides/getting-started.md) · [Issues](https://github.com/zzj3720/dimi/issues) · [中文](README.zh-CN.md)

![Demo of using Dimi](./docs/media/intro.gif)

## What is Dimi

Dimi is an AI coding agent that runs in your terminal — it can read and edit code, run shell commands, search files, fetch web pages, and choose the next step based on the feedback it receives. It has one provider runtime for Dimi, Codex, Grok, Claude, Gemini, cloud services, and compatible custom endpoints.

## Install

This is a source build with no separate release channel. Clone it instead of using an old installer or npm `latest` package:

```sh
git clone https://github.com/zzj3720/dimi.git
cd dimi
vp install
vp run dev:cli
```

To update, run `git pull --ff-only && vp install`. `dimi upgrade` checks the Dimi GitHub Releases for newer versions and shows the update command. See [Getting Started](docs/en/guides/getting-started.md).

## Quick Start

From the cloned checkout, start the interactive UI:

```sh
cd dimi
vp run dev:cli
```

On first launch, run `/login` inside Dimi and choose a provider and its supported OAuth, API-key, or cloud-identity method. You can add or overlay compatible endpoints in `~/.dimi/models.json`; see [Providers and models](docs/en/configuration/providers.md). After login, try your first task:

```
Take a look at this project and explain its main directories.
```

## Key Features

- **Source-first development.** Run the current checkout with `vp run dev:cli`; provider and model behavior is verified from the same code you edit.
- **Blazing-fast startup.** The TUI is ready in milliseconds, so starting a session never feels heavy.
- **Purpose-built TUI.** A carefully tuned interface, optimized end to end for long, focused agent sessions.
- **Video input.** Drop a screen recording or demo clip into the chat and let the agent watch what is hard to describe in words — turn a reference clip into a LUT, a long video into a short, a screen recording into working code, and more.
- **AI-native MCP configuration.** Add, edit, and authenticate Model Context Protocol servers conversationally with `/mcp-config`, without hand-editing JSON.
- **Rich plugin ecosystem.** Install skills, MCP servers, and data sources from the marketplace or any GitHub repo, with each install's trust level surfaced up front.
- **Subagents for focused, parallel work.** Dispatch built-in `coder`, `explore`, and `plan` subagents in isolated contexts while keeping the main conversation clean.
- **Lifecycle hooks.** Run local commands at key points to gate risky tool calls, audit decisions, trigger desktop notifications, or connect to your own automation.

## Docs

- [Getting Started](docs/en/guides/getting-started.md)
- [Interaction and approvals](docs/en/guides/interaction.md)
- [Sessions](docs/en/guides/sessions.md)
- [Using in IDEs (ACP)](docs/en/guides/ides.md)
- [Configuration](docs/en/configuration/config-files.md)
- [Command reference](docs/en/reference/dimi-command.md)

## Develop

Requirements: Node.js ≥ 24.15.0, pnpm 10.33.0.

```sh
git clone https://github.com/zzj3720/dimi.git
cd dimi
vp install
```

```sh
vp run dev:cli  # run the CLI in dev mode
vp test         # run tests
vp run typecheck # TypeScript check
vp run lint     # oxlint
vp run build    # build all packages
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full contribution guide.

## Community

- [Issues](https://github.com/zzj3720/dimi/issues)
- For security vulnerabilities, see [SECURITY.md](SECURITY.md).

## Acknowledgements

Our TUI is built on top of [`pi-tui`](https://github.com/earendil-works/pi-mono/tree/main/packages/tui). We thank the authors of `pi-tui` for their valuable work.

## License

Released under the [MIT License](LICENSE).
