# Codex Sidebar 设计逆向（02-sidebar-code）

> 目标：从 codex 源码 bundle 逆向左侧边栏的**代码级设计**，作为 dimi native-client 复刻的实现依据。
> 与 `02-sidebar.md`（DOM 实测版）互补：本文以 bundle 组件 JSX + CSS 规则 + 运行中 computed 值为准，标注「代码」/「实测」来源。
> 源文件：
> - JS：`/tmp/codex_asar/webview/assets/app-initial-iBPGfcXU.js`（15MB，minified，组件名已哈希化）
> - CSS：`app-44wrUC9v.css`（Tailwind + 自定义类）、`app-initial-BSHZIbh1.css`（CSS Modules 哈希类 + 主题变量）
> - 复核：CDP `Runtime.evaluate` + `getComputedStyle`（运行中 codex，窗口 1686×960 深色主题）
> 约定：`代码` = bundle 中 className/JSX 原文；`实测` = CDP computed 值；`推断` = 由代码与实测外推；`无法确定` = 读不到/测不到。

---

## 0. 结论速览（代码级）

| 项 | 值 | 证据 |
|---|---|---|
| 侧栏宽 | 275px；宽度 token `--spacing-token-sidebar: clamp(240px, 275px, min(520px, calc(100vw - 320px)))`；resize 应用条件 `>= 240` | `Uqr` style.width + CSS token；实测 275 |
| 背景 | `rgba(40,40,40,0.7)`（`browser:bg-token-main-surface-primary`） | 实测 |
| padding-top | 46px（`var(--height-toolbar)`，全屏时 0px） | `Uqr` paddingTop |
| 行圆角 | 12.5px = `--radius-lg` = `.625rem × 1.25` | CSS + 实测（corner scale 1.25） |
| 图标按钮圆角 | 10px = `--radius-md` = `.5rem × 1.25` | CSS + 实测 |
| 导航项字号 | **13px**（`text-sm` → 本环境 `--text-sm:13px`，由 JS 内联注入 html）；lh 18.5714px | 实测 |
| 会话项行高 | 30px（滚动容器内 `[--height-token-row:30px]`）；新对话 29px（header 内默认 29px） | 代码 + 实测 |
| hover 背景 | `rgba(255,255,255,0.08)`（`bg-token-list-hover-background`） | 类 + 实测 |
| 文字色 | `color-mix(in oklab, var(--vscode-foreground) 85%, transparent)` → 0.85 白 | `_Navigation_` CSS |
| footer 高 | 46px（`--height-toolbar`），发丝线代码 `h-[0.5px] bg-token-foreground/10`（DPR=2 实测 1px、0.1 白） | 代码 + 实测 |
| 滚动 mask | `vertical-scroll-fade-mask`：mask 渐变 + `edge-fade` scroll-timeline 动画 | CSS |

---

## 1. 组件结构（代码级层级树）

bundle 内组件（minified 名）与 DOM 结构对应：

```
aside.app-shell-left-panel            ← 组件 Uqr（`<aside>`，style.width + paddingTop）
└── div.max-w-full.overflow-hidden    ← Uqr 内层（minWidth/width 内联，拖拽透明过渡）
    └── div.select-none.box-border.flex.h-full.w-full.isolate.flex-col
        └── nav._Navigation_1m7sz_2   ← 组件 Ovu（CSS Module: flex column flex:1）
            ├── div.relative.z-10.flex.shrink-0.flex-col.gap-2.px-row-x.pb-(--sidebar-scroll-header-spacing)   ← header 块（ze）
            │   ├── div.ms-2.flex.items-center.pe-1        ← brand 行（Ie）
            │   │   ├── 模式切换按钮  ← 组件 H8l（jvu 包装；Radix DropdownMenu，aria-label 见 §8）
            │   │   └── div.ms-auto.flex.items-center.gap-1 ← 右侧按钮组（Fe）
            │   │       ├── 搜索按钮   ← 组件 T6l（26×26，icon 16，`ms-auto translate-x-0.5`）
            │   │       ├── 优先级按钮 ← 组件 k8l/A8l（26×26，activity filter，aria-pressed + coachmark）
            │   │       └── （Browser activity 按钮 _Tc 仅在媒体标签>0 时渲染，h-6 w-8）
            │   └── 新对话按钮         ← 组件 o8l（`sidebar-item`，29px 行）
            └── div.vertical-scroll-fade-mask …            ← 滚动容器（组件 hfu，props 见 §3）
                ├── div.flex.shrink-0.flex-col.gap-2       ← 滚动区固定块（topContent=Se）
                │   └── div.shrink-0.px-row-x
                │       └── div.flex.flex-col.gap-1
                │           └── div.flex.flex-col.gap-px   ← 导航项列表（T8 通用行；feature-gate 决定显示项）
                │               ├── 站点 Sites（30px，u8l；`/sites` 路由）
                │               ├── 已安排 Scheduled（30px；`/automations` + mark-all-read 菜单）
                │               ├── 插件 Plugins/Skills（30px；`/skills`，可带 New chip）
                │               └── …（Projects/Pull requests/Library/Security/Debug/MCP 由 feature gate 决定）
                ├── div.contents                          ← 分组列表（DnD 上下文 Fsu 包裹）
                │   ├── 置顶 section（组件 sfu，条件：showPinnedSection）
                │   ├── idu 加载骨架屏（loading 时）
                │   └── 项目/最近 section（组件 tcu→E8/Usu + 会话行 Ztu/F7l/d7l）
└── div.absolute.inset-x-0.bottom-0.z-20                  ← footer 容器（Ke，46px）
    ├── div.relative.px-row-x                             ← 上层卡（We）
    │   ├── gSc 用量警示卡（chatgpt 认证时，mb-2）
    │   └── Q4l onboarding checklist（未完成时，mb-2）
    └── Svu：footer 主体
        ├── gvu/wgu（electron/browser 分发）
        │   ├── 发丝线（h-[0.5px] bg-token-foreground/10）
        │   ├── bgu 用户行（29px，头像 18 + 名字，hover 0.08）
        │   └── Ggu/zgu 帮助按钮/菜单（32×32，icon 18）
        └── （更新横幅 K_u / 公告 _vu）
```

**关键组件索引（bundle 偏移，app-initial-iBPGfcXU.js）**：

| 组件 | 偏移 | 职责 |
|---|---|---|
| `Uqr` | ~4182958 | `<aside>` shell：宽度、paddingTop、resize handle |
| `Jvu`/`Kvu` | shell | AppShell 挂载：`Tj.Root` 布局，`LeftPanel` = `Fvu`；sidebar trigger（`tUr`，actionId=sidebar-trigger） |
| `Fvu` | ~13998500 | sidebar 根 wrapper：dnd drop 根、`ring` 拖拽态、渲染 `Ovu` |
| `Ovu` | 13988554 | 主 Navigation：组装 brand 行/新对话/滚动容器/footer；CSS 变量注入 |
| `jvu` | 13996975 | 模式切换逻辑（onModeSelect → nTc 切模式 / 导航 returnLocation） |
| `H8l` | 13400040 | 模式切换按钮 UI（Radix DropdownMenu、`menuWide`、font-openai-sans） |
| `o8l` | 13370755 | header 块导航项（新对话，c8l/s8l 按模式分派；可带 Quick chat trailing） |
| `T8` | ~13366xxx | 通用导航项基类（所有 nav item 的行容器/选中态/交互 trailing） |
| `i8l` | 13367039 | desktop 固定导航块（Projects/Sites/Scheduled/Plugins/Security/Debug/MCP） |
| `E8` | 13340317 | 通用 section 容器（分组标题行 + 折叠动画 + gap-px 列表） |
| `Usu` | 13680601 | 分组标题 toggle 按钮（chevron 显隐/旋转、拖拽手柄） |
| `tcu` | — | 可折叠 section（px-row-x + drop indicator + 折叠时状态徽标） |
| `T6l` | 13362363 | brand 行**搜索按钮**（`aria-label="Search"`、`ms-auto translate-x-0.5`） |
| `_Tc` | 11505818 | Browser activity 媒体指示按钮（audio/camera/mic 标签 >0 才渲染；`h-6 w-8`） |
| `k8l`/`A8l` | 13382291 | 优先级按钮（activity view filter toggle，`aria-pressed` + coachmark） |
| `hfu` | 13847798 | 滚动容器（vertical-scroll-fade-mask；loading→idu 骨架屏） |
| `$fu` | 13862411 | 滚动内容编排（priority/unified/connection/list/project 视图分支 + sticky headers） |
| `idu` | 13808032 | **加载骨架屏**（Loading chats + 行 skeleton） |
| `sfu` | 13843805 | 置顶（Pinned）section（线程 + 项目组，sortable + 排序菜单） |
| `$du` | 1383xxxx | pinned 线程 sortable 列表（onPinnedOrderChange） |
| `t9l`/`i9l` | 1344xxxx | 项目文件夹行 / 右侧 hover 操作区 |
| `Ztu`/`feu` | 13516038 | 本地/云会话行（hover 操作、右键菜单、归档确认） |
| `F7l`/`d7l`/`_nu` | — | 基础行骨架（hover rail O7l、状态点 Utu、标题/摘要/meta） |
| `veu`/`jtu` | 13492289 | hover 操作组容器 / 置顶小按钮（O7l rail 内） |
| `gSc`/`_Sc` | 11464361 | footer 用量警示卡（usage alert，仅 chatgpt 认证） |
| `Q4l` | 13325086 | footer onboarding checklist 卡（"Getting started"） |
| `Svu`→`gvu`/`wgu`→`bgu`→`ugu` | 13987896 | footer 主体（发丝线 + 用户行 + 个人资料 popover，按平台分发） |
| `Ggu`/`zgu` | — | 帮助按钮 / 帮助菜单（electron） |
| `Avu` | 13996173 | **browser/chrome-extension 版 brand 行**（46px toolbar：模式按钮+搜索+优先级+tUr） |

---

## 2. 布局

### 2.1 侧栏整体（`Uqr`）

```jsx
<aside className={`app-shell-left-panel pointer-events-auto relative flex overflow-visible browser:bg-token-main-surface-primary ${t ? 'cursor-col-resize' : ''}`}
       style={{ width: leftPanelAnimatedWidth, paddingTop }}>
  <div className="max-w-full overflow-hidden" style={{ minWidth: sidebarWidth, width: sidebarWidth, opacity: ... }}>
    {children /* nav 内容 */}
  </div>
  <ResizeHandle defaultSize={275} ... />   // lRr
</aside>
```

- `代码`：width 来自 store `leftPanelWidth`（默认 275）；paddingTop = `var(--height-toolbar)`（46px），全屏/无菜单栏时 `0px`
- `实测`：aside 275×960，bg `color(srgb 0.156863 0.156863 0.156863 / 0.7)`，padding-top 46px，overflow visible
- `代码`：内层 `overflow-hidden` + 内联 width（resize 动画用 `leftPanelAnimatedWidth` + opacity 过渡）

### 2.2 nav wrapper（`_Navigation_1m7sz_2`）

```css
._Navigation_1m7sz_2{flex-direction:column;flex:1;min-height:0;display:flex}
._Navigation_1m7sz_2{--color-token-foreground:color-mix(in oklab, var(--vscode-foreground) 85%, transparent)}
```

- `实测`：nav 275×914（=960−46），flex column，flex:1

### 2.3 滚动容器（hfu 渲染）

```jsx
<div {...sidebarScroll}
     className={`vertical-scroll-fade-mask relative isolate flex min-h-0 flex-1 flex-col gap-4 overflow-x-hidden overflow-y-auto
                 pb-[calc(var(--sidebar-footer-height)+var(--padding-row-x))]
                 [--height-token-row:30px] [--radius-token-row:10px] [contain:layout_paint]
                 ${sticky ? stickySectionHeaders : ''} ${dragging ? 'pointer-events-none' : ''}
                 ${topContent == null ? '-mt-2 pt-6' : '-mt-[var(--sidebar-scroll-header-spacing,8px)] pt-[var(--sidebar-scroll-content-top-padding,var(--sidebar-scroll-header-spacing,8px))]'}`}
     onScroll={handleScroll}>
```

- `代码`：`gap-4` = 16px 分组间距；`pb-[calc(footer-height + padding-row-x)]` = 46+8 = **54px** 底部留白
- `代码`：`[--height-token-row:30px]`、`[--radius-token-row:10px]` 作用域覆盖
- `实测`：scroll [0,115 275×845]，padding `1px 0 54px`，gap 16px，mask `linear-gradient(...)` 顶部 1px / 底部 86px 淡出（动画值，见 §5.5）
- 滚动处理：`onScroll` 节流（`e.timeStamp - last >= yfu`）→ `gh()`（预估新会话可见性?）；`scrollTop>0` 时回调 `onScrolledContentUnderHeaderChange(true)`，并记录 `scrollTop`

### 2.4 header 块（ze）与滚动区固定块（Se）

```jsx
// header 块：brand 行 + 新对话
<div className={`relative z-10 flex shrink-0 flex-col gap-2 px-row-x pb-(--sidebar-scroll-header-spacing)
                 ${sticky ? 'after:pointer-events-none after:absolute after:inset-x-0 after:bottom-0 after:h-[0.5px] after:bg-token-foreground/10 after:content-[\'\']' : ''}`}>
```

- `代码`：`px-row-x` = 8px 水平 padding；`pb-(--sidebar-scroll-header-spacing)` = padding-bottom 未滚动 1px / 滚动到 header 之下 4px（JS 注入，见 §2.5）
- sticky 时底部 0.5px 分隔线 `bg-token-foreground/10`（`rgba(255,255,255,0.1)`）
- `实测`：header 块 [0,46 275×70]（46+70=116 → scroll 起点），pb 1px、gap 8px

```jsx
// 滚动区固定块（站点/已安排/插件）
const topContent = <div className="flex shrink-0 flex-col gap-2"> {navItems /* i8l */} {null} </div>;
```

- `实测`：[0,116 275×92] = 3×30 + 2×1px；嵌套 `div.shrink-0.px-row-x` → `div.flex.flex-col.gap-1`（4px）→ `div.flex.flex-col.gap-px`（**1px**）

### 2.5 作用域 CSS 变量（Ovu 注入）

Ovu 根 div 内联 style（style=M），状态 `C = scrolledContentUnderHeader`（滚动内容是否已滚到 header 之下）：

```js
[v, y] = useState(0);            // footer 高度（px），ResizeObserver 实测
[x, S] = useState(false);        // C = scrolledContentUnderHeader
D = 'calc(var(--spacing) * 2)';  // 8px
D = C ? 'var(--spacing)' : '1px';  // 滚动后 4px / 未滚动 1px
O = `${v}px`;                    // --sidebar-footer-height（46px）
k = C ? 'calc(var(--spacing) * 4)' : D;  // 滚动后 16px / 未滚动 1px
A = C ? D : '0px';               // 滚动后 4px / 未滚动 0px
j = {
  '--sidebar-footer-height': O,
  '--sidebar-scroll-content-top-padding': '1px',
  '--sidebar-scroll-header-fade-distance': k,
  '--sidebar-scroll-header-fade-start': A,
  '--sidebar-scroll-header-spacing': D,
}
```

根 div className：`relative flex min-h-0 flex-1 flex-col overflow-hidden [--height-token-mode-switch:32px] [--height-token-nav-row:30px] [--padding-row-cell-x:8px] [--padding-row-x:8px] [--radius-token-row:10px]`

- 未滚动：header 块 `pb-1px`、滚动区 `-mt-1px`/`pt-1px`、mask 顶部淡出 0→1px
- 滚动到 header 之下（`C=true`）：header 块 `pb-4px`、mask 顶部淡出 4px→16px，header 块追加 `after:` 0.5px 分隔线（`bg-token-foreground/10`）
- footer 高度由 `useRef` + ResizeObserver 实测（`T = e => y(c_(e).height)`），注入 `--sidebar-footer-height`，滚动区用它算底部留白
- 滚动回调：`onScroll` 250ms 节流 → `gh()`（通知外部）+ `onScrolledContentUnderHeaderChange(scrollTop>0)` + 保存 scrollTop

### 2.6 Shell 挂载与宽度 token

```jsx
// Jvu（AppShell 挂载）：
<Tj.Root>
  <Ixc/>                                        {/* 标题栏 */}
  <Exc threadKeys={u}/>
  <Tj.HeaderAction actionId="sidebar-trigger" slotPosition="left"><tUr/></Tj.HeaderAction>  {/* 折叠按钮 */}
  {!collapsed && <Tj.HeaderAction actionId="new-chat" slotPosition="left">…</Tj.HeaderAction>}
  <Tj.LeftPanel><Fvu desktopNavItemsEnabled/></Tj.LeftPanel>     {/* 侧栏内容 */}
  …
</Tj.Root>
```

- `tUr` @4108249 = sidebar trigger（折叠/展开按钮，`actionId=sidebar-trigger`）；折叠时 new-chat header action 隐藏
- 侧栏宽度 token（app-44wrUC9v.css）：`--spacing-token-sidebar: clamp(240px, 275px, min(520px, calc(100vw - 320px)))`
  - 首选 **275px**；窗口窄时下限 **240px**；窗口宽时上限 520px（`min(520, 100vw-320)`）
  - 与 `Uqr` resize 逻辑一致：`e >= 240` 才应用新宽度
- electron 悬浮用量卡：`fixed bottom-[var(--padding-row-x)] left-[var(--padding-row-x)] z-30 w-[calc(var(--spacing-token-sidebar)-2*var(--padding-row-x))]`（pointer-events-none 容器 + pointer-events-auto 卡片）

---

## 3. 组件明细（边距/内距/视觉态）

### 3.1 模式切换按钮（H8l / jvu）

- `代码` 类：`no-drag cursor-interaction items-center gap-1 border whitespace-nowrap select-none focus:outline-none disabled:cursor-not-allowed disabled:opacity-40 flex rounded-full text-token-foreground enabled:hover:bg-token-list-hover-background enabled:active:bg-token-foreground/15 data-[state=open]:bg-token-list-hover-background …`
- 尺寸：`h-8`（32px），`px-2`（8px），圆角 `rounded-full` → **15px**（实测）
- 内联：`-ms-2`（margin-left:-8px，抵消容器 ms-2）；`!text-[17px] !leading-6`；`font-medium`（500）
- 文字 span：`truncate font-openai-sans font-semibold`（**600**）；实测 17px/24px、color 0.85
- chevron：14×14，`--color-token-foreground/50` → 实测 `rgba(255,255,255,0.498)`；向下 path
- hover：`enabled:hover:bg-token-list-hover-background`（0.08）；active：`enabled:active:bg-token-foreground/15`（0.15）；open：`data-[state=open]:bg-token-list-hover-background`（0.08）
- 行为：Radix DropdownMenu 触发器；`aria-label = sidebarElectron.productMode.trigger`（"Switch mode, current mode: {mode}"）；`aria-expanded` 由 Radix 管理；菜单 `p-1.5`（6px）、`menuWide` 类
- 禁用态（`codexOnly` 等）：渲染为 `<div className="-ms-2 flex h-8 min-w-0 items-center px-2 text-[17px] leading-6 text-token-foreground select-none">`（非按钮）

### 3.2 brand 右侧按钮组（Fe）

```jsx
<div className="ms-auto flex items-center gap-1">
  {searchButton}   {/* T6l：搜索 */}
  {priorityButton} {/* k8l/A8l：优先级（activity view），entrypoint=header_icon 时 */}
</div>
```

- 搜索按钮（T6l）：`<tm className="ms-auto translate-x-0.5" aria-label={Search} color="ghost" size="icon" onClick={O6l}>` + `icon-xs` 放大镜；外层 `vh` tooltip（shortcut label = `searchChats` 快捷键）；`ms-auto` 右推 + `translate-x-0.5`（2px 光学偏移）
- 实测：26×26，padding 4px，radius **10px**，color 0.498，icon 16×16（放大镜 vb `0 0 16 16`）；`aria-label="搜索"`（zh）
- 优先级按钮（k8l/A8l）：`<tm aria-label={f} aria-pressed={active} color={active ? 'accentSubtle' : 'ghost'} size="icon" onClick={...}>`；label 三态：未启用=「View activity」/ needsAttention=「View activity, needs attention」/ 启用=「Turn off activity view」；图标 `needsAttention ? 带圆点 filter : priority 齿轮`（icon-xs）；`aria-pressed` 反映启用态；coachmark popover `side='left' sideOffset=12`
- 实测：26×26，同搜索按钮样式；icon 16×16（齿轮+星 vb `0 0 20 20`）；`aria-label="优先级，需要关注"`（zh）
- Browser activity（_Tc，非本环境）：`h-6 w-8`（24×32）、`me-0.5`；仅媒体标签存在时渲染；`aria-label` 为媒体菜单标题

### 3.3 导航项（sidebar-item 基类）

统一类（新对话/站点/已安排/插件/会话项/文件夹行共用）：

```
sidebar-item focus-visible:outline-token-border relative h-[var(--height-token-row)]
px-[var(--padding-row-cell-x,var(--padding-row-x))] py-row-y cursor-interaction
shrink-0 items-center overflow-hidden text-left text-sm focus-visible:outline
focus-visible:outline-2 focus-visible:outline-offset-2 disabled:cursor-not-allowed
disabled:opacity-50 gap-2 flex w-full hover:bg-token-list-hover-background
```

CSS：
```css
.sidebar-item{border-radius:var(--radius-lg);corner-shape:var(--codex-corner-shape)}
.sidebar-item-icon{width:calc(var(--spacing) * 6);height:calc(var(--spacing) * 6);justify-content:center;align-items:center;display:flex}
```

- 圆角 `--radius-lg` = 12.5px（scale 1.25）；padding `5px 8px`（py-row-y=5px、px-row=8px）
- 文字 `text-sm`：本环境 13px / 18.5714px、weight 445（`--vscode-font-weight:445`）、color 0.85 白
- 图标 16×16（icon-xs），icon↔文字 `gap-2` 8px
- 文字截断 `.text-fade-truncate`：`white-space:nowrap; overflow:hidden; mask-image:linear-gradient(to right,#000 calc(100% - 1rem),transparent)`
- hover：`hover:bg-token-list-hover-background` = 0.08

各导航项差异：

| 项 | 位置 | 行高 | 图标 vb | 说明 |
|---|---|---|---|---|
| 新对话 | header 块 | 29px（header 内 `--height-token-row` 默认 29px） | `0 0 16 16` | `o8l` 组件 |
| 站点 Sites | 滚动固定块 | 30px（`[--height-token-row:30px]`） | `0 0 16 16` | `u8l`；`/sites` 路由；仅 `MUa==='available'` 显示 |
| 已安排 Scheduled | 同 | 30px | `0 0 16 16` | `/automations` 路由；`Sh` 菜单（mark-all-read）+ 未读徽标 `j3l` |
| 插件 Plugins/Skills | 同 | 30px | `0 0 16 16` | `/skills` 路由；插件宿主时 label 带 New chip（`M6l`） |
| （可选）Projects/Pull requests/Library/Security/Debug/MCP | 同 | 30px | 各异 | feature gate / env 条件；MCP apps 为动态项（`q3l`） |

**T8 通用导航项机制**（所有 nav item 的基类，含选中态）：
- 行容器类（见 §3.3 统一类）+ `fullWidth`（默认 w-full）/ `hoverBackground`（默认 true）
- 选中态：`isActive` → `aria-current="page"` + 背景（default variant：`bg-token-list-hover-background`；accent variant：`bg-token-text-link-foreground/10 hover:bg-token-text-link-foreground/15`）+ 文字 `text-token-list-active-selection-foreground` + 图标 `text-token-list-active-selection-icon-foreground`
- 标签区：`div.flex.min-w-0.items-center.text-base.gap-2`（+ `flex-1` 全宽时）+ 16px 图标槽 `span.flex.w-4.shrink-0.items-center.justify-center` + `span.text-fade-truncate` 文字
- `trailing`（非交互徽标，如未读）与 `interactiveTrailing`（交互按钮，hover 显示）分开；interactiveTrailing 时行内层变为 `button.flex.min-w-0.flex-1` + trailing

### 3.4 分组标题（section title：E8 容器 + Usu toggle）

实测类：`group/nav-section-title flex items-center justify-between gap-2 pe-0.5 ps-2`；高 25px；padding `0 2px 0 8px`；gap 8px

**E8 通用 section 容器**（nav 分组/标题行的骨架）：
- 标题行：`group/nav-section-title flex items-center justify-between gap-2` + `pe-0.5 ps-2`（4px/8px）；标题区默认类 `min-w-0 flex-1 text-base font-medium text-token-input-placeholder-foreground opacity-75`
- children 列表：`flex flex-col gap-px`（1px 间隔）；有 title 时 `pt-1`（4px）
- 折叠动画：AnimatePresence，height 0→auto + opacity（`Ck` 缓动）
- titleActions 显隐包装：`pointer-events-none opacity-0 group-focus-within/nav-section-title:pointer-events-auto group-focus-within/nav-section-title:opacity-100 group-hover/nav-section-title:pointer-events-auto group-hover/nav-section-title:opacity-100 has-[[data-state=open]]:pointer-events-auto has-[[data-state=open]]:opacity-100`（**无过渡类，瞬时**）

**Usu toggle 按钮**（chevron + 拖拽手柄）：
- 类：`group/section-toggle flex min-w-0 flex-1 items-center gap-1 rounded-md py-0.5 pe-1 text-left focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2`（padding `2px 4px 2px 0`、圆角 10px）
- `aria-expanded = !collapsed`；`onClick` 切换折叠
- chevron（lf，icon-2xs=14px）：`shrink-0 transition-transform group-hover/section-toggle:opacity-100 group-focus-visible/section-toggle:opacity-100 sidebar-hover-icon-tint` + 显隐：**折叠时 `opacity-100` 常显 + `-rotate-90`；展开时 `opacity-0`（hover 才显示）+ `rotate-0`**
- 标题文字：14px/21px、weight 500、实测色 0.498（`text-token-input-placeholder-foreground` 基础 + opacity-75 叠加）
- `tp.sidebarSectionToggle` 加 `data-app-action-sidebar-section-toggle=""`

**tcu 可折叠 section**（项目/最近等实际使用）：
- 容器：`section.relative.px-row-x`（水平 padding 8px）+ `data-app-action-sidebar-section` 系列属性；拖拽排序激活时 `opacity-20`
- drop indicator：`absolute -top-2 right-0 left-0 z-10`（前）/ `-bottom-2`（后）内 `A5` 放置指示线
- 折叠时 titleTrailing 显示状态徽标 `j3l`（如 Scheduled 未读）

**右侧操作按钮**：`.sidebar-icon-button` = 24×24、radius `--radius-md`（10px）、padding 4px、color 0.425、icon 渲染 14×16
- 各 section 按钮（aria-label）：项目 → 「项目侧边栏选项」(ellipsis) + 「添加新项目」(plus)；最近 → 「聊天侧边栏选项」(ellipsis) + 「筛选聊天和工作」(filter) + 「新对话」(newChat)；置顶 → 「Pinned options」(sort 菜单，`h-6 w-6 rounded-md !p-1 opacity-75 hover:opacity-100`)

### 3.5 文件夹行（folder/cwd row）

实测类：`sidebar-item group/folder-row group relative flex h-[var(--height-token-row)] cursor-interaction items-center justify-between overflow-x-hidden text-sm text-token-foreground hover:bg-token-list-hover-background`

- 行高 30px；`aria-expanded`；`data-sidebar-project-kind`
- 图标容器：`-mx-[3px] flex size-[var(--height-token-row)] shrink-0 items-center justify-center`（30×30，左右 −3px）
- 文件夹图标 16×16（vb `0 0 16 16`）
- 名称：14px(13px)/…、445、0.85、`text-fade-truncate` + `pe-1`
- 右侧操作：`max-w-[50%] gap-1`；默认 `w-0 opacity-0`，`group-hover/folder-row:w-auto group-hover/folder-row:opacity-100 focus-within:w-auto focus-within:overflow-visible …`
- 按钮：24×24（sidebar-icon-button），「<名> 的项目操作」(ellipsis) + 「在 <名> 中开始新聊天」(newChat)

### 3.6 会话项

实测类（选中项）：

```
group relative cursor-interaction py-row-y text-sm hover:bg-token-list-hover-background
focus-visible:outline-offset-[-2px] h-[var(--height-token-row)] sidebar-item pe-1
ps-[var(--padding-row-cell-x,var(--padding-row-x))] …
```

- 行高 30px；padding `5px 4px 5px 8px`；radius 12.5px
- 左侧：16px 空图标槽 `flex w-4 shrink-0 items-center justify-center`（无可见内容）→ 文字起点 x=40
- 标题：`text-fade-truncate` + hover marquee（`_viewport_1ozkg_1 _animateOnGroupHover_1ozkg_53`）；14px(13px)/20px、445
- hover 后缀：`hidden shrink-0 text-token-text-tertiary group-hover:inline`（默认隐藏，hover 显示「工作」标签，0.498 色）
- 选中态：`aria-current="page"` + `bg-token-list-hover-background`（持久 0.08）+ 文字纯白
- 默认右侧徽标：`absolute end-0 top-0 z-10 min-w-[52px] … pe-1 group-hover:hidden` → 20×20 box + 14×14 双箭头图标（0.498），**hover 整块隐藏**
- hover 操作层（置顶/归档，O7l rail + veu 容器）：

```jsx
// O7l rail 常量：
E7l = `icon-2xs`
D7l = `absolute end-0 top-0 z-10 flex h-full items-center justify-end gap-2 pe-0.5`
O7l = `${D7l} me-0.5 w-[52px]`        // 置顶/归档 hover rail（52px 宽、右 2px）
k7l = `absolute end-0 top-0 z-10 flex h-full min-w-[52px] items-center justify-end gap-2 pe-1`  // 默认徽标 rail

// veu（hover 操作组容器）：
<div className="flex items-center gap-2 opacity-0 group-hover:opacity-100 [&:has(:focus-visible)]:opacity-100">
  <tm className="!h-5 !w-5 !p-0 [&>svg]:!h-4 [&>svg]:!w-4 sidebar-hover-icon-button-tint" aria-label onClick={stopPropagation+fn}>…
</div>
```

- 按钮 20×20（`!h-5 !w-5 !p-0`）、icon 16×16（`[&>svg]:!h-4 [&>svg]:!w-4`）、radius 10px、color 0.425（`sidebar-hover-icon-button-tint`：默认 50% mix，hover/focus-visible 全色）
- 置顶按钮（jtu）：`flex h-5 w-5 items-center justify-center leading-none sidebar-hover-icon-tint` + `icon-2xs`（14px）pin 图标（`translate-x-px` 光学偏移 1px）；有未读会话时 hover 用 `<span className="block h-5 w-5"/>` 占位
- 置顶常驻规则（Itu）：`isPinned` → rest 也显示 jtu（常驻置顶按钮）；否则 rest=null、hover=jtu
- 归档按钮（Xtu）：`aria-label=Archive chat` + 有 heartbeat 自动化时先弹确认对话框（Eeu）
- **显隐为类级瞬时切换（opacity-0 → group-hover:opacity-100，无独立 transition）**
- 数据属性：`data-app-action-sidebar-thread-active/pinned/host-id/kind/title` 等（tp helper，见 §7.9）
- 可拖拽：listitem `role=listitem`、`aria-roledescription="sortable"`、`tabindex=0`、行内 `cursor-grab active:cursor-grabbing`
- 状态点（Utu，title 左侧）：`size-3` 容器内 8px 圆点（`size-2 rounded-full`），颜色映射 qtu：active=`bg-token-charts-green`、follower=purple、inactive=yellow、needs-resume=red、read-only=orange；外包 tooltip（"Subscribed: active/follower/idle" / "Needs resume" / "Read-only snapshot"）
- 行 meta（时间/工作树）：`text-xs leading-4 text-token-text-tertiary`（0.498）

### 3.7 footer（Ke：发丝线 + 用户行 + 帮助；上层叠放用量卡/onboarding 卡）

```jsx
<div ref={footerRef} className="absolute inset-x-0 bottom-0 z-20">   {/* Ovu 的 Ke */}
  <div className="relative px-row-x">
    {usageAlert}   {/* gSc className="mb-2"：用量警示卡，仅 chatgpt 认证时 */}
    {checklist}    {/* Q4l className="mb-2"：onboarding checklist，me 标志时 */}
  </div>
  {Svu /* footer 主体：发丝线 + 用户行 + 帮助（按平台分发） */}
</div>
```

**真实 footer 链路（Svu 按平台分发）**：

```jsx
<Fragment>
  <ak electron><gvu/></ak>                      {/* electron：公告横幅 + 更新横幅 + 用户行 + 帮助 */}
  <ak browser><wgu trailing={<Ggu/>}/></ak>     {/* browser：用户行 + 文档按钮 */}
  <ak chromeExtension extension><wgu trailing={null}/></ak>
</Fragment>
```

- **wgu**（footer 行容器）：`[container-type:inline-size] relative w-full shrink-0`
  - 发丝线：`aria-hidden pointer-events-none absolute inset-x-0 top-0 z-10 h-[0.5px] bg-token-foreground/10` —— 代码 **0.5px**（DPR=2 下实测渲染 1px），色 `rgba(255,255,255,0.1)`
  - 行：`flex h-toolbar items-center gap-2 px-row-x`（46px、gap 8px、水平 padding 8px）
  - 左：`min-w-0 flex-1` 内 `bgu` 用户按钮
- **bgu**（用户按钮）：`flex h-[var(--height-token-row)] min-w-0 flex-1 cursor-interaction items-center gap-2 sidebar-item px-[var(--padding-row-cell-x,var(--padding-row-x))] text-left text-base text-token-foreground outline-none hover:bg-token-list-hover-background`（footer 内 29px）
  - `aria-label`：已登录「Open profile menu」/ 未登录「Open settings」（点击导航 `/settings/...`）
  - 头像：img `icon-sm shrink-0 rounded-full`（18×18、9999px）；无图时字母头像 `icon-sm flex … rounded-full bg-token-charts-purple/10 text-[10px] leading-none font-medium text-token-charts-purple`
  - 名字：`min-w-0 flex-1 truncate`、纯白、14px/21px、445
  - 已登录时包 `ugu` 个人资料 popover（`sidebarFooter:{modelProviderName, profileIdentity, width}`，width=container query）
  - electron 追加设置齿轮（Jmu）+ Codex Micro 电量（_pu），外层 `div.flex.min-w-0.flex-1.items-center.gap-0.sidebar-item`
- **Ggu/zgu**（帮助）：`tm {aria-label:"Open Codex docs", className:"size-8 shrink-0", color:"ghost", size:"icon", uniform, icon-sm 文档图标}`（实测 32×32、icon 18×18、radius 10px）
  - electron 时包 `zgu` 帮助菜单 popover：Set up mobile / Set up Chrome extension / Set up remote / What's new / Keyboard shortcuts / Help / Internal debug（`sidebarHelp.*`）；browser 时直接打开 docs
  - 实测 footer 帮助按钮 32×32、padding `4px 0`、color 0.498、`aria-label="打开帮助菜单"`
- **gSc**（用量警示卡，electron）：仅 `authMethod==='chatgpt'` 且存在 alert 时渲染；`rounded-2xl border border-token-border bg-token-main-surface-primary p-3 text-left text-token-foreground`，内部：标题 `text-base font-medium`（"{remaining}% usage remaining"）+ dismiss X 按钮（`size-5 rounded-full hover:bg-token-list-hover-background`）+ `text-sm text-token-text-secondary` 重置时间 + `progress.h-1.5 w-full rounded-full`（`[&::-webkit-progress-value]:bg-token-foreground`，bar bg `bg-token-foreground/10`）+ CTA 按钮（`h-9 flex-1 justify-center`，primary/outline）；action 分发：buy_credits/upgrade_plan/open_workspace_billing/notify_workspace_owner；entryPoint=`sidebar_usage_alert`
- **Q4l**（onboarding checklist，electron）：`me`（未完成）时渲染；`section.group.relative.z-40.overflow-hidden.rounded-2xl.bg-token-main-surface-primary/70.text-token-foreground.shadow-lg.ring-[0.5px].ring-token-border`；头部按钮（折叠 toggle）`px-3 pt-3 pb-2`（折叠时 `px-3 py-1.5`）+ 进度环 `size-3.5`（fK，size=14）+ 标题 `text-sm leading-5 font-medium`（"Getting started"）+ 进度文本 `text-xs leading-5 text-token-text-tertiary tabular-nums`（"n of m"，**group-hover:opacity-0 / 菜单打开时隐藏**）+ 右侧操作（hover 显示）`size-6 rounded-md` + 折叠 chevron `icon-2xs -rotate-90`；展开内容 `px-2 pb-2`：进度条 `h-1.5 rounded-full bg-token-foreground/10` + checklist 项（完成项 `line-through`）
- 实测（当前环境）：footer [0,914 275×46]、用户行 29px、帮助 32×32/icon 18、发丝线 1px（0.5px 逻辑 × DPR2）

---

## 4. 透明度/视觉态汇总

| 状态 | 元素 | 值 | 机制 |
|---|---|---|---|
| 默认 | 会话 hover 操作（置顶/归档） | `opacity:0` + `pointer-events` 不拦 | `opacity-0 group-hover:opacity-100 [&:has(:focus-visible)]:opacity-100` |
| hover | 同上 | `opacity:1` | group-hover（**无 transition，瞬时**） |
| hover | 分组标题 chevron | 0 → `sidebar-hover-icon-tint` 全色 | `group-hover/section-toggle:opacity-100` |
| 折叠 | 分组标题 chevron | **opacity-100 常显** + `-rotate-90`（展开态才 hover 显示 + rotate-0） | Usu 显隐类 |
| 默认 | 分组标题右侧操作按钮 | `opacity:0` + `pointer-events:none` | 同上包装类 |
| hover/聚焦 | 同上 | `opacity:1` + `pointer-events:auto` | `group-hover/focus-within` + 弹层打开 `has-[[data-state=open]]` |
| hover | 文件夹行右侧操作 | `w-0 opacity-0` → `w-auto opacity-100` | `group-hover/folder-row:*`（宽度动画，非透明） |
| hover | 会话默认徽标 | 隐藏 | `group-hover:hidden` |
| hover | 会话标题后缀 | 显示 | `group-hover:inline`（hidden→inline） |
| hover | 导航项/会话项/文件夹行背景 | `rgba(255,255,255,0.08)` | `hover:bg-token-list-hover-background` |
| active | 模式切换按钮 | `rgba(255,255,255,0.15)` | `enabled:active:bg-token-foreground/15` |
| 选中 | 会话项背景 + 文字 | bg 0.08 持久 + 纯白字 | `aria-current=page` + 类 |
| sticky | header 块底部分隔线 | 0.5px `rgba(255,255,255,0.1)` | `after:` 伪元素 |
| 拖拽中 | sidebar 根 | `ring-1 ring-token-border bg-token-bg-secondary/40` | Fvu 拖拽态 |

---

## 5. CSS 关键规则（代码原文）

### 5.1 尺寸 token

```css
--spacing:.25rem
--height-toolbar:46px
--height-token-mode-switch:32px
--height-token-nav-row:30px            /* 硬编码覆盖 */
--height-token-row:30px                /* 滚动容器内联 [--height-token-row:30px] */
--padding-row-x:calc(var(--spacing) * 2)   /* 8px；root 内联覆盖 8px */
--padding-row-cell-x:8px
--padding-row-y:calc(var(--spacing) * 1.25)  /* 5px（body 级） */
--radius-md-base:.5rem; --radius-lg-base:.625rem; --corner-radius-scale:1.25  /* 实测 */
--text-base:14px; --text-sm:13px（html 内联注入）; --text-sm--line-height:calc(1.25/.875)=1.42857
--vscode-font-weight:445
```

### 5.2 自定义类

```css
.sidebar-item{border-radius:var(--radius-lg);corner-shape:var(--codex-corner-shape)}
.sidebar-item-icon{width:calc(var(--spacing) * 6);height:calc(var(--spacing) * 6)}  /* 24px */
.sidebar-icon-button{width:calc(var(--spacing) * 6);height:calc(var(--spacing) * 6);border-radius:var(--radius-md);corner-shape:var(--codex-corner-shape);padding:var(--spacing)!important}  /* 24×24、10px、4px */
.sidebar-hover-icon-button-tint{color:var(--color-token-foreground)!important}
.sidebar-hover-icon-button-tint{color:color-mix(in oklab, var(--color-token-foreground) 50%, transparent)!important}
.sidebar-hover-icon-button-tint:hover,.sidebar-hover-icon-button-tint:focus-visible{color:var(--color-token-foreground)!important}
/* .sidebar-hover-icon-tint 同构（无 !important） */
```

### 5.3 滚动 mask（两层机制：edge-fade 动画 + _headerFadeMask 静态渐变）

```css
/* A. vertical-scroll-fade-mask：scroll-timeline 驱动的动画类 */
.vertical-scroll-fade-mask{
  mask:linear-gradient(to bottom in oklch,
        oklch(60% 0 0/0), oklch(85% 0 0) var(--top-fade) calc(100% - var(--bottom-fade)), oklch(60% 0 0/0));
  animation-name:edge-fade;animation-timing-function:linear;
  animation-fill-mode:both;animation-timeline:scroll(self y);
}
@keyframes edge-fade{
  0%,1%{--top-fade:0;--bottom-fade:var(--edge-fade-distance,1rem)}   /* 顶部不淡出，底部淡出 1rem */
  99%,to{--top-fade:var(--edge-fade-distance,1rem);--bottom-fade:0}  /* 滚动到底：顶部淡出、底部不淡出 */
}

/* B. _headerFadeMask_rqnbz_1（pfu.headerFadeMask，滚动容器上的 CSS Module 类）：静态精确渐变 */
._headerFadeMask_rqnbz_1{
  --edge-fade-distance:calc(var(--spacing) * 10);                    /* 40px */
  --sidebar-scroll-footer-edge:calc(100% - var(--sidebar-footer-height));            /* 100% - 46px */
  --sidebar-scroll-footer-fade-distance:calc(var(--spacing) * 10);   /* 40px */
  --sidebar-scroll-footer-fade-start:calc(var(--sidebar-scroll-footer-edge) - var(--sidebar-scroll-footer-fade-distance)); /* 100% - 86px */
  --sidebar-scroll-header-mask-distance:var(--sidebar-scroll-header-fade-distance,var(--sidebar-scroll-header-spacing,calc(var(--spacing) * 2)));
  --sidebar-scroll-header-mask-start:var(--sidebar-scroll-header-fade-start,0px);
  --sidebar-scroll-mask-image:linear-gradient(to bottom,
    transparent 0, transparent var(--sidebar-scroll-header-mask-start),
    black calc(var(--sidebar-scroll-header-mask-start) + var(--sidebar-scroll-header-mask-distance)),
    black var(--sidebar-scroll-footer-fade-start),
    #000000e0 calc(var(--sidebar-scroll-footer-edge) - var(--spacing) * 6),
    #00000085 calc(var(--sidebar-scroll-footer-edge) - var(--spacing) * 3),
    #0000002e calc(var(--sidebar-scroll-footer-edge) - var(--spacing)),
    transparent var(--sidebar-scroll-footer-edge), transparent 100%);
  -webkit-mask-image:var(--sidebar-scroll-mask-image);mask-image:var(--sidebar-scroll-mask-image);
}
```

- 未滚动（`--sidebar-scroll-header-fade-start:0px`、`--sidebar-scroll-header-fade-distance:1px`）：顶部 transparent→black 1px 内完成（顶部 1px 淡出）
- 底部：从 `100% - 86px` 开始淡出，`edge-24px` 处 88%（`#000000e0`）→ `edge-12px` 处 52%（`#00000085`）→ `edge-4px` 处 18%（`#0000002e`）→ `edge`（=100%−46px，footer 上缘）透明
- `实测` mask 静态值：`linear-gradient(rgba(0,0,0,0) 0px, rgba(0,0,0,0) 0px, rgb(0,0,0) 1px, rgb(0,0,0) calc(100% - 86px), rgba(0,0,0,0))` — 与上完全吻合
- `--sidebar-scroll-header-mask-distance` 在 sticky headers 开启时另有 `--sidebar-scroll-header-mask-distance:calc(var(--spacing)/2)`（2px，`_stickySectionHeaders_rqnbz_48`）
- 滚动条：`scrollbar-color: rgba(255,255,255,0.082) rgba(0,0,0,0)`；webkit 无自定义（macOS overlay）

### 5.4 标题 marquee（会话标题 hover 滚动）

```css
._overflowingViewport_1ozkg_33{
  mask-image:linear-gradient(to right, transparent, #000 var(--marquee-left-fade), #000 calc(100% - var(--marquee-right-fade)), transparent);
}
.group:hover ._animateOnGroupHover_1ozkg_53 ._scrolling_1ozkg_52{
  animation:_marqueeTextScroll_1ozkg_1 var(--marquee-cycle-duration) var(--marquee-scroll-timing) infinite;
  will-change:transform;
}
@keyframes _marqueeTextScroll_1ozkg_1{to{transform:translateX(calc(-1 * var(--marquee-scroll-distance)))}}
```

- 变量：`--marquee-cycle-duration`（周期）、`--marquee-scroll-timing`（缓动）、`--marquee-scroll-distance`（位移）、`--marquee-left-fade/right-fade`（mask 左右淡出）—— **具体数值未定位（无法确定）**
- 触发：`.group:hover` 或 `.animate` 类（`.animateOnGroupHover`）；会话行本身有 `group` 类

### 5.5 Navigation CSS Module

```css
._Navigation_1m7sz_2{flex-direction:column;flex:1;min-height:0;display:flex}
._Navigation_1m7sz_2{--color-token-foreground:color-mix(in oklab, var(--vscode-foreground) 85%, transparent)}
```

### 5.6 文字截断

```css
.text-fade-truncate{white-space:nowrap;overflow:hidden;mask-image:linear-gradient(to right,#000 calc(100% - 1rem),transparent)}
```

（app-44wrUC9v.css @55620 完整规则：`text-overflow:clip; white-space:nowrap; width:100%; min-width:0; -webkit-mask-image: linear-gradient(to right, #000 calc(100% - var(--text-fade-truncate-distance,1rem)), transparent)`）

### 5.7 图标尺寸（icon-* scale）

```css
.icon-3xs/xxs/2xs/xs/sm/base/md/lg → 10/12/14/16/18/20/24/28px
```

- sidebar 内实际用到：icon-xs=16px（导航/会话图标）、icon-2xs=14px（chevron/徽标）、icon-sm=18px（帮助图标）

---

## 6. 字号/字重表

| 元素 | font-size | line-height | weight | 来源 |
|---|---|---|---|---|
| 模式切换按钮 | 17px（`!text-[17px]`） | 24px（`!leading-6`） | 按钮 500 / 文字 span 600 | 实测/类 |
| 导航项（新对话/站点/已安排/插件） | 13px（`text-sm`） | 18.5714px（13×1.42857） | 445（`--vscode-font-weight`） | 实测 |
| 分组标题 | 14px（`text-base`） | 21px | 500 | 实测 |
| 文件夹行名称 | 13px（text-sm） | 18.5714px | 445 | 实测 |
| 会话项标题 | 13px（text-sm，行内 leading-5=20px 由父级设定） | 20px | 445 | 实测 |
| 会话 hover 后缀 | 14px | — | — | 实测 |
| 用户行 | 14px | 21px | 445 | 实测 |
| 用户行名字 | 14px | 21px | 445 | 实测 |
| 头像字母 | 11px（dimi 实现） | — | 600（dimi 实现） | dimi |

> 注意：`--text-sm` 在 CSS 中默认 12px、browser/chrome-extension 分支 13px；**本 Electron 窗口由 JS 内联注入 html style `--text-sm:13px`**（实测），因此所有 `text-sm` 元素为 13px。实测文档 02-sidebar.md 记录的 14px 与本环境不符（来源未定位，可能是窗口/版本差异：无法确定）。

---

## 7. 行为逻辑（代码级）

### 7.1 模式切换（jvu → H8l）
- 点击打开 Radix DropdownMenu（`aria-expanded` 由 Radix 管）；选项切换回调 `onModeSelect`
- `jvu.onModeSelect`：若当前在 codex 路由且切到 work → 返回 `returnLocation` 或 `/`；否则调用 `nTc(store, {codexLocalAccessStatus, currentMode, nextMode, startNewConversation})`（切模式并可能开新会话）
- `codexOnly`（无认证）→ 禁用，渲染为 div

### 7.2 搜索（T6l / E6l）
- 搜索按钮（T6l）：`onClick={O6l}` → `dp.dispatchHostMessage({type:'chat-search-command-menu'})` — **搜索是全局命令菜单事件**（host 打开 chat-search 命令菜单），不是侧栏内部 state
- tooltip 显示 `openCommandMenu` 快捷键；导航项形态（E6l，showSearchNavItem 时）带快捷键 label `x6l`（`opacity-0 group-hover:opacity-100`）
- `hfu` 内 `x`（= loading prop，未加载完成时）为 true 则渲染 `idu` 加载骨架屏（`chatgptConversations.sidebar.loading`="Loading chats"），内容 div 加 `hidden`

### 7.2b 优先级（k8l/A8l，activity view filter）
- `onClick`：`if (priorityFilter == null) { enable(r); return } disable(r)` — **toggle 启用/关闭**；另注册全局快捷键 `togglePriorityFilter`（`zk`）
- `aria-pressed` 反映启用态；启用时按钮 `color=accentSubtle`
- `needsAttention` 时图标带圆点 + label「View activity, needs attention」
- coachmark（引导弹层）：popover `side='left' sideOffset=12`、`align='start'`、`arrowAlignment='start'`、title=「View activity」、description=「See chats that are unread, active, or awaiting a response」
- label 三态 i18n：`priorityThreads.filterByPriority` / `needsAttention` / `turnOffPriorityFilter`

### 7.3 新对话
- header 块新对话按钮（o8l）：点击新建会话（`GV.newChat` label；aria 由组件提供）
- 最近 section 标题行右侧也有新对话按钮（24×24 icon）

### 7.4 站点/已安排/插件（真实路由）
- 站点 Sites（u8l）：`onClick → navigate('/sites')`；`isActive: pathname.startsWith('/sites') && !startsWith('/sites/library')`；hover/聚焦预取（`m8l` 并行预取 hhc/ghc）；仅 `MUa==='available'` 显示
- 已安排 Scheduled：`onClick → navigate('/automations')`；右键菜单（`Sh`）含 `mark-all-read`（`u`=markAllRead）；`isActive: /automations`；有未读时 trailing 徽标 `j3l`
- 插件 Plugins/Skills：`onClick → navigate('/skills', {state})`（埋点 + plugin directory entrypoint）；`isActive: /skills`；插件宿主时 label=「Plugins」+ New chip（`M6l`：campaign 2026-02-24~03-03、`variant='new'` → `!border-transparent !bg-token-text-link-foreground/10 !text-token-text-link-foreground font-semibold` + base chip `rounded-sm border px-0.5 py-px text-s font-light`），否则 label=「Skills」
- Projects（l8l）：`onClick → navigate('/projects')`；hover/菜单打开时显示 trailing：组织/排序菜单（p6l）+ 新建项目按钮（aria-label「Add new project」，color=ghostTertiary、size=icon、icon-xs）；新建按 `y6l` 模式走 direct-chatgpt / direct-local / project-type-dialog
- 其它：Pull requests（`/pull-requests`）、Library（`/library`）、Security（rg gate + 插件就绪才显示，`/security`）、Debug（仅 dev/agent env，打开 DebugModal）、MCP apps（`q3l` 动态项，route `/mcp-app/{server}/{tool}`）

### 7.5 分组折叠
- section toggle 按钮 `aria-expanded` 切换；chevron 折叠态旋转（推断 rotate-90，`transition: transform 0.15s cubic-bezier(0.4,0,0.2,1), opacity 0.15s ease` 在 dimi 中已按此实现；codex 侧 chevron 的完整 transition 列表未完整提取：无法确定）
- 折叠状态存于 store（`sidebarSection` 系列 key，`tp.sidebarSection({collapsed, header, children})`）

### 7.6 会话 hover 操作
- 置顶：`aria-label="置顶聊天"` → 置顶该会话（调 store；`pinnedThreadIds` 排序，`onPinnedOrderChange`）
- 归档：`aria-label="归档聊天"` → 归档会话
- 均为 hover/focus 显隐（§4），点击后 `comingSoon` 在 dimi 是占位；codex 真实行为未点测：无法确定

### 7.7 用户行/帮助
- 用户行（bgu）：`aria-label` 已登录「Open profile menu」/ 未登录「Open settings」；已登录点击打开个人资料 popover（ugu，`sidebarFooter:{modelProviderName, profileIdentity, width}`），未登录点击导航 `/settings/...`
- 帮助（Ggu/zgu）：`aria-label="Open help menu"`（electron 包菜单）/"Open Codex docs"（browser 直开文档）；electron 帮助菜单项：Set up mobile、Set up Chrome extension、Set up remote、What's new、Keyboard shortcuts、Help、Internal debug（`sidebarHelp.*`）
- 两者由 Svu 平台分发；gSc 用量卡 / Q4l onboarding 卡在 footer 上方 `mb-2` 叠放（非按钮）

### 7.8 拖拽
- 会话项 `role=listitem`、`aria-roledescription="sortable"`、`tabindex=0`、行内 `cursor-grab active:cursor-grabbing`
- dnd-kit：PointerSensor `activationConstraint:{distance:6}`（6px 拖拽激活）+ KeyboardSensor；拖拽中全局 `cursor:not-allowed` 样式注入
- 拖拽排序：`onPinnedOrderChange`（$du：`yas/vas` 计算可见/新顺序）；置顶分组线程排序写 store + localStorage（`unified-sidebar-pinned-order-v1`）
- 跨容器 drop 分发：`pinned` → pin；`chats`←`pinned` → unpin；`project:/cloud` → 跨项目移动（工作树解析 Ifu + 缺失目录确认 Yfu）；`custom:` → 移入/移出自定义 section
- 放置指示线 A5：`before:` 0.5px 线 + `after:` 8px 圆点（`bg-token-text-link-foreground`），拖拽目标行前后显示
- 根 wrapper Fvu：拖拽中 `ring-1 ring-token-border bg-token-bg-secondary/40`；另一窗口拖入时 `pt-2`
- 文件夹行可拖（drag handle）；section 可拖排序（`qsu` controller，sectionKeys=['chats','threads']）

### 7.9 会话行右键菜单（Ztu）
顺序（条件显示）：置顶/取消置顶 → pin 相关 → 重命名会话（双击标题也触发）→ 修改连接颜色（remote）→ 归档聊天（有 heartbeat 时先确认）→ 标为已读/未读 → fork（fork-into-local / fork-into-worktree）→ 显示/隐藏 active 状态点（dev）→ 分隔线 → 打开文件夹 → 复制工作目录 → 复制会话 ID → 复制链接 → 在新窗口打开
- `tp` data 属性 helper（DOM 自动化锚点）：`[data-app-action-sidebar-scroll/section/section-toggle/project-row/thread-row/thread-title]` 等

### 7.10 openFolder 事件
- Ovu 通过 `zk('openFolder', handler)` 订阅全局事件（打开指定文件夹树）
- `zk('newTask', …)` 注册新建任务命令；`zk('quickChat', …)`（codex 模式 + chatgpt allowed 时）；`zk('togglePriorityFilter', …)` 优先级快捷键

---

## 8. i18n key 清单（sidebar 相关）

| key（前缀 `sidebarElectron.`） | defaultMessage（英文） | 用途 |
|---|---|---|
| `productMode.trigger` | `Switch mode, current mode: {mode}` | 模式切换按钮 aria |
| `productMode.*`（chatGpt/work/codex/description.*） | ChatGPT / Work / Codex … | 模式菜单选项 |
| `newThread` / `workNewChatNavLink` | New chat / … | 新对话 |
| `search` | `Search` | 搜索按钮 aria |
| `taskNavigation` | `Scheduled task folders` | nav aria-label |
| `projectsNavLink` | `Projects` | 项目导航 |
| `pullRequestsRouteNavLink` | `Pull requests` | 导航 |
| `libraryRouteNavLink` | `Library` | 导航 |
| `sitesRouteNavLink` | `Sites` | 站点 |
| `inboxRouteNavLink` | `Scheduled` | 已安排 |
| `skillsRouteNavLink` / `skillsAppsRouteNavLink` | `Skills` / `Plugins` | 插件 |
| `skillsAppsRouteNavLink.newChip` | `New` | 插件 New chip |
| `securityRouteNavLink` / `debugNavLink` | `Security` / `Debug` | 导航 |
| `addGenericWorkspaceRoot` | `Add new project` | 新建项目 |
| `automationsMarkAllRead` | `Mark all as read` | 已安排菜单 |
| `priorityThreads.filterByPriority` | `View activity` | 优先级按钮 |
| `priorityThreads.needsAttention` | `View activity, needs attention` | 优先级按钮（需关注） |
| `priorityThreads.turnOffPriorityFilter` | `Turn off activity view` | 优先级按钮（启用中） |
| `priorityThreads.coachmark.description` | `See chats that are unread, active, or awaiting a response` | coachmark |
| `priorityThreads.options/restoreDefaults/showSection/showWork/showChat/showPinned/showScheduled/markAllAsRead/archivePriorityThreads/emptyState/recent.*/details.*/archive*` | — | 优先级菜单/分组 |
| `pinnedThreads` | `Pinned` | 置顶分组标题 |
| `pinnedThreads.optionsLabel` | `Pinned options` | 置顶排序按钮 |
| `sortMenu.*` | — | 置顶排序菜单 |
| `pinThread` / `unpinThread` | Pin / Unpin | 置顶/取消 |
| `renameThreadDialogCancel/Save` | — | 重命名会话 |
| `sidebarCustomSections.*` | createTitle/editTitle/newChat/optionsAriaLabel/emptySection | 自定义分组 |
| `codex.sidebarTaskRow.hideActiveStatus/showActiveStatus` | — | 会话行状态 |
| `sidebarHelp.*` / `codex.profileFooter.*` / `codex.profileDropdown.*` | — | footer 帮助/用户菜单 |
| `sidebar.browserMedia.*` | Browser activity … | 媒体指示菜单 |
| `app.sidebar.hide/show/tooltip` | Hide/Show sidebar / Toggle sidebar | 侧栏显隐（header 侧） |
| `sidebarElectron.usageAlert.*` | `{remaining}% usage remaining` / `Dismiss usage alert` / `Resets {time}` … | 用量警示卡 |
| `coreUsage.cta.buyCredits/upgrade` | `Add credits` / `Upgrade` | 用量卡 CTA |
| `sidebarOnboardingChecklist.*` | `Getting started` / `Expand checklist` / `{progressCount} of {totalCount}` / `Hide setup` / `Import agent setup` … | onboarding checklist |
| `sidebar.updateBanner.*` | `Downloading update…` / `Restart now` / `Restarting…` / `Update` | 更新横幅 |
| `chatgptConversations.sidebar.loading` | `Loading chats` | 加载骨架屏 |
| `sidebarElectron.quickChatNavLink` | `Quick chat` | Quick chat 按钮 |
| `sidebarElectron.newThread` / `workNewChatNavLink` | `New chat` | 新对话 |
| `sidebarElectron.renameThreadDialogCancel/Save` | — | 重命名会话对话框 |
| `codex.cloudTaskRow.archiveTask` | `Archive chat` | 云任务行归档 |
| `sidebarElectron.priorityThreads.emptyState` | `Nothing needs attention` | 优先级空态 |
| `sidebarElectron.priorityThreads.recent.today/yesterday` | `Today` / `Yesterday` | 日标题 |

---

## 9. Resize handle（lRr）

- `defaultSize: 275`；`getCurrentSize: () => store.get()`
- `onResizingChange`；`setSize: (e) => { store.set('leftPanelAnimated', false); const t = e >= 240; … if (!t) return; store.set(…, xmr(e)); store.set(…, e) }` — **宽度低于 240px 时不应用/隐藏侧栏**（240 为下限判据）
- `onResizeEnd: (e) => { e < 240 || persist(xmr(e)) }`
- DOM：`[role=separator]`、`.group.absolute.flex.touch-none`，16px 宽、`top:-46px`（延伸到 header 上方）、`z-20`、`cursor:col-resize`
- 中线：`sidebar-resize-handle-line` 1px 宽、默认 opacity 0（hover 显示）

---

## 10. 无法确定 / 未覆盖清单

- marquee 变量具体数值（`--marquee-cycle-duration`/`--marquee-scroll-timing`/`--marquee-scroll-distance`/`--marquee-left-fade`/`--marquee-right-fade`）：CSS 只给出 var() 引用，数值定义未定位
- 置顶/归档/搜索/优先级菜单的真实点击结果（未点测真实 UI）
- Browser activity 按钮在本环境不渲染（媒体标签=0），其布局只按代码类记录
- resize 拖拽后宽度的持久化与动画细节（CDP Input 被禁）
- 折叠动画精确缓动（`Ck` 的 duration/easing 值未展开；dimi 按 0.15s + rotate-90 实现是合理近似）
- unified 列表（Mdu/Ndu）内部虚拟化细节（`anu`）与 chatgpt 项目组行（Ilu/Ulu/mau）未展开（属 project 视图，非本次范围）
- `mz`（菜单）/`uz`（popover）/`tm`（按钮）共享 UI 组件内部实现未展开

---

## dimi 差距

> 对照 `apps/native-client/DimiNative/src/renderer/components/Sidebar.vue` + `Sidebar.styles.ts`（当前实现，已按实测文档实现大部分）。本表基于 **codex bundle 代码 + 运行 computed**，与 02-sidebar.md 的差距表互补。

### A. 结构差异

| # | 差异 | codex（代码级） | dimi 现状 | 严重度 |
|---|---|---|---|---|
| A1 | header 块间距变量 | `pb-(--sidebar-scroll-header-spacing)`：未滚动 1px / 滚动到 header 之下 4px + 0.5px 分隔线 | `sidebarTop` padding `0 8px 1px`（固定 1px，无滚动态切换） | 中：滚动态 4px + 分隔线未实现 |
| A2 | 滚动区上边距 | `-mt-[var(--sidebar-scroll-header-spacing,8px)] pt-[var(--sidebar-scroll-content-top-padding,1px)]`（未滚动 -1px/1px） | `sessions` marginTop -1px、pt 1px ✓ | 低：已一致 |
| A3 | 站点/已安排/插件嵌套 | `gap-1`（4px）外 + `gap-px`（1px）内两层；固定块为动态导航列表（T8），feature gate 决定显示项 | `navBlockScroll` 只有 `gap:1px` + 固定三项 | 低：当前环境视觉一致（1px）；扩展项未实现 |
| A4 | footer 结构 | 三层：Svu（发丝线+用户行+帮助，平台分发）+ gSc 用量卡（mb-2）+ Q4l onboarding 卡（mb-2） | 单层 flex：用户行 + 帮助按钮；无用量卡/onboarding 卡 | 高：信息架构缺失 |
| A5 | 模式切换菜单 | Radix DropdownMenu（ChatGPT/ChatGPT Work/Codex + 描述 + 对勾），`p-1.5`、`menuWide`、`data-[state=open]` 高亮 | 纯按钮，无菜单 | 高 |
| A6 | 会话项 marquee | hover 标题滚动动画（`_overflowingViewport_1ozkg_33` + `_marqueeTextScroll_1ozkg_1`） | 无 | 中 |
| A7 | 加载骨架屏 | idu：标题行 1 条 + 4 行 skeleton（gap-px、px-2、pt-1） | `emptyRow`「加载中…/暂无会话」 | 中 |
| A8 | 宽度 token | `--spacing-token-sidebar: clamp(240px, 275px, min(520px, calc(100vw - 320px)))` | 硬编码 275 + localStorage 记忆 | 低：窄窗口收窄行为未实现 |
| A9 | 导航项 trailing | T8 支持 `trailing`（未读徽标）与 `interactiveTrailing`（hover 操作按钮，如 Quick chat） | 无 | 中 |
| A10 | 选中态 accent variant | T8 `activeVariant='accent'`（link 色 10%→15%） | 仅 default variant（0.08） | 低 |
| A11 | 会话项右键菜单 | 15 项（pin/rename/archive/read/fork/copy…） | 无 | 高 |
| A12 | 状态点 | Utu：8px 圆点（active/follower/inactive/needs-resume/read-only 五色） | 无 | 中 |
| A13 | 用户行头像 | img 18×18 / 字母头像 `bg-token-charts-purple/10 text-[10px]` | `sb-avatar` 白底 0.85 + 黑字 | 低 |

### B. 数值差异（codex 实测 vs dimi）

| # | 属性 | codex（本环境实测） | dimi 现状 | 说明 |
|---|---|---|---|---|
| B1 | 导航项字号 | **13px / 18.5714px** | `font.xs`=14px / 21px | `--text-sm` 由 JS 注入 13px；02-sidebar.md 的 14px 记录与本环境不符 |
| B2 | 会话项字号 | 13px / 20px | 14px / 21px（`font.xsLh`） | 同 B1 |
| B3 | 模式切换字体族 | `font-openai-sans` 显式类 + `truncate`；button 500 / span 600；**代码 className `rounded-xl`(12px)，实测渲染 15px**（tm 默认 `rounded-full` 覆盖） | brandFamily 含 "OpenAI Sans" ✓，weight 500/600 ✓，radius 15px | 基本一致 |
| B4 | 会话项选中态 | bg 0.08 + 纯白字（`aria-current`） | `sessionItemActive` bg 0.08 + 白 ✓ | 一致 |
| B5 | 置顶/归档图标偏移 | pin svg `translate-x-px` | `.sb-pin` translateX(1px) ✓ | 一致 |
| B6 | 徽标/操作层 | `me-0.5 pe-0.5`（2px）、`w-[52px]`、`gap-2`（8px）；badge rail `min-w-[52px] pe-1` | 同 ✓ | 一致 |
| B7 | 发丝线 | 代码 `h-[0.5px] bg-token-foreground/10`（0.5px 逻辑；DPR=2 实测 1px） | `::before` 1px 0.1 | 低：DPR=2 下视觉一致 |
| B8 | 滚动 mask 底部 | 86px 淡出（`_headerFadeMask`：edge−40px 起，88%→52%→18%→透明） | `scrollMask` 底部 40px 线性 | 代码层为多档透明度渐变 + 动画，dimi 静态近似 |
| B9 | 帮助按钮 | 32×32、`padding:4px 0`、icon 18×18 | 同 ✓ | 一致 |
| B10 | 搜索按钮 | `ms-auto translate-x-0.5`（2px 光学偏移） | 无偏移 | 低 |
| B11 | 分组标题 chevron | 折叠时 opacity-100 常显 + `-rotate-90`；展开时 opacity-0（hover 显示）+ rotate-0 | `.sb-chevron` 默认 opacity 0、collapsed 时 rotate-90（无折叠常显） | 低：折叠态显示差异 |
| B12 | 置顶按钮图标尺寸 | 会话行 hover（veu/Xtu）：`[&>svg]:!h-4 !w-4`=**16×16**；Pinned 分组 rest 常驻（jtu）：icon-2xs=**14×14** | `sb-hover-btn svg` 16×16 ✓（会话行一致；无 jtu 常驻场景） | 低：两处尺寸并存，dimi 只有 16px 一种 |

### C. 行为差异

| # | 差异 | codex | dimi |
|---|---|---|---|
| C1 | 模式切换 | 真菜单（ChatGPT/ChatGPT Work/Codex + 描述 + 对勾），切模式 + 可选开新会话 | 占位按钮（无菜单） |
| C2 | 站点/已安排/插件 | 真实路由（`/sites`、`/automations`、`/skills`）+ 预取 + 未读徽标 + New chip | `comingSoon` 提示 |
| C3 | 置顶/归档 | 真实排序（`onPinnedOrderChange`，持久化 localStorage）/归档 store 操作 | `comingSoon` 提示 |
| C4 | 搜索 | `dispatchHostMessage('chat-search-command-menu')` 全局命令菜单 + 快捷键 | `comingSoon` |
| C5 | 优先级 | activity filter toggle（`aria-pressed` + `accentSubtle` 启用色 + coachmark + 快捷键 + Activity view 选项菜单） | `comingSoon` |
| C6 | 拖拽排序 | dnd-kit 完整（6px 激活距离、sortable、跨容器 drop、A5 指示线、持久化） | 无拖拽 |
| C7 | resize 下限 | `e >= 240` 才应用宽度（CSS clamp 下限也 240） | MIN_W 200 | **差异**：codex 下限 240，dimi 200 |
| C8 | 用户行/帮助 | 个人资料 popover（`codex.profileFooter.*`）/帮助菜单（`sidebarHelp.*`） | `Msg.SettingsOpen()`（打开设置页） | 差异 |
| C9 | 会话行右键菜单 | 15 项上下文菜单（rename/archive/fork/copy…）+ 双击改名 | 无 | 差异 |
| C10 | 用量/引导 | 用量警示卡（余额 CTA）+ onboarding checklist（进度 + Hide setup） | 无 | 差异 |
| C11 | 分组折叠动画 | E8 AnimatePresence：height 0→auto + opacity（`Ck` 缓动） | 无动画（v-if 直接切换） | 中 |
| C12 | 滚动态 header 变量 | `scrolledContentUnderHeader` 驱动 spacing 1px→4px + 0.5px 分隔线 + mask 顶部淡出 | 无滚动态切换 | 低 |

### D. 已一致（代码级确认）

- 275px 宽 + 46px padding-top + `--height-toolbar` ✓
- sidebar-item 基类：padding 5px 8px、radius 12.5px、icon 16、gap 8px、hover 0.08、`text-fade-truncate` mask ✓
- 分组标题：25px、padding 0 2px 0 8px、chevron 默认 opacity 0 + hover 显示、操作按钮 24×24 opacity 0→hover 1 ✓
- 文件夹行：30px、图标容器 -mx-3px 30×30、右侧操作 w-0→w-auto ✓
- 会话项：30px、16px 空槽、hover 后缀 hidden→inline、徽标 52px 覆盖层 hover 隐藏、置顶/归档 20×20 + 52px 覆盖层 opacity 0→1 ✓
- footer：46px、用户行 29px、帮助 32×32/18px、发丝线 0.1 ✓
- resize handle：16px、top -46、z-20、col-resize、中线 1px ✓
