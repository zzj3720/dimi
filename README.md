# k-3720

[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE) · [中文](README.zh-CN.md)

k-3720 is an independent, multi-model coding agent runtime and client project. It is forked from [Kimi Code](https://github.com/MoonshotAI/kimi-code) and retains substantial portions of that codebase. Its terminal interface and related interaction code also make extensive use of [Pi](https://github.com/earendil-works/pi), especially `pi-tui`.

This project is not an official Moonshot AI, Kimi, Pi, or Earendil project and is not endorsed by or affiliated with those projects or their maintainers.

## Status

k-3720 is under active development. It currently provides:

- a terminal UI for local agent sessions;
- asynchronous tool execution and wait/resume behavior;
- a native Android client;
- an end-to-end encrypted bridge between a local runtime and mobile clients; and
- an opaque WebSocket relay that forwards encrypted traffic without reading prompts or responses.

The runtime is designed to work with multiple model providers. Provider availability and authentication depend on local configuration.

## Development

Requirements: Node.js 24.15.0 or newer and pnpm 10.33.0. This repository also supports the `vp` command wrapper.

```sh
git clone https://github.com/zzj3720/k-3720.git
cd k-3720
vp install
vp run dev:cli
```

Common repository tasks:

```sh
vp run typecheck
vp lint
vp test
vp build
```

The Android client and relay setup are documented in [apps/mobile/README.md](apps/mobile/README.md).

## Upstream documentation

Much of the original CLI behavior and configuration comes from Kimi Code. Its [upstream documentation](https://moonshotai.github.io/kimi-code/en/) remains useful for inherited features, but names, commands, defaults, and supported integrations may differ in k-3720.

## Acknowledgements

- [Kimi Code](https://github.com/MoonshotAI/kimi-code), by Moonshot AI, is the upstream project from which k-3720 was forked. This repository retains and modifies substantial portions of its agent runtime, CLI, server, protocol, and supporting packages.
- [Pi](https://github.com/earendil-works/pi), by Mario Zechner and its contributors, is the source of substantial TUI and terminal interaction code used by this project. The vendored `packages/pi-tui` package originated from `@earendil-works/pi-tui`.
- The project also depends on many other open-source packages. Their licenses remain applicable to their respective components and distributions.

These acknowledgements describe source lineage only; they do not imply sponsorship, endorsement, or affiliation.

## License

k-3720 is distributed under the [MIT License](LICENSE). The original Moonshot AI notice is retained, and the k-3720 notice applies to subsequent modifications.

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for the retained Kimi Code and Pi license notices.
