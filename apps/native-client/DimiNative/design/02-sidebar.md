# Codex Sidebar 设计逆向（02-sidebar）

> 目标：dimi native-client 像素级复刻 codex 左侧边栏的唯一设计依据。
> 测量方法：CDP `Runtime.evaluate` + `getBoundingClientRect()` / `getComputedStyle()`（codex 页面无法 JS 滚动，所有元素无论视口位置均可读）。窗口实测 **1686×960**，深色主题，`app://-/index.html`（Electron）。
> 来源标记：`实测` = CDP 计算样式/几何；`类` = DOM 上的 Tailwind/组件类（hover/active 状态以此为准）；`推断` = 由类与已测状态合理外推；`无法观察` = 环境限制（CDP Input 被禁、页面状态不可控）。
> 注意：codex 交互元素 `cursor` 全站为 `default`（`--cursor-interaction` 在 mac 解析为 default），不是 pointer。

---

## 0. 结构总览（实测）

```
aside.app-shell-left-panel           [0,0 275×960]   bg rgba(40,40,40,0.7)  padding-top:46px
├── div.max-w-full.overflow-hidden   [0,46 275×914]
│   └── nav._Navigation_             [0,46 275×914]  flex column, flex:1 1 0%
│       ├── div.relative.z-10        [0,46 275×70]   header block（padding 0 8px 1px, gap 8）
│       │   ├── div.ms-2.pe-1        [16,46 251×32]  brand 行（32px）
│       │   │   ├── button 模式切换  [8,46 108×32]   "ChatGPT" + chevron
│       │   │   └── div.ms-auto.gap-1 [207,49 56×26] 搜索 26×26 + 优先级 26×26
│       │   └── div.flex-col.gap-1   [8,86 259×29]   新对话（29px）
│       └── div.vertical-scroll-fade-mask [0,115 275×845]  overflow-y:auto
│           ├── div.shrink-0.gap-2   [0,116 275×92]  站点/已安排/插件（3×30 + 2×1px）
│           └── section.relative     [0,224 275×716] 项目 section
│               └── 分组标题 25px + 文件夹树（folder row 30px + 会话项 30px）
└── div.absolute.bottom-0.z-20       [0,914 275×46]  footer（46px）
    ├── 发丝线 1px（rgba(255,255,255,0.1)）
    └── 用户行按钮 [8,923 219×29] + 帮助按钮 [235,921 32×32]
[role=separator]                     [267,-46 16×1006] resize handle（right:0, top:-46, z-20）
```

---

## 1. Sidebar 整体

| 属性 | 值 | 来源 |
|---|---|---|
| 宽度 | 275px（默认） | 实测 aside 275×960 |
| 背景 | `rgba(40, 40, 40, 0.7)`（即 `color(srgb 0.156863 0.156863 0.156863 / 0.7)`） | 实测 |
| 边框 | 无（0px，透明） | 实测 |
| 圆角 | 0 | 实测 |
| display / position | `flex` / `relative` | 实测 |
| padding-top | 46px（让出 header） | 实测 |
| 内层裁切 | `.max-w-full.overflow-hidden`（overflow hidden），aside 本身 `overflow-visible` | 实测 |
| 滚动容器 | `.vertical-scroll-fade-mask`：`overflow-x:hidden; overflow-y:auto`；`padding: 1px 0 54px 0`；`margin-top:-1px`；`gap:16px`；`flex:1 1 0%` | 实测 |
| 滚动底部留白 | 54px（= footer 46px + `--padding-row-x` 8px） | 实测 |
| 滚动 mask 渐变 | `mask-image: linear-gradient(...)`：顶部 ~1px 淡出，底部 ~40px 淡出（容器 var：`--sidebar-scroll-header-fade-distance:1px`、`--sidebar-scroll-header-fade-start:0px`、`--sidebar-footer-height:46px`） | 实测 |
| 滚动条 | `scrollbar-width:auto`；`scrollbar-color: rgba(255,255,255,0.082) rgba(0,0,0,0)`；`::-webkit-scrollbar` 无自定义（透明/auto）→ macOS overlay 滚动条 | 实测 |
| 滚动区导航固定块 | `.flex.shrink-0.flex-col.gap-2`，92px 高（3×30px 按钮 + 2×1px gap） | 实测 |
| section 之间间距 | 16px（scroll 容器 `gap:16px`） | 实测 |

---

## 2. Brand 行（32px）

| 属性 | 值 | 来源 |
|---|---|---|
| 容器 | `.ms-2.flex.items-center.pe-1`；`margin-left:8px`；`padding-right:4px`；高 32px（`--height-token-mode-switch:32px`） | 实测/类 |
| 按钮（模式切换） | `no-drag cursor-interaction items-center gap-1 ... h-8 rounded-xl px-2 !text-[17px] !leading-6 font-medium -ms-2`；`margin-left:-8px`（抵消容器 ms-2）；`padding:2px 8px`；`gap:4px`；`border:1px solid transparent`；圆角 **15px** | 实测 |
| 按钮 aria | `aria-label="切换模式，当前模式：ChatGPT"`；`aria-expanded=false`；`data-state=closed` | 实测 |
| 按钮字体 | `17px/24px`，weight 500（font-medium） | 实测 |
| 按钮文字色 | `rgba(255,255,255,0.85)` | 实测 |
| 文字 span | `17px/24px`，weight **600**（font-semibold），family 优先 `"OpenAI Sans"`（其次 -apple-system/system-ui/Segoe UI/sans-serif），`truncate` | 实测/类 |
| 文字 hover | `enabled:hover:bg-token-list-hover-background` = `rgba(255,255,255,0.08)` | 类 |
| 按钮 active | `enabled:active:bg-token-foreground/15` = `rgba(255,255,255,0.15)` | 类 |
| 按钮 open（下拉展开） | `data-[state=open]:bg-token-list-hover-background`（0.08） | 类 |
| chevron | 14×14（icon-2xs）；viewBox `0 0 20 21`；**path = 向下 chevron（与 dimi `sectionChevron` 相同 path）**；颜色 `rgba(255,255,255,0.498)`；位于文字后 4px（文字 [17,50 72×24]，chevron [93,55]） | 实测 |
| 右侧按钮组 | `.ms-auto.flex.items-center.gap-1`，`[207,49 56×26]`，gap 4px | 实测 |
| 搜索按钮 | 26×26；`aria-label="搜索"`；padding 4px；gap 4px；圆角 10px；border 1px transparent；color `rgba(255,255,255,0.498)`；图标 16×16（icon-xs，viewBox `0 0 16 16`，放大镜） | 实测 |
| 优先级按钮 | 26×26；`aria-label="优先级，需要关注"`；同搜索样式；图标 16×16（icon-xs，viewBox `0 0 20 20`，齿轮 + 星角标，path0 = dimi `gear` path0） | 实测 |

---

## 3. 导航项（新对话 / 站点 / 已安排 / 插件）

| 属性 | 新对话（header 区） | 站点 / 已安排 / 插件（滚动区顶） | 来源 |
|---|---|---|---|
| 位置 | nav header block 内 `[8,86 259×29]` | 滚动区固定块内 `[8,116/147/178 259×30]` | 实测 |
| 行高 | **29px**（`--height-token-row = calc(14px*1.5 + .25rem*2)` = 21+8） | **30px**（滚动子树内 `--height-token-row` 被覆写为 30px，即 `--height-token-nav-row:30px`） | 实测 |
| padding | `5px 8px`（`py-row-y` 实测 5px；`ps-[var(--padding-row-cell-x)]`=8px） | 同左 | 实测 |
| 圆角 | **12.5px**（`.sidebar-item { border-radius: var(--radius-lg) }`） | 同左 | 实测/类 |
| 图标 | 16×16（icon-xs） | 16×16（icon-xs） | 实测 |
| icon↔文字 gap | 8px（gap-2） | 同左 | 实测 |
| 文字字体 | `14px/21px`，weight 445（可变字重），family -apple-system… | 同左 | 实测 |
| 文字颜色 | `rgba(255,255,255,0.85)`（oklab 0.85） | 同左 | 实测 |
| 图标颜色 | `rgba(255,255,255,0.85)` | 同左 | 实测 |
| hover 背景 | `hover:bg-token-list-hover-background` = `rgba(255,255,255,0.08)` | 同左 | 类 |
| 文字截断 | `.text-fade-truncate`：`white-space:nowrap; overflow:hidden; mask-image:linear-gradient(to right,#000 calc(100% - 1rem), transparent)` —— **右缘淡出，不是省略号** | 同左 | 类 |
| 按钮类 | `sidebar-item ... h-[var(--height-token-row)] px-[var(--padding-row-cell-x,var(--padding-row-x))] py-row-y cursor-interaction shrink-0 items-center overflow-hidden text-left text-sm ... gap-2 flex w-full hover:bg-token-list-hover-background` | 同左 | 类 |
| cursor | default（`cursor-interaction` 在 mac = `var(--cursor-interaction)` = default） | 同左 | 实测 |
| 图标 path | 新对话=dimien `newChat`（vb 16）；站点=`sites`（vb 16）；已安排=`scheduled`（vb 16）；插件=`plugins`（vb 16） | 同左 | 实测 |

---

## 4. 分组标题（section title，如「项目」「最近」）

| 属性 | 值 | 来源 |
|---|---|---|
| 行容器 | `.group/nav-section-title.flex.items-center.justify-between`；高 **25px**；`padding: 0 2px 0 8px`；`gap: 8px` | 实测 |
| toggle 按钮 | `.group/section-toggle.flex.min-w-0.flex-1.items-center.gap-1.rounded-md.py-0.5.pe-1.text-left.cursor-default`；`aria-expanded=true`（展开态） | 实测/类 |
| toggle padding | `2px 4px 2px 0`；gap 4px；圆角 10px | 实测 |
| 标题文字 | `14px/21px`，weight **500**，颜色 `rgba(255,255,255,0.498)`（muted），`truncate` | 实测 |
| chevron | 14×14（icon-2xs）；viewBox `0 0 20 21`；向下 path（= dimi `sectionChevron`） | 实测 |
| chevron 默认 | **`opacity:0`**（展开态 `rotate-0`，不可见） | 实测 |
| chevron 显示时机 | 悬停标题行 `group-hover/section-toggle:opacity-100`、键盘聚焦 `group-focus-visible/section-toggle:opacity-100`；类 `sidebar-hover-icon-tint`（默认 50% mix `rgba(255,255,255,0.425)`，hover 全色） | 类 |
| chevron 颜色 | 默认 `rgba(255,255,255,0.425)`；hover 后亮起（0.85/白） | 实测/类 |
| chevron transition | `transform 0.15s cubic-bezier(0.4,0,0.2,1)` 等（折叠态旋转角推断 rotate-90，未实测） | 实测/推断 |
| 右侧操作包装 | `shrink-0 pointer-events-none opacity-0 group-focus-within/nav-section-title:pointer-events-auto group-focus-within/nav-section-title:opacity-100 group-hover/nav-section-title:pointer-events-auto group-hover/nav-section-title:opacity-100 has-[[data-state=open]]:pointer-events-auto has-[[data-state=open]]:opacity-100` —— **默认 opacity 0 + pointer-events none，悬停标题行 / 内含弹层打开时显示** | 类 |
| 操作按钮 | 24×24（`.sidebar-icon-button { width/height:24px; border-radius:var(--radius-md)=10px; padding:4px }`）；border 1px transparent；color `rgba(255,255,255,0.425)`；gap 4px | 实测 |
| 操作图标渲染 | 14×16（icon-xs class，实测受按钮内边距影响渲染为 14 宽 × 16 高） | 实测 |

分组标题右侧按钮（按 section 不同）：

| section | 按钮 aria-label | 图标（viewBox / path） | 渲染 |
|---|---|---|---|
| 项目 | 项目侧边栏选项 | ellipsis `0 0 21 21`（= dimi `ellipsis`） | 14×16 |
| 项目 | 添加新项目 | plus `0 0 20 20`（= dimi `plus`） | 14×16 |
| 最近 | 聊天侧边栏选项 | ellipsis `0 0 21 21` | 14×16 |
| 最近 | 筛选聊天和工作 | filter `0 0 16 16`（3 条横线，dimi 无） | 14×16 |
| 最近 | 新对话 | newChat `0 0 16 16`（= dimi `newChat`） | 14×16 |

---

## 5. 文件夹行（cwd / folder row，如 k-3720）

| 属性 | 值 | 来源 |
|---|---|---|
| 分组容器 | `.group/cwd.relative.flex.flex-col`；`role=listitem`；`aria-label=<目录名>`；`data-sidebar-project-kind="local"` | 实测 |
| 树引导线 | `.pointer-events-none.absolute.top-[var(--height-token-row)].bottom-0.left-0`（32×N）—— **实测全透明（bg/bgImage/border 均无），是拖拽树的占位，无可见渲染** | 实测 |
| folder row 按钮 | `.sidebar-item.group/folder-row.group.relative.flex.h-[var(--height-token-row)].cursor-interaction.items-center.justify-between.overflow-x-hidden.text-sm.text-token-foreground.hover:bg-token-list-hover-background`；高 **30px**；圆角 12.5px；`aria-expanded=true` | 实测/类 |
| 图标容器 | `-mx-[3px] flex size-[var(--height-token-row)] shrink-0 items-center justify-center`（30×30，-3px 左右 margin） | 实测 |
| 文件夹图标 | 16×16（icon-xs）；viewBox `0 0 16 16`（文件夹，dimi 无此 path）；颜色 0.85 | 实测 |
| 名称 | `14px/21px`，weight 445，颜色 `rgba(255,255,255,0.85)`；`text-fade-truncate` + `pe-1`；无计数徽标（名称区只有名字） | 实测 |
| 右侧 hover 操作 | `max-w-[50%] gap-1`：默认 `w-0 opacity-0`，`group-hover/folder-row:w-auto opacity-100` | 类 |
| 「k-3720 的项目操作」按钮 | 24×24；ellipsis 图标 14×16；aria-label=`<名> 的项目操作`；border 1px transparent；radius 10px；color 0.425；`aria-expanded=false`（下拉） | 实测 |
| 「在 k-3720 中开始新聊天」按钮 | 24×24；newChat 图标 14×16；aria-label=`在 <名> 中开始新聊天` | 实测 |
| 会话项嵌套 | `.overflow-hidden` → `.pt-0.5.pb-2`（上 2px / 下 8px） | 实测 |

---

## 6. 会话项

| 属性 | 值 | 来源 |
|---|---|---|
| 行高 | **30px**（`h-[var(--height-token-row)]`，滚动区上下文 = 30px） | 实测 |
| padding | `5px 4px 5px 8px`（左 8px = `--padding-row-cell-x`；右 4px = `pe-1`） | 实测 |
| 圆角 | 12.5px（sidebar-item） | 实测 |
| **左侧图标** | **没有可见图标**。存在 16px 宽预留槽（`.flex.w-4.shrink-0.items-center.justify-center`，实测高 0、内容 0×0 空），文字因此从 **x=40**（8 padding + 16 槽 + 8 gap）开始 | 实测 |
| 文字 | `14px/20px`（text-base leading-5），weight 445，颜色 `rgba(255,255,255,0.85)` | 实测 |
| 文字截断 | `.text-fade-truncate`（mask 右缘淡出）+ **hover 时标题 marquee 滚动**（`_viewport_1ozkg_1 _animateOnGroupHover_1ozkg_53` 类，动画参数未测） | 类 |
| hover 后缀 | 标题后 `<span class="hidden shrink-0 text-token-text-tertiary group-hover:inline">工作</span>` —— 默认隐藏，悬停显示（颜色 0.498） | 实测 |
| hover 背景 | `hover:bg-token-list-hover-background` = `rgba(255,255,255,0.08)` | 类 |
| **选中态** | `bg-token-list-hover-background` **持久化**（= `rgba(255,255,255,0.08)`）+ 文字变**纯白 `rgb(255,255,255)`**；`aria-current="page"`；无独立 active 类 | 实测 |
| 默认右侧徽标 | `.absolute.end-0.top-0.z-10.min-w-[52px]...pe-1.group-hover:hidden` 内 20×20（h-5 min-w-5）badge，图标 14×14（icon-2xs）**双箭头（下入托盘 + 上右）**，颜色 `rgba(255,255,255,0.498)`；hover 时整块 `group-hover:hidden` | 实测 |
| 列表分隔 | 项目间 `gap-px`（1px）；每个 listitem `::after` 1px 但**背景透明 —— 无可见分隔线** | 实测 |
| 数据属性 | `data-app-action-sidebar-thread-active/pinned/host-id/kind/title/...`（如 `thread-kind="local"`、`thread-pinned="false"`） | 实测 |
| 可拖拽 | listitem `role=listitem`、`aria-roledescription="sortable"`、`tabindex=0`；行内 `cursor-grab active:cursor-grabbing` | 实测/类 |

### 6.1 hover 操作（置顶 / 归档）

| 属性 | 值 | 来源 |
|---|---|---|
| 包装层 | `absolute end-0 top-0 z-10 h-full w-[52px] items-center justify-end gap-2 pe-0.5 me-0.5` + `opacity-0 group-hover:opacity-100 [&:has(:focus-visible)]:opacity-100` | 实测/类 |
| 默认透明度 | `opacity:0`；悬停会话项 → `opacity:1`；**无 transition 类（瞬时切换）** | 实测/类 |
| 按钮尺寸 | **20×20**（`!h-5 !w-5 !p-0`） | 实测 |
| 按钮样式 | radius 10px；border 1px transparent；color `rgba(255,255,255,0.425)`；gap 4px；类 `sidebar-hover-icon-button-tint`（默认 50% mix 0.425，`hover/focus-visible` 变全色） | 实测/类 |
| 图标尺寸 | **16×16**（`[&>svg]:!h-4 [&>svg]:!w-4`） | 实测 |
| 置顶 | aria-label=「置顶聊天」；viewBox `0 0 20 20` 单 path（pin，svg 带 `translate-x-px` 光学偏移 1px） | 实测 |
| 归档 | aria-label=「归档聊天」；viewBox `0 0 20 20` 双 path（箱子+盖） | 实测 |
| 位置 | 包装右缘距行右缘 2px（me-0.5）；置顶 [215,290]、归档 [243,290]，按钮间距 8px | 实测 |

---

## 7. 底部用户行（46px）

| 属性 | 值 | 来源 |
|---|---|---|
| 容器 | `.absolute.inset-x-0.bottom-0.z-20`，高 **46px**（`h-toolbar`）；`.flex.items-center.gap-2`（gap 8px） | 实测 |
| 发丝线 | `.pointer-events-none.absolute.inset-x-0.top-0`，高 1px，色 `rgba(255,255,255,0.1)` | 实测 |
| 用户行按钮 | `aria-label="打开个人资料菜单"`；高 **29px**（footer 内 `--height-token-row`=29px）；`padding: 0 8px`；`gap:8px`；圆角 12.5px；hover bg 0.08 | 实测 |
| 头像 | `<img>` 18×18，`rounded-full`（border-radius 9999px），类 `icon-sm shrink-0` | 实测 |
| 用户名 | `14px/21px`，weight 445，颜色 **纯白 `rgb(255,255,255)`**；`truncate`；`flex:1` | 实测 |
| 帮助按钮 | `aria-label="打开帮助菜单"`；**32×32**；padding `4px 0`；gap 4px；radius 10px；border 1px transparent；color `rgba(255,255,255,0.498)` | 实测 |
| 帮助图标 | **18×18**（icon-sm）；viewBox `0 0 20 20` 双 path（外圈 + 问号）= dimi `help` ✓ | 实测 |

---

## 8. Resize handle

| 属性 | 值 | 来源 |
|---|---|---|
| 元素 | `[role=separator]`，`.group.absolute.flex.touch-none` | 实测 |
| 几何 | **16px 宽**；`top:-46px; right:0; bottom:0` → 高 = 窗口高 + 46（实测 1006px），**拖拽热区延伸到 header（越出窗口顶部 46px）** | 实测 |
| 位置 | x=267（275-16=259? 实测 rect [267,-46 16×1006]，即 handle 中心线在 275） | 实测 |
| z-index | 20 | 实测 |
| cursor | `col-resize` | 实测 |
| 中线 | `.sidebar-resize-handle-line.pointer-events-none.m-auto.opacity-0`，1px 宽、满高；默认 `opacity:0`（悬停 handle 时显示；具体 hover 规则未截全，dimi 已用 `hover/active → opacity 1` 近似） | 实测/推断 |
| 拖拽宽度范围 | **无法观察**（CDP Input 被禁，不能真实拖拽；dimi 的 200–480 是猜测值，需实现时人工验证） | 无法观察 |

---

## 9. 图标清单（对照 dimi `icons.ts`）

| 用途 | codex svg | 渲染尺寸 | dimi icons.ts 对应 |
|---|---|---|---|
| brand chevron（向下） | viewBox `0 0 20 21`，path = 向下 chevron | 14×14 | **`sectionChevron`（path 完全一致）；dimi 现在用了 `chevronDown`（不同 path）✗** |
| 搜索 | viewBox `0 0 16 16`，放大镜 | 16×16 | 无 ✗ |
| 优先级 | viewBox `0 0 20 20`，齿轮 + 星角标（path0 = dimi `gear` path0） | 16×16 | 无（需新增，可复用 gear path0 + 星 path）✗ |
| 新对话 | viewBox `0 0 16 16` 双 path | 16×16 | `newChat` ✓ |
| 站点 | viewBox `0 0 16 16` | 16×16 | `sites` ✓ |
| 已安排 | viewBox `0 0 16 16` | 16×16 | `scheduled` ✓ |
| 插件 | viewBox `0 0 16 16` | 16×16 | `plugins` ✓ |
| section chevron（向下） | viewBox `0 0 20 21` | 14×14 | `sectionChevron` ✓ |
| 侧边栏选项（项目/聊天） | ellipsis viewBox `0 0 21 21` 3 path | 14×16 | `ellipsis` ✓ |
| 添加新项目 | plus viewBox `0 0 20 20` | 14×16 | `plus` ✓ |
| 筛选聊天和工作 | viewBox `0 0 16 16` 3 横线 | 14×16 | 无 ✗ |
| 文件夹（cwd） | viewBox `0 0 16 16` | 16×16 | 无 ✗ |
| 置顶 | viewBox `0 0 20 20` 单 path（svg 类 `translate-x-px`） | 16×16 | 无 ✗ |
| 归档 | viewBox `0 0 20 20` 双 path | 16×16 | 无 ✗ |
| 会话项默认徽标 | viewBox `0 0 20 20` 单 path（双箭头） | 14×14 | 无 ✗ |
| 帮助 ? | viewBox `0 0 20 20` 双 path | 18×18 | `help` ✓ |

> 新增图标完整 path 见测量附件（`/tmp/sb_icons.txt`，本仓库不携带）；实现时把 pin/archive/search/priority/filter/folder/badge 的 `d` 数据加入 `src/renderer/icons.ts`。

---

## 10. 行为 / 可访问性

| 元素 | aria-label / title | 点击行为（推断） |
|---|---|---|
| 模式切换 | 「切换模式，当前模式：ChatGPT」 | 打开模式菜单（ChatGPT / Codex 等） |
| 搜索 | 「搜索」 | 打开/聚焦搜索 |
| 优先级 | 「优先级，需要关注」 | 打开优先级列表 |
| 新对话（导航行 + 最近标题行） | — / 「新对话」 | 新建会话 |
| 站点 / 已安排 / 插件 | — | 切到对应视图（当前为占位） |
| section toggle | — | 折叠/展开分组（`aria-expanded` 切换） |
| 分组操作按钮 | 「项目侧边栏选项」「添加新项目」「聊天侧边栏选项」「筛选聊天和工作」 | 对应菜单/操作 |
| 文件夹行 | aria-label=<目录名> | 折叠/展开该目录树；hover 出现「项目操作」「在该目录开始新聊天」 |
| 会话项 | aria-label=<标题>；`data-state=closed`、`aria-current=page`（选中） | 打开会话；hover 出现置顶/归档 |
| 置顶 / 归档 | 「置顶聊天」「归档聊天」 | 置顶/归档会话 |
| 用户行 | 「打开个人资料菜单」 | 打开个人资料菜单 |
| 帮助 | 「打开帮助菜单」 | 打开帮助菜单 |
| resize handle | role=separator | 拖拽调宽 |

通用：所有可点元素 `cursor: default`（mac）；图标按钮基类 `transition: all`（实测 computed `transition=all`），但 hover 操作的显隐是类级 `opacity-0 → group-hover:opacity-100`（无独立 transition，瞬时）。

---

## 11. 无法观察清单

- 拖拽 resize 后的宽度范围（min/max）与是否持久化 —— 无法观察（CDP Input 被禁）。
- 分组折叠后 chevron 的最终旋转角（推断 rotate-90，未实测）；chevron 淡入/旋转的精确动画时长（transition 定义含 0.15s transform，但完整列表被截断）。
- 会话项标题 marquee 动画参数（滚动速度/延迟，只有类名，未解析 keyframes）。
- 置顶/归档的实际菜单与结果（不点击真实 UI）。
- resize line 的 hover 显隐规则（类被截断，dimi 实现为 hover/active → opacity 1 是合理近似）。
- 侧栏隐藏/窄窗口行为（本窗口 1686px 全宽展示）。
- 会话项徽标图标语义（双箭头，可能是导入/导出/同步指示，仅记录形状）。

---

## dimi 差距（codex 实测 vs dimi 当前代码）

对照文件：`apps/native-client/DimiNative/src/renderer/components/Sidebar.vue`、`Sidebar.styles.ts`、`styles/theme.ts`、`icons.ts`。

### A. 结构缺失（需要新增）

| # | 差异 | codex（实测） | dimi 现状 |
|---|---|---|---|
| A1 | 模式切换品牌行 | 按钮 = 「ChatGPT」+ chevron（aria「切换模式…」、hover 0.08、active 0.15、open 0.08），右侧搜索 + 优先级按钮 | brand 行只有「Dimi」+ chevronDown + 一个 newChat 图标按钮；无搜索/优先级、无 hover/active 态 |
| A2 | 分组标题右侧操作 | 每组合名行 hover/聚焦时出现 24×24 操作按钮（项目：ellipsis + plus；最近：ellipsis + filter + newChat） | 无 |
| A3 | 文件夹行（cwd 树） | 「项目」section 下每个目录是 folder row：文件夹图标 16×16 + 名称 + hover 出现「项目操作」「在该目录开始新聊天」；会话项嵌套缩进 | dimi 把 cwd 直接渲染成扁平「分组标题 + 会话项」，无文件夹图标、无 hover 操作、无树形结构 |
| A4 | 会话项 hover 操作 | 置顶 20×20 + 归档 20×20（右侧 52px 覆盖层，opacity 0→100） | 无 |
| A5 | 会话项默认右侧徽标 | 20×20 badge + 双箭头图标 14×14（0.498 色），hover 时隐藏 | 无 |
| A6 | 会话项 hover 后缀 | 标题后「工作」标签（hidden → group-hover:inline） | 无 |
| A7 | 标题 marquee | 会话标题 hover 时滚动动画 | 无 |
| A8 | 滚动区 mask | 顶部 1px + 底部 40px 淡出渐变 | 无 |
| A9 | 会话列表 1px 间距 + 组内结构 | 项目间 gap-px（1px）、组内 pt-0.5/pb-2、组间 16px | dimi sessionGroup gap 4px，结构不同 |

### B. 数值/样式差异

| # | 属性 | codex（实测） | dimi 现状 |
|---|---|---|---|
| B1 | 新对话行高 | 29px（站点/已安排/插件 30px） | navItem 统一 30px |
| B2 | 导航项文字行高 | 14px/21px | `font.xs`=14px，`lineHeight: 20px` |
| B3 | 导航/会话文字截断 | mask 右缘淡出（text-fade-truncate） | `text-overflow: ellipsis` |
| B4 | 会话项文字行高 | 14px/20px（leading-5） | 14px/21px（`font.xsLh`） |
| B5 | 会话项文字起点 | x=40（8 + 16 预留槽 + 8 gap） | x=16（8 padding + 8 gap，无预留槽） |
| B6 | 会话项选中态 | bg `rgba(255,255,255,0.08)` + 纯白字 | `hover5` = `rgba(255,255,255,0.05)` + 白字 |
| B7 | 分组 chevron | 默认 opacity 0，hover 标题行显示，折叠 rotate-90，transition 0.15s | 始终可见，无 hover 逻辑 |
| B8 | 底部高度 | 46px，发丝线 1px `rgba(255,255,255,0.1)` | padding 6px 8px 推算 ≈43px，发丝线 0.5px |
| B9 | 用户行高 | 29px | padding 5px + lh 21px → ≈31px |
| B10 | 底部按钮图标 | 帮助「?」18×18 | gear 齿轮 18×18（且 title=Settings，点击行为不同） |
| B11 | brand 字体族 | "OpenAI Sans" 优先 | `font.family` 无 OpenAI Sans |
| B12 | brand 字重 | 按钮 500 / 文字 span 600 | 容器 600 |
| B13 | cursor | 全部 `default` | 交互元素 `pointer` |
| B14 | scroll 底部留白 | 54px（清 footer） | 16px |
| B15 | header block 底距 | 1px | sidebarTop `padding-bottom: 4px` |

### C. 图标差异

| # | 差异 | 说明 |
|---|---|---|
| C1 | brand chevron path 错误 | dimi 用 `chevronDown`（vb 0 0 20 20）；codex 用 `sectionChevron`（vb 0 0 20 21）path。渲染尺寸 14×14 两者一致 |
| C2 | 缺图标 | search、priority（可复用 gear path0 + 星）、filter、folder、pin、archive、badge 双箭头 均不在 icons.ts |
| C3 | dimi brandActions 按钮 | newChat 图标按钮在 codex 中不存在（新对话在导航行 + 最近标题行） |

### D. 已一致（无需改动）

- sidebar 宽 275px、背景 `rgba(40,40,40,0.7)`、padding-top 46px ✓
- 导航项：padding 5px 8px、圆角 12.5px、icon 16×16、gap 8px、文字 0.85、hover 0.08 ✓
- 分组标题基础：14/21 w500、0.498 色、padding 2px 4px 2px 0、圆角 10px、minHeight 25px、chevron 14×14 ✓
- resize handle：16px 宽、top -46、right 偏移后同位置、z-20、cursor col-resize、中线 1px 默认透明 ✓
- 底部：帮助按钮 32×32、radius 10、icon 18×18（dimi 有 `help` path ✓，只是被 gear 占用）
- 会话项：高 30px、radius 12.5px、hover 0.08、无左侧可见图标（dimi 也没有，仅缺 16px 预留槽缩进）✓ 部分
- 图标：newChat/sites/scheduled/plugins/sectionChevron/ellipsis/plus/help path 与 codex 完全一致 ✓
