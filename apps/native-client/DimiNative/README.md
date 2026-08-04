# Dimi Native Client (Electron)

Dimi 桌面客户端（Electron），目标是 **TUI 1:1 功能复刻**——用户视角功能与
`apps/dimi` TUI 完全一致，不做 UX 优化。

## 架构

```
src/
  main/index.js      Electron 主进程：窗口 + REST 代理（无 CORS）+ SSE 流桥
  main/preload.mjs   contextBridge：window.dimi.request / subscribeEvents
  renderer/
    index.html       视图骨架
    styles.css       深色主题样式
    app.js           Model / Msg / update reducer（镜像 TUI 架构）+ slash menu 补全
    main.js          DOM 事件绑定、键盘处理、REST/SSE 对接、提交/steer/审批/提问
    view.js          model → DOM 渲染投影（纯函数）
scripts/
  cdp-check.mjs      CDP 冒烟检查（DOM 状态探测）
  cdp-e2e.mjs        端到端验证（选会话→发消息→SSE 流式渲染）
  cdp-errors.mjs     CDP 异常捕获
```

设计原则（镜像 TUI）：
- 所有业务逻辑在 renderer 的 reducer 里；main 进程只做传输
- 键盘语义严格对照 `editor-keyboard.ts`（Ctrl+C 分层、Ctrl+S steer、Esc 分层等）
- slash menu 输入 `/` 自动触发 fuzzy 命令菜单，Tab 接受加空格重开参数补全

## 启动

```bash
npm install
# 连接本地 dimi server（默认 http://127.0.0.1:58627，无需 token）
npm start

# 指定 server + token
DIMI_SERVER_URL=http://127.0.0.1:58628 DIMI_TOKEN=<token> npm start

# 开发模式（CDP 调试端口）
DIMI_SERVER_URL=... DIMI_TOKEN=... npx electron . --remote-debugging-port=9222
```

## 验证

```bash
# 冒烟检查（需 CDP 端口）
node scripts/cdp-check.mjs "ws://127.0.0.1:9222/devtools/page/<id>"

# 端到端（选会话→发消息→等流式回复）
node scripts/cdp-e2e.mjs "ws://127.0.0.1:9222/devtools/page/<id>"
```

## 已实现（对照 TUI 审计修复后）

- 连接 server：REST 代理 + SSE 事件流（meta / sessions / messages / prompts / events）
- 会话 picker：fuzzy 搜索、↑↓/Enter/Esc 两段、Ctrl+A scope、分页加载
- slash menu：输入 `/` 自动 fuzzy 命令菜单、参数补全、Tab 接受加空格、@mention
- 提交路由：idle 直接发 / busy+steer 注入 / busy+queue 排队 / compacting 排队
- 审批面板：SSE 触发、↑↓/数字键/Enter/Esc（服务器 reject）、feedback、Ctrl+E 预览
- 提问面板：多题 tabs、数字/空格/↑↓、Other 输入、review 页、未答标记
- 快捷键：Ctrl+C 分层、Ctrl+S steer、Ctrl+D、Esc 分层、Ctrl+B detach、Ctrl+T、
  Ctrl+O、Ctrl+- undo、Shift+Tab plan、↑↓ 历史/队列
- bash 模式：`!` 前缀、busy 排队、turn 结束 drain
- slash 命令：/undo /compact /btw 接线（其余标注 "not wired"）
