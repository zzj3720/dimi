# k-3720

[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE) · [文档](docs/zh/guides/getting-started.md) · [Issues](https://github.com/zzj3720/k-3720/issues) · [English](README.md)

k-3720 是一个独立的多模型编程 Agent Runtime 与客户端项目。本项目 fork 自 [Kimi Code](https://github.com/MoonshotAI/kimi-code)，并保留和修改了其中大量代码；终端界面及相关交互代码也大量使用了 [Pi](https://github.com/earendil-works/pi)，特别是 `pi-tui`。

本项目不是 Moonshot AI、Kimi、Pi 或 Earendil 的官方项目，也未获得这些项目或其维护者的背书，彼此不存在隶属关系。

## 当前状态

k-3720 仍在积极开发，目前包含：

- 统一连接 Kimi、Codex、Grok、Claude、Gemini、云服务和兼容自定义端点的供应商 Runtime；
- 用于本地 Agent 会话的终端 UI；
- 异步工具执行与 wait/resume 机制；
- 原生 Android 客户端；
- 连接本地 Runtime 与移动客户端的端到端加密 Bridge；
- 只转发密文、无法读取 prompt 或响应内容的 WebSocket Relay。

## 安装

这是没有独立发布通道的源码构建。请 clone 本仓库，不要使用旧 Kimi Code 安装脚本或 npm 的 `latest` 包：

```sh
git clone https://github.com/zzj3720/k-3720.git
cd k-3720
vp install
vp run dev:cli
```

更新时运行 `git pull --ff-only && vp install`。`kimi upgrade` 会提示本构建未配置自动升级。

首次启动时运行 `/login`，选择供应商及其支持的 OAuth、API 密钥或云身份登录方式。兼容端点可以在 `~/.kimi-code/models.json` 中添加或覆盖；详见[供应商与模型](docs/zh/configuration/providers.md)。

Android 客户端和 Relay 配置见 [apps/mobile/README.md](apps/mobile/README.md)。

## 本地开发

环境要求：Node.js 24.15.0 或更高版本、pnpm 10.33.0。本仓库也支持 `vp` 命令包装器。

```sh
vp install
vp run dev:cli
vp run typecheck
vp lint
vp test
vp build
```

完整贡献流程见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 文档

- [快速上手](docs/zh/guides/getting-started.md)
- [交互与审批](docs/zh/guides/interaction.md)
- [会话](docs/zh/guides/sessions.md)
- [在 IDE 中使用（ACP）](docs/zh/guides/ides.md)
- [配置](docs/zh/configuration/config-files.md)
- [命令参考](docs/zh/reference/kimi-command.md)

## 致谢

- [Kimi Code](https://github.com/MoonshotAI/kimi-code) 是 Moonshot AI 开发的上游项目，k-3720 fork 自该项目，并继续保留和修改其中大量 Agent Runtime、CLI、Server、Protocol 与配套代码。
- [Pi](https://github.com/earendil-works/pi) 是 Mario Zechner 及贡献者开发的项目。k-3720 的 TUI 与终端交互代码大量使用了 Pi，仓库内的 `packages/pi-tui` 源自 `@earendil-works/pi-tui`。
- 本项目还依赖许多其他开源软件，各组件和分发继续遵守其各自许可证。

以上致谢只用于说明代码来源，不代表相关项目或维护者对 k-3720 提供赞助、背书或存在隶属关系。

## 许可证

k-3720 基于 [MIT License](LICENSE) 分发。许可证中保留 Moonshot AI 的原始版权声明，并为后续修改增加 k-3720 的版权声明。

Kimi Code 与 Pi 的保留许可证声明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
