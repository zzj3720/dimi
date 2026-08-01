# Dimi

[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE) · [文档](docs/zh/guides/getting-started.md) · [Issues](https://github.com/zzj3720/dimi/issues) · [English](README.md)


![Dimi 的使用演示](./docs/media/intro.gif)


## 什么是 Dimi

Dimi 是一个运行在终端里的 AI 编程 agent，可以帮你读写代码、执行 shell 命令、检索文件、抓取网页，并根据反馈自主决定下一步动作。它使用同一套供应商运行时连接 Dimi、Codex、Grok、Claude、Gemini、云服务和兼容的自定义端点。

## 安装

这是没有独立发布通道的源码构建。请 clone 本仓库，不要使用旧安装脚本或 npm 的 `latest` 包：

```sh
git clone https://github.com/zzj3720/dimi.git
cd dimi
vp install
vp run dev:cli
```

更新时运行 `git pull --ff-only && vp install`。`dimi upgrade` 会检查 Dimi 的 GitHub Releases 是否有新版本并显示更新命令。详见[快速上手](docs/zh/guides/getting-started.md)。

## 快速开始

在 clone 得到的 checkout 中启动交互界面：

```sh
cd dimi
vp run dev:cli
```

首次启动时，在 Dimi 里输入 `/login`，选择供应商及其支持的 OAuth、API 密钥或云身份登录方式。兼容端点可以在 `~/.dimi/models.json` 中添加或覆盖；详见[供应商与模型](docs/zh/configuration/providers.md)。登录完成后，可以先让它熟悉项目：

```
帮我看一下这个项目的目录结构，简单介绍一下每个目录是做什么的
```

## 核心特性

- **二进制发行，零环境依赖** 一行命令安装，不需要预装 Node.js，不用折腾 PATH，也不会和全局模块冲突。
- **极速启动** TUI 在毫秒级就绪，开一个新会话没有任何心智负担。
- **精致的 TUI 体验** 端到端打磨的交互界面，专为长时间、专注的 Agent 会话优化。
- **视频也能输入** 把屏幕录像、演示视频拖进对话，让 Agent 看那些难以用文字描述的东西——把参考片段做成 LUT、把长视频剪成短视频、把录屏变成代码，等等。
- **AI-native 的 MCP 配置** 通过 `/mcp-config` 对话式添加、编辑、认证 MCP 服务器，无需手写 JSON。
- **丰富的插件生态** 从插件市场或任意 GitHub 仓库安装 skills、MCP 服务器和数据源，每次安装都会标明来源的信任级别。
- **子 Agent 聚焦并行工作** 内置 `coder`、`explore`、`plan` 子 Agent 在隔离上下文中处理子任务，主对话保持清爽。
- **生命周期 hooks** 在关键节点执行本地命令：拦截高风险工具调用、审计决策、发送桌面通知，或对接你自己的自动化脚本。

## 文档

- [快速上手](docs/zh/guides/getting-started.md)
- [交互与审批](docs/zh/guides/interaction.md)
- [会话](docs/zh/guides/sessions.md)
- [配置](docs/zh/configuration/config-files.md)
- [命令参考](docs/zh/reference/dimi-command.md)

## 本地开发

环境要求：Node.js ≥ 24.15.0，pnpm 10.33.0。

```sh
git clone https://github.com/zzj3720/dimi.git
cd dimi
vp install
```

```sh
vp run dev:cli  # 以开发模式运行 CLI
vp test         # 运行测试
vp run typecheck # TypeScript 检查
vp run lint     # 运行 oxlint
vp run build    # 构建所有包
```

完整贡献流程见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 社区

- [Issues](https://github.com/zzj3720/dimi/issues)
- 安全漏洞反馈，请见 [SECURITY.md](SECURITY.md)。

## 致谢

我们的 TUI 构建在 [`pi-tui`](https://github.com/earendil-works/pi-mono/tree/main/packages/tui) 之上。我们衷心感谢 `pi-tui` 作者的工作。

## 许可证

基于 [MIT](LICENSE) 协议发布。
