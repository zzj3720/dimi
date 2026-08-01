# Getting started

## What is Dimi

Dimi is an AI agent that runs in the terminal, helping you carry out software development tasks and day-to-day terminal operations — reading and modifying code, running shell commands, searching files, fetching web pages, and autonomously planning and adjusting its next steps based on feedback as it works.

It fits scenarios such as:

- **Writing and modifying code**: implementing new features, fixing bugs, completing refactors
- **Understanding a project**: exploring an unfamiliar codebase and answering questions about architecture and implementation
- **Automating tasks**: batch-processing files, running builds and tests, chaining multiple scripts together

The CLI is written in TypeScript and runs on Node.js. This repository is a source build and does not yet have an independent package registry or release channel.

## Installation

Clone this repository and run the development CLI. Do not use an old install script or `@moonshot-ai/kimi-code@latest`: those point to a different product release.

::: tip Before you install
Dimi is a fully interactive TUI application. For the best visual experience, run it in a terminal with true-color and ligature support, such as [Kitty](https://sw.kovidgoyal.net/kitty/) or [Ghostty](https://ghostty.org/).
:::

Requires Node.js 24.15.0 or later and pnpm 10.33.0:

```sh
git clone https://github.com/zzj3720/dimi.git
cd dimi
vp install
vp run dev:cli
```

## Upgrade and uninstall

**Upgrade**: update the source checkout, then reinstall dependencies if they changed:

```sh
git pull --ff-only
vp install
vp run dev:cli
```

`kimi upgrade` / `kimi update` deliberately reports that automatic upgrades are not configured for this build. It never installs an older Dimi release.

**Uninstall**: remove the cloned checkout. Your local data under `~/.dimi/` is separate; remove it only if you also intend to delete sessions and credentials.

## First launch

From the cloned checkout, start the source CLI:

```sh
cd dimi
vp run dev:cli
```

To run a single instruction without entering the interactive UI, use `-p`:

```sh
vp run dev:cli -- -p "Take a look at this project's directory structure"
```

To resume the previous session, add `-c`:

```sh
vp run dev:cli -- -c
```

On first launch, connect a provider. In the interactive UI, enter `/login`:

```
/login
```

`/login` first asks for a supported authentication method, then lists the matching built-in providers. The generated catalog includes Kimi Code, OpenAI Codex, xAI/Grok, OpenAI, Anthropic, Gemini, cloud providers, and other API services:

- **OAuth** — available for Kimi Code, OpenAI Codex, xAI, Anthropic, GitHub Copilot, OpenRouter, and Radius when their account flow is selected
- **API key** — enter and securely save the selected provider's key
- **Cloud identity** — Amazon Bedrock can use an AWS credential chain; Vertex can use Google Cloud ADC or a service account

After login, choose one of that provider's currently available models. To skip the two selectors when you already know the provider, run `/login <provider>`, for example `/login openai`.

To sign out, enter `/logout` to clear the current credentials.

You can also connect from the checkout with `vp run dev:cli -- login <provider>`, or use a provider's standard API-key environment variable. To connect a compatible endpoint that is not built in, add it to `models.json` with an explicit context window and output limit. See [Providers and models](../configuration/providers.md) for the full catalog, custom-provider, and model-selection flow.

## Your first conversation

Once logged in, describe a task in natural language. A good starting point is to let Dimi familiarize itself with the project:

```
Take a look at this project's directory structure and briefly describe what each directory is for.
```

Dimi automatically calls file-reading, search, and other tools to browse the relevant content before responding. Read-only operations are executed automatically by default without requiring confirmation. For operations that modify files or run shell commands, it asks for your confirmation before proceeding.

You can also describe a more concrete task directly:

```
Add a function in src/utils that converts any string to kebab-case, and add a unit test for it.
```

Dimi plans the steps, modifies the code, runs the tests, and tells you what it did at each step.

::: tip Not sure what to do? Type `/help`
Type `/help` at any time to open the built-in command and keyboard shortcut panel. Use `↑`/`↓` to browse and `Esc` to close. To exit, type `/exit`, press `Ctrl-C` twice, or press `Ctrl-D` with the input box empty.
:::

## Common commands and keyboard shortcuts

For a first-time user, the following is all you need to know:

**Session commands**

| Command     | Description                                                            |
| ----------- | ---------------------------------------------------------------------- |
| `/new`      | Start a new session, clearing the current context                      |
| `/sessions` | Browse session history and choose one to resume                        |
| `/model`    | Switch the current model                                               |
| `/compact`  | Manually compress the context to free up tokens                        |
| `/fork`     | Fork the current session, keeping history but continuing independently |

**Most-used keyboard shortcuts**

| Shortcut    | Description                                                                    |
| ----------- | ------------------------------------------------------------------------------ |
| `Esc`       | Interrupt streaming output / close a popup                                     |
| `Ctrl-C`    | Interrupt output; press twice while idle to exit                               |
| `Shift-Tab` | Toggle Plan mode                                                               |
| `Ctrl-S`    | Inject a message mid-stream without waiting for the current response to finish |
| `Ctrl-O`    | Collapse / expand tool output and compaction summaries                         |

For the full list, type `/help` or visit [Slash commands reference](../reference/slash-commands.md) and [Keyboard shortcuts](../reference/keyboard.md).

## Where data is stored

Dimi stores its local data under `~/.dimi/` by default — config files, session records, and logs. This source build has no active update channel. To move data elsewhere, point to a new path via the `DIMI_CODE_HOME` environment variable. For the full directory layout, see [Data locations](../configuration/data-locations.md) and [Environment variables](../configuration/env-vars.md).

## Next steps

- [Interaction and input](./interaction.md) — input box operations, approval flow, Plan mode, and YOLO mode explained
- [Sessions and context](./sessions.md) — resuming sessions, compressing context, exporting sessions
- [Common use cases](./use-cases.md) — prompt examples for typical tasks
