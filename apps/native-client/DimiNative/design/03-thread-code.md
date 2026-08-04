# 03 · 消息列表（Thread）模块设计逆向 — 基于 codex bundle + 真实 DOM 复核

> 逆向对象：ChatGPT.app（codex 桌面端）webview bundle
> 主 JS：`/tmp/codex_asar/webview/assets/app-initial-iBPGfcXU.js`（14.9MB，minified）
> 本地会话 Thread 模块 chunk：`local-conversation-thread-HJvjyIe5.js`（337KB）
> CSS：`app-44wrUC9v.css`（635KB，tailwind v4 + 主题 token）、`app-initial-BSHZIbh1.css`（99KB，markdown CSS module）
> 复核方式：CDP 直连运行中的 codex 实例查询真实 DOM 与 computed style（查询语句见文末）
> 本文档是**设计**文档（组件结构/布局/边距/透明度/行为），不是颜色数值表。

---

## 0. 一句话总结

消息列表是一条**虚拟化纵向列表**：外层滚动容器内是 768px 居中列（`--thread-content-max-width:48rem`），两侧 16px 内边距后内容列宽 **736px**；每条 turn 是 `py-2`（8px×2）的行，行间由列表 gap 6px 分隔（turn 间距合计 22px）；turn 内部是 `gap-3`（12px）的纵向 flex，块与块之间再用 16px 分隔条（`s8c`）隔开。用户消息右对齐、气泡 `max-w-[77%]` + 20px 圆角 + 白 5% 底；assistant 消息无气泡占满全列；思考区默认折叠，展开用 height+opacity 动画（300ms cubic-bezier(0.19,1,0.22,1)），预览内容封顶 140px 底部渐隐；操作行默认 `opacity-0`，`group-hover` 显现（用户复制行与 assistant 操作行同规则）。

---

## 1. 组件结构（完整层级树）

### 1.1 消息列容器（Thread）

真实 DOM（CDP 实测，当前运行实例）：

```
div.thread-scroll-container.relative.h-full.overflow-x-hidden（滚动容器，padding: 32px 0 0）
└─ div.flex.min-h-full.shrink-0.flex-col.justify-start（宽度 1411px = 视口）
   └─ div.mx-auto.w-full.max-w-(--thread-content-max-width).px-toolbar
      │   computed: max-width 768px, padding 0 16px 32px, margin 0 auto（水平居中）
      │   ← 736px 内容列的由来：768 − 16×2
      └─ div[data-thread-find-target="conversation"].flex.min-h-full.flex-1.flex-col.gap-1.5.py-5
          │   computed: gap 6px, padding 20px 0, width 736px
          └─ （虚拟化行）div.relative.shrink-0          ← 每条 turn 一行
              └─ div.flex.flex-col                     ← 虚拟化测量包装
                 └─ div                                ← 行内容
                    └─ div.[&_[data-virtualized-turn-content]]:[content-visibility:visible]
                       └─ div.flex.flex-col.gap-1.5    ← turn 组容器（gap 6px，容纳多个内容块）
                          └─ div.group.flex.flex-col.py-2   ← ★ turn 本体（py-2 = 8px 上下）
```

- 列表本身是**虚拟化列表**：本地会话用 `Pm`/`OD`（VirtualizedTurnList），`gapPx: 6`；ChatGPT 会话页用 `Ki`，同样 `gapPx: 6`。
- turn 行有 `data-local-conversation-final-assistant`（assistant-item 时）、`data-content-search-turn-key`、`scroll-mt-4`（用户消息锚点）等搜索/滚动定位属性。
- `--thread-content-max-width` 在 electron 下为 `48rem`（768px），定义于 `[data-codex-window-type=electron] body`；另有 `42rem`/`40rem`/`100%`/`480px`/`500px` 变体类（extension/窄窗等场景），`.markdownContent` 内部再重置为 `40rem`（640px，供表格等宽元素 min-width 计算）。

### 1.2 Turn（H$l 组件）

```
div.group.flex.flex-col.py-2                        ← turn（py-2 = 8px 上下）
└─ div.flex.flex-col.gap-3                          ← turn 内容器（gap-3 = 12px）
   ├─ h4.sr-only.select-none                        ← 无障碍标题（如「你说：」/ assistant 角色）
   ├─ 用户消息块（见 1.3）
   ├─ 思考区 mAl（见 1.5）
   ├─ 工具活动块（exec/terminal 卡片，见 1.6）
   ├─ assistant 消息块（见 1.4）
   ├─ 计划/搜索/附件/状态等补充块
   └─ …（块与块之间由 s8c 分隔条隔开）
```

- turn 内部把消息/活动组织为**有序 item 数组**（`vn`）：顺序为 model-changed → 用户消息（可多个）→ model-rerouted → 代理活动折叠组 → automation-update → assistant-after → assistant-item → tool-outputs → post-assistant-items → mcp-server-elicitation → proposed-plan → thinking-placeholder → turn-diff → remote-task → personality-changed → forked-from-conversation → end-resource → thread-handoff-operation。
- 多 item 时每两个相邻块之间插入 `s8c` 分隔条：`div[aria-hidden].w-full`，高度 `var(--conversation-item-gap, 16px)`（grouped 变体 4px）。单 item 时 turn 内容容器为 `flex flex-col gap-0 [contain-intrinsic-size:auto_240px] [content-visibility:auto]`（虚拟化内容可见性优化）。
- 单条 item 的容器：`div.flex.flex-col`（`data-local-conversation-final-assistant` 标记在 assistant-item 上）。

### 1.3 用户消息块（hsl）

真实 DOM：

```
div.group.flex.w-full.flex-col.items-end.justify-end.gap-1   ← 用户消息根（右对齐，gap 4px）
├─ 图片/附件行：div.hide-scrollbar.flex.max-w-full.flex-row-reverse.self-end.overflow-x-auto
│   └─ div.flex.min-w-max.items-end.gap-2                     ← 上传图 + appshot 缩略图
├─ 附件胶囊行：div.hide-scrollbar.flex.max-w-full.flex-row-reverse.self-end.overflow-x-auto
│   └─ div.flex.min-w-max.items-center.gap-2                  ← 父上下文/前置上下文/附件/PR 胶囊
├─ ★ 气泡：div[data-user-message-bubble].tabindex=0
│       .bg-token-foreground/5.max-w-[77%].min-w-0.overflow-hidden.break-words
│       .rounded-2xl.px-3.py-2.[&_.contain-inline-size]:[contain:initial]
│       .text-start.focus-visible:ring-2.focus-visible:ring-token-focus-border
│       .focus-visible:outline-none
│   └─ div.flex.flex-col.items-end.gap-1
│      └─ div.relative.w-full.min-w-0.text-size-chat
│         └─ div > div.text-size-chat.whitespace-pre-wrap     ← 用户消息保留换行
│            └─ div._markdownContent_1q3nk_74（用户气泡专用覆写类，见 §6.2）
│               └─ p._markdownText_1q3nk_112._paragraph_1q3nk_103
└─ div.flex.flex-row-reverse.items-center.gap-1               ← 底部行（chips + 操作）
   ├─ 状态行：div.ms-1.me-1.flex.items-center.gap-2           ← 状态图标 + 状态文案（Hook blocked / Sent as goal…）
   ├─ 模式 chips：span.text-token-description-foreground.text-xs  ← 「Review mode」「PR fix」「Auto resolve conflicts」「N comments」等
   └─ ★ 操作行：div.me-1.ms-1.flex.items-center.gap-2
          .opacity-0.group-focus-within:opacity-100.group-hover:opacity-100
      ├─ span（发送时间，hover 才显）
      ├─ hook 统计 Aol
      └─ div.flex.items-center.gap-0.5
         ├─ 复制按钮（tm Button, ghost, icon, icon-xs）26×26px
         └─ 编辑按钮（仅可编辑时）26×26px
```

- 用户在消息列表**无头像**；头像（`g2` role=user）由 turn 外层在消息上方渲染（`flex flex-col items-end gap-2` 的兄弟节点，CDP 中位于气泡外）。
- 有 reaction/头像场景：气泡外包 `div.relative.mb-2.flex.w-full.justify-end`，头像绝对定位 `absolute -end-0.5 -bottom-0.5 translate-x-1/4 translate-y-1/4`（右下角骑缝）。

### 1.4 Assistant 消息块（jvl → W6 case assistant-message）

```
div.group.flex.min-w-0.flex-col                       ← assistant 消息根（group 供操作行 hover）
├─ 头像 g2（role=assistant）
├─ ★ markdown 内容（LJa 渲染，无气泡、无内边距，占满列宽）
│   容器类：[&>*:last-child]:mb-0 [&>ol:first-child]:mt-0 [&>ul:first-child]:mt-0
│   （仅引用型消息额外加 inline [&>p:last-child]:inline）
├─ processTargets（kml）：已完成的最终答复才显示「Worked for Xs」徽标组
├─ 自动化引用（nml）
├─ 搜索结果：div.mt-3（R_l 结果卡）
├─ after 内容：div.mt-3（assistant-after，包装在 ak electron 容器）
└─ ★ 操作行（Fvl，见 1.7）
```

### 1.5 思考区（mAl = ReasoningItem）

```
erl（活动项包装：header + body）
├─ header：Xnl（可折叠头）
│  └─ i1（思考 spinner/脉冲动画，active=流式中）+ span.text-token-conversation-header.text-size-chat.truncate
│     └─ ★ 思考按钮文案：「Thinking」/「Thought for {elapsed}」/「Thought」
│        （中文 locale：「思考了 {elapsed}」→ 实测「思考了 2m 0s」）
└─ body：rf.div（framer-motion）animate={height: 实测px 或 0, opacity: 1 或 0}
        transition=Ck（300ms cubic-bezier(0.19,1,0.22,1)）
        className=overflow-visible | overflow-hidden
        style={pointerEvents: auto | none}
   └─ Irl（测量高度 ref）variant=flush
      └─ eAl（边缘渐隐容器）className=[--edge-fade-distance:1rem]
          maxHeightByState={preview: 8.75rem, expanded: 8.75rem, collapsed: 0px}
          viewState=expanded
         └─ markdown（LJa），容器类：
            text-token-conversation-body [&_*]:text-token-non-assistant-body-descendant
            break-words text-size-chat [&_*]:text-size-chat
            [&>h1]:mt-2 [&>h2]:mt-2 [&>h3]:mt-2
            [&>h1+*]:mt-1 [&>h2+*]:mt-1 [&>h3+*]:mt-1 [&>p+p]:mt-1
```

- **思考按钮本身**（CDP 实测）：
  `button.inline-flex.max-w-full.min-w-0.cursor-interaction.items-center.gap-0.5.self-start.p-0.text-left.text-token-text-tertiary.select-none.hover:text-token-text-primary`
  computed：**height 21px、line-height 21px**；`aria-expanded=false`（折叠态）；chevron 为 20×20 svg `icon-xs.shrink-0.transition-transform.duration-relaxed`，展开时 `rotate-180`。
- 思考正文渲染特殊覆写（与正文不同）：h1/h2/h3 `mt-2`（8px），标题后首个元素 `mt-1`（4px），段落间 `mt-1`（4px），p 用 `m-0`，ul/ol `my-0 ps-4`。
- 思考内容**有摘要提取**：取正文末尾第一个非注释段（`lAl`），或正文开头的 `**加粗**` 段（`vAl`），折叠时按钮下方显示摘要文本。

### 1.6 工具活动卡片（exec/terminal，Rbl）

```
erl（活动项）
├─ header：Xnl（可折叠头）
│  ├─ 状态图标（成功/失败/进行中）+ summary 文案（「Run X」「Open file」「Search…」等）
│  └─ 展开 chevron（hover 才显：opacity-0 group-hover:opacity-100，旋转 transition）
└─ body：rf.div 同上 height/opacity 动画（0.3s）
   └─ Irl（测量）→ Pbl（shell 卡片）
      ├─ 命令 + 输出（Pbl：bg-token-text-code-block-background，radius 12.5px 等）
      └─ Pbl.Footer（isInProgress / 成功 / 失败退出码 / 中断）
```

- 折叠状态 `useState('collapsed')`；展开时通过 `Wyl` 上报 `toolActivityTurnKey`。
- 流式中且非后台终端时，每 1s 刷新 elapsed 时间（`xX(O, 1000)`）；初始 `startedAtMs ?? (inProgress ? Date.now() : null)`。
- 状态文案（`ixl` summary 行）：进行中 + 后台运行 / 后台完成 / 被中断 各有不同前缀（`text-token-conversation-summary-leading/trailing`），hover 变 `text-token-foreground`。

### 1.7 操作行（Fvl = AssistantMessageActions）

```
div.mt-1.5.flex.h-5.items-center.justify-start.gap-0.5
    .electron:-translate-x-1.extension:-translate-x-1.5
    .[&_button]:focus-visible:ring-2…        ← 整行（computed: marginTop 6px, height 20px, gap 2px）
├─ L（操作组）：
│  div.flex.h-full.items-center.gap-0.5
│      .opacity-0.group-focus-within:opacity-100.group-hover:opacity-100   ← 默认 hover 显
│  ├─ 复制按钮（cB CopyButton，iconOnly icon-xs）—— 仅 copyText 非空时
│  ├─ additionalActions（外部注入）
│  ├─ persistentAdditionalActions（常显注入）
│  ├─ 报告/评分（Svl）：thumbs_up「Good response」+ thumbs_down「Bad response」
│  │    ghost icon 按钮 24×24px（CDP 实测），thumbs_down rotate-180，aria-pressed=选中态
│  ├─ fork 按钮（tm ghost icon）：aria-label「Continue in new chat from here」
│  │    （中文「在新聊天中继续」），isForking 时显示 spinner，forkDisabled 禁用
│  ├─ autoReviewStats（gml）
│  ├─ hookStats（Aol）
│  ├─ completedThreadGoal（Ivl）：divider（h-3 border-s）+ 图标 +「Goal achieved in {time}」
│  └─ memoryCitationEntries（Lvl）
└─ R（发送时间）：span.ms-1.5.flex.h-full.items-center[data-assistant-message-sent-time]
     .opacity-0.group-hover:opacity-100（timestampHoverOnly 时）
     .opacity-0.group-focus-within:opacity-100.group-hover:opacity-100（默认）
```

- **整行隐藏规则**：无任何动作项（复制/统计/评分/fork/注入）且 `!showTimestampWithoutActions` 时，整行 return null。
- **行可见性**：默认 `opacity-0`，`group-hover` / `group-focus-within` 时 `opacity-100`；`alwaysShowActions=true` 时无 opacity 类（常显）。
- **electron/extension 平台偏移**：`electron:-translate-x-1`（−4px）、`extension:-translate-x-1.5`（−6px）——整行左移补偿。
- 复制：`getCopyText()` 生成纯文本（markdown→文本，路径资源解析），`getCopyHtml()` 生成 HTML；`navigator.clipboard.writeText`；assistant 消息侧先发 `copy`/`assistant_message` 分析事件。
- 用户操作行差异（hsl）：用户侧**无 fork/评分**，只有 复制 + 编辑（可编辑时）；同样 `opacity-0 group-hover:opacity-100`；复制成功后图标变 check、1.5s 后恢复；无内容时显示「(No content)」（`mb-px text-size-chat text-token-description-foreground`）。

---

## 2. 布局（Layout）

| 层级 | 规则 | 值 |
|---|---|---|
| 滚动容器 | `thread-scroll-container` | `padding: 32px 0 0`，`overflow-x: hidden` |
| 列壳 | `mx-auto w-full max-w-(--thread-content-max-width) px-toolbar` | max-width **768px**（`48rem`）、水平居中、`padding: 0 16px 32px` |
| 消息列 | `flex min-h-full flex-1 flex-col gap-1.5 py-5` | **gap 6px**、**padding 20px 0**、内容宽 **736px** |
| 虚拟化行 | `div.relative.shrink-0` + 测量包装 | 行间 gap 6px（`gapPx: 6`） |
| turn 本体 | `group flex flex-col py-2` | **padding 8px 0** → 相邻 turn 间距 = 6 + 8 + 8 = **22px** |
| turn 内 | `flex flex-col gap-3` | **gap 12px** |
| turn 内块分隔 | `s8c` | 高度 **16px**（`--conversation-item-gap`，grouped 4px） |
| 用户消息根 | `flex w-full flex-col items-end justify-end gap-1` | 右对齐，**gap 4px** |
| 用户气泡 | `max-w-[77%] rounded-2xl px-3 py-2` | max-width **77%**、padding **8px 12px**、radius **20px**（`--radius-2xl: 1.25rem` electron 比例 1.25）、`text-align: start`、`word-break: break-word`、`overflow: hidden` |
| assistant 内容 | 无气泡/无内边距 | 占满 736px 列宽 |
| 思考按钮 | `inline-flex … self-start` | 高 **21px**、行高 21px、`gap-0.5` |
| 思考折叠动画 | rf.div animate height/opacity | transition `{duration: 0.3, ease: [0.19,1,0.22,1]}` |
| 思考内容封顶 | eAl maxHeightByState | preview/expanded **8.75rem（140px）**，`--edge-fade-distance: 1rem` |
| assistant 操作行 | `mt-1.5 flex h-5 items-center justify-start gap-0.5` | **margin-top 6px**、**高 20px**、**gap 2px**、`electron:-translate-x-1`（−4px） |
| 用户操作行 | `me-1 ms-1 flex items-center gap-2` | 左右 4px、gap 8px |

### 2.1 736px 的由来（已证实）

`--thread-content-max-width: 48rem`（=768px）定义在 `[data-codex-window-type=electron] body`；列壳 `px-toolbar` 提供 16px 侧内边距；`768 − 16×2 = 736px` 即消息列内容宽。CDP 实测链条：`thread-scroll-container(1411px) → 列壳(768px, pad 0 16px 32px) → 消息列(736px, pad 20px 0)`。CSS 中另有 `40rem`（640px，markdown 内容内）/`42rem`/`100%`/`480px`/`500px` 变体，用于 extension/特殊窗口。

---

## 3. 边距 / 内距（px 汇总）

### 3.1 消息列 / turn
- 消息列：`gap 6px`，`padding: 20px 0`
- turn：`padding: 8px 0`（上下）；turn 间距合计 **22px**
- turn 内容器：`gap 12px`
- turn 内块分隔（s8c）：16px（`--conversation-item-gap` 默认；`--conversation-grouped-item-gap: 4px` 用于 grouped 变体）
- 底部 padding：列壳 `padding-bottom: 32px`（消息列之后到 composer 前的留白）

### 3.2 用户消息
- 根：`gap 4px`；外层容器（W6 user-message case）：`flex flex-col items-end gap-2`（8px）
- 气泡：`padding: 8px 12px`，`border-radius: 20px`，`max-width: 77%`
- 气泡内内容包装：`flex flex-col items-end gap-1`（4px）；文本 `text-size-chat`（14px）+ `whitespace-pre-wrap`
- 操作行：`margin: 0 4px`（me-1 ms-1），`gap: 8px`（items-center gap-2）；按钮组 `gap 2px`（gap-0.5）
- 无内容占位：「(No content)」`margin-bottom: 1px`

### 3.3 Assistant 消息
- 根：`flex flex-col`，无内边距
- 头像：在消息上方（`flex flex-col gap-3` 容器内第一个子元素）
- markdown 容器：无 padding；覆写 `[&>*:last-child]:mb-0 [&>ol:first-child]:mt-0 [&>ul:first-child]:mt-0`
- 搜索结果 / after：`margin-top: 12px`（mt-3）
- 操作行：`margin-top: 6px`，`height: 20px`，`gap: 2px`，`electron: translate-x -4px`

### 3.4 思考区
- 思考按钮：`gap 2px`（gap-0.5），`padding: 0`，高度 21px
- 思考 body：`padding-top: 0`（由 erl 控制），内容间距 `gap-0`（eAl contentClassName）
- 思考正文 markdown 特殊覆写：h1/h2/h3 `margin-top: 8px`；标题后首个元素 `margin-top: 4px`；段落间 `margin-top: 4px`；p `margin: 0`；ul/ol `margin: 0` + `padding-inline-start: 16px`

### 3.5 工具卡片
- 卡片：`overflow-hidden rounded-lg border border-token-border-heavy`
- 内部：命令 + 输出区 `font-mono`，`text-size-chat-sm`（12px/20px 经 `_markdownTextSmall`），输出区 `overflow-x: auto`
- 展开动画同思考区（height/opacity 300ms）

---

## 4. 透明度 / 视觉态

### 4.1 操作行显隐（核心）
- **assistant 操作行**：整行 `opacity-0` → `group-hover:opacity-100` / `group-focus-within:opacity-100`（组 = 整个 turn 或消息根 `group`）。`alwaysShowActions=true` 时移除 opacity 类（常显，无过渡动画）。
- **用户操作行**：同样 `opacity-0 group-focus-within:opacity-100 group-hover:opacity-100`。
- **发送时间**：assistant 侧 `ms-1.5` 独立 span，`timestampHoverOnly=true` 时仅 `group-hover` 显，否则随操作行显；用户侧发送时间同样 hover 显。
- **无动作且无时间戳时整行不渲染**（return null），不占位。
- CDP 复核注意：当前最新 turn 的 assistant 操作行 computed opacity 为 1（`alwaysShowActions` 场景或 hover 中），历史 turn 为 0。

### 4.2 思考展开动画（已证实）
- 状态：`useState(false)` 默认**折叠**；流式中若内容非空且会话状态非暂停 → 自动展开。
- 动画目标：`animate={{ height: 实测内容高度px 或 0, opacity: 1 或 0 }}`，`initial: false`（首次渲染不播）。
- 过渡：`transition: { duration: 0.3, ease: [0.19, 1, 0.22, 1] }`（`Ck`）。
- 折叠态容器：`overflow-hidden` + `pointer-events: none`；展开态 `overflow-visible` + `pointer-events: auto`。
- 测量：`useElementHeight`（`J2`）在展开时 attach ref，得到内容实测高度；折叠时 `S = 0`。
- 内容封顶：`maxHeightByState: {preview: 8.75rem, expanded: 8.75rem, collapsed: 0px}` + `[--edge-fade-distance:1rem]` → 思考正文展开后仍封顶 140px，底部 16px 渐隐。
- chevron：`transition-transform duration-relaxed`，展开 `rotate-180`。

### 4.3 工具卡片展开
- 同款 height/opacity 动画（`{height: 实测px/0, opacity: 1/0}`，`Ck` 过渡），初始 `collapsed`；流式中自动 refresh elapsed（1s 间隔）。
- 展开时上报 `toolActivityTurnKey`（用于活动时间线）。
- 状态色：成功（exitCode 0）/失败/中断分别由 summary 前缀 + 图标 + footer 表达（`text-token-charts-green` 等 token，非写死）。

### 4.4 Hover 态
- 思考按钮：`text-token-text-tertiary` → `hover:text-token-text-primary`。
- 工具 summary：`text-token-conversation-summary-leading/trailing` → `group-hover/activity-header:text-token-foreground`；header 内其他文本 hover 时由 `text-token-foreground/30` → `text-token-foreground`（带 `@media(hover:hover)` 保护）。
- 复制/编辑按钮：`text-token-text-tertiary`，`enabled:hover:bg-token-list-hover-background`，`enabled:active:bg-token-foreground/15`，`focus-visible:ring-2 ring-token-focus-border ring-offset-0`；electron 下 `rounded-md` + `p-1`（CDP 实测 26×26px 含内边距）。
- 评分按钮（wvl）：ghost icon，`aria-pressed` 标记选中；thumbs_down `rotate-180`。

### 4.5 状态色
- 状态行（用户消息下方）：Hook blocked / Sent as goal / Hook feedback 等，`text-token-description-foreground` + `icon-2xs`。
- 模式 chips：`text-token-description-foreground text-xs`。
- 思考中 spinner：`i1`（脉冲动画组件）在流式中显示，文案「Thinking」。

---

## 5. 行为逻辑

### 5.1 思考折叠/展开
- 按钮：`aria-expanded` 反射展开态；点击 `onToggle`（`setExpanded(prev => !prev)`）。
- 流式自动展开条件：`s && content非空 && (会话状态 null 或 'resumed')`；结束后回到用户控制（默认折叠）。
- 摘要提取：`lAl`（末段非注释）或 `vAl`（开头 `**bold**` 段剥除后 trim）；摘要用于折叠态按钮下文案与 header 文本。
- elapsed 计时：`yAl` 用 `Date.now()` 基准 + 1s 轮询（`xX(g, e ? 1000 : null)`），`Thought for {elapsed}`（中文「思考了 2m 0s」，秒恒显）。
- 无法确定：思考摘要折叠态是否在按钮旁以内联文本展示（有 `u` 摘要变量，但 CDP 当前实例无折叠摘要 DOM 可核对——折叠时只看到按钮）。

### 5.2 用户消息编辑（已证实）
- 触发：气泡 `onDoubleClick`（可编辑时）或操作行编辑按钮（`Edit message`）。
- 流程：点击后初始化 draft（`store.set(draft, turnId, messageText)`）→ 气泡替换为编辑区 `div.w-full.p-px > psl(composer)`（含取消/提交）→ 提交调用 `onEditMessage(text)` 后清 draft。
- 复制：`navigator.clipboard.writeText(cleanText)`，成功图标变 check、1500ms 恢复；先发分析事件 `copy`/`user_message`。
- 不可编辑场景：实现计划合成消息（`Yes, implement this plan`）不可编辑；隐藏时 `hideUserMessageActions`。

### 5.3 评分/报告（thumbs）
- 点击 thumbs → 发 `turn_rating` 分析事件 + 打开反馈对话框（选项 + 详情 + 可选安全举报路径）。
- 选中态 `aria-pressed`；再点同项取消。

### 5.4 Fork（在新聊天中继续）
- 仅 `completed` 的 assistant 消息可 fork；`forkDisabled` 或 `isForking` 时禁用并显示 spinner。
- `onClick` 先 `stopPropagation`；文案「Continue in new chat from here」。

### 5.5 markdown 宽表格（已证实）
- 表格包装：`_tableWrapper`（`width: fit-content; margin-inline: var(--thread-content-margin, 24px)`）。
- 滚动：`_tableScroller`（`overflow-x: auto; display: flex; justify-content: safe center; scrollbar-width: thin`）——**超宽表格横向滚动**。
- 出血：`_tableContainer`（`width: calc(100% + (24px*2)); margin-inline: -24px`）向两侧各出血 24px。
- `_tableWideBlock`：`--wide-block-width = min(markdown-wide-block-max-width + 8rem, container-max-width)`，`width: calc(wide-block-width + 48px)`，`margin-left` 居中再偏移 −24px，实现居中扩展。
- 表格本体：`width: fit-content; min-width: min(100cqw − toolbar×2, thread-max-width − toolbar×2); border-collapse: separate; border-spacing: 0; font-size: 14px; margin: 0`。

### 5.6 虚拟化 / 滚动
- 虚拟列表 `gapPx: 6`；turn 内容 `content-visibility: auto` + `contain-intrinsic-size: auto 240px`（估算高度 240px）。
- 自动滚动：OD 中 followMode（`prework_follow`/`user_follow`/`static`），最新 turn 高度/页脚测量驱动。
- 搜索定位：`data-content-search-turn-key`、`scroll-mt-4` 锚点；thread find 用 `[data-thread-find-target=conversation]`。

### 5.7 复制内容规则
- assistant 复制：markdown→纯文本（`getCopyText`，含 cwd/路径解析），另带 HTML（`getCopyHtml` 从选区 DOM 提取）。
- 用户复制：`H$n(L)`（cleanText：去 system 块、3+ 换行压 2）。

---

## 6. Markdown 布局（CSS module，`_markdownContent_1q3nk_74` 体系）

> 数据源：`app-initial-BSHZIbh1.css`；字号基线 `--markdown-font-size = var(--codex-chat-font-size)`，CDP 实测当前 electron 实例 **14px**；行高 `calc(font-size + 8px)` = **22px**（14px 字号下）。

### 6.1 根容器
```css
._markdownContent_1q3nk_74 {
  --markdown-font-size: var(--codex-chat-font-size);
  --markdown-line-height: calc(var(--markdown-font-size) + 8px);
  --thread-content-max-width: 40rem;          /* 640px，表格 min-width 计算用 */
  color: var(--text-primary);
  font-size: var(--markdown-font-size);
  line-height: var(--markdown-line-height);
  overflow-wrap: anywhere;
}
._markdownContent_1q3nk_74 > :first-child { margin-top: 0; }
._markdownContent_1q3nk_74 > :last-child  { margin-bottom: 0; }
```

### 6.2 用户气泡内 markdown 覆写（CDP 实测类名）
```
[&>*:first-child]:mt-0                        /* 首元素 margin-top 0 */
[&_li+li]:!mt-0 [&_li>ol]:!mt-0 [&_li>p+p]:!mt-0 [&_li>ul]:!mt-0
[&_ol]:!ps-6 [&_ul]:!ps-6                     /* 列表 padding-inline-start 24px */
[&_p]:!m-0                                    /* 段落 margin 0（气泡内不留段距） */
[&_p+p]:!mt-5                                 /* 相邻段落 margin-top 20px */
```
（注意：`!mt-5` 在 Tailwind v4 = 20px；气泡内段落间分隔用 20px，与正文 11px 不同。无法确定这是有意设计还是新版本调整——以当前 DOM 为准记录。）

### 6.3 各元素边距/布局（全部 px 化）

| 元素 | 规则 |
|---|---|
| 段落 `_paragraph` / `_markdownBlock` | `margin: 0 0 11px`（0.6875rem）；块级 `display: flow-root` |
| 标题 `_heading` | `margin: 20px 0 10px`；`font-weight: 600`；`line-height: 1.25` |
| h1 | `font-size: 24px` |
| h2 | `font-size: 20px` |
| h3 | `font-size: 17px`（类名 `_heading3` 存在，值未单独取到；按 h4/h5/h6 梯度推断 17px，**待复核**） |
| h4 | `font-size: 17px; line-height: 22px` |
| h5 | `font-size: 16px`（推断，**待复核**） |
| h6 | `font-size: 15px; line-height: 20px` |
| 列表 `_list` | `margin: 0; padding-inline-start: 21px`（1.3125rem）；`list-style-position: outside`；嵌套时 `margin-top: 8px; margin-bottom: 0` |
| ul `_unorderedList` | `list-style-type: disc`（嵌套 circle） |
| ol `_orderedList` | `list-style-type: decimal` |
| 任务列表 `_taskList` | `padding-inline-start: 0; list-style-type: none` |
| 列表项 `_listItem` | `padding-inline-start: 2px`（0.125rem）；`display: list-item`；`margin-top: 8px`（0.5rem） |
| 引用 `_blockquote` | `margin: 0 0 8px`；`padding: 8px 0 8px 24px`；`line-height: 24px`；`position: relative`（左侧竖条由伪元素实现）；首/末子元素 margin 清零 |
| 行内代码 `_inlineMarkdown` | `background: color-mix(in srgb, list-hover-background 60%, foreground 6%)`；`font-family: mono`；`border-radius: 6px`；`padding: 1px 6px`；`font-size: 0.92em`；`display: inline`；`box-decoration-break: clone`；`overflow-wrap: anywhere; word-break: break-word` |
| 代码块 `_codeBlock` | `margin: 14px 0`（上下各 14px） |
| 代码块占位 `_codeBlockPlaceholder` | `border-radius: 12.5px`（`--radius-lg` electron 1.25 比例）；`background: text-code-block-background`；`font-size: var(--codex-chat-code-font-size)`；`padding: 8px`；`line-height: 20px`；`overflow-x: auto` |
| 小号 markdown 文本 `_markdownTextSmall` | `font-size: 12px; line-height: 20px` |
| 表格 `_table` | `width: fit-content; min-width: min(calc(100cqw − toolbar×2), calc(40rem − toolbar×2)); border-collapse: separate; border-spacing: 0; table-layout: auto; text-align: start; margin: 0; font-size: 14px` |
| 表格容器 `_tableContainer` | `width: calc(100% + 48px); margin-inline: -24px`（两侧出血 24px） |
| 表格滚动 `_tableScroller` | `overflow-x: auto; display: flex; justify-content: safe center; scrollbar-width: thin` |
| 表格包装 `_tableWrapper` | `width: fit-content; margin-inline: 24px; pointer-events: auto` |
| 表头单元格 `_tableHeaderCell` | `border-bottom: 1px solid border-medium; text-align: start; padding: 8px 0`；`font-weight: 600; line-height: 16px` |
| 单元格 `_tableCell` | `border-bottom: 1px solid border-light; vertical-align: top; white-space: normal; padding: 10px 24px 10px 0`（右内边距 24px，末列 0）；末行 `padding-bottom: 24px` |
| 数字单元格 `_numericTableCell` | 存在（对齐/等宽细节未展开） |
| 分隔线 `_horizontalRule` | `clear: both; border: 0; border-top: 1px solid border-medium; height: 0; margin: 28px 0` |
| 图片段落 `_mediaParagraph` | 存在（图片内联布局） |
| Mermaid `_mermaidBlock/_mermaidSurface` | 存在（代码块内 mermaid 渲染 + 展开按钮） |
| KaTeX | 不在此 module；bundle 内置 KaTeX 字体；display 公式 `margin: 14px 0`（dimi 侧对齐值，codex 侧**无法确定**——当前实例无公式可测） |

### 6.4 链接
- 链接颜色/下划线：markdown 链接用 `text-token-text-link-foreground`（token，非 module 内定义）；`hover:underline`。**无法确定**具体下划线样式细节（当前实例无链接可测）。

---

## 7. 设计 token 关联（非颜色表，仅结构相关）

- `--thread-content-max-width`：electron `48rem`；markdown 内 `40rem`；变体 42rem/100%/480px/500px。
- `--thread-content-margin`：默认 **24px**（宽表格出血量、`_tableWrapper` margin、`--thread-wide-block-inline-shift` 基准）；当前 electron 实例该变量未显式设置（用默认）。
- `--radius-2xl`：`calc(1rem * 1.25)` = **20px**（气泡圆角，非 tailwind 默认 16px）。
- `--radius-lg`：`calc(.625rem * 1.25)` = **12.5px**（代码块/卡片圆角）。
- `--codex-chat-font-size`：CDP 实测 **14px**；CSS 内有 16px/17px 变体类（extension/其他窗口），`--codex-chat-font-size:17px` 类存在但当前实例未应用。
- `--conversation-item-gap`：默认 16px（turn 内块分隔高度）。
- `--padding-toolbar`：16px（列壳侧内边距 `px-toolbar`）。

---

## 8. dimi 差距（对照 `.worktrees/native-client` 实现）

> dimi 文件：`apps/native-client/DimiNative/src/renderer/components/Transcript.vue`、`Transcript.styles.ts`、`renderer/markdown.ts`、`renderer/styles/global.ts`、`renderer/styles/theme.ts`（子代理已通读并核实）。

### 8.1 一致项（dimi 已对齐 codex）
- 列宽 736px：dimi `threadWrap maxWidth: 736px`（直设）≈ codex 768 − 16×2；**窄窗口行为不同**：codex 是 max-width + margin auto，窄于 768 时自适应收缩；dimi 直设 736px 会导致窄窗溢出或需另行处理。
- turn 间距 22px：dimi `thread gap 6 + turn padding 8×2 = 22px` —— 与 codex 完全一致。
- turn 内间距 12px：dimi `turnContent gap 12px` —— 一致。
- 用户气泡：dimi `maxWidth 77% / padding 8px 12px / radius 20 / bg rgba(255,255,255,0.05)` —— 与 codex 实测完全一致（含 `--radius-2xl` 20px 的 electron 覆写）。
- 用户消息 markdown `pre-wrap` + `text-align: start` —— 一致。
- markdown 数值：p 11px、标题 20/10 + 24/20/17/15 字号、列表 21px、li 8px、引用 8/24/24、hr 28、行内 code 1px 6px/radius 6/0.92em、pre 14px/8px/12.5px/12px/20px —— 与 codex CSS module 全部吻合（dimi 系按 codex 实测还原）。
- 思考按钮文案「思考了 Xs / Xm Ys」与格式 —— 一致。
- 操作行 `mt 6px / h 20px / gap 2px / electron translateX(-4px)` —— 一致。

### 8.2 差异项（dimi 与 codex 不符或未实现）
1. **assistant 操作行显隐**：dimi 注释称「codex build 无 opacity class、始终可见」并据此**常显**；但真实 codex DOM 是 `opacity-0 group-focus-within:opacity-100 group-hover:opacity-100`（默认 hover 显，仅 `alwaysShowActions` 场景常显）。**dimi 需要改成 hover 显**（或至少提供配置）。
2. **思考区展开动画**：codex 有 height+opacity 动画（300ms cubic-bezier(0.19,1,0.22,1)）+ 内容封顶 140px + 底部渐隐 + `pointer-events` 切换 + 实测高度测量；dimi 用 `v-if` 直接挂卸（注释明确放弃动画）。视觉近似但行为不同（无过渡、无高度测量）。
3. **编辑用户消息**：codex 支持双击气泡/编辑按钮进入编辑（draft store + composer 替换气泡）；dimi **完全未实现**（无编辑入口、无 draft、无重试）。
4. **回复优秀/不佳/继续聊天**：codex 有完整流程（评分分析事件 + 反馈对话框、fork 按钮 completed 才可用、spinner/禁用态）；dimi 三个按钮是**占位符，无 `@click`**。
5. **宽表格滚动**：codex `_tableScroller` 有 `overflow-x: auto` + `justify-content: safe center`；dimi 表格容器**无 overflowX**（超宽直接溢出）。dimi 需补横向滚动。
6. **代码块复制按钮**：codex 代码块占位带操作（`_codeBlockPlaceholder` + 复制能力）；dimi **未实现**代码块内复制。
7. **复制成功反馈**：codex 复制按钮图标变 check、1.5s 恢复；dimi 静默复制无反馈。
8. **发送时间戳**：codex assistant 操作行 hover 显示 sent time（`data-assistant-message-sent-time`）；dimi **无时间戳**。
9. **行内间距细节**：codex turn 内块间用 16px 分隔条（s8c `--conversation-item-gap`）；dimi 的 turn 内统一 12px gap，**无 16px 块分隔层**。
10. **用户气泡内 markdown 特殊覆写**：codex 气泡内 `[&_p]:!m-0 [&_p+p]:!mt-5 [&_ol/ul]:!ps-6 [&_li+li]:!mt-0`；dimi 用户气泡复用同一 `.md` 管线，无这些覆写（气泡内段落间距、列表缩进与 codex 不同）。
11. **字号行高**：codex `.markdownContent` 行高 = `font-size + 8px` = **22px**（14px 字号）；dimi `chatLh: 21px`（注释称「从 22 修正为 21」）。**与 codex 实测不符**（codex 为 22px）。
12. **虚拟化**：codex 虚拟列表 + `content-visibility: auto`（contain-intrinsic 240px）；dimi 全量渲染（`v-for`），长会话无虚拟化。
13. **无障碍**：codex turn 内有 `h4.sr-only` 角色标题（「你说：」等）；dimi 无。
14. **头像**：codex 用户消息在列表内有头像节点（`g2` role=user，位于气泡外）；dimi 用户消息**无头像**（仅气泡）。
15. **思考区工具卡片归属**：codex 工具活动是独立 item 块（在 turn 内与思考区分开、16px 分隔）；dimi 把工具卡片渲染在**思考区内**（`thinkingBlock > toolsCol`），与 codex 的结构不同（codex 的 reasoning item 只含推理文本，工具是 exec/mcp 独立卡片）。
16. **状态/模式 chips**：codex 用户消息下有「Review mode / PR fix / Auto resolve conflicts / N comments / References prior conversation」chips + 状态行（Hook blocked/Sent as goal）；dimi **未实现**这些 chips 与发送状态行。
17. **`displayMode` 死状态**：dimi store 保留 `displayMode` + `ExpandToggle` 消息但 Transcript 不消费（迁移遗留）；codex 的展开态在 turn 组件内按 item 类型自动决策。

### 8.3 dimi 侧备注（事实核对）
- dimi 操作行 4 按钮（复制/回复优秀/回复不佳/继续）常显、按钮 26×26、radius 10、hover 0.078/active 0.15 —— 尺寸与 codex 的 26×26 electron 按钮一致，但显隐规则需按 8.2-1 调整。
- dimi 用户复制行 `opacity 0→1, transition 0.12s ease` hover 显 —— 与 codex 用户操作行规则一致。
- dimi 无「无内容」占位、无「No content」状态。
- dimi welcome 页 736px 列内 `padding: 80px 16px 32px`（codex 空态页未纳入本文档范围）。

---

## 9. 验证记录

- 字节级源码切片：`app-initial-iBPGfcXU.js` 中组件函数 `W6`（TurnContent，@13025486）、`H$l`（Turn，@13238186）、`K$l`（turn 内容列表，@13252756）、`mAl`（ReasoningItem，@12953049）、`hsl`（UserMessage，@12116412）、`jvl`（AssistantMessageContent，@12397876）、`Fvl`（AssistantMessageActions，@12406057）、`Svl/wvl`（评分，@12389567）；`chatgpt-conversation-page` chunk 中 Thread 容器（`flex min-h-full flex-1 flex-col gap-1.5 py-5`）；`local-conversation-thread` chunk 中 `SD`（turn 包装）、`OD`（虚拟列表）、容器 `relative flex flex-col gap-3`。
- CSS：`app-initial-BSHZIbh1.css` markdown module 全套数值；`app-44wrUC9v.css` 中 `--thread-content-max-width:48rem`（electron body）、`--codex-chat-font-size`、`--radius-2xl/lg`。
- CDP 实测（`/tmp/cdp_eval.js`，运行实例）：
  - 容器类名 `flex min-h-full flex-1 flex-col gap-1.5 py-5`，gap 6px、padding 20px、父列 768px、内容 736px
  - turn `group flex flex-col py-2`；turn 内 `flex flex-col gap-3`
  - 用户气泡 `max-w-[77%] rounded-2xl px-3 py-2`，radius 20px、padding 8px 12px、bg oklab(1/0.05)、maxWidth 77%
  - 思考按钮 `inline-flex max-w-full min-w-0 cursor-interaction …` 104×21px，文案「思考了 2m 0s」，`aria-expanded=false`
  - 操作行 `mt-1.5 flex h-5 items-center justify-start gap-0.5 electron:-translate-x-1 …`，marginTop 6px、height 20px、gap 2px
  - 复制按钮 26×26px（electron p-1 + icon-sm）；评分按钮 24×24px
  - `.text-size-chat` computed 14px；`--codex-chat-font-size` 14px；body 16px
  - `--radius-2xl` = 1.25rem（20px）；`--radius-lg` = 0.625rem×1.25（12.5px）

## 10. 无法确定 / 待复核

- h3 与 h5 的精确字号（类名存在，CSS 未单独抓到；按梯度推断 h3=17px、h5=16px）。
- KaTeX display 公式在 codex 的精确 margin（当前实例无公式；dimi 用 14px 0 对齐）。
- 链接的精确 hover/下划线样式（当前实例无链接）。
- 思考摘要折叠态在按钮旁的内联展示形式（`u` 摘要变量存在，当前 DOM 未呈现折叠摘要）。
- `--conversation-item-gap` 是否被更上层 CSS 覆写（默认 16px，未发现覆写规则）。
- 时间戳（`lsl`）的文案格式（未展开；中文 locale 格式未知）。
- 头像（`g2`）的精确尺寸与样式（未展开）。
- 「思考了」文案的 elapsed 格式函数 `_Ms` 的具体实现（CDP 已见「思考了 2m 0s」，秒恒显）。
- 用户气泡内 `[&_p+p]:!mt-5`（20px）是否新版本设计（与正文 11px 段距差异明显，以当前 DOM 为准）。
