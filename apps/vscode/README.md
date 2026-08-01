# Dimi

AI coding assistant for VS Code, built for long-context workflows and complex coding tasks.

## Features

- **Works alongside you**: Dimi autonomously explores your codebase, reads and writes code, and runs terminal commands with your permission
- **Thinking controls**: Toggle reasoning or choose a model-supported thinking effort
- **Provider-aware models**: Distinguish and select same-named models across configured providers
- **Native editor integration**: Review AI-proposed changes directly in VS Code's diff viewer
- **MCP support**: Extend capabilities with Model Context Protocol servers
- **Slash commands**: Quick actions like `/init` to analyze your project and `/compact` to manage context

## Install

Dimi requires VS Code 1.100.0 or later.

This source build is not published to the VS Code Marketplace. Run the extension from this repository's VS Code development workflow, then open a folder and click the Dimi icon in the Activity Bar. It can use a provider already configured in the shared Dimi home (`auth.json` and optional `models.json`).

The extension runs the Dimi Node SDK in the VS Code Extension Host. When
the extension and the Dimi terminal app resolve to the same
`DIMI_CODE_HOME`, they share `config.toml`, `models.json`, MCP configuration, login state, and
sessions. The system-level `DIMI_CODE_HOME` environment variable is supported;
there is no separate VS Code setting for it. Do not run the same session from
both applications at the same time, because cross-process session locking is
not guaranteed.

After upgrading from version 0.5.x, the extension prompts before migrating any
legacy data it finds. Migration copies or merges data into the current Dimi
home and does not delete the legacy source. Legacy Dimi OAuth and MCP OAuth
credentials are not copied, so those connections must be authorized again.
See [the changelog](CHANGELOG.md) for the full compatibility notes.

## Docs

Source and project documentation: [zzj3720/dimi](https://github.com/zzj3720/dimi/tree/main/docs)

## License

[Apache-2.0](LICENSE)
