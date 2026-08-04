# 06 · Codex 全局布局与主题（App shell / layout / 颜色 / 字体 / 滚动条）

> 目标：作为 dimi native-client 复刻 codex **全局壳层与主题**的唯一设计依据。
> 所有数值均为 2026-08-04 对 codex（ChatGPT.app Electron，CDP 9223，窗口实测 **1686×960**，dpr=1）DOM 的实测，不是猜测。
> 方法：`node /tmp/cdp_eval.js 9223 2BD48C26A4CB5B5E0DDF8CF3D349EE76 '<表达式>'`。
> 与 `CODEX_DESIGN.md` 冲突处以本文档为准（本文档修正了其中 2 处错误：chat 行高 22→21px、composer 底距 16→17px）。

---

## 0. 全局分层与几何（实测）

```
窗口 1686×960（dpr=1，深色主题，html class: electron-dark electron-opaque）
┌────────────────────────────────────────────────────────────────────┐
│ Header  46px  fixed z-30 透明无边框，横跨全窗口（含 sidebar 上方）   │
├──────────────┬─────────────────────────────────────────────────────┤
│ Sidebar      │ 内容区 main (x=275, w=1411, #181818)                │
│ 275px        │                                                     │
│ rgba(40,40,  │   消息列 736px @ x=455        │ 右浮动面板 316px     │
│  40,0.7)     │   ┌──────────────────────┐   │ @ x=1370 y=59 z-40  │
│              │   │  thread 消息列表      │   │ ┌───────────────┐   │
│              │   │  (无水平内边距)        │   │ │ 卡片 300px     │   │
│              │   │                      │   │ │ bg #2d2d2d     │   │
│              │   └──────────────────────┘   │ └───────────────┘   │
├──────────────┴──────────────────────────┴───┴─────────────────────┤
│ Composer 736×98 @ x=455 y=845（与消息列 1:1 同 x 同宽，底部距窗口 17px）│
└────────────────────────────────────────────────────────────────────┘
```

### 实测几何表

| 区域 | 窗口坐标（x, y, w, h） | 关键样式 |
|---|---|---|
| body / html | 1686×960 | bg `rgb(20,20,20)`=#141414，color #fff，overflow hidden |
| Sidebar `aside.app-shell-left-panel` | 0, 0, **275**, 960 | bg `color(srgb 0.156863 0.156863 0.156863 / 0.7)` = **rgba(40,40,40,0.7)** |
| Sidebar resize handle | 267, -46, **16**, 1006 | `w-4`(16px)，`right-0 translate-x-2`，**延伸到 header 上方**（y=-46），cursor-col-resize |
| Main 内容区 `main._MainContentSurface_` | 275, 0, **1411**, 960 | bg **#181818**（`--color-token-main-surface-primary`） |
| Header `header._Header_` | 0, 0, **1686**, 46 | fixed，z-30，透明 bg，无边框，覆盖 sidebar 上方 |
| Header 左区 | 0, 0, 275, 46 | 内容 padding-left **88px** → 按钮组从 x=88 开始 |
| 消息列容器 | **455**, —, **736**, — | 列内 `gap-1.5 py-5`，**无水平内边距** |
| 右浮动面板容器 | **1370**, **59**, **316**, **889** | absolute right-0，top 12px，bottom 12px（相对内容区），z-40 |
| 面板卡片 | 1370, 59, **300**, 75+ | 容器 `pe-4`(16px) 内缩；bg #2d2d2d，radius 25px |
| Composer `form` | **455**, **845**, **736**, **98** | 与消息列同 x 同宽；底 = 943，距窗口底 **17px** |

### 消息列定位公式（实测验证）

```
有右面板：x = sidebarW + (panelX − sidebarW − 736) / 2
          = 275 + (1370 − 275 − 736) / 2 = 275 + 179.5 = 454.5 → 455 ✓
          panelX = 窗口宽 − 316（面板宽）= 1686 − 316 = 1370
验证：消息列右缘 455+736=1191，面板左缘 1370，两侧空隙各 179.5px，居中 ✓
无右面板（推断，未实测）：x = sidebarW + (窗口宽 − sidebarW − 736) / 2
          = 275 + (W − 1011) / 2
```
- **composer 与消息列 1:1 对齐**：form x=455 w=736 与消息列完全一致（硬约束）。
- 右面板宽 316 从可用区右缘扣除（面板容器 `right-0` 顶到窗口右缘 1686）。

### 右侧浮动面板特性（实测）

- 结构：`pointer-events-none absolute right-0 top-12px bottom-12px z-40`（容器 316px，含 `pe-4` 16px 右内边距）→ 实际卡片 **300px**。
- 卡片：`rounded-3xl`（实测 **25px**）、bg `rgb(45,45,45)`、`pt-2.5`（top padding 10px）、`pointer-events-auto`。
- 阴影（elevation-prominent，与 composer 胶囊同款）：
  `rgba(255,255,255,0.157) 0 0 0 0.5px`（0.5px 白色发丝边）+ `rgba(0,0,0,0.04) 0 3px 7.5px 0` + `rgba(0,0,0,0.05) 0 0 20px 0`。
- 何时出现：与 header 右侧「切换固定摘要」按钮（title/aria-label）联动（当前会话已开启时可见）。面板内容 = 会话上下文/摘要。**「仅开启固定摘要时显示」为推断**（未实测关闭态）。

### Header 内部布局（实测）

| 按钮 | x | 尺寸 | 样式实测 |
|---|---|---|---|
| 隐藏边栏 | 88 | 28×28 | radius **12.5px**（rounded-lg），1px 透明边框，color text-tertiary |
| 返回 | 120 | 28×28 | 同上（disabled:opacity-40） |
| 前进 | 152 | 28×28 | 同上 |
| 会话标题 | 289 | 187×24 | **14px / 500 / 24px**，radius 10px，padding 0 6px，hover `bg-token-list-hover-background` |
| More（ChatGPT 对话操作） | 494 | 28×28 | radius **10px**（`rounded-full electron:rounded-md`），padding 4px，1px 透明边框 |
| 分享 pill | 1544 | **66×28** | radius 12.5px，padding 0 8px，14px/445/18px |
| 固定摘要 | 1616 | 28×28 | bg `bg-token-foreground/5`（白 5%），radius 12.5px |
| 切换侧边栏 | 1650 | 28×28 | radius 12.5px |
| 菜单按钮（切换侧边栏）@x=0 | 0 | 28×28 | **位于 invisible 幽灵层**（当前 sidebar 可见时不渲染可见版；左侧 0–88 留空） |

- header 左区 padding-left 实测 **88px**（class `ps-[max(var(--spacing-token-safe-header-left),0.5rem)]`，变量实测 0px，但 computed 88px —— 具体覆盖来源未定位，直接按实测 88px 实现）。

### Composer 内部（实测）

```
form 736×98 @ (455,845)
└─ 胶囊 div（bg #2d2d2d + blur(16px) + 阴影，radius 25，overflow-y auto）
   └─ 附件区 h=14
      └─ footer grid: grid-cols-[minmax(0,auto)_auto_minmax(0,1fr)] gap-x-5px px-2 mb-2
         ├─ 左按钮区 col1: 28×28 @ x=463（添加文件，svg 16×16）
         ├─ 输入行 col-span-full row1: -mx-2（→ 与胶囊同宽 736，x=455, h=48）
         │    └─ 编辑器 px-3: x=467, w=712, h=44（min-h 44，max-h-[25dvh]）
         └─ 右按钮区 col3: x=501, w=682, justify-end
              ├─ 模型 pill: 111×28 @ x=1008（文本 13px + chevron svg 14×14）
              ├─ 听写: 28×28 @ x=1119
              └─ 发送: 28×28 @ x=1155（bg #fff 圆，svg 16×16；当前禁用 opacity 0.5）
```
- 按钮统一样式：28×28、radius **9999px**（rounded-full）、`border: 1px solid transparent`、color `rgba(255,255,255,0.498)`、**13px / 445 / 18px**。
- 发送按钮：`bg-token-foreground`（#fff）、`transition-opacity`、禁用时 `opacity:0.5`（实测，非 0.4）。

---

## 1. 颜色 token（全部实测）

> 来源列 = 实测出处（CSS 变量 / computed style）。`--color-token-*` 为 :root 上定义的设计变量。

| token | 精确值 | 用途 | 实测来源 |
|---|---|---|---|
| body bg | `rgb(20,20,20)` = **#141414** | 页面底色（body/html） | `getComputedStyle(body).backgroundColor` |
| main surface | **#181818** | 内容区底色（sidebar 右侧全部） | `--color-token-main-surface-primary`；main computed `rgb(24,24,24)` |
| sidebar bg | `rgba(40,40,40,0.7)` | 左侧边栏 | aside computed `color(srgb 0.156863 0.156863 0.156863 / 0.7)` |
| dropdown / 面板 bg | `rgb(45,45,45)` = **#2d2d2d** | 右浮动面板、下拉层 | `--color-token-dropdown-background`；面板卡片 computed |
| composer bg | `rgba(45,45,45,0.96)` + `blur(16px)` | composer 胶囊（electron 下实色 #2d2d2d） | `--color-token-input-background`；胶囊 computed `rgb(45,45,45)`、`backdrop-filter: blur(16px)` |
| 文本主 primary | **#ffffff** | 标题 / 正文 / 前景 | `--color-token-text-primary`、`--color-token-foreground` |
| 文本次级 secondary | `rgba(255,255,255,0.65)` | 次要文字 | `--color-token-text-secondary` = `color-mix(in srgb, #ffffff 65%, transparent)` |
| 文本弱 tertiary | `rgba(255,255,255,0.498)` | 图标 / 弱文字 / 占位 / 按钮图标 | `--color-token-text-tertiary`；header/composer 按钮 computed |
| description 前景 | `rgba(255,255,255,0.498)` | 描述性文字（面板内） | `--color-token-description-foreground` |
| 边框默认 border | `rgba(255,255,255,0.084)` | 常规分隔线 | `--color-token-border-default` |
| 边框重 border-heavy | `rgba(255,255,255,0.156)` | 强调边框 | `--color-token-border-heavy` |
| 列表 hover | `rgba(255,255,255,0.078)` | 导航 / 列表 / 标题按钮 hover | `--color-token-list-hover-background` |
| 弱底 hover5 | `rgba(255,255,255,0.05)` | 固定摘要按钮底、用户气泡底 | `bg-token-foreground/5` computed `oklab(0.999994 … / 0.05)` |
| 链接 / 主色 | `rgb(131,195,255)` = **#83C3FF** | markdown 链接（暗色主题浅蓝） | `--color-token-link` |
| focus 边框 | `rgba(131,195,255,0.76)` | 输入 / 按钮焦点态 | `--color-token-focus-border` |
| 用户气泡底 | `rgba(255,255,255,0.05)` | 用户消息右对齐气泡 | `bg-token-foreground/5` computed |
| 滚动条 thumb | `rgba(255,255,255,0.084)` | 滚动条滑块 | `--color-token-scrollbar-slider-background`；thread-scroll-container `scrollbar-color` 首值 `rgba(255,255,255,0.082)` |
| 滚动条 thumb hover/active | `rgba(255,255,255,0.156)` | 滑块 hover/按压 | `--color-token-scrollbar-slider-hover-background` / `-active-background` |
| 滚动条 track | `rgba(0,0,0,0)` 透明 | 轨道 | thread-scroll-container `scrollbar-color` 次值 |

### 层级关系（实测确认）
- body **#141414**（最底）→ 内容区 main **#181818**（sidebar 右侧）→ 消息列容器**透明**（透出 #181818）→ 面板 / composer 胶囊 **#2d2d2d**。
- **消息列背景与内容区一致（都是 #181818），与 body 不同**。
- sidebar 自身是半透明 `rgba(40,40,40,0.7)`，透出 #141414。

---

## 2. 字体 token（全部实测）

> codex 使用**可变字体**：全局 `--vscode-font-weight: 445`，正文/按钮大量使用字重 **445**（非 400/500）。
> 实测 `font-size` 直接可读；`line-height` 为 computed（px）。

| 用途 | size | weight | line-height | 实测来源 |
|---|---|---|---|---|
| 字体族 sans | — | — | — | `--font-sans` = `-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif` |
| 字体族 mono | — | — | — | `--font-mono` = `ui-monospace, "SFMono-Regular", "SF Mono", Menlo, Consolas, "Liberation Mono", monospace` |
| body 基准 | 16px | 445（`--vscode-font-weight`） | — | body computed fontSize / root 变量 |
| chat 正文 `.text-size-chat` | **14px** | **445** | **21px** | computed（修正旧文档 22px 的误记） |
| 用户气泡文本 | **16px** | 445 | **24px** | 用户气泡 computed |
| markdown h2 | **20px** | **600** | **25px** | `h2._heading2_` computed |
| markdown ul/ol | 14px | 445 | **22px** | `ul._list_` computed |
| markdown blockquote | 14px | 445 | **24px** | `blockquote._blockquote_` computed |
| header 会话标题 | **14px** | **500** | **24px** | header 标题按钮 computed |
| 按钮文字（header/composer） | **13px** | 445 | **18px** | 分享 pill / composer 按钮 computed |
| 链接 | — | — | — | `--text-link-decoration: none`（**无下划线**） |
| markdown h1 / h3 / code / pre / KaTeX 字体 | — | — | — | **无法观察**（当前会话 DOM 无这些元素；h2 以上层级未出现 h1/h3） |

- 文档旧值「text-size-chat 14/22px」实为 ul 的行高；chat 正文行高 **21px**。
- 无自定义 webfont：全部依赖系统字体栈，无 @font-face（样式表扫描未见）。

---

## 3. 滚动条（实测）

| 项 | 值 | 来源 |
|---|---|---|
| 主消息列滚动容器 `.thread-scroll-container` | `overflow-x: hidden; overflow-y: auto` | computed |
| 滚动条样式 | `scrollbar-width: auto`；`scrollbar-color: rgba(255,255,255,0.084) rgba(0,0,0,0)`（thumb 8.4% 白 / track 透明） | computed |
| 是否占布局宽度 | **否**（`offsetWidth − clientWidth = 0`，overlay 滚动条，macOS 默认） | 实测 |
| thumb hover / active | `rgba(255,255,255,0.156)` | `--color-token-scrollbar-slider-hover-background`（变量级，未触发 hover 验证） |
| 终端面板（xterm，若出现） | `::-webkit-scrollbar { width: 10px; height: 10px }`；thumb 用 slider 变量 | 样式表规则 |
| 表格滚动容器 `._tableScroller_` | `scrollbar-width: thin` | 样式表规则 |
| thumb 宽度 / 圆角 | **无法观察**（webkit 伪元素几何不可经 CDP 读取；overlay 不占位；xterm 规则未设圆角） | — |

---

## 4. 窄窗口 / sidebar 隐藏行为

- **无法观察**：CDP Input 事件挂起（不可拖动 / 不可点击），Electron 窗口无法经 JS `resizeTo`；当前无法切换到 sidebar 隐藏态。
- 依据 DOM 结构推断（未实测）：sidebar 隐藏后（header 最右「切换侧边栏」或左区按钮切换），消息列公式退化为 `x = (W − 736) / 2`；header 左区 padding-left 88px 处的「菜单按钮」会在该状态下出现（invisible 幽灵按钮 @ x=0 暗示此逻辑）。

---

## 5. dimi 差距（codex 实测 vs dimi 当前代码）

对照 `src/renderer/styles/theme.ts`、`App.styles.ts`、`styles/global.ts`。

### 5.1 已一致 ✅
| 项 | codex 实测 | dimi 现状 |
|---|---|---|
| body bg | #141414 | `colors.bgUnder` #141414 ✅ |
| 内容区 surface | #181818 | `colors.surface` #181818，`mainCol` background ✅ |
| sidebar bg | rgba(40,40,40,0.7) | `colors.sidebarBg` 一致 ✅ |
| composer 底 | #2d2d2d | `colors.surface3` / `composerBg` ✅ |
| 文本主 | #ffffff | `colors.text` ✅ |
| 文本弱 | 0.498 | `colors.textTertiary` 0.498 ✅ |
| header 46 / sidebar 275 / thread 736 | 实测一致 | `size.headerH/sidebarW/threadMaxW` ✅ |
| composer radius 25 | 25px | `size.composerRadius` ✅ |
| header 标题 14/500/24 | 实测 | 文档与实现一致 ✅ |

### 5.2 有差异 ⚠️
| # | 项 | codex 实测 | dimi 当前 | 修正建议 |
|---|---|---|---|---|
| 1 | 链接 / 主色 | **#83C3FF**（浅蓝） | `primary` **#0285ff**（深蓝） | 暗色主题下 markdown 链接应改 #83C3FF |
| 2 | focus 边框 | rgba(131,195,255,**0.76**) | `borderFocus` rgba(2,133,255,0.7) | 与 #1 同源：浅蓝系 |
| 3 | 文本次级 | rgba(255,255,255,**0.65**) | `textDim` **0.7** | 降为 0.65 |
| 4 | chat 行高 | **21px** | `font.chatLh` **22px** | 改 21px（ul 才是 22px） |
| 5 | 按钮行高 | **18px** | `font.smLh` 18.57px | 改 18px |
| 6 | 字体族 sans | `-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif` | 多了 `PingFang SC / Microsoft YaHei` | codex 无显式中文栈；可保留（macOS 系统回退等效）或对齐 |
| 7 | 字体族 mono | 含 `SFMono-Regular`、`Consolas`、`Liberation Mono` | 少三项 | 补全 |
| 8 | 边框默认 | rgba(255,255,255,**0.084**) | `border` **0.08** | 0.4% 差，可视为一致（可选对齐） |
| 9 | 边框重 | rgba(255,255,255,**0.156**) | `borderHeavy` **0.16** | 0.4% 差，可视为一致（可选对齐） |
| 10 | 列表 hover | rgba(255,255,255,**0.078**) | `hoverStrong`/`hover8` **0.08** | 0.2% 差，可视为一致 |
| 11 | 用户气泡 | radius **20px**、padding **8px 12px**、max-w **77%**、文本 16/445/24 | 需核对 dimi Transcript 气泡 | 按左侧实测对齐 |
| 12 | composer 底部距窗口 | **17px**（form 底 943，窗口 960） | 文档旧值 16px | 按 17px（若 dimi 用容器 pb 需核对） |
| 13 | composer 胶囊模糊/阴影 | `blur(16px)` + elevation 阴影（0.5px 白发丝 + 双层黑阴影） | theme 无阴影 token；需核对 Composer.styles.ts | 补齐 blur + 阴影 |
| 14 | 发送按钮禁用 opacity | **0.5** | 需核对 dimi | 按 0.5 |

### 5.3 缺失 ❌
| # | 项 | codex | dimi |
|---|---|---|---|
| 1 | 右浮动面板（316px，z-40，top/bottom 12px，卡片 300px radius 25 #2d2d2d + elevation 阴影） | 有 | **无对应实现** |
| 2 | 滚动条自定义 | thumb rgba(255,255,255,0.084) / hover 0.156，overlay，track 透明 | **无自定义滚动条**（依赖系统默认，全仓 grep 无 scrollbar 规则） |
| 3 | 消息列定位公式 | `x = sidebarW + (panelX − sidebarW − 736)/2`（含面板扣除） | `App.styles.ts` 只有 `mainCol flex-1`，**无面板感知的居中公式**（需在 Transcript/Composer 容器实现） |
| 4 | header 左区 padding-left 88px | 实测 88px | 需核对 dimi HeaderBar |
| 5 | 0.5px 发丝阴影 token（`rgba(255,255,255,0.157) 0 0 0 0.5px`） | 面板/composer 共用 | 无 |
| 6 | 全局可变字重 445 | `--vscode-font-weight: 445` | global.ts body 已设 445 ✅（但组件多处仍用 400/500，需统一为 445 默认） |

### 5.4 无法观察 / 待实测
- h1/h3/code/pre/KaTeX 字体（当前会话 DOM 无）。
- 滚动条 thumb 宽度与圆角（webkit 伪元素不可读）。
- sidebar 隐藏后的重排、header 菜单按钮可见态、面板关闭态（Input 事件挂起，无法交互）。

---

## 附：实测命令留档

```bash
# 窗口/body 基准
node /tmp/cdp_eval.js 9223 2BD48C26A4CB5B5E0DDF8CF3D349EE76 '(() => ({ inner: [window.innerWidth, window.innerHeight], dpr: window.devicePixelRatio, bodyBg: getComputedStyle(document.body).backgroundColor, bodyFont: getComputedStyle(document.body).fontFamily }))()'
# 读设计变量
node /tmp/cdp_eval.js 9223 2BD48C26A4CB5B5E0DDF8CF3D349EE76 '(() => { const r = getComputedStyle(document.documentElement); const n = ["--color-token-main-surface-primary","--color-token-dropdown-background","--color-token-input-background","--color-token-list-hover-background","--color-token-text-secondary","--color-token-text-tertiary","--color-token-border-default","--color-token-border-heavy","--color-token-link","--color-token-focus-border","--color-token-scrollbar-slider-background","--color-token-scrollbar-slider-hover-background"]; const v = {}; n.forEach(x => v[x] = r.getPropertyValue(x).trim()); return v; })()'
```
