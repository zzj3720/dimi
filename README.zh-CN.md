# k-3720

[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE) · [English](README.md)

k-3720 是一个独立的多模型编程 Agent Runtime 与客户端项目。本项目 fork 自 [Kimi Code](https://github.com/MoonshotAI/kimi-code)，并保留和修改了其中大量代码；终端界面及相关交互代码也大量使用了 [Pi](https://github.com/earendil-works/pi)，特别是 `pi-tui`。

本项目不是 Moonshot AI、Kimi、Pi 或 Earendil 的官方项目，也未获得这些项目或其维护者的背书，彼此不存在隶属关系。

## 当前状态

k-3720 仍在积极开发，目前包含：

- 用于本地 Agent 会话的终端 UI；
- 异步工具执行与 wait/resume 机制；
- 原生 Android 客户端；
- 连接本地 Runtime 与移动客户端的端到端加密 Bridge；
- 只转发密文、无法读取 prompt 或响应内容的 WebSocket Relay。

Runtime 以支持多种模型提供商为目标；实际可用的模型和认证方式取决于本地配置。

## 本地开发

环境要求：Node.js 24.15.0 或更高版本、pnpm 10.33.0。本仓库也支持 `vp` 命令包装器。

```sh
git clone https://github.com/zzj3720/k-3720.git
cd k-3720
vp install
vp run dev:cli
```

常用仓库命令：

```sh
vp run typecheck
vp lint
vp test
vp build
```

Android 客户端和 Relay 配置见 [apps/mobile/README.md](apps/mobile/README.md)。

## 上游文档

原始 CLI 的许多行为和配置继承自 Kimi Code，因此其[上游中文文档](https://moonshotai.github.io/kimi-code/zh/)仍可用于了解继承功能；但 k-3720 的名称、命令、默认值和支持的集成可能不同。

## 致谢

- [Kimi Code](https://github.com/MoonshotAI/kimi-code) 由 Moonshot AI 开发，是 k-3720 的 fork 上游。本仓库保留并修改了其中大量 Agent Runtime、CLI、Server、协议和配套 package 代码。
- [Pi](https://github.com/earendil-works/pi) 由 Mario Zechner 及其贡献者开发，是本项目大量 TUI 和终端交互代码的来源。仓库内 vendored 的 `packages/pi-tui` 源自 `@earendil-works/pi-tui`。
- 本项目还依赖许多其他开源软件；它们各自的许可证继续适用于对应组件及发行物。

以上致谢仅用于说明代码来源，不代表赞助、背书或隶属关系。

## 许可证

k-3720 基于 [MIT License](LICENSE) 发布。根许可证保留 Moonshot AI 的原始版权 notice，并为 k-3720 的后续修改增加相应 notice。

Kimi Code 与 Pi 的完整保留声明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
