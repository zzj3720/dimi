# @moonshot-ai/kimi-code

> The Starting Point for Next-Gen Agents

[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE) · [Repository](https://github.com/zzj3720/k-3720)

## What is Kimi Code CLI

Kimi Code CLI is an AI coding agent that runs in your terminal. It can read and edit code, run shell commands, search files, fetch web pages, and choose the next step based on the feedback it receives. It works out of the box with Moonshot AI's Kimi models and can connect to other supported providers.

## Install

This is a source build with no independent package or release channel. Clone the repository instead of using an old install script or npm `latest` package:

```sh
git clone https://github.com/zzj3720/k-3720.git
cd k-3720
vp install
vp run dev:cli
```

For updates, use `git pull --ff-only && vp install`; `kimi upgrade` deliberately reports that automatic upgrades are unavailable for this build.

## Quick Start

From the cloned checkout, start the interactive UI:

```sh
cd k-3720
vp run dev:cli
```

On first launch, run `/login` inside Kimi Code CLI and choose a supported authentication method for the provider: OAuth, an API key, or a cloud identity where available. After login, try a first task:

```
Take a look at this project and explain the main directories.
```

## Key Features

- **Source-first development.** Run the current checkout with `vp run dev:cli`; provider and model behavior is verified from the same code you edit.
- **Blazing-fast startup.** The TUI is ready in milliseconds, so opening a session never feels heavy.
- **Polished TUI.** A carefully tuned interface designed for long, focused agent sessions.
- **Video input.** Drop a screen recording or demo clip into the chat — let the agent watch instead of typing out what's hard to describe in words.
- **AI-native MCP configuration.** Add, edit, and authenticate Model Context Protocol servers conversationally via `/mcp-config` — no hand-editing JSON.
- **Subagents for focused, parallel work.** Dispatch built-in `coder`, `explore`, and `plan` subagents in isolated context windows; the main conversation stays clean.
- **Lifecycle hooks.** Run local commands at key points — gate risky tool calls, audit decisions, fire desktop notifications, wire into your own automation.

## Documentation

- Full docs: ../../docs/en/
- 中文文档: ../../docs/zh/
- Getting Started: ../../docs/en/guides/getting-started.md

## Repository & Issues

- Source: https://github.com/zzj3720/k-3720
- Issues: https://github.com/zzj3720/k-3720/issues
- Security: see SECURITY.md in the main repository

## License

MIT
