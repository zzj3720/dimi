# 开始使用

## Dimi 是什么

Dimi 是一个运行在终端中的 AI Agent，帮助你完成软件开发任务和日常的终端操作——阅读和修改代码、执行 Shell 命令、搜索文件、抓取网页，并在执行过程中根据反馈自主规划和调整下一步行动。

它适用于以下场景：

- **编写和修改代码**：实现新功能、修复 bug、完成重构
- **理解项目**：探索陌生的代码库，解答架构和实现层面的问题
- **自动化任务**：批量处理文件、运行构建与测试、串联多个脚本

整套 CLI 以 TypeScript 编写，运行在 Node.js 之上。本仓库是源码构建，目前没有独立的软件包仓库或发布通道。

## 安装

请克隆本仓库并运行开发版 CLI。不要使用旧安装脚本或 `@moonshot-ai/kimi-code@latest`，它们指向的是另一份产品发布。

::: tip 安装之前
Dimi 为全交互式 TUI 应用，推荐在支持真彩色与连字的现代终端中运行以获得最佳体验，例如 [Kitty](https://sw.kovidgoyal.net/kitty/) 或 [Ghostty](https://ghostty.org/)。
:::

需要 Node.js 24.15.0 或更高版本以及 pnpm 10.33.0：

```sh
git clone https://github.com/zzj3720/dimi.git
cd dimi
vp install
vp run dev:cli
```

## 升级与卸载

**升级**：更新源码 checkout；依赖有变化时重新安装：

```sh
git pull --ff-only
vp install
vp run dev:cli
```

`kimi upgrade` / `kimi update` 会明确提示本构建未配置自动升级，绝不会安装旧的发布版。

**卸载**：删除 clone 出来的目录即可。`~/.kimi-code/` 下的本地数据独立保存；只有同时希望删除会话和凭据时才删除它。

## 第一次启动

在 clone 得到的源码 checkout 中启动交互界面：

```sh
cd dimi
vp run dev:cli
```

只想执行一条指令而不进入交互界面时，使用 `-p`：

```sh
vp run dev:cli -- -p "帮我看一下这个项目的目录结构"
```

继续上一次会话加 `-c`：

```sh
vp run dev:cli -- -c
```

首次启动时需要连接一个供应商。在交互界面中输入 `/login`：

```
/login
```

`/login` 会先询问支持的认证方式，再列出匹配的内置供应商。生成的目录包括 Kimi Code、OpenAI Codex、xAI/Grok、OpenAI、Anthropic、Gemini、云供应商和其他 API 服务：

- **OAuth** — 选择账号流程时，Kimi Code、OpenAI Codex、xAI、Anthropic、GitHub Copilot、OpenRouter 和 Radius 支持
- **API 密钥** — 输入并安全保存所选供应商的密钥
- **云身份** — Amazon Bedrock 可使用 AWS 凭据链；Vertex 可使用 Google Cloud ADC 或 service account

登录后，从该供应商当前可用的模型中选择一个。如果已经知道供应商，可以运行 `/login <provider>` 跳过前两个选择器，例如 `/login openai`。

需要退出登录时，输入 `/logout` 清除当前凭证。

也可以在 checkout 中运行 `vp run dev:cli -- login <provider>`，或使用供应商的标准 API 密钥环境变量。未内置的兼容端点可在 `models.json` 中添加，但必须显式写出上下文窗口和输出限制。完整目录、自定义供应商和模型选择流程见[供应商与模型](../configuration/providers.md)。

## 第一个对话

登录完成后，用自然语言描述任务即可。先让它熟悉当前项目：

```
帮我看一下这个项目的目录结构，简单介绍一下每个目录是做什么的
```

Dimi 会自动调用文件读取、搜索等工具浏览相关内容后给出回答。只读操作默认自动执行无需确认；对于会修改文件或执行 Shell 命令的操作，默认会在执行前征求确认。

也可以直接描述更具体的任务：

```
在 src/utils 里新增一个函数，用来把任意字符串转成 kebab-case，并补一个单元测试
```

Dimi 会规划步骤、修改代码、运行测试，并在每一步告诉你它做了什么。

::: tip 不知道能做什么？输入 `/help`
随时在输入框输入 `/help`，可以打开内置的命令和快捷键面板，按 `↑`/`↓` 翻看，`Esc` 关闭。退出时输入 `/exit`，或按 `Ctrl-C` 两次，或在输入框为空时按 `Ctrl-D`。
:::

## 常用命令与快捷键速查

第一次使用时，记住下面这些就够了：

**会话相关命令**

| 命令        | 说明                           |
| ----------- | ------------------------------ |
| `/new`      | 开启新会话，清空当前上下文     |
| `/sessions` | 浏览历史会话，选择恢复         |
| `/model`    | 切换当前使用的模型             |
| `/compact`  | 手动压缩上下文，释放 token     |
| `/fork`     | 派生当前会话，保留历史独立继续 |

**最常用快捷键**

| 快捷键      | 说明                           |
| ----------- | ------------------------------ |
| `Esc`       | 中断流式输出 / 关闭弹窗        |
| `Ctrl-C`    | 中断输出；空闲时连按两次退出   |
| `Shift-Tab` | 切换 Plan 模式                 |
| `Ctrl-S`    | 输出中途插入消息，无需等待结束 |
| `Ctrl-O`    | 折叠 / 展开工具输出和压缩摘要  |

想看完整列表，输入 `/help` 或访问[斜杠命令参考](../reference/slash-commands.md)和[键盘快捷键](../reference/keyboard.md)。

## 数据存放在哪里

Dimi 的本地数据默认保存在 `~/.kimi-code/` 下，包含配置文件、会话记录和日志。本源码构建没有启用更新通道。如需迁移到别处，通过 `KIMI_CODE_HOME` 环境变量指定新路径。完整说明见[数据路径](../configuration/data-locations.md)和[环境变量](../configuration/env-vars.md)。

## 下一步

- [交互与输入](./interaction.md) — 输入框操作、审批流程、Plan 模式和 YOLO 模式详解
- [会话与上下文](./sessions.md) — 恢复会话、上下文压缩、导出会话
- [常见使用案例](./use-cases.md) — 典型任务的 prompt 示例
