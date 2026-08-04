# Codex 桌面客户端：全局布局 + 主题系统设计逆向

> 证据来源：`/tmp/codex_asar/webview/assets/` 的 `app-initial-iBPGfcXU.js`（主 bundle）、`app-44wrUC9v.css`（主 CSS）、`app-initial-BSHZIbh1.css`（模块 CSS），以及运行中 codex 桌面窗口的 CDP 实测（窗口 1686×960，`electron-dark electron-opaque` 主题，暗色）。
> 所有"实测"值均为 `getComputedStyle` / `getBoundingClientRect` 结果；测不到或读不到的一律标注「无法确定」。
> 结论先行：**消息列外层 max-width 是 48rem（768px），实测 736px 是减去两侧 px-toolbar（16px×2）后的内容盒**。不是 42rem。

---

## 0. 窗口与主题载体

- 运行时 `<html class="electron-dark electron-opaque">`；CSS 通过 `.electron-dark` / `.electron-light` 切换主题变量，`data-codex-window-type=electron|browser|chrome-extension` 区分窗口宿主（电子 / 网页 / 扩展）。
- **主题变量分两层注入**：
  1. 静态层：CSS 里 `.electron-dark{...}` / `.electron-light{...}` 定义 `--color-*` 基础色板（white-alpha / gray 阶梯）；
  2. 动态层：JS 把一个主题对象（含 `surfaceUnder`、`controlBackground`、`elevatedPrimary` 等派生色）**以 inline style 写到 `<html>` 上**（`"--color-background-surface-under": e.surfaceUnder` 等）。所以 `#141414`、`#2d2d2d`、`#363636` 不以字面量出现在静态 CSS 里，只有运行时才能读到——实测值见 §3。
- 根断点：Tailwind v4 语义 `@media (width>=20rem / 40rem / 48rem / 64rem / 80rem / 96rem)`；主内容区用 CSS 容器查询 `container: app-shell-main-content/inline-size`。

---

## 1. 布局结构

### 1.1 层级树（CDP 实测，1686×960）

```
body  (bg: --color-background-surface-under #141414)
└─ div.app (block, 1686×960)
   ├─ span.hidden
   ├─ div.relative.flex.flex-col (1686×960)                    ← 应用容器
   │  ├─ [可选] 应用菜单栏行（app-shell-chrome / application-menu）
   │  └─ div.relative.isolate.flex.max-h-full.min-h-0.w-full.flex-1   ← shell 行（flex row）
   │     ├─ aside.app-shell-left-panel (275×960, x=0)          ← 侧边栏（停靠态）
   │     │   · padding-top: var(--height-toolbar)=46px（无应用菜单时，让出固定 header）
   │     │   · background: color(srgb 0.157 0.157 0.157 / 0.7)  = #282828 @70%（editor-background 的 70% 混合）
   │     │   ├─ div.max-w-full.overflow-hidden（width/minWidth = 侧栏宽）
   │     │   ├─ 顶部 h-toolbar 拖拽条（46px, .draggable）
   │     │   └─ 拖拽手柄（absolute inset-y-0 -end-1 z-2 w-2 = 8px 命中区，cursor-col-resize）
   │     └─ main._MainContentSurface (1411×960, x=275)         ← 主内容面
   │        · background: var(--color-token-main-surface-primary) = #181818
   │        · box-shadow: var(--elevation-prominent)（0.5px hairline + 双层黑阴影）
   │        · [data-app-shell-main-surface=default]：overflow:hidden；win32 还带 border-top-left-radius: radius-lg
   │        ├─ header._Header (fixed, 46px, z-30, 透明, -webkit-app-region:drag, pointer-events:none)
   │        │   · [application-menu-bar=false]: inset-inline:0; top:0
   │        │   · [application-menu-bar=true]:  top: var(--height-toolbar-sm)=36px
   │        ├─ div.relative.isolate.flex.min-h-0.flex-1.overflow-hidden (1411×960)
   │        │  └─ div._MainContentViewport (flex col, min-width:0, container: app-shell-main-content/inline-size)
   │        │     · --thread-content-top-inset: calc(spacing*8) = 32px
   │        │     · --thread-floating-content-top/bottom-inset: calc(spacing*3) = 12px
   │        │     · [data-app-shell-right-panel-full-width=true]: flex:none; width:0; overflow:hidden
   │        │     └─ div._MainContentFrame (margin-top: var(--height-toolbar)=46px; flex:1)
   │        │        · [data-app-shell-thread-edge-divider=true]: border-top:.5px solid border-heavy
   │        │        └─ … 线程列 …
   │        │           └─ div.thread-scroll-container (1411×913, y=47, overflow-y:auto, overflow-x:hidden)
   │        │              · display:flex; flex-direction: column-reverse（底部锚定的聊天流）
   │        │              └─ div.flex.min-h-full.shrink-0.flex-col.justify-start（内容列，高度>视口）
   │        │                 ├─ …消息…
   │        │                 └─ div.sticky.bottom-0.z-10.mt-auto.w-full.pb-4 (1411)   ← composer 停靠槽
   │        │                    └─ div.relative.z-10.flex.flex-col.mx-auto.w-full.max-w-(--thread-content-max-width).px-toolbar (768, x=439)
   │        │                       └─ form.relative.flex.flex-col (736, x=455)
   │        │                          └─ div.relative.flex.w-full.flex-col.gap-2 (736)
   │        │                             └─ div.composer-surface-chrome (736×98, y=846)
   │        │                                · bg: --color-background-control #2d2d2d @90% + backdrop-filter blur(16px)
   │        │                                · border-radius: 25px（= radius-3xl）
   │        └─ [线程内右浮动内容] div.absolute.top-12px.right-0.bottom-12px.z-40 (316×889, x=1370, 透明)
   └─ span.fixed.inset-0.z-60.mx-auto.my-2.max-w-(--composer-adjacent-max-width) (790×944, x=448, y=8)
      · 这是 composer 的"相邻内容"层（建议卡片/自动补全浮层），pointer-events-none 容器
```

### 1.2 关键尺寸公式

| 区域 | 公式 | 实测/取值 |
|---|---|---|
| 侧边栏宽 | `--spacing-token-sidebar: clamp(240px, 275px, min(520px, calc(100vw - 320px)))`；JS 状态默认 275、min 240、max 520，持久化 `localStorage['sidebar-width']` | 275px |
| 主列宽 | `100% - 侧边栏宽`（flex row，`main` flex:1, min-width:0） | 1686−275=1411px |
| 主面高度 | `100dvh`，无独立 header 占位（header 是 fixed 覆盖层） | 960px |
| header | `--height-toolbar: 46px`；内容帧 `margin-top: var(--height-toolbar)` 让位；边缘加 0.5px 分隔线 | 46px，线程区 y=47 |
| 消息列外层 | `width: min(100%, var(--thread-content-max-width))`，`--thread-content-max-width: 48rem`（=768px），`mx-auto` 居中，`px-toolbar` 16px×2 | 768px，内容盒 736px |
| composer 表面 | 与消息列内容盒对齐：768 − 2×16 = **736px**；垂直 `sticky bottom-0` + `pb-4`(16px) | 736×98，底距窗口 16px |
| composer 相邻层 | `--composer-adjacent-max-width = calc(48rem + 2×overhang − 2×inset)`，`--composer-inline-overhang: calc(spacing×6)=24px`，`--home-composer-inline-inset: calc(spacing×3.25)=13px` → 768+48−26 = **790px**，`mx-auto my-2` | 790×944 |
| 线程内右浮动面板 | `absolute top/bottom: 12px, right:0, z-40`，宽由内容定（实测容器 316px） | 316×889 |
| 线程滚动 | `overflow-y:auto; overflow-x:hidden; flex-direction:column-reverse; scroll-padding-bottom: var(--thread-scroll-padding-bottom)` | 1411×913 |

**验证 736 之谜**：`--thread-content-max-width` 在 bundle 里出现 6 个值——`:root` 基值 `none`、codex 窗口 `body` 覆盖为 `48rem`，另有组件级 arbitrary 类 `[--thread-content-max-width:42rem / 100% / 480px / 500px]`（只用于特定内嵌组件，不是全局线程）。运行时的全局值实测就是 `48rem`。用户实测的 736px = 768 − px-toolbar×2，即消息列**内容盒**宽，不是外层 max-width。

### 1.3 两个侧边栏渲染路径（JSX 证据）

- **停靠态**：`app-shell-left-panel` 作为 shell 行内 flex 子项，条件 `p && (j || L || l)`（minified，j/L/l 语义无法确定），外包 `AnimatePresence`；`paddingTop = w ? '0px' : 'var(--height-toolbar)'`（无应用菜单时 46px）；宽度状态 + `lRr` 手柄（defaultSize 275，`getCurrentSize`/`setSize`/`onResizeEnd` 持久化）。
- **浮动态**：`nYr` 组件在 `_ && !D && !l` 时渲染 `app-shell-floating-left-panel`——`pointer-events-auto fixed bottom-0 left-0 z-[42]`，`top-(--height-toolbar-sm)`(36px) 或 `top-0`；Framer Motion 动画 `initial {opacity:0, x:-8} → {opacity:1, x:0}`；`rounded-lg` + `electron:elevation-prominent`（用 `--elevation-sidebar` 弱化阴影）。宽度仍由 `floatingLeftPanelWidth` 控制，可拖拽（`cursor-col-resize`）。
- 侧边栏开关按钮带 `data-app-shell-sidebar-trigger`，配 pointermove 捕获监听（指针移出触发区自动收起，`closest('[data-app-shell-sidebar-trigger]')`）。
- 宽度动画：`AD = {type:'spring', duration:.5, bounce:.1}`；`prefers-reduced-motion` 时 duration=0。

---

## 2. 间距系统

### 2.1 基准

`--spacing: .25rem`（4px）。整个系统是 **Tailwind v4 的 `calc(var(--spacing) * N)` 派生网格**：`gap-1.5=6px`、`px-2=8px`、`gap-2=8px`、`px-3=12px`、`p-4/px-4/pb-4=16px`、`px-5/py-5=20px`、`p-6=24px`、`p-8=32px`……全部是 4 的整数倍（或半格 1.5/2.5/3.25 等）。

### 2.2 派生 token 表（基准 4px 的"命名化"用法）

| token | 公式 | 值 | 用途 |
|---|---|---|---|
| `--padding-row-x` | spacing×2 | 8px | 行内左右留白（列表项、浮动面板离边） |
| `--padding-row-y` | spacing×1.25 | 5px（electron） | 行内上下留白；`--height-token-nav-row = text-base×1.5 + 2×padding-row-y` = 31px |
| `--padding-toolbar` | spacing×4 | 16px | 消息列横向内边距（`px-toolbar`），composer 表单对齐基准 |
| `--padding-panel-base` | spacing×5 | 20px（electron，浏览器端 3×=12px） | 面板/区块留白（`px-panel`/`pt-panel`/`pb-panel`） |
| `--composer-inline-overhang` | spacing×6 | 24px | composer 相邻内容（建议等）可超出消息列的单侧宽度 |
| `--home-composer-inline-inset` | spacing×3.25 | 13px | home 场景 composer 相邻内容向内的收缩 |
| `--radius-token-composer-single-line` | spacing×5.5 | 22px | 单行 composer 圆角 |
| `--spacing-token-button-composer` | spacing×7 | 28px | composer 按钮尺寸（sm=20px） |
| `--thread-content-top-inset` | spacing×8 | 32px | 线程内容顶部起始偏移 |
| `--thread-floating-content-top/bottom-inset` | spacing×3 | 12px | 线程内右浮动面板上下边距 |
| `--height-toolbar` / `--height-toolbar-sm` | 固定 | 46px / 36px | 主 header / 应用菜单行高 |

### 2.3 层级间距关系（设计意图）

- **一行三档**：紧凑行内 8px（row）→ 区块 16px（toolbar）→ 面板 20px（panel）。控件 28px（composer 按钮）与行高 31px（nav-row）都由 `text × 1.5 + padding×2` 推出，说明**字体尺度与间距尺度是同一个生成器的两个输出**，不是两套拍脑袋的数字。
- **4px 网格是"公约数"**：所有尺寸（含圆角 base、line-height 目标、行高）都能被 4 整除或落在 4 的分数格上（6=1.5×4、13=3.25×4、22=5.5×4），保证任意两个元素拼在一起都对齐。
- **composer 的"超宽但不越界"**：overhang 24px > panel 20px > toolbar 16px——composer 相邻层（790px）刻意比消息列（768px）宽 24px/侧，让"建议/补全"这类辅助内容有一个比正文更宽的呼吸带，但又被 `mx-auto` 钉在列中心；表单本身（736px）严格等于消息列内容盒，形成"正文、表单、辅助层"三个递进宽度。

---

## 3. 主题设计意图

### 3.1 颜色层级（暗色实测值；亮色取 palette 与 CSS 规则）

**文本（同层级也是图标层级）**

| token | 暗色值 | 含义 |
|---|---|---|
| `--color-text-foreground` | `#ffffff` | 正文/标题，100% |
| `--color-text-foreground-secondary` | `rgba(255,255,255,.71)` | 次文本（约 70%） |
| `--color-text-foreground-tertiary` | `rgba(255,255,255,.498)` | 说明/禁用（50%）；`--vscode-descriptionForeground` 与 `--vscode-disabledForeground` 都指向它 |

意图：**用同一把"前景透明度"刻度表达信息层级**——primary/secondary/tertiary 不是三个色相，而是前景色的 100%/70%/50% 不透明度；图标 `--color-icon-primary/secondary/tertiary` 复用同一刻度（90%/70%/50%），保证文字与图标在任何表面上都能按同一优先级呈现。禁用态不新增颜色，直接降到 tertiary。

**表面（surface / under / elevated）**

| token | 暗色实测 | 含义 |
|---|---|---|
| `--color-background-surface` | `#181818`（gray-900） | 主内容面（main surface、侧边栏外背景） |
| `--color-background-surface-under` | `#141414` | 应用最底层（body）；JS 由 `surface + ink 混合` 推导，暗色=比 surface 更黑一档 |
| `--color-background-editor-opaque` | `rgb(40,40,40)`（#282828） | 编辑器/代码区、侧边栏底（侧栏 = editor-background @70%） |
| `--color-background-panel` | `#232323` | 面板层 |
| `--color-background-control` | `rgba(45,45,45,.96)`（#2d2d2d @96%） | 输入框/composer 底 |
| `--color-background-elevated-primary` | `rgba(54,54,54,.96)` | 悬浮层（弹层/浮卡）主底 |
| `--color-background-elevated-secondary` | `rgba(255,255,255,.032)` | 任意表面上的"提亮一档"微调色（白 3%） |

意图：**从上到下 5 档灰阶**（body 20 → surface 24 → panel 35 → editor 40 → control 45），每档约 +6~8 RGB。方向与直觉相反——**离用户越近/越可交互，颜色越亮**：正文站在最暗的画布（#141414）上，输入框（#2d2d2d）比内容面亮，悬浮层（#363636）最亮。elevated-primary 用 96% 半透明灰而不是纯色，让它在任何底色上都能"透出"一档；elevated-secondary 是白 3% 的万能提亮。亮色主题对称：surface=白、under=gray-50、elevated-primary=白 70%。

**边框（light / default / heavy）**

| token | 暗色实测 | 含义 |
|---|---|---|
| `--color-border-light` | `rgba(255,255,255,.042)` | 最弱分隔（约 4%） |
| `--color-border` / `--color-border-default` | `rgba(255,255,255,.084)` | 默认边框（约 8%），滚动条 thumb 复用 |
| `--color-border-heavy` | `rgba(255,255,255,.156)` | 强分隔/描边（约 16%），elevation hairline 复用 |
| `--color-border-focus` | `rgba(131,195,255,.76)` | 焦点环（#83C3FF @76%，亮蓝系） |
| `--color-border-warning/error` | 橙/红 @40%（暗） | 状态边框，与 status 色系同源 |

意图：**边框 = 前景色的低透明度派生**，不是独立色相。light/default/heavy 依次 4%/8%/16%，语义为"可忽略的分隔线 → 结构边缘 → 需要被看见的描边"。因为都挂在 `--color-text-foreground` 上，暗/亮主题各只需一个 alpha 旋钮（暗色混白、亮色混黑），整个边框体系自动翻转。

**强调/状态色**：暗色用 300 级（`blue-300 #339cff`、`green-300 #40c977`、`red-300 #ff6764`、`orange-300 #ff8549`），亮色用 500 级（`#00a240`、`#e02e2a`…）——**暗色降饱和提亮、亮色加深**，保证两种背景下对比度一致；状态底色是强调色低 alpha 大块（如暗色 success 底 = green-400 16%）。link 色用亮蓝 `#83C3FF`（focus 同族）。

**阴影**：`--elevation-stroke: 0 0 0 .5px var(--color-token-border-heavy)`（0.5px 白描边）；`--elevation-prominent: stroke + 0 3px 7.5px rgba(0,0,0,.04) + 0 0 20px rgba(0,0,0,.05)`；`--elevation-sidebar` 同构但更弱（7.5px 的 alpha 0.03、20px 的 0.02）。意图：**"hairline 描边 + 软阴影"代替硬边框**——浮动表面（主内容面、composer、浮动面板）用 0.5px 亮描边切出轮廓，再用两段极淡黑阴影表达高度，比 1px 实线边框"轻"得多。

**滚动条**：`scrollbar-color: var(--color-token-scrollbar-slider-background) transparent`；thumb=border（8%）、hover/active=border-heavy（16%）、track 透明。**滚动条就是边框色的复用**；不定义 `::-webkit-scrollbar` 宽度，保持 macOS 覆盖式滚动条（不占布局宽度）。部分滚动容器另加 `::-webkit-scrollbar{display:none}`（如横向标签条）。

### 3.2 字体层级

| token | 运行时值（electron） | 用途 |
|---|---|---|
| `--text-xs` | 12px（浏览器 11px） | 徽标/辅助 |
| `--text-sm` | 13px（浏览器 12px） | 按钮/次要文本 |
| `--text-base` | 14px | 正文（消息文本实测 14px） |
| `--text-lg` | 16px | 段标题 |
| `--text-xl` | 28px | hero / 空状态大标题 |
| `--text-2xl` | 36px | 更大标题 |
| `--text-heading-sm/md/lg` | 18 / 18(electron) / 24px | 面板标题（md 在浏览器=20px） |
| `--font-sans` | `-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif` | UI 字体（electron 下运行时解析为系统栈，无 OpenAI Sans） |
| `--font-mono` | `ui-monospace, "SFMono-Regular", "SF Mono", Menlo, Consolas, ...` | 代码/数字 |
| `--vscode-font-weight` | 445 | 可变字体默认字重（正文实测 445） |

行高 token 写成 `calc(X/Y)`：`text-xs: calc(1/.75)=1.333`、`text-sm: calc(1.25/.875)=1.4286`、`text-base: calc(1.5/1)=1.5`、`text-lg: calc(1.75/1.125)=1.556`、`text-xl: calc(1.75/1.25)=1.4`。分子/分母都是 **4px 网格目标行高 ÷ 参考字号**（16/12、20/14、24/16、28/18、28/20）——即**设计意图是每行文字都落在 4px 网格上**。运行时因为字号被压小（sm 13px、xs 12px），比例出现漂移（13×1.4286≈18.57px）；消息 markdown 段又显式覆盖为 22px（实测 `_paragraph` 类 lh=22px，父级 `text-size-chat` 为 21px）。结论：**行高体系的目标是网格对齐，实际渲染由组件级覆盖决定**。

### 3.3 圆角体系（为什么 scale 1.25）

- base 阶梯：`--radius-{2xs..4xl}-base` = .125 / .25 / .375 / .5 / .625 / .75 / 1 / 1.25 / 1.5rem（即 2px 一档：2/4/6/8/10/12/16/20/24px）。
- 最终值：`--radius-X = base × --corner-radius-scale`，`--codex-corner-radius-scale: 1.25`（`:root` 与 electron 覆盖均为 1.25）。所以**运行时**：xs=5、sm=7.5、md=10、lg=12.5、xl=15、2xl=20、3xl=25、4xl=30px。`--radius-full: 9999px` 做胶囊。
- 配套 `--codex-corner-shape: superellipse(1.5)`（超椭圆，介于圆角矩形与圆之间，更"顺"）。
- **设计意图**：圆角体系拆成「base 网格 + 一个全局缩放旋钮 + 一个形状函数」三部分。base 提供 2px 阶梯的纪律；scale 让整个产品可以一键调圆润度（无障碍/平台偏好/品牌调性），`1.25` 使圆角落在 base 阶梯之间、视觉上更饱满；superellipse 让曲线不呆板。composer 25px（3xl）与主内容面 win32 的 12.5px（lg）都是这个体系的产物。

---

## 4. 行为逻辑

### 4.1 侧边栏显隐与重排

- **停靠**：侧边栏是 shell 行内 flex 子项，`main` 以 `flex:1; min-width:0` 吃掉剩余宽度——**侧栏显隐 = 宽度状态变化，不重排 DOM**（`AnimatePresence` + spring 动画 `{type:'spring', duration:.5, bounce:.1}` 平滑过渡宽度）。
- **拖拽**：手柄 8px 命中区（`absolute inset-y-0 -end-1 w-2`）；`setSize` 里 `e >= pmr(240)` 才接受（<240 视为收起意图：`Omr(...,false)` 关闭侧栏），`xmr(e) = clamp(e, 240, 520)`，`onResizeEnd` 持久化 `localStorage['sidebar-width']`。min 240 / max 520 / 默认 275。
- **窄窗口**：`--spacing-token-sidebar: clamp(240px, 275px, min(520px, calc(100vw - 320px)))` 保证侧栏永远给主列留 ≥320px；更窄时侧栏切换为**浮动覆盖层**（fixed left, z-42, top 36/0px, 滑入动画 x:-8→0 + 淡入, rounded-lg, elevation-sidebar 阴影），主列独占全宽。触发浮动态的确切断点条件在 minified JS 里（`_ && !D && !l`、`j || L || l`），**无法确定**。
- **自动收起**：`data-app-shell-sidebar-trigger` 按钮 + 全局 pointermove 捕获（指针离开触发区即收起）。

### 4.2 主列与 header

- header 是 **fixed 覆盖层**（z-30、透明、`-webkit-app-region:drag` 拖拽区、`pointer-events:none`），交互按钮以 `no-drag` 元素悬浮其上；内容帧用 `margin-top: var(--height-toolbar)` 让位，线程区顶部加 0.5px `border-top: border-heavy` 分隔线（`data-app-shell-thread-edge-divider`）。
- 主内容面带 `--elevation-prominent` 阴影（0.5px 描边+软影），**在画布上"浮起"一档**；win32 左上角 radius-lg 圆角。

### 4.3 滚动

- 线程容器：`overflow-y:auto; overflow-x:hidden; flex-direction: column-reverse; scroll-padding-bottom: var(--thread-scroll-padding-bottom)`。column-reverse 是经典的**底部锚定聊天流**——内容超长时视口自然停在底部，新消息不会把滚动位置顶走。
- composer 以 `sticky bottom-0 z-10 mt-auto` 挂在内容列底部：**滚到最底时贴底，滚上去时随内容上移**（不是全局 fixed）。
- 滚动条：仅 `scrollbar-color`（thumb=8% 白 / hover=16% 白 / track 透明），不设 webkit 宽度 → 覆盖式滚动条不占布局宽（实测 `scrollWidth===clientWidth`）。
- 消息列内嵌 `[overflow-anchor:none]`、`[content-visibility:auto]`（虚拟化渲染长列表）。

### 4.4 右浮动面板与容器查询

- 线程内右浮动内容：`absolute right-0, top/bottom 12px, z-40`，实测容器 316px，透明底（内容卡片自带 elevation）。
- 主内容视口是容器查询上下文（`container: app-shell-main-content/inline-size`）；detail-panel 头部在容器 <899px 时切到紧凑网格 `[@container_app-shell-detail-panel_(max-width:899px)]:grid-cols-[auto_minmax(0,1fr)_auto]`。
- 右面板"全宽模式"：`[data-app-shell-right-panel-full-width=true]` 时主视口 `flex:none; width:0; overflow:hidden`（右面板接管整列）。

---

## 5. dimi 差距（对照 `App.vue` / `App.styles.ts` / `styles/theme.ts`）

### 5.1 布局结构

| 维度 | codex | dimi | 差距 |
|---|---|---|---|
| header 定位 | fixed 覆盖层，z-30，透明，`-webkit-app-region:drag`，pointer-events:none；内容帧 `margin-top:46px` 让位 + 0.5px 分隔线 | `App.vue` 把 `<HeaderBar/>` 放在 shell 行**外、普通流**里（`app > HeaderBar + shell`），header 占掉布局高度而不是覆盖 | header 是否 fixed/drag-region 未在 App.vue 体现（HeaderBar.vue 未读，**无法确定**），但**主列让位机制不同**：dimi 是流式让位，codex 是固定覆盖+margin |
| 主面阴影 | `main` 带 `elevation-prominent`（0.5px 描边 + 双层阴影） | `mainCol` 只有 `background: #181818`，无阴影/描边 | 缺主面"浮起一档"的视觉 |
| 右浮动面板 | 线程内 absolute right-0 z-40 316px 容器（实测存在） | theme.ts 有 `panelW: 316px` token 与注释（"为 future 面板预留"），App.vue/App.styles.ts 未挂载 | 只有 token，无实现 |
| 容器查询/响应式 | `@container app-shell-main-content/inline-size` + 899px 断点 + `width>=20/40/48/64/80/96rem` 断点 | dimi 无容器查询，无窄窗口重排 | 缺窄窗口行为（见 5.4） |
| 侧栏动画 | spring .5s bounce .1；浮动态滑入 | `App.vue`: `Sidebar v-if="state.sidebarVisible"`——直接销毁/重建，无动画、无浮动态 | 显隐是"移除 DOM"而非"宽度+覆盖层" |

### 5.2 间距系统

| 维度 | codex | dimi | 差距 |
|---|---|---|---|
| 基准 | `--spacing: .25rem`（4px），全系统 `calc(spacing×N)` | theme.ts 无 spacing 基准，全部硬编码 px（46/275/736/28/25…） | 缺 4px 网格，无法派生；硬编码值散落各组件 |
| 派生 token | padding-row/toolbar/panel、composer overhang/inset、nav-row 高 = `text×1.5+padding×2` | 无对应派生链 | 尺寸之间没有"同一生成器"关系 |
| composer 宽度 | 736px = 48rem 内容盒（768−2×16），外层 max-width 768 | `threadMaxW: '736px'` 直接当列宽 | 方向对（对齐内容盒），但 dimi 把它当"列宽"而非"外层 768 的内容盒"，缺 px-toolbar 概念；如按 736 渲染消息列会略窄于 codex 消息列外框 |
| 头部高度 | 46px 且为 fixed | `headerH: '46px'` ✓ | 数值一致，机制不同（见 5.1） |

### 5.3 主题

| 维度 | codex 实测（暗色） | dimi theme.ts | 差距 |
|---|---|---|---|
| 主面/底层 | surface #181818 / under #141414 | `surface:#181818` ✓ `bgUnder:#141414` ✓ | 一致 |
| 控件/面板 | control #2d2d2d@96、panel #232323、editor-opaque #282828 | `surface2/surface3:'#2d2d2d'`、`sidebarBg: rgba(40,40,40,.7)` ✓ | 大体一致；缺 panel #232323 档 |
| 文本层级 | 100% / 70% / 50% | text #fff / textDim **0.65** / textMuted 0.5 / textTertiary 0.498 | `textDim` 0.65 ≠ codex secondary 0.71；多出 textMuted 0.5 与 tertiary 0.498 两个几乎重复的档 |
| 边框 | light 4% / default 8% / heavy 16% / focus #83C3FF@76% | border 0.084 ✓ heavy 0.156 ✓ focus ✓ | 缺 `border-light` 档（dimi 只用 default/heavy） |
| 状态色 | 暗色 300 级（green-300 #40c977、red-300 #ff6764…） | success `#00a240`、warning `#e25507`、error `#e02e2a` = **亮色 500 级** | 用了亮主题的 accent 级，暗色下比 codex 更深更饱和 |
| 圆角 | md=10 / lg=12.5 / xl=15 / 2xl=20 / 3xl=25（base×1.25） | radius.md=12 / lg=16 | dimi 的 md/lg 与 codex 最终值**不一致**；缺 scale 概念（codex 一键调圆润）与 superellipse 形状 |
| 字体 | 正文 14/22px（实测）、sm 13、xs 12、weight 445 | chat 14 / chatLh **21px**、sm 13/lh18、xs **14px**/lh21、weight 445 ✓ | `chatLh 21px` 与实测 22px 不符（theme.ts 注释"设计修正 22→21"，实测 codex markdown 段是 22px）；`font.xs:14px` 名实不符（codex xs=12px） |
| 滚动条 | thumb 8%/hover 16%/track 透明，覆盖式 | `scrollbarThumb .084 / hover .156 / track 0`，`scrollbarWidth:auto`，不用 webkit 伪元素 ✓ | 一致（dimi 有注释解释为何不用 ::-webkit-scrollbar，正确） |
| elevation | hairline + 3px 7.5px + 20px 双层黑影 | `elevation.prominent` 同值 ✓ | 一致 |
| 运行时主题 | JS 注入 ~40 个 CSS 变量 | `runtimeVars: --bg/--surface/--text` 三个；且 `--surface` 尚未被消费（theme.ts 自注 known limitation） | dimi 运行时主题是 3 变量最小集，主列/弹层/输入框仍吃静态色 |

### 5.4 行为

| 维度 | codex | dimi | 差距 |
|---|---|---|---|
| 侧栏拖拽 | 8px 手柄、min240/max520/默认275、localStorage 持久化、<240 收起 | App.vue 无拖拽；theme.ts 有 `sidebarResizeW: 16px` token 但无逻辑 | 只有 token，无实现 |
| 窄窗口 | 侧栏变浮动覆盖层 + 滑入动画 + 阴影 | 无 | 缺 |
| 滚动 | column-reverse 底部锚定 + sticky composer + content-visibility | 未在 App.vue/App.styles.ts 体现（Transcript 组件内，**未读**） | **无法确定** dimi 消息流是否底部锚定 |
| 线程滚动偏移 | `scroll-padding-bottom` + `overflow-anchor:none` | 无对应 | 缺 |

### 5.5 结论（改 dimi 优先级建议）

1. **补 4px spacing 基准与派生链**（row 8 / toolbar 16 / panel 20），把硬编码尺寸收口到 `calc(var(--spacing)*N)` 或等价常量。
2. **圆角改用 base×scale** 体系：md=10、lg=12.5、xl=15、2xl=20、3xl=25；把 `radius.md:12 / lg:16` 修正为 codex 实测值。
3. **正文行高改 22px**（与实测一致）；`textDim` 0.65→0.71，删掉重复的 0.5/0.498 档，补齐 `border-light` 4%。
4. **主列加 elevation-prominent 描边+阴影**；状态色切到暗色 300 级。
5. 行为层：侧栏拖拽持久化、窄窗口浮动覆盖、sticky composer、column-reverse 底部锚定，按 5.1/5.4 逐项补齐（Transcript 组件内部实现本次未读，需单独核对）。

---

## 6. 无法确定清单

- 侧栏停靠↔浮动切换的精确断点/条件（minified 变量 `j/L/l/_/D/k`）。
- `--color-background-control` 的静态 CSS 定义位置（运行时由 JS 主题对象注入，只有实测值）。
- header 除 `-webkit-app-region: drag` 外的窗口控制细节；dimi HeaderBar.vue 内部实现。
- 右面板（app-shell-detail-panel）"全宽模式"的触发交互。
- composer 内部输入控件度量（当前 DOM 无 textarea 可测）。
- 实测中内容列存在 `transform: translateX(-158px)`（`matrix(1,0,0,1,-158,0)`），疑似侧栏拖拽/入场动画残留，与设计 token 无关，未深究。
- `_mr=.5` 在 `pmr(e)=e*_mr` 中的单位含义（可能为窗口缩放换算），只影响拖拽阈值判断，不影响最终 clamp 值。
