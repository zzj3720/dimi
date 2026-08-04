# 01 · Header（顶栏）设计逆向 · 源码版

> 本文档从 **codex（ChatGPT.app）解包 bundle 源码**逆向 Header 模块的真实设计，是 `01-header.md`（DOM 实测版）的**代码级补充与修正**。两者冲突时以本文档为准（本文档同时用运行中实例的 CDP DOM 复核了关键数值）。
>
> **源码位置**
> - `app-initial-iBPGfcXU.js`（15MB 主 JS）：AppShell Header 组件（`jqr`）、HeaderAction 注册（`CYr`）、左区按钮组（`tUr`）、标题（`P9l`）、Button 基类（`tm`）、thread-header 布局（`qfc`）、local conversation header 模块（`$q` / `T7o`：HeaderButton `b7o`、IconButton `z5o`）
> - `chatgpt-conversation-page-B-_0AAL7.js`（lazy chunk）：ChatGpt 会话页的 header 内容与右侧动作注册（分享 `Ec`、More `Dc`、固定摘要 `Is`、侧栏开关）
> - `app-initial-BSHZIbh1.css`：`_Header_khftr_1` 等 CSS module
> - `app-44wrUC9v.css`：Tailwind 工具类 + design tokens
> - `zh-CN-ClxN0P7N.js`：i18n 文案（aria-label 的真实来源）
>
> **复核方式**：`node /tmp/cdp_eval.js 9223 <pageId> '<expr>'`（CDP `Runtime.evaluate`，只读、不发 Input 事件）。当前实例 = 深色主题，窗口 1686×960，DPR 1，会话标题「论文arXiv 2605.28975解析」，sidebar 打开。

---

## 0. 总览：Header 是一个「注册式 slot 系统」，不是写死的组件

Codex 的顶栏由 **AppShell 层**（`app-initial` 里的 `AppShell.Header` / `jqr`）提供骨架，页面（ChatGpt 会话页等）通过 `<HeaderAction actionId align order slotPosition>` 把**按钮注册**进三个 store（center / left / right），骨架再按 `align` 分组渲染。这就是为什么实测 DOM 里左/右区出现 `data-test-id="header-shell-slot"` 的容器——它们是**可插拔的 slot**，宽度由 sidebar/right-panel 的动画宽度驱动。

### 0.1 Header 元素本身（CSS module `_Header_khftr_1`，全部经 CDP 复核）

| 属性 | 值 | 来源 |
|---|---|---|
| 元素 | `<header class="_Header_khftr_1">` | CSS module + 实测 |
| 高 | `var(--height-toolbar)` = **46px**（`--height-toolbar-sm` = 36px） | CSS `--height-toolbar:46px` + 实测 |
| 定位 | `position:fixed`；默认 `inset-inline:0; top:0` | CSS + 实测 |
| z-index | **30** | CSS + 实测 |
| 背景 | `var(--header-tint)` → 默认 `var(--codex-titlebar-tint, transparent)` = 透明 | CSS + 实测 `rgba(0,0,0,0)` |
| pointer-events | **none**（每个按钮靠 `pointer-events-auto` 恢复） | CSS + 实测 |
| 拖拽 | `-webkit-app-region:drag`；**`header button { -webkit-app-region:no-drag }`** | CSS |
| user-select | none | CSS + 实测 |
| display | flex；align-items:center；min-width:0 | CSS + 实测 |
| 无边框/阴影/backdrop | 无 | 实测 |

**两个 data 属性分支（条件样式）**：
- `[data-app-shell-header-edge-scroll=true]` → `--header-tint:transparent`（**强制透明背景**）。触发条件见 §7。
- `[data-app-shell-application-menu-bar=true]`（Win/Linux 有应用菜单时）→ `right:0; top:var(--height-toolbar-sm)`（36px），且 header 的 inline style 被设为 `{left: spring(leftPanelAnimatedWidth)}`——**header 整体右移一个 sidebar 宽**，顶部另有一条 36px 的 `_ApplicationMenuTopBar_1e9gb_2`（含 `tUr` 左组 + 菜单栏 `KLr`）。
- `[data-app-shell-application-menu-bar=false]` → `inset-inline:0; top:0`（macOS 实测路径）。

### 0.2 总布局（当前 sidebar 打开、窗口 1686px，实测）

```
0            88   120  152  180        275/283              ~476  494       1544   1610 1616 1644 1650 1678 1686
├────────────┼────┼────┼────┼──────────┼────────────────────┼─────┼────────┼──────┼────┼────┼────┼────┼──────┤
│ (空 88px)  │隐藏│返回│前进│           │ 标题按钮(289..476) │云   │ More   │ 分享 │    │摘要│    │切换│ (8px)│
│ safe-left  │    │    │    │ 左 slot   │ 主区 (flex-1)      │     │(494..) │ pill │    │固定│    │侧栏│      │
│            │    │    │    │ (275px)   │                    │     │        │      │    │摘要│    │    │      │
└────────────┴────┴────┴────┴──────────┴────────────────────┴─────┴────────┴──────┴────┴────┴────┴────┼──────┘
                                                                                                        │
                                         右 slot (w=36: 28 按钮 + pe-2 8px) ←────────────────────────────┘
```

---

## 1. 组件结构（层级树）

### 1.1 AppShell Header 骨架（`jqr`，app-initial @~4178000）

```jsx
<Sh items={dmr}>                       // 右键上下文菜单（注册的 HeaderContextMenuItem）
  <rf.header className="_Header_khftr_1"
             data-app-shell-application-menu-bar={t}
             data-app-shell-header-edge-scroll={e}
             style={t ? { left: spring(leftPanelAnimatedWidth) } : {}}>
    <Mqr entries={lmr} fitWidth={headerLeftWidth} slotWidth={leftPanelAnimatedWidth} side="start"/>
    <div aria-hidden={hidden}
         data-testid="app-shell-header-context-menu-surface"
         className="pointer-events-none relative ms-2 flex h-full min-w-0 flex-1 isolate
                    items-center gap-1.5 overflow-hidden [contain:layout_paint]
                    {hidden && 'invisible'} {rightEntries? 'pe-1.5' : 'pe-2'}">
      {centerContent &&                                     // ← AppShell.Header 内容（标题等），flex-1
        <div className="pointer-events-none w-full min-w-0 flex-1 [&_a/_button/_input/_select/_textarea]:pointer-events-auto">{centerContent}</div>}
      {startItems.length > 0 &&                             // align=start 条目（imr store）
        <div className="flex shrink-0 items-center gap-1.5">{startItems.map(Iqr)}</div>}
      {endItems.length > 0 &&                               // align=end 条目（imr store）
        <div className="ms-auto flex shrink-0 items-center gap-1.5">{endItems.map(Pqr)}</div>}
    </div>
    {centerItems.length > 0 &&                              // align=center 条目 → 绝对居中 overlay
      <div className="pointer-events-none fixed inset-x-0 top-[inherit] flex h-toolbar {invisible}">
        <div className="h-full shrink-0" style={{width: leftPanelAnimatedWidth}}/>
        <div className="flex min-w-0 flex-1 items-center justify-center gap-1.5">{centerItems}</div>
        <div className="h-full shrink-0" style={{width: rightPanelAnimatedWidth}}/>
      </div>}
    <Mqr entries={umr} fitWidth={headerRightWidth} slotWidth={rightPanelAnimatedWidth} side="end"/>
  </rf.header>
</Sh>
```

要点：
- **左/右 slot** 由 `Mqr` 渲染；**主区中央**内容由页面通过 `AppShell.Header`（`SYr`）写入 `Qpr` store 再被 `l` 读取。
- `imr`（center 槽内条目）、`amr`（left slot）、`omr`（right slot）是三个独立 store；`smr`/`dmr` 是右键菜单条目。`fmr = {center:imr, left:amr, right:omr}`。
- 每个条目包装层：start/center → `pointer-events-auto flex shrink-0 items-center no-drag`；end → `no-drag pointer-events-auto flex shrink-0 items-center`（第一个加 `ms-auto`）。

### 1.2 slot 容器 `Mqr`（左/右区外壳）

```jsx
function Mqr({entries, fitWidth, side, slotWidth}) {
  const hasEnd = entries.some(e => e.align === 'end');
  const pad = cx({
    'ps-[max(var(--spacing-token-safe-header-left),0.5rem)]': side === 'start',
    'pe-2': (side === 'start' && hasEnd) || side === 'end',
  });
  return <>
    {/* 不可见测量层：fixed 左上，测量条目自然宽度 → fitWidth.set(宽) */}
    <div aria-hidden className={cx('invisible pointer-events-none fixed top-0 left-0 min-w-max [&_*]:![view-transition-name:none]', pad)}>
      <Nqr entries={entries}/>
    </div>
    {/* 真实 slot：width=slotWidth, minWidth=测量宽 */}
    <rf.div data-test-id="header-shell-slot"
            className={cx('pointer-events-none relative h-full shrink-0 [container-type:inline-size]', pad)}
            style={{width: slotWidth, minWidth: `${fitWidth}px`}}>
      <Nqr entries={entries} fillSlot/>
    </rf.div>
  </>;
}
```

- 左 slot：`width = leftPanelAnimatedWidth`（sidebar 宽，spring 动画，默认 275、clamp 240–520），`minWidth = 条目自然宽`（实测 180 = 88 safe + 92 按钮组）。
- 右 slot：`width = rightPanelAnimatedWidth`（右面板宽，关闭时 0），`minWidth = 自然宽`（实测 36 = 28 按钮 + pe-2 8px）。
- **测量层机制**：`invisible fixed top-0 left-0 min-w-max` 渲染同一份条目，ResizeObserver 把自然宽度写回 `headerLeftWidth`/`headerRightWidth`，成为 slot 的 `minWidth`。实测 DOM 里 x=0 的「幽灵按钮」（88/120/152 与 0/1650 两组）就是测量层（01-header.md §1.0 的「不可见测量层」结论被源码证实，且**它渲染的是同一批按钮**，不是单独的菜单按钮）。

### 1.3 slot 内容 `Nqr`（按 align 分组）

```jsx
function Nqr({entries, fillSlot}) {
  const start = entries.filter(align==='start');
  const center = entries.filter(align==='center');
  const end = entries.filter(align==='end');
  const wrapper = cx('inline-flex h-full items-center gap-1.5',
                     fillSlot ? 'pointer-events-none w-full' : 'no-drag pointer-events-auto w-auto');
  return <div className={wrapper}>
    {start.map(Iqr)}                       // 每项: pointer-events-auto flex shrink-0 items-center no-drag
    {center.length > 0 && <div className="mx-auto flex shrink-0 items-center gap-1.5">{center.map(Fqr)}</div>}
    {end.map(Pqr)}                         // 每项: 第一个 ms-auto
  </div>;
}
```

### 1.4 左区按钮组（`tUr`，注册为 `slotPosition="left"` 的 `HeaderAction`，app-initial @~4107800）

```jsx
function tUr({hideUnreadBadge, onToggleSidebar}) {
  const sidebarVisible = useStore(FD);          // 侧栏是否可见
  const unread = useStore($Hr);
  const canBack = useStore(vIr), canForward = useStore(yIr);
  const icon = sidebarVisible ? <DRr/> : <wRr/>; // 隐藏边栏图标 / 显示边栏图标
  if (!hideUnreadBadge && !sidebarVisible && unread > 0) icon = <xRr/>;  // 未读角标图标
  return <div className="flex items-center gap-1">   {/* gap-1 = 4px */}
    <iUr ariaLabel={sidebarVisible ? '隐藏边栏' : '显示边栏'}   // app.sidebar.hide / app.sidebar.show
         shortcut={toggleSidebar快捷键} tooltipContent="切换边栏"  // app.sidebar.tooltip
         viewTransitionName="sidebar-trigger" isSidebarTrigger
         onClick={toggleSidebar} onPointerEnter={previewOn} onPointerLeave={previewOff}>
      {icon}   {/* className="icon-xs" */}
    </iUr>
    {/* back/forward 仅 electron + extension 渲染 */}
    <ak electron extension>
      <iUr ariaLabel="返回" disabled={!canBack} shortcut={navigateBack} onClick={navigateBackCmd}>{backIcon}</iUr>
      <iUr ariaLabel="前进" disabled={!canForward} shortcut={navigateForward} onClick={navigateForwardCmd}>{forwardIcon}</iUr>
    </ak>
  </div>;
}
```

- 每个 `iUr` = `tm` Button（`color=ghost, size=toolbar, uniform`）+ `vh` Tooltip（`delayOpen` 默认、带快捷键提示）。
- 图标：隐藏侧栏 `DRr` / 显示侧栏 `wRr`（viewBox 0 0 20 20，fill none）；未读态 `xRr`（viewBox 0 0 16 16）。前进图标 = 返回图标 `-scale-x-100`（`-scale-x-100{scale:-1 1}`）。

### 1.5 主区中央内容：thread-header（`qfc`/`Xt`，app-initial @~11254000）

```jsx
function qfc({start: title, startActions, env, project, secondary, trailing, hostConfig}) {
  const titleRow =
    <div className="flex min-w-0 items-center gap-0.5 {project==null && 'ps-2'}">   {/* p */}
      {project && <Jfc project={project}/>}          // 项目 pill（ghostActive toolbar uniform 按钮 + hover card）
      {title && <div className="max-w-[320px] min-w-0 truncate focus-within:overflow-visible">{title}</div>}
    </div>;
  const envIcon = env ? (remote? hostIcon : worktree? Vfc : cloud? Hfc : local Ufc) : null;  // icon-2xs (14px)
  const actions =
    <div className="flex min-w-0 items-center gap-1">{[envIcon, secondary, startActions]}</div>;  // g, gap-1 = 4px
  const col1 = <div className="text-md flex min-w-0 items-center gap-0 truncate text-base
                               focus-within:overflow-visible electron:font-medium">{titleRow}{actions}</div>;  // _
  const col2 = <div className="flex items-center justify-end gap-1.5">{trailing}</div>;                      // y
  return <div className="grid w-full min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4
                         draggable electron:h-toolbar extension:py-row-y">{col1}{col2}</div>;                 // grid
}
```

- 根 grid：两列 `minmax(0,1fr) auto`，列间距 `gap-x-4` = 16px；`electron:h-toolbar` = 46px 高；`draggable`（可拖拽窗口）。
- 实测（CDP）：grid @x=283 w=1255；col1 `_` @283 w=1239；标题按钮 @289 w=187；More @494（在 `g` 内，`flex items-center gap-1` 4px）。
- **修正 01-header.md**：More 在 **col1 内**（title 后 `gap-1` 4px，中间隔着 env 图标 14px + 4px），**不是** grid col2（16px 列距）。实测 x：476(标题右缘) + 14(云图标) + 4(gap) = 494。无 env 图标时会话 More 应在 476+4 = 480。

### 1.6 标题（`P9l`/`En`，app-initial @~13466000，按是否可重命名分三支）

```jsx
function P9l({className, onRename, title, titleValue}) {
  if (onRename == null || titleValue == null)   // 不可重命名 → 纯 div
    return <div className={cx('-ms-0.5 min-w-0 truncate px-1.5 text-base leading-6 font-medium text-token-foreground', className)}>{title}</div>;
  // 可重命名（ChatGpt 页实际路径）→ button，点击进入内联重命名
  const btn = cx('no-drag -ms-0.5 min-w-0 cursor-interaction truncate rounded-md px-1.5 text-left
                 text-base leading-6 font-medium text-token-foreground
                 hover:bg-token-list-hover-background focus-visible:bg-token-list-hover-background focus-visible:outline-none', className);
  return <button type="button" className={btn} onClick={() => setEditing(true)}>{title}</button>;
  // editing → input: 'no-drag -ms-0.5 h-6 min-w-0 rounded-md border border-token-focus-border
  //                   bg-token-input-background px-1.5 text-base leading-6 font-medium text-token-input-foreground outline-none'
  //         Enter 提交 / Esc 取消 / Blur 提交
}
```

- ChatGpt 页调用：`<En className="max-w-[320px]" onRename={...} title={P} titleValue={P}/>` → **按钮分支 + max-w-[320px]**。实测 class 全中（§4）。
- **点击行为 = 内联重命名**（01-header.md 曾「无法观察」，源码确定）。

### 1.7 右区按钮（chatgpt chunk，`HeaderAction align="end"` 注册 + 中央内容内的动作）

| 按钮 | 注册方式 | actionId / order | 组件 | 说明 |
|---|---|---|---|---|
| More | **不是 HeaderAction**：作为 `x.Header` 中央内容的 `startActions` 渲染（col1 内，标题后） | — | `<B size="icon" color="ghost" className="no-drag" aria-label="ChatGPT 对话操作">` + ellipsis `icon-sm` | 对话操作下拉（Dc） |
| 分享 | `HeaderAction` | `chatgpt-conversation-share` / order 100 / align end | `Ec`：`<B size="toolbar" color="ghost" className="enabled:text-token-text-primary enabled:hover:text-token-text-primary">` + `icon-xs` + `<span className="hidden electron:inline">分享</span>` | 分享对话框 |
| 固定摘要 | `HeaderAction` | `chatgpt-thread-summary-panel-toggle` / order 250 / align end | `Is`：`<HeaderButton label="切换固定摘要" pressed={isPinned} shortcut onClick>`（b7o，默认 w5o 图标 `icon-xs`） | 钉住摘要面板 |
| 侧栏开关 | `HeaderAction` | `chatgpt-conversation-side-panel` / order 300 / align end + slotPosition right | `<HeaderButton label="切换侧边栏" pressed={isOpen} onClick>` + columns 图标 `icon-xs rotate-180` | 切换右侧浏览器面板 |

- 右 slot 内按 order 排序渲染（100 → 250 → 300）。
- 分享 disabled：会话未加载完成 / streaming 时 `disabled:true`（tooltip `shareDisabled.notReady` / `shareDisabled.streaming`），此时走 Button 通用 disabled（opacity 40%）。
- **条件渲染**：页面组件 `fc({conversationId, isTemporaryChat: a, sharedConversationId: o})` 里 `c = o != null`（**共享会话副本**）。分享在 `a || c`（临时会话或共享会话）时不注册；More 与 cloud 指示在 `c`（共享会话）时不渲染（`startActions: c ? null : …`）；handoff pending 时 More 仍渲染但传 `isHandoffPending`。
- 固定摘要切换逻辑在 displayMode==='overlay' 时变成 popover 触发器。

### 1.8 Button 基类（`tm`，所有 header 按钮的底座）

```js
const className = cx(
  'no-drag cursor-interaction items-center gap-1 border whitespace-nowrap select-none
   focus:outline-none disabled:cursor-not-allowed disabled:opacity-40',
  layout==='balanced' ? 'grid grid-cols-[1fr_auto_1fr]' : 'flex',
  size==='large' ? 'rounded-lg' : H7e[size],   // 圆角按 size
  $7e[color],                                   // 配色按 color
  e9e[size],                                    // 尺寸按 size
  minWidth && 'min-w-0',
  uniform && 'aspect-square shrink-0 items-center justify-center !px-0',
  className);
```

**变体字典（H7e 圆角 / e9e 尺寸 / $7e 配色）**——header 相关的取值：

| size | 圆角 | 尺寸 class |
|---|---|---|
| `toolbar` | `rounded-lg`（12.5px） | `h-token-button-composer px-2 py-0 text-base leading-[18px]`（高 28px） |
| `icon` | `rounded-full electron:rounded-md`（web pill 9999 / electron 10px） | `electron:p-1 electron:[&>svg]:icon-sm flex items-center justify-center p-0.5` |

| color | class |
|---|---|
| `ghost` | `text-token-text-tertiary enabled:hover:bg-token-list-hover-background enabled:active:bg-token-foreground/15 data-[state=open]:bg-token-list-hover-background border-transparent` |
| `ghostActive` | `text-token-foreground enabled:hover:bg-token-list-hover-background enabled:active:bg-token-foreground/15 data-[state=open]:bg-token-list-hover-background border-transparent` |
| `secondary` | `text-token-foreground bg-token-foreground/5 enabled:hover:bg-token-foreground/10 enabled:active:bg-token-foreground/15 data-[state=open]:bg-token-foreground/10 border-transparent` |

| 附加 prop | class |
|---|---|
| `uniform` | `aspect-square shrink-0 items-center justify-center !px-0`（28×28 方按钮） |
| `disabled` | `disabled:cursor-not-allowed disabled:opacity-40`（opacity 0.4） |
| 焦点 | `focus:outline-none`（无可见 focus 环） |

**HeaderButton `b7o`（固定摘要/侧栏开关用）** = `tm` size `toolbar` + `pressed ? 'secondary' : 'ghost'` + `aria-label` + `aria-pressed` + `title` + `uniform`，默认子元素 `w5o` 图标（3 点列表）`icon-xs`，外包 Tooltip（`delayOpen` + shortcut）。

**IconButton `z5o`（local conversation header 模块）** = `tm` size `icon` + `size-token-button-composer`（28×28）+ `focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-token-focus-border`，图标容器 `icon-sm flex items-center justify-center [&_svg]:size-full`。

---

## 2. 布局规则（数值汇总，全部经 CSS token 解析 + CDP 复核）

| 项 | 值 | 来源 |
|---|---|---|
| header 高 | 46px（`--height-toolbar`） | CSS |
| 左 slot 宽 | sidebar 动画宽（默认 275，clamp 240–520；`xmr(e)=min(max(e,240),520)`，localStorage `sidebar-width`） | `wmr`/`Uqr` |
| 左 slot padding-left | `max(var(--spacing-token-safe-header-left), 0.5rem)` = max(88,8) = **88px** | `Mqr` + 实测 |
| 左 slot padding-right | `pe-2`（8px，仅当左 slot 内有 align=end 条目；实测有） | `Mqr` |
| 左 slot minWidth | 测量自然宽 = 180px（88 safe + 92 按钮组） | 实测 |
| 主区 margin-left | `ms-2` = 8px（x=283 = 275+8） | `jqr` + 实测 |
| 主区 flex | `flex-1 min-w-0`；`gap-1.5`（6px）；右条目存在时 `pe-1.5`（6px）否则 `pe-2` | `jqr` + 实测 |
| 右 slot 宽 | rightPanel 动画宽（关闭时 0）→ 实际显示 minWidth 36px（28 按钮 + pe-2 8px） | `Mqr` + 实测 |
| 左按钮组 | `flex items-center gap-1`（gap 4px）；条目容器 `inline-flex h-full items-center gap-1.5`（gap 6px） | `tUr`/`Nqr` + 实测 |
| 标题 | `-ms-0.5`（margin-left −2px）、`px-1.5`（padding 0 6px）、`max-w-[320px]`、高 24px（leading-6） | `P9l` + 实测 |
| thread-header grid | `grid-cols-[minmax(0,1fr)_auto] gap-x-4`（16px）；`electron:h-toolbar`（46px） | `qfc` + 实测 |
| 标题行内 gap | 标题/project `gap-0.5`（2px）；env/actions `gap-1`（4px）；col1 内 `gap-0`（0） | `qfc` |
| 分享 pill | `h-token-button-composer`（28 高）+ `px-2`（0 8px）+ `gap-1`（icon-text 4px） | `Ec`/`tm` + 实测 |
| More 按钮 | size `icon`：electron `p-1`（4px），圆角 `rounded-md`（10px） | `Dc`/`tm` + 实测 |

---

## 3. 边距 / 内距（逐元素 px）

| 元素 | 值 |
|---|---|
| header | padding 0 |
| 左 slot 容器 | padding-inline-start 88px（`ps-[max(--spacing-token-safe-header-left,0.5rem)]`），padding-inline-end 8px |
| 主区容器 | margin-left 8px（ms-2），padding-inline-end 6px（pe-1.5） |
| 右 slot 容器 | padding-inline-end 8px（pe-2） |
| 左按钮组（tUr） | 内部 gap 4px（gap-1） |
| 条目外层（Nqr/Iqr） | gap 6px（gap-1.5） |
| 标题按钮 | margin-left −2px（-ms-0.5）；padding 0 6px（px-1.5）；圆角 10px（rounded-md） |
| More 按钮 | padding 4px（electron:p-1） |
| 分享 pill | padding 0 8px（px-2）；icon↔文字 gap 4px（gap-1） |
| 固定摘要/侧栏开关 | padding 0（uniform !px-0） |
| 标题行 project/title gap | 2px（gap-0.5）；无 project 时 title 前 ps-2（8px） |
| env 图标↔More | 4px（gap-1） |
| grid 列间距 | 16px（gap-x-4） |
| cloud 图标（env） | icon-2xs 14px，`translate-x-px`（+1px），`text-token-description-foreground` |

---

## 4. 透明度 / 视觉态

> 所有 token 均为 VS Code 主题变量（`--vscode-*`），深色主题实测值见下表。hover/active 用 `enabled:` 变体 = **disabled 时完全不触发**。transition 实测 `all 0s`（**无过渡动画**，见 §6.1）。

| 状态 | 背景 | 文字/图标颜色 | 来源 |
|---|---|---|---|
| ghost default | transparent | `text-token-text-tertiary` = `rgba(255,255,255,0.498)` | $7e + 实测 |
| ghost hover | `bg-token-list-hover-background` = `--vscode-list-hoverBackground` ≈ **白 8%** | 不变 | $7e + 实测 |
| ghost active | `bg-token-foreground/15` = `color-mix(in oklab, foreground 15%, transparent)` ≈ **白 15%** | 不变 | $7e + 实测 |
| ghost `data-[state=open]`（菜单开） | 同 hover（白 8%） | 不变 | $7e |
| ghostActive default | transparent | `text-token-foreground` = 白 | $7e |
| secondary default（pressed 固定摘要） | `bg-token-foreground/5` ≈ **白 5%**（实测 `oklab(0.999994 …/0.05)`） | 白 | $7e + 实测 |
| secondary hover | `bg-token-foreground/10` ≈ 白 10% | 白 | $7e |
| secondary active | `bg-token-foreground/15` ≈ 白 15% | 白 | $7e |
| disabled | — | opacity **0.4** + `cursor:not-allowed` | tm 基类 + 实测（前进按钮） |
| 标题 hover | `hover:bg-token-list-hover-background`（白 8%） | 白（保持） | P9l + 实测 token |
| 标题 focus-visible | 同 hover（白 8%）+ `outline-none` | 白 | P9l |
| 分享 hover | 白 8%；文字 class `enabled:hover:text-token-text-primary` 保持纯白 | 白 | Ec/class |
| 分享 active | 白 15% | 白 | tm |
| 分享 disabled | opacity 0.4 | 白（class 覆盖为 primary） | tm + class |
| cursor（enabled） | `cursor-interaction` → electron 下 computed `default`（不是 pointer） | — | tm + 实测 |

---

## 5. 字号 / 字重 / 行高

| 元素 | 字号 | 字重 | 行高 | 来源 |
|---|---|---|---|---|
| toolbar 按钮（隐藏/返回/前进/分享/固定摘要/侧栏） | 14px（`text-base`，本 app `--text-base:14px`） | **445**（继承 `--vscode-font-weight` 变量字体默认） | 18px（`leading-[18px]`） | e9e + 实测 |
| 标题 | 14px | **500**（`font-medium`） | 24px（`leading-6` = `calc(var(--spacing)*6)`） | P9l + 实测 |
| 标题 input（重命名态） | 14px | 500 | 24px | P9l |
| 分享文字 | 14px | 445 | 18px | 实测 |
| env/secondary 文字（若存在） | `text-size-chat` 或继承 | normal | 18px | qfc |
| 字体族 | `-apple-system, "system-ui", "Segoe UI", sans-serif`（`--font-sans`，无 PingFang） | — | — | 实测（01-header.md §0） |

---

## 6. 行为逻辑

### 6.1 无过渡动画
Button 无 `transition` 类；标题无 transition；实测 computed `transition: all 0s`。hover/active/pressed 全部**即时切换**。

### 6.2 sidebar 触发按钮（隐藏/显示边栏）
- aria-label 随 `FD` store：可见 → `app.sidebar.hide`「隐藏边栏」；隐藏 → `app.sidebar.show`「显示边栏」。
- 图标随状态：可见 → `DRr`（面板收起）；隐藏 → `wRr`（面板展开）；**隐藏且有未读** → `xRr`（16×16 未读图标，`hideUnreadBadge` 可关）。
- `onPointerEnter` → `FD.set(ID, true)`、`onPointerLeave` → `false`（hover 预览边栏），unmount 时 cleanup 置 false。
- 点击 → `dispatchCommand('toggleSidebar','sidebar_trigger')`（有 `onToggleSidebar` 时优先）。
- tooltip `app.sidebar.tooltip`「切换边栏」；`viewTransitionName="sidebar-trigger"`；`data-app-shell-sidebar-trigger` 属性。
- 同一组件在应用菜单栏模式（Win/Linux）下出现在 36px 的 `ApplicationMenuTopBar` 里。

### 6.3 返回 / 前进
- `disabled = !canBack / !canForward`（vIr / yIr 信号，真实历史栈）。
- 点击 → `dispatchCommand('navigateBack','sidebar_back')` / `navigateForward`。
- **仅 electron + extension 渲染**（`<ak electron extension>`）；web 端整组不出现。
- 前进图标 = 返回图标 `-scale-x-100`（scale −1 1，无 rotate）。

### 6.4 标题
- 可重命名（ChatGpt 页）：点击 → 内联 `<input>`（`h-6`、`rounded-md`、`border border-token-focus-border`、`bg-token-input-background`）；Enter/Blur 提交 `onRename`，Esc 取消；自动全选。
- 不可重命名：纯 div，无 hover、无点击。
- 外层 `max-w-[320px]`、`truncate`；标题行容器 `focus-within:overflow-visible`（重命名时允许溢出）。

### 6.5 More（对话操作）
- 点击 → 打开对话操作菜单（pin / 在新窗口打开 / debug panel / handoff / 更多），`data-[state=open]` 时背景 = hover（白 8%）。

### 6.6 分享
- disabled 条件：会话未加载完成（`shareDisabled.notReady`）或 streaming（`shareDisabled.streaming`），tooltip 相应文案。
- **文字「分享」只在 electron 显示**（`hidden electron:inline`）；web 端仅图标。
- 点击 → 分享对话框（创建公开链接等）。

### 6.7 固定摘要（pinned summary）
- `pressed={isPinned}` → color `secondary`（白 5% 底）；`aria-pressed` 同步。
- displayMode `overlay` 时变 Popover 触发器；`pinned` 时保留布局空间。默认图标 `w5o`（3 点列表）。

### 6.8 侧栏开关（切换右侧浏览器面板）
- `pressed={isOpen}` + columns 图标 `rotate-180`（`rotate:180deg`）；`onClick` 展开/折叠右侧浏览器面板。

### 6.9 header 右键菜单
- 整个 header 被 `Sh` 包裹：收集 `HeaderContextMenuItem`（smr/dmr）→ 有 `electronBridge.showContextMenu` 时弹原生菜单，否则自定义菜单。

### 6.10 edge-scroll（标题栏透明）
`isHeaderEdgeScroll = layout==='thread-edge-scroll' && mainContentWidth >= 96rem(1536px) && !hidden` → header `data-app-shell-header-edge-scroll=true` → tint 透明。

### 6.11 安全区（macOS 红绿灯）
`--spacing-token-safe-header-left = windowControlsOverlay.left / zoom + 6px`（实测 88px）；`--spacing-token-safe-header-right = right / zoom`（实测 0）。mac 平台常量兜底：`HA.mac.legacy {left:82}`、`HA.mac.modern {left:92}`（没有 WCO 时的 fallback）。

### 6.12 隐藏态
`DD` store 为 true 时主区与 center overlay 加 `invisible`（视觉隐藏但保留布局）。

---

## 7. 条件逻辑（哪些 class 在什么条件下加）

| 条件 | 加的 class / 行为 |
|---|---|
| sidebar 可见（FD=true） | 左组图标=DRr，aria=「隐藏边栏」；不可见→wRr/「显示边栏」 |
| sidebar 隐藏 + 有未读 + 未 hideUnreadBadge | 图标=xRr（未读角标） |
| 历史栈 canBack=false | 返回按钮 `disabled`（opacity 40 + not-allowed） |
| 历史栈 canForward=false | 前进按钮 `disabled`（实测当前会话） |
| 非 electron/extension | 返回/前进整组不渲染 |
| align=end 条目存在于左 slot | 左 slot 加 `pe-2` |
| 右 slot 有条目 | 主区 `pe-1.5`（否则 `pe-2`） |
| 主区隐藏（DD=true） | 主区 + center overlay `invisible` |
| right 面板开（rightPanelAnimatedWidth>0） | 右 slot width>0（否则塌到 minWidth 36px） |
| app-menu-bar 开 | header `right:0; top:36px; left:sidebar宽`；另渲染 36px ApplicationMenuTopBar |
| thread-edge-scroll + 主区宽≥1536 + 非隐藏 | `data-app-shell-header-edge-scroll=true` → tint 透明 |
| 标题可重命名 | button 分支（hover 白 8%、rounded-md、click→rename）；不可重命名→div |
| 标题编辑中 | div 变 input（focus-border、input-background、h-6） |
| 分享 disabled（未加载/streaming） | Button disabled（opacity 40）；tooltip 文案切换 |
| 分享文字 | `hidden electron:inline`（web 隐藏） |
| 固定摘要 pressed | color=secondary（白 5%/10%/15% 底）+ `aria-pressed` |
| summary displayMode=overlay | 按钮变 Popover 触发器 |
| 临时会话（isTemporaryChat=`a`）或共享会话（`c`） | 分享不注册（`a \|\| c`）；More/cloud 指示在共享会话时不渲染（`startActions: c ? null : …`） |
| 侧栏 pressed | `aria-pressed=true` + secondary 底 |

---

## 8. 图标系统（header 相关）

| 按钮 | 图标组件 | svg class | 渲染尺寸 | viewBox | 说明 |
|---|---|---|---|---|---|
| 隐藏边栏 | DRr | `icon-xs` | 16×16 | 0 0 20 20 | fill none |
| 显示边栏 | wRr | `icon-xs` | 16×16 | 0 0 20 20 | 与 DRr 镜像 |
| 未读角标 | xRr | `icon-xs` | 16×16 | **0 0 16 16** | 尺寸不同 |
| 返回 | back | `icon-xs` | 16×16 | 0 0 20 20 | — |
| 前进 | back | `icon-xs -scale-x-100` | 16×16 | 0 0 20 20 | 同一 path，scale −1 1 |
| 标题 | 无 | — | — | — | 纯文字 |
| More | ellipsis | `icon-sm` | **18×18** | 0 0 21 21 | 3 圆点 |
| 分享 | share | `icon-xs` | 16×16 | 0 0 20 20 | — |
| 固定摘要 | w5o | `icon-xs` | 16×16 | 0 0 20 20 | 3 点竖排列表 |
| 侧栏开关 | columns | `icon-xs rotate-180` | 16×16 | 0 0 20 20 | `rotate:180deg` |
| cloud 指示 | 云图标 | `icon-2xs` | 14×14 | 0 0 16 16 | `translate-x-px` |

- 尺寸类（CSS）：`icon-3xs` 10 / `icon-xxs` 12 / `icon-2xs` 14 / `icon-xs` 16 / `icon-sm` 18 / `icon-base` 20 / `icon-md` 24 / `icon-lg` 28（渲染尺寸由 class 决定，与 svg width/height 属性无关）。
- 与 dimi `icons.ts` 对照：`hideSidebar` ✓、`back` ✓、`forward`（=back path）✓、`share` ✓、`ellipsis` ✓、`dots` ✓（= codex w5o，**与固定摘要图标同 path**）、`menu` ✓（= columns 图标，代码里 `rotate(180deg)`）。01-header.md 说「固定摘要 icon path 未捕获」已过时——dimi 的 `dots` 就是它。

---

## 9. 与 01-header.md（DOM 实测版）的差异 / 修正

| # | 01-header.md 说法 | 源码事实 | 结论 |
|---|---|---|---|
| C1 | 左区「组容器 `inline-flex h-full items-center gap-1.5`」「组内按钮容器 `flex items-center gap-1`」 | 完全一致：Nqr wrapper + tUr 组 | ✓ 源码证实 |
| C2 | x=0「不可见测量层内含 切换侧边栏 按钮」 | 测量层渲染的是**同一批左/右条目**（含左组与右组），不是额外菜单按钮；`Mqr` 的 `invisible fixed top-0 left-0 min-w-max` | 修正语义 |
| C3 | More 位置「标题右缘+grid gap 16px ≈ 18px」 | More 在 **col1 内** startActions（`g`：gap-1 4px），当前实测 494 = 476+14(云图标)+4；无云图标时 = 480 | 修正（不是 col2/16px） |
| C4 | 标题点击行为「无法观察」 | 点击 → **内联重命名**（input 模式） | 源码确定 |
| C5 | 前进 disabled 原因「无前进历史」 | `canForward` 来自历史栈信号（vIr/yIr），点击 dispatch `navigateForward` | 源码确定（不是 dimi 的列表边界） |
| C6 | 「隐藏 sidebar 后是否出现菜单按钮无法观察」 | 左组第一按钮始终是 sidebar-trigger（tUr），**没有 x=0 独立菜单按钮**；aria/图标随 FD 切换 | 源码确定 |
| C7 | 分享 pill「文字 x=1573…」 | 文字 `hidden electron:inline`（仅 electron 显示） | 补充 |
| C8 | 固定摘要图标未捕获 | = dimi `dots` path（w5o） | 更新 |
| C9 | header 圆角 token「rounded-lg→12.5px、rounded-md→10px」 | `--radius-lg-base:.625rem × scale 1.25 = 12.5px`；`--radius-md-base:.5rem × 1.25 = 10px`；`--codex-corner-radius-scale:1.25` | ✓ token 级证实 |
| C10 | 字体族无 PingFang | `--font-sans` = `-apple-system, "system-ui", "Segoe UI", sans-serif` | ✓ |
| C11 | 01-header.md 未记录 app-menu-bar 模式 | Win/Linux 有应用菜单时 header 下移 36px 并右移 sidebar 宽；顶部另有一条 36px 菜单栏 | 新增 |
| C12 | 01-header.md 未记录 edge-scroll 透明 | `thread-edge-scroll && 主区宽≥1536 && !hidden` → tint 透明 | 新增 |

---

## 10. dimi 差距（dimi `HeaderBar.vue` + `HeaderBar.styles.ts` + `styles/theme.ts` + `icons.ts`）

> dimi 文件：`apps/native-client/DimiNative/src/renderer/components/HeaderBar.vue`、`HeaderBar.styles.ts`、`styles/theme.ts`、`icons.ts`。

### 10.1 结构 / 布局差异

| # | 项 | codex（源码事实） | dimi 当前 | 影响 |
|---|---|---|---|---|
| L1 | 左区安全距 | `ps-[max(var(--spacing-token-safe-header-left),0.5rem)]`，token = 窗口控制钮 left/zoom+6 = **88px**（随窗口位置/平台变化） | `paddingLeft: 88` 硬编码 | 语义不同：dimi 固定 88，不随 macOS 红绿灯位置/缩放/平台（Win 无红绿灯时应为 0+8）自适应 |
| L2 | 左区菜单按钮 | **不存在**；第一个按钮就是 sidebar-trigger（aria 随 FD 变「隐藏/显示边栏」） | dimi 渲染 menu 按钮 @x=0（`title="Menu"`）再 `marginLeft:56` 推组到 x=88 | dimi 多一个按钮；且 margin 56 是硬算（codex 是 padding 88 在 slot 上） |
| L3 | 右区最右按钮 | 「切换侧边栏」28×28（columns icon rotate-180，注册 order 300） | **没有**；`headerRight` 用 `paddingRight:42` 留空白 | 缺按钮 + 空白语义错误 |
| L4 | 固定摘要按钮 | 有（order 250，pressed 态白 5% 底，`aria-pressed` + tooltip + shortcut） | dimi 在 x=1616 放「Refresh」占位（icons.dots） | 行为与 pressed 态均不同 |
| L5 | 右区间距 | slot `pe-2`（8px）+ 条目间 `gap-1.5`（6px）；右 slot 宽由 rightPanel 驱动 | gap 6 ✓，但 `paddingRight:42`（应为 8px + 一个 28px 按钮位） | 右缘留白语义错误 |
| L6 | 主区左距 | `ms-2`（8px）+ 标题 `-ms-0.5`（−2px）→ 标题 x=289 | `paddingLeft:14` → 标题 x=297 | 差 8px |
| L7 | 主区 gap | `gap-1.5`（6px）；thread-header grid `gap-x-4`（16px） | headerMain gap 16 ✓（title↔More）；但 codex 的 More 实际在 col1 内 gap-1（4px，隔着 env 图标） | More 相对标题的位置规则不同 |
| L8 | 标题行 | qfc grid 结构（col1: title+env+actions，col2: trailing）；`electron:h-toolbar` 46px | dimi 无 env 图标 / 无 col2 | 缺 env/cloud 指示器位 |
| L9 | 测量层 | 每 slot 有 invisible 测量层 → minWidth = 自然宽 | 无 | 窄内容/多按钮时 codex 保证 slot 不小于内容宽 |

### 10.2 样式差异

| # | 项 | codex | dimi 当前 | 影响 |
|---|---|---|---|---|
| S1 | 图标按钮圆角 | `rounded-lg` = **12.5px** | `borderRadius:10`（theme `iconBtnRadius:'12.5px'` 存在但 HeaderBar 未用） | 圆角不对 |
| S2 | 图标按钮 hover 背景 | `bg-token-list-hover-background` ≈ **白 8%** | `colors.hover` = 白 5% | 亮度不足 |
| S3 | 图标按钮 hover 颜色 | **不变**（ghost 保持 tertiary 0.498） | `'&:hover': { color: colors.text }` → 变纯白 | 与 codex 不符 |
| S4 | active 背景 | `bg-token-foreground/15`（白 15%） | `rgba(255,255,255,0.15)` ✓ | 一致（仅 ghost 有） |
| S5 | 字重 | 445（继承变量字体默认） | 未设（继承） | 在非变量字体环境会偏差 |
| S6 | cursor | enabled = `default`（cursor-interaction 在 electron computed 为 default）；disabled = `not-allowed` | `cursor:pointer` | 鼠标样式不符 |
| S7 | 分享 pill 圆角 | `rounded-lg` 12.5px | `borderRadius:9999` | 形状不符 |
| S8 | 分享 pill padding | `px-2` = 0 8px | `0 10px` | 宽 66 vs dimi 74 |
| S9 | 分享字号 | `text-base` 14px / 445 / 18px | `font.sm` 13px | 字号不符 |
| S10 | More 边框 | `border-transparent`（无可见边框） | `iconBtnBordered` → `borderColor: rgba(255,255,255,0.08)` | 多一条可见边框 |
| S11 | More padding/圆角 | electron `p-1` 4px + `rounded-md` 10px | 未设（flex center） | 点击/视觉差异 |
| S12 | 标题元素 | **`<button>`**（可重命名，hover 白 8% 底、rounded-md 10px、px-1.5、-ms-0.5、max-w-320） | `<button>` ✓（dimi 已改成 button，但 class 是 `headerTitle` css：hover `colors.hover8` ✓、radius 10 ✓、padding 0 6px ✓、maxWidth 320 ✓） | 基本一致；缺口在 dimi 点击行为 = 打开 picker，codex = 内联重命名 |
| S13 | 标题字号/行高/字重 | 14 / 24 / 500 | 14 / 24 / 500 ✓ | 一致 |
| S14 | 前进 disabled | `opacity:0.4; cursor:not-allowed` + `disabled:true`（历史栈信号） | `'&:disabled': { opacity:0.4 }` + `navSession` 列表循环 | 视觉 ✓；disabled 来源不同（dimi 用列表边界模拟，可接受） |
| S15 | transition | 无（`all 0s`） | 无显式 transition（0s） | 一致 |
| S16 | 字体族 | `-apple-system, "system-ui", "Segoe UI", sans-serif` | 多了 PingFang SC / Microsoft YaHei | 中文回退差异（dimi 可保留） |
| S17 | header pointer-events | 整体 none，按钮 auto；`-webkit-app-region: drag`（按钮 no-drag） | `pointerEvents:'none'` + 按钮 auto ✓，但 **无 app-region drag/no-drag** | macOS 拖拽行为缺失 |
| S18 | 分享文字 electron-only | `hidden electron:inline`（web 端只显示图标） | dimi 总是显示「分享」 | web 端多出文字 |

### 10.3 行为 / 可访问性差异

| # | 项 | codex | dimi 当前 | 影响 |
|---|---|---|---|---|
| A1 | aria-label vs title | **全部 aria-label**；固定摘要/侧栏带 `aria-pressed`；固定摘要额外有 title；tooltip 单独 | 全部 title（无 aria-label、无 aria-pressed） | 无障碍/工具提示来源不同 |
| A2 | 固定摘要 | pressed → secondary（白 5% 底）+ 切换 summary panel 钉住 | 无 pressed 态（本地 ref toggle 只切背景，且 dimi 用 Refresh 图标占位） | 缺 pressed 视觉 + 功能 |
| A3 | 分享行为 | 分享对话框；disabled 于未加载/streaming | 点击开帮助弹窗（占位） | 行为占位（产品确认） |
| A4 | More 行为 | 对话操作菜单（pin/quick chat/debug/handoff…） | 点击开帮助弹窗（占位） | 行为占位 |
| A5 | 标题点击 | 内联重命名（Enter 提交 / Esc 取消 / 自动全选） | 打开 session picker | 行为不同（dimi 无重命名功能） |
| A6 | 返回/前进 | 历史栈导航（navigateBack/Forward 命令） | 列表顺序前后切换 | 语义接近，实现不同 |
| A7 | sidebar 触发 | 点击 dispatch `toggleSidebar`；hover 预览（pointerenter/leave）；tooltip + shortcut | `sidebar_toggle` dispatch ✓；无 hover 预览 | 缺预览/tooltip/shortcut |
| A8 | 返回/前进渲染平台 | 仅 electron/extension（web 端隐藏） | 总是渲染 | web 端多出按钮 |
| A9 | 主区 center overlay | align=center 条目绝对居中（左右面板宽 spacer） | 无 | 未来 home-composer-mode-toggle 等居中按钮位 |
| A10 | header 右键菜单 | 注册式 HeaderContextMenuItem → 原生/自定义菜单 | 无 | 缺右键菜单 |

### 10.4 建议的 dimi 改动清单（按影响排序）

1. **S7/S8/S9** 分享 pill：圆角 9999 → 12.5px、padding 10 → 8px、字号 13 → 14px/445。
2. **S10** 删除 `iconBtnBordered`（More 不再带可见边框）。
3. **S2/S3** hover 背景 0.05 → 0.08、去掉 hover 变色（保持 tertiary）。
4. **S1** 图标按钮圆角 10 → 12.5px（用 theme 的 `iconBtnRadius`）。
5. **L2/L3/L4/L5** 右区改为「分享 pill + 固定摘要（pressed 态白 5% 底 + aria-pressed）+ 切换侧边栏」三件套，去掉 `paddingRight:42`；左区去掉 x=0 菜单按钮（sidebar-trigger 本身承担切换，aria 随显隐变化）。
6. **L6** 主区左距 `paddingLeft:14` → `ms-2`(8) + 标题 `-ms-0.5`(−2)。
7. **S17** header 加 `-webkit-app-region: drag`、按钮 `no-drag`。
8. **S6** cursor: pointer → default（disabled 保持 not-allowed）。
9. **S12/A5** 标题点击 → 内联重命名（input 态样式：`h-6 rounded-md border focus-border bg-input-background`）。
10. **A1** title → aria-label（固定摘要加 `aria-pressed`）。
11. **S5** 字重 445 进 theme token。
12. **L1** 安全左距改为变量驱动（`--spacing-token-safe-header-left`），或至少保留 88 硬编码并注释来源。

### 10.5 留待后续确认（源码无法确定 / 依赖运行时环境）

- `--vscode-*` 系列 token 的具体颜色值随主题变化（本文档给出的是当前深色主题实测值；`text-tertiary`≈白 0.498、`list-hover-background`≈白 8%）。
- hover/active 的**视觉确认**无法用 CDP Runtime 模拟（Input 事件会挂起）；本文档的 hover/active 值来自 class + token 解析，与 01-header.md 实测的 1 个 hover 样本一致。
- `cursor-interaction` 在 web 端可能解析为 pointer（electron 下实测 default）——dimi 是 web 渲染环境，需自行验证。
- 窄窗口（<1536px 主区）下 edge-scroll 关闭后 header tint 的来源（`--codex-titlebar-tint`）由宿主设置，bundle 内无固定值。
- Win/Linux app-menu-bar 模式（36px 顶栏 + header 右移）未在 mac 实例实测，仅源码。
- dimi 的 Refresh 占位、帮助弹窗等产品行为差异需产品确认后再实现。
