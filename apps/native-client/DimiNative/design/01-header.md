# 01 · Header（顶栏）设计逆向

> 目标：作为 dimi native-client 像素级复刻 codex Header 模块的**唯一设计依据**。
> 测量环境：codex（ChatGPT.app）桌面客户端，深色主题，窗口 1686×960，DPR 1，会话标题「论文arXiv 2605.28975解析」。
> 测量方式：CDP `Runtime.evaluate` + `getBoundingClientRect()` / `getComputedStyle()`（CDP Input 鼠标/键盘事件会挂起，故 hover 仅实测了 1 个按钮，其余标注）。
> 坐标系：`getBoundingClientRect()` 的 x/y 为视口坐标（header 本身 fixed 于顶部）。

---

## 0. 总览（已验证，修正了 CODEX_DESIGN.md 的错误）

| 属性 | 值 | 来源 |
|---|---|---|
| header 元素 | `HEADER._Header_khftr_1`（header 内唯一 `position:fixed` 且高 46px 的元素） | 实测 DOM |
| 高 | **46px** | 实测 |
| 宽 | 1686px（= 视口宽） | 实测 |
| 定位 | `position:fixed; top:0; left:0; right:0` | 实测 computed |
| z-index | **30** | 实测 computed |
| 背景 | `rgba(0, 0, 0, 0)`（全透明） | 实测 computed |
| 边框 | `0px solid`（无边框，无底部分隔线） | 实测 computed |
| backdrop-filter | none | 实测 computed |
| box-shadow | none | 实测 computed |
| padding | 0 | 实测 computed |
| pointer-events | **none**（header 整体不接事件；每个按钮自身 `pointer-events:auto`） | 实测 computed |
| user-select | none | 实测 computed |
| display | flex；flex-direction: row；align-items: center | 实测 computed |
| 字体族 | `-apple-system, "system-ui", "Segoe UI", sans-serif` | 实测 computed（**没有 PingFang SC**） |

> ⚠️ CODEX_DESIGN.md 原写「圆角 10、分享 pill 圆角 9999、More 带可见边框、菜单按钮在 x=0」——**均与实测不符**，以本文档为准（详见 §7 dimi 差距）。

### 布局分区（当前 sidebar 打开、窗口 1686px）

```
0            88   120  152  180        275/283                ~475  494    1544   1610 1616 1644 1650 1678 1686
├────────────┼────┼────┼────┼──────────┼──────────────────────┼─────┼──────┼──────┼────┼────┼────┼────┼──────┤
│ (空 88px)  │隐藏│返回│前进│           │ 标题按钮              │More │      │ 分享 │    │摘要│    │切换│ (8px)│
│ safe-left  │    │    │    │ 左区容器  │ 主区 (flex-1)        │     │      │ pill │    │固定│    │侧栏│      │
│            │    │    │    │ (275px)   │                      │     │      │      │    │摘要│    │    │      │
└────────────┴────┴────┴────┴──────────┴──────────────────────┴─────┴──────┴──────┴────┴────┴────┴────┴──────┘
```

- 左区容器：`width:275px; min-width:180px`（inline style），`padding-left: max(var(--spacing-token-safe-header-left), 0.5rem)`。
- **`--spacing-token-safe-header-left: 88px`** 由根应用 div（`DIV.relative.flex.flex-col`）的内联 style 设置——这是左区 88px 空白的来源（疑似为 macOS 红绿灯/安全区预留）。
- 主区容器：`ms-2`（margin-left 8px）→ x=283 = 275 + 8。
- 右区容器：`ms-auto`（推右），`gap-1.5`（6px）。
- 最右容器：`width:0; min-width:36px`，`pe-2`（padding-right 8px）。

---

## 1. 左区（sidebar 上方 275px 区域）

### 1.0 「菜单」按钮（切换侧边栏 @ x=0）

| 属性 | 值 | 来源 |
|---|---|---|
| 存在性 | **当前不可见**。x=0 处只有 `invisible pointer-events-none fixed top-0 left-0` 的测量层（w=36），内含一个 `aria-label="切换侧边栏"` 的按钮（28×28），但 `visibility:hidden` | 实测 DOM |
| 隐藏 sidebar 后是否出现 | **无法观察**（CDP Input 挂起，无法点击） | 无法观察 |
| 结论 | 在「sidebar 打开」这一状态下，**header 左上角没有可见菜单按钮**，左区第一个可见按钮是「隐藏边栏」（x=88） | 实测 |

### 1.1 按钮组（隐藏边栏 / 返回 / 前进）

| 属性 | 值 | 来源 |
|---|---|---|
| 组容器 | `inline-flex h-full items-center gap-1.5`（x=88, y=0, w=187, h=46） | 实测 |
| 组内按钮容器 | `flex items-center gap-1`（x=88, y=9, w=92, h=28），gap = **4px**（gap-1） | 实测 |
| 按钮垂直位置 | y=9（(46−28)/2，垂直居中） | 实测 |

| 按钮 | x | 尺寸 | aria-label |
|---|---|---|---|
| 隐藏边栏 | **88**（88..116） | 28×28 | `隐藏边栏` |
| 返回 | **120**（120..148） | 28×28 | `返回` |
| 前进 | **152**（152..180） | 28×28 | `前进` |

**前进按钮当前 disabled**（无前进历史）：`opacity:0.4; cursor:not-allowed; disabled:true`。实测 computed。

### 1.2 通用图标按钮样式（隐藏边栏/返回/前进；同款也用于右区最右「切换侧边栏」）

| 属性 | 值 | 来源 |
|---|---|---|
| 尺寸 | 28×28（`aspect-square` + `h-token-button-composer`） | 实测 |
| padding | 0（类名 `px-2 py-0 !px-0` 最终 0） | 实测 computed |
| 边框 | `1px solid rgba(0,0,0,0)`（`border-transparent`，始终透明） | 实测 computed |
| 圆角 | **12.5px**（`rounded-lg` → 该 app 的 token 为 12.5px） | 实测 computed |
| 背景（default） | `rgba(0,0,0,0)` | 实测 computed |
| 图标颜色 | `rgba(255,255,255,0.498)`（`text-token-text-tertiary`） | 实测 computed |
| 图标尺寸 | **16×16**（svg class `icon-xs`；svg 属性 viewBox `0 0 20 20`，fill 属性 `none` → currentColor 继承按钮色） | 实测 |
| 字号/字重/行高（按钮自身） | 14px / **445** / 18px | 实测 computed |
| cursor | **default**（`cursor-interaction` 在 electron 下解析为 default；不是 pointer） | 实测 computed |
| transition | `all 0s ease`（**无过渡动画，状态即时切换**） | 实测 computed |

### 1.3 图标 path（与 dimi `icons.ts` 对应关系）

| 按钮 | 图标 | svg class | viewBox | 说明 |
|---|---|---|---|---|
| 隐藏边栏 | 面板收起图标 | `icon-xs` | 0 0 20 20 | path 与 dimi `hideSidebar` 相同（已捕获） |
| 返回 | 左箭头 | `icon-xs` | 0 0 20 20 | path 与 dimi `back` 相同 |
| 前进 | 右箭头 | `icon-xs -scale-x-100` | 0 0 20 20 | **与返回同一 path**，CSS `-scale-x-100` 水平翻转（dimi 用 `forward` 图标 + `scaleX(-1)`，path 相同，等效） |

---

## 2. 主区（x=283 起，flex-1）

| 属性 | 值 | 来源 |
|---|---|---|
| 容器 | `ms-2 flex h-full min-w-0 flex-1`（x=283, w=1367, h=46），margin-left **8px** | 实测 |
| 内部 grid | `grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4`（列间距 **16px**） | 实测 computed |
| 内层 | `ps-2`（padding-left 8px）→ 内容起点 x=291 | 实测 |

### 2.1 会话标题（是一个 BUTTON，不是 span）

| 属性 | 值 | 来源 |
|---|---|---|
| 元素 | `<button>`（可点击，hover 有背景） | 实测 DOM |
| 位置 | x=289, y=11（= 283 + 8 − 2；`ps-2` 8px + 自身 `-ms-0.5` **margin-left: -2px**） | 实测 |
| 尺寸 | 高 **24px**；宽内容自适应（本会话标题 12 字符 → 宽 186.75px，含 padding） | 实测 |
| padding | `0 6px`（px-1.5） | 实测 computed |
| 字体 | **14px / 24px（leading-6）/ weight 500（font-medium）** | 实测 computed |
| 颜色 | `rgb(255,255,255)`（`text-token-foreground`） | 实测 computed |
| 文本对齐 | left；`truncate`（overflow hidden + text-overflow ellipsis + white-space nowrap） | 实测 computed/类名 |
| max-width | **320px**（`max-w-[320px]` 在外层 div，宽 184.8px 时未截断） | 实测 |
| 圆角 | **10px**（`rounded-md`） | 实测 computed |
| hover 背景 | `rgba(255,255,255,0.08)`（`hover:bg-token-list-hover-background`，token 同 §3.1；未逐按钮实测，类名同 1.1 实测按钮） | 类名 + 实测 token |
| 光标 | default | 实测 computed |
| aria/title | 无（纯标题按钮） | 实测 |
| 点击行为 | 打开会话标题菜单/跳转，**无法观察**（Input 挂起） | 无法观察 |
| 外层 | 标题外层另有 `flex min-w-0 items-center gap-0.5 ps-2` 容器（x=283, w=192.8）与 `text-md ... truncate` 容器（x=283, w=1239, h=28, y=9） | 实测 |

### 2.2 More 按钮（aria-label「ChatGPT 对话操作」）

| 属性 | 值 | 来源 |
|---|---|---|
| 位置 | x=493.8, y=9（标题按钮右缘 ~475.8 + grid gap 16px + 偏移 ≈ 18px） | 实测 |
| 尺寸 | 28×28 | 实测 |
| padding | **4px**（`electron:p-1`；web 端 `p-0.5`=2px，**electron 端 4px**） | 实测 computed |
| 边框 | `1px solid rgba(0,0,0,0)`（**透明，无可见边框**） | 实测 computed |
| 圆角 | **10px**（`rounded-full electron:rounded-md`；web 端是 pill，**electron 端 10px**） | 实测 computed |
| 背景（default） | transparent | 实测 |
| 图标颜色 | `rgba(255,255,255,0.498)` | 实测 computed |
| 图标 | **18×18**（svg class `icon-sm`），viewBox `0 0 21 21`，3 个圆点 path（与 dimi `icons.ellipsis` 相同） | 实测 |
| 字号/字重/行高 | 14px / 500 / 21px | 实测 computed |
| hover 背景 | `rgba(255,255,255,0.08)`（`enabled:hover:bg-token-list-hover-background`） | 类名 + 实测 token |
| active 背景 | `rgba(255,255,255,0.15)`（`enabled:active:bg-token-foreground/15`） | 类名（未实测） |

---

## 3. 右区（x=1544 起，`ms-auto`）

| 属性 | 值 | 来源 |
|---|---|---|
| 容器 | `ms-auto flex shrink-0 items-center gap-1.5`（x=1544, y=9, w=100, h=28），gap **6px** | 实测 |
| 内容 | 分享 pill（66px）+ gap 6 + 固定摘要（28px）= 100px | 实测 |

### 3.1 分享 pill（aria-label「分享」）

| 属性 | 值 | 来源 |
|---|---|---|
| 位置 | x=1544, y=9 | 实测 |
| 尺寸 | **66×28** | 实测 |
| padding | `0 8px` | 实测 computed |
| 边框 | `1px solid rgba(0,0,0,0)`（透明） | 实测 computed |
| 圆角 | **12.5px**（`rounded-lg`，**不是 pill 9999**） | 实测 computed |
| 背景（default） | transparent | 实测 |
| 文字颜色 | **`rgb(255,255,255)`**（`text-token-text-tertiary` 被 `enabled:text-token-text-primary` 覆盖） | 实测 computed |
| 文字 | 「分享」，14px / **445** / 18px（text-base leading-[18px]） | 实测 computed |
| 图标 | 上传/分享图标 **16×16**（`icon-xs`），viewBox `0 0 20 20`，与 dimi `icons.share` 相同 | 实测 |
| 内部布局 | icon x=1553（= 1544 + border 1 + padding 8），16×16；gap **4px**；文字 x=1573，28×18（右缘 1601，按钮内右侧留 8px） | 实测 |
| hover | 背景 `rgba(255,255,255,0.08)`；文字保持白色（`enabled:hover:text-token-text-primary`） | 类名 + 实测 token |
| active | 背景 `rgba(255,255,255,0.15)` | 类名（未实测） |

### 3.2 固定摘要按钮（aria-label + title「切换固定摘要」）

| 属性 | 值 | 来源 |
|---|---|---|
| 位置 | x=1616, y=9 | 实测 |
| 尺寸 | 28×28，圆角 **12.5px**，边框 1px 透明，padding 0 | 实测 |
| 图标 | 竖排三点列表/摘要图标 **16×16**（`icon-xs`），viewBox `0 0 20 20`，`fill="currentColor"` | 实测 |
| 图标颜色 | `rgb(255,255,255)`（`text-token-foreground`） | 实测 |
| **当前状态（ON）** | 背景 `bg-token-foreground/5` = `oklab(0.999994 0.0000455678 0.0000200868 / 0.05)` ≈ **white 5%**（本会话开了固定摘要，按钮带淡白底） | 实测 computed |
| hover（ON 态） | `enabled:hover:bg-token-foreground/10` ≈ white 10% | 类名（未实测） |
| active | `enabled:active:bg-token-foreground/15` ≈ white 15% | 类名（未实测） |
| OFF 态 | **无法观察**（当前为 ON；切换需要点击，Input 挂起） | 无法观察 |

### 3.3 最右按钮（aria-label「切换侧边栏」）

| 属性 | 值 | 来源 |
|---|---|---|
| 位置 | x=1650, y=9（最右容器 `pe-2` padding-right 8px → 1650 + 28 + 8 = 1686） | 实测 |
| 尺寸 | 28×28，圆角 12.5px，边框 1px 透明，padding 0，同 §1.2 | 实测 |
| 图标 | columns/侧栏图标 **16×16**（`icon-xs rotate-180`），viewBox `0 0 20 20`，**CSS 旋转 180°** | 实测 |
| 图标 path | 与 dimi `icons.menu` 相同（columns 图标） | 实测 path 对比 |
| hover/active | 同 §1.2（`enabled:hover:bg-token-list-hover-background` = white 8%） | 类名 + 实测 token |
| 行为 | 切换 sidebar 显隐（与左区旧的「菜单按钮」同一 aria-label） | 实测 aria |

---

## 4. 状态表（default / hover / active / disabled）

> transition 实测为 `all 0s ease` —— **所有状态即时切换，无过渡动画**。

| 按钮 | default 背景 | hover 背景 | active 背景 | disabled | 颜色变化 |
|---|---|---|---|---|---|
| 隐藏边栏 | transparent | `rgba(255,255,255,0.08)`（**实测**） | `rgba(255,255,255,0.15)`（类名，未实测） | — | 颜色不变（保持 0.498） |
| 返回 | transparent | white 8%（类名） | white 15%（类名） | — | 颜色不变 |
| 前进 | transparent | —（disabled 不触发） | — | `opacity:0.4; cursor:not-allowed`（实测） | 颜色不变 |
| 标题 | transparent | white 8%（类名） | —（无 active 类） | — | 文字保持白色 |
| More | transparent | white 8%（类名） | white 15%（类名） | — | 颜色不变 |
| 分享 | transparent | white 8%（类名） | white 15%（类名） | — | 文字保持白色（`enabled:hover:text-token-text-primary`） |
| 固定摘要（ON） | white 5%（实测） | white 10%（类名） | white 15%（类名） | — | 保持白色 |
| 切换侧边栏 | transparent | white 8%（类名） | white 15%（类名） | — | 颜色不变 |

- **hover 颜色变化：无**。图标保持 default 颜色（普通按钮 0.498，分享/固定摘要 白色），只有背景变化。
- focus：普通按钮 `focus:outline-none`（无可见 focus 样式）；标题按钮 `focus-visible:bg-token-list-hover-background focus-visible:outline-none`（与 hover 同底）。focus 视觉**无法实测**（键盘事件挂起），以上为类名事实。
- cursor：除 disabled 外全部 `default`（不是 pointer）。

---

## 5. 图标系统（header 相关）

| 图标 | 尺寸类 | 渲染尺寸 | viewBox | dimi icons.ts 对应 |
|---|---|---|---|---|
| 隐藏边栏 | `icon-xs` | 16×16 | 0 0 20 20 | `hideSidebar` ✓ |
| 返回 | `icon-xs` | 16×16 | 0 0 20 20 | `back` ✓ |
| 前进 | `icon-xs -scale-x-100` | 16×16 | 0 0 20 20 | `forward`（= back path + scaleX(-1)）✓ |
| 标题（无图标） | — | — | — | — |
| More | `icon-sm` | **18×18** | 0 0 21 21 | `ellipsis` ✓（3 path） |
| 分享 | `icon-xs` | 16×16 | 0 0 20 20 | `share` ✓ |
| 固定摘要 | `icon-xs` | 16×16 | 0 0 20 20 | 需捕获（竖排三点） |
| 切换侧边栏 | `icon-xs rotate-180` | 16×16 | 0 0 20 20 | `menu` ✓ |

- 所有 svg `fill="none"`（继承 currentColor）或 `fill="currentColor"`，无描边属性。
- 固定摘要的 icon path 未在 dimi `icons.ts` 中（dimi 目前用 `dots` 图标放在 Refresh 按钮位置）。

---

## 6. 行为 / 可访问性（aria-label、title、点击行为）

| 按钮 | aria-label | title | 点击行为 |
|---|---|---|---|
| 隐藏边栏 | 隐藏边栏 | 无 | 隐藏 sidebar |
| 返回 | 返回 | 无 | 回退到上一会话 |
| 前进 | 前进 | 无 | 前进到下一会话（当前 disabled） |
| 标题 | 无 | 无 | **无法观察**（Input 挂起）；是 button，hover 有底 |
| More | ChatGPT 对话操作 | 无 | 打开对话操作菜单 |
| 分享 | 分享 | 无 | 打开分享菜单 |
| 固定摘要 | 切换固定摘要 | 切换固定摘要 | 开关固定摘要面板（当前开） |
| 切换侧边栏 | 切换侧边栏 | 无 | 切换 sidebar 显隐 |

- codex **全部用 aria-label，不用 title**（唯一例外：固定摘要按钮两者都有）。

---

## 7. 窄窗口 / 隐藏 sidebar 行为

- 窄窗口：**无法观察**（改视口需 CDP 仿真/调整窗口，Input 类调用会挂起，未执行）。
- 隐藏 sidebar：**无法观察**（需点击按钮触发，Input 挂起）。已知事实：sidebar 打开时 `--spacing-token-safe-header-left: 88px`（根 div inline style），左组落在 x=88；x=0 的「切换侧边栏」按钮只存在于不可见测量层。sidebar 隐藏后的 header 布局无法观察。

---

## 8. dimi 差距（dimi `HeaderBar.vue` + `HeaderBar.styles.ts` + `styles/theme.ts` + `icons.ts`）

> dimi 文件：`apps/native-client/DimiNative/src/renderer/components/HeaderBar.vue`、`HeaderBar.styles.ts`、`styles/theme.ts`、`icons.ts`。

### 8.1 布局差异

| # | 项 | codex 实测 | dimi 当前 | 影响 |
|---|---|---|---|---|
| L1 | 左区菜单按钮 | **没有可见菜单按钮**；左区第一个按钮是「隐藏边栏」@x=88（安全左距 88px 由 `--spacing-token-safe-header-left` 提供） | `headerSide` 里先渲染 menu 按钮 @x=0（`title="Menu"`，切 sidebar），再 `marginLeft:56` 把隐藏边栏推到 x=88 | 多了一个按钮；安全左距的语义不同（dimi 用 margin 硬算） |
| L2 | 右区最右按钮 | 「切换侧边栏」28×28 @x=1650（columns icon rotate-180） | **没有**；dimi `headerRight` 用 `paddingRight:42` 留空白 | 缺按钮 + 空白语义不同 |
| L3 | 固定摘要按钮 | 有 @x=1616（ON 态 white 5% 底，图标为竖排三点） | 无；dimi 在 x=1616 位置放「Refresh」按钮（`icons.dots`，点击刷新会话列表） | 缺按钮/图标/状态样式；行为也不同 |
| L4 | 右区间距 | gap **6px**（gap-1.5），最右容器 `pe-2`（8px） | gap 6 ✓，但 `paddingRight:42`（应为 8px + 28px 按钮位） | 右缘留白语义错误 |
| L5 | 主区左距 | `ps-2`（8px）+ 标题 `-ms-0.5`（-2px）→ 标题 x=289（相对主区 283） | `paddingLeft:14` → 标题 x=297 | 差 8px |

### 8.2 样式差异

| # | 项 | codex 实测 | dimi 当前 | 影响 |
|---|---|---|---|---|
| S1 | 图标按钮圆角 | **12.5px** | `borderRadius:10`（theme `iconBtnRadius: '12.5px'` 但 HeaderBar 未用） | 圆角不对 |
| S2 | 图标按钮 hover 背景 | `rgba(255,255,255,0.08)` | `colors.hover` = `rgba(255,255,255,0.05)` | hover 底亮度不足 |
| S3 | 图标按钮 hover 颜色 | **不变**（保持 0.498） | `'&:hover': { color: colors.text }` → 变纯白 | hover 时图标变色，与 codex 不符 |
| S4 | 普通按钮字号/字重 | 14px / **445** / 18px | 14px / 继承 / 18px（未设 445） | 字重 token 缺失 |
| S5 | cursor | `default`（disabled 除外） | `cursor:pointer` | 鼠标样式不符 |
| S6 | 分享 pill 圆角 | **12.5px**（rounded-lg） | `borderRadius:9999` | 形状不符（不是胶囊） |
| S7 | 分享 pill padding | `0 8px` | `0 10px` | 宽 66 vs dimi 应 66（8+16+4+28+8+2）；padding 10 → 74px 宽 | 
| S8 | 分享文字 | 14px / 445 / 18px | `font.sm` = **13px** / 18px | 字号不符 |
| S9 | More 按钮边框 | **透明**（同其他按钮） | `iconBtnBordered` → `borderColor: rgba(255,255,255,0.08)` | 多了一条可见边框（CODEX_DESIGN.md 原记载有误，以实测为准） |
| S10 | More 按钮 padding | electron 端 4px | 未设（flex center） | 4px padding 影响点击/视觉 |
| S11 | 标题元素 | **`<button>`**（hover 白 8% 底、圆角 10px、可点击） | `<span>`（不可点击，无 hover 底） | 交互差异 |
| S12 | 标题字号/行高/字重 | 14px / **24px** / **500** | 14px / 24px / 500 ✓ | 无差异 |
| S13 | 标题 max-width | 320px ✓ | 320 ✓ | 无差异 |
| S14 | 前进 disabled | `opacity:0.4; cursor:not-allowed`（真实 disabled 属性） | `'&:disabled': { opacity:0.4 }` + 无 disabled 逻辑（`navSession` 仅在列表内循环） | 前进按钮在 dimi 永不 disabled |
| S15 | transition | `all 0s`（无动画） | 无显式 transition（默认 0s） | 一致 |
| S16 | 字体族 | `-apple-system, "system-ui", "Segoe UI", sans-serif` | 多了 `PingFang SC, Microsoft YaHei` | 中文回退不同（dimi 更贴近中文系统，可保留） |
| S17 | header pointer-events | header 整体 none，按钮 auto | header 未设（默认 auto） | 拖拽/穿透行为差异 |

### 8.3 行为/可访问性差异

| # | 项 | codex 实测 | dimi 当前 | 影响 |
|---|---|---|---|---|
| A1 | aria-label vs title | 全部用 **aria-label**（无 title，固定摘要除外） | 全部用 **title**（Menu/隐藏边栏/Back/Forward/More/Share/Refresh） | 无障碍/工具提示来源不同 |
| A2 | More 按钮语义 | aria「ChatGPT 对话操作」 | title「More」，点击 `Msg.HelpOpen()` 打开帮助弹窗 | 行为占位（设计文档只记录视觉与 aria，行为需产品确认） |
| A3 | 分享按钮行为 | 分享菜单 | 点击 `Msg.HelpOpen()` 打开帮助弹窗 | 行为占位 |
| A4 | 固定摘要 | 开关固定摘要面板（ON 态带 5% 底） | 无此按钮（Refresh 占位） | 缺功能 |
| A5 | 返回/前进语义 | 会话历史导航 | dimi `navSession(delta)` 在会话列表中前后切换 | 语义接近（dimi 用列表顺序模拟历史） |
| A6 | 标题按钮 | 可点击（hover 有底） | span 不可点击 | 交互差异 |

### 8.4 建议的 dimi 改动清单（按影响排序）

1. **S1** 图标按钮圆角 10 → 12.5px（`HeaderBar.styles.ts` `iconBtn` 直接用 theme 的 `iconBtnRadius`）。
2. **S6/S7/S8** 分享 pill：圆角 9999 → 12.5px、padding 10 → 8px、字号 13 → 14px/445。
3. **S9** 删除 `iconBtnBordered`（More 不再带边框）。
4. **S3/S2** hover：背景 `0.05 → 0.08`、去掉 hover 变色。
5. **S11** 标题 span → button（hover 白 8% 底、圆角 10px、padding 0 6px、margin-left -2px）。
6. **L2/L3/L4** 右区改为「分享 pill + 固定摘要（含 ON 态白 5% 底）+ 切换侧边栏」三件套，去掉 `paddingRight:42`。
7. **L1** 左区去掉 x=0 菜单按钮，改为安全左距（可硬编码 88px 或引入 `--spacing-token-safe-header-left` 变量）。
8. **S5** cursor: pointer → default。
9. **S14** 前进按钮加 disabled 状态（opacity 0.4 + not-allowed）。
10. **A1** title → aria-label。
11. **S4** 按钮字重 445 进 theme token。

### 8.5 留待后续确认

- 隐藏 sidebar 后 header 的布局（x=0 菜单按钮是否出现）——**无法观察**（Input 挂起）。
- 固定摘要 OFF 态的样式——**无法观察**（当前 ON）。
- 窄窗口下的省略/压缩行为——**无法观察**。
