# 05-code. 弹窗 / 菜单 / 图标系统（codex bundle 源码逆向）

> 目标：作为 dimi native-client 复刻 codex 弹窗/菜单/图标系统的**源码级设计依据**（姊妹篇 `05-dialogs-icons.md` 是 DOM 实测版；本文档以 bundle 源码为准，实测交叉验证）。
> 数据来源：`/tmp/codex_asar/webview/assets/app-initial-iBPGfcXU.js`（主 JS，15MB）、`app-44wrUC9v.css`（Tailwind v4）、`app-initial-BSHZIbh1.css`（CSS Modules + 主题），以及 2026-08-04 对运行中 codex（Electron，深色）的 CDP 实时复核。
> 关键背景：codex 弹窗/菜单是 **React Native Web 组件 + Radix 式 primitives**，样式全部是 Tailwind 工具类 + CSS 变量；`codex-dialog` / `codex-dialog-overlay` 只是 JS 标记类（CSS 中无对应规则）。所有 radius token 在 electron 下乘 `--corner-radius-scale: 1.25`；`--text-sm: 13px`（非 Tailwind 默认 14px）。
> 与 05-dialogs-icons.md 冲突时：源码数学以本文为准；实现与本文冲突时，先改文档再改代码。

---

## 1. 图标系统

### 1.1 尺寸阶梯（CSS 精确规则，`app-44wrUC9v.css`）

| 类 | 规则 | 渲染 |
|---|---|---|
| `.icon-3xs` | `{width:10px;height:10px}` | 10×10 |
| `.icon-xxs` | `{width:12px;height:12px}` | 12×12 |
| `.icon-2xs` | `{width:14px;height:14px}` | 14×14 |
| `.icon-xs` | `{width:16px;height:16px}` | 16×16 |
| `.icon-sm` | `{width:18px;height:18px}` | 18×18 |
| `.icon-base` | `{width:20px;height:20px}` | 20×20 |
| `.icon-md` | `{width:24px;height:24px}` | 24×24 |
| `.icon-lg` | `{width:28px;height:28px}` | 28×28 |
| `.icon-button` | `{width:calc(var(--spacing)*6);height:...;border-radius:var(--radius-md);padding:var(--spacing)}` | 24×24 按钮图标（radius-md=10px，padding 4px） |
| `.icon-tint` | `{color:color-mix(in oklab, var(--color-token-foreground) 50%, transparent)}` | 前景 50% 的弱化色 |

- 渲染尺寸由 CSS 类决定，与 svg 的 `width`/`height` 属性、viewBox 无关（同 05-dialogs-icons.md 实测结论）。
- `--spacing: .25rem`（4px）→ 阶梯全是 4px 的整数倍（10 除外）。

### 1.2 用途（bundle 组件代码 + 实时 DOM 交叉验证）

实时 DOM 枚举（190 个 svg）：`icon-xs` ×46、`icon-2xs` ×12、`icon-sm` ×2、`icon-2xs icon-tint` ×2、`icon-tint` ×2（无 icon-base/md/lg 实例）。

| 尺寸类 | 用途（源码出处） |
|---|---|
| `icon-xs` 16 | 菜单项默认左/右图标（`lz`/`Foa`：`L=i==null?'icon-xs':'icon-sm'`、`B=h??'icon-xs'`）；弹窗关闭按钮 X（`Fci`：`icon-xs`）；菜单行内 chevron（`moa`：`icon-xs`）；模式切换/搜索/加号/复制等消息行按钮；`ItemIcon` 显式 `size='xs'` 时 |
| `icon-sm` 18 | 菜单项显式 `size='sm'` 的 `ItemIcon`（`esa={xs:'icon-xs',sm:'icon-sm',md:'icon-md'}`）；header「对话操作」ellipsis、帮助按钮 |
| `icon-2xs` 14 | 模式/分组 chevron、表格小按钮、云端聊天小徽标（带 `icon-tint`） |
| `icon-md` 24 | `ItemIcon size='md'`（菜单大图标场景） |

- 菜单项图标外层 span（`Ioa` = ItemIcon）：`inline-flex items-center justify-center leading-none` + 尺寸类 + `pz.icon`。
- `pz.icon`：`shrink-0 opacity-75 group-focus:opacity-100 group-hover:opacity-100` — 常态 75% 透明度，行 hover/focus 时提升到 100%。
- 弹窗 header 图标瓦片：`h-9 w-9 shrink-0 items-center justify-center rounded-xl p-2` + 默认 `bg-token-foreground/5`（36×36 瓦片，radius 15，padding 8，内部图标 16~20）。

### 1.3 dimi icons.ts 缺哪些（对照 codex 菜单/弹窗实际使用）

dimi `icons.ts` 现有 29 个 key：menu、hideSidebar、back、forward、share、dots、more、ellipsis、newChat、sites、scheduled、plugins、gear、chevronDown、sectionChevron、copy、plus、send、help、pin、archive、search、cloud、folder、mic、thumbsUp、spinner、close、radio。

已在 codex 菜单/弹窗组件中确认使用、但 dimi 缺失的图标（lucide 命名，资源文件存在性已验证）：

- **check**（菜单 checkbox/radio 选中勾、命令面板选中项）— dimi 目前用 `radio`（vb 17）替代选中指示，语义/路径都不同。
- **chevronRight**（子菜单箭头，`moa` 用 `icon-xs`；资源 `chevron-right-*.js` 存在）— dimi 只有 chevronDown。
- **refreshCw / rotateCw**（重试/刷新菜单项；资源存在）— dimi 无。
- **trash2 / trash**（删除/移除菜单项；资源 `archive-x` 等存在，删除类菜单在 thread overflow 中使用）— dimi 无。
- **pencil / edit**（重命名菜单项；资源存在）— dimi 无。
- **externalLink**（「在新窗口打开」菜单项；资源存在）— dimi 无。
- **download**（导出/下载菜单项；资源存在）— dimi 无。
- **command**（⌘K 命令面板标识；资源存在）— dimi 无。
- **sparkles / magic**（AI 生成类菜单项；资源存在）— dimi 无。
- **copy** 已有（dimi copy vb 21，与实测消息复制一致）✓；**search/plus/settings/help/close(X)** 已有 ✓。

> 注：资源文件存在 ≠ 当前 UI 全部渲染到；上表按「codex 菜单组件中实际使用的 icon 名」+「资源存在性」双重确认。`check`/`chevronRight` 两个是菜单系统**必缺**的，其余按复刻范围补齐。

---

## 2. 弹窗结构（codex Dialog kit）

源码组件：`VN`（Dialog 包装）→ `zci`（DialogContent）→ `KN`（DialogHeader）/ DialogBody / DialogFooter。全部基于 Radix Dialog 系 primitives（`Ysi` 导出 Root/Portal/Overlay/Content/Title/Trigger…）。

### 2.1 层级与类（自上而下）

```
Portal
├── Overlay   `extension:bg-token-editor-background/80 electron:bg-[#00000022] codex-dialog-overlay fixed inset-0 z-50`
└── Content   `codex-dialog left-1/2 top-1/2 z-50 -translate-x-1/2 -translate-y-1/2 outline-none fixed`
              + `bg-token-dropdown-background/90 text-token-foreground ring-token-border max-w-[92vw] rounded-3xl ring-[0.5px] ring-token-border shadow-lg backdrop-blur-xl overflow-hidden`
              + 尺寸类（见 2.3）
              └── headerActions 容器（可选）：`absolute top-5 right-5 flex items-center gap-1 no-drag`
              └── DialogClose 按钮：`no-drag cursor-interaction rounded p-1 leading-none text-token-foreground/80 hover:bg-token-toolbar-hover-background focus:outline-none focus-visible:ring-1 focus-visible:ring-token-focus-border` + `absolute top-4 right-4` + `icon-xs` X
```

- **backdrop（遮罩）**：`fixed inset-0 z-50`，无 blur。electron 下 `bg-[#00000022]` = **黑 13.3%**；浏览器扩展下 `bg-token-editor-background/80`（editor-bg=rgb(40,40,40) @80%）。`codex-dialog-overlay` 仅作 JS 标记（`Guc()` 用它检测弹窗是否打开）。
- **panel（面板）**：`bg-token-dropdown-background/90` = **rgb(45,45,45) @90%**；`rounded-3xl` = `calc(1.25rem × 1.25)` = **25px**（源码数学；旧实测文档记 24px，建议以 25px 复核）；`ring-[0.5px]` + `ring-token-border` = 0.5px 描边 `rgba(255,255,255,0.084)`；`shadow-lg` = `0 4px 8px -2px rgba(0,0,0,0.10)`；`backdrop-blur-xl` = **blur(24px)**；`max-w-[92vw]`；`overflow-hidden`（`contentOverflow='visible'` 时 `overflow-visible`）。
- 面板无 padding：内边距由 Body/Header 各自提供。
- 关闭按钮：24×24 可点区域（`p-1` 4px + 16px 图标），定位 `top-4 right-4`（16px/16px），圆角 `rounded`=4px，常态前景 80%，hover `bg-token-toolbar-hover-background`（白 7.8%），focus-visible 1px `ring-token-focus-border`（`rgba(131,195,255,0.76)`）。`no-drag` 使 electron 拖拽区不拦截点击。

### 2.2 标题 / 内容 / footer

- **DialogHeader（`KN`）**：
  - 根：`flex flex-col items-start gap-3`
  - 图标瓦片：`flex h-9 w-9 shrink-0 items-center justify-center rounded-xl p-2` + 默认 `bg-token-foreground/5`
  - 标题：`heading-dialog min-w-0 font-semibold` → `font-size: var(--text-heading-md)=20px; font-weight: 600(语义); letter-spacing:-.36px; line-height:28px`（变体：lg=24px、base=20px、sm=text-sm）
  - 副标题：`text-token-description-foreground` + `text-base leading-normal tracking-normal`（变体 `text-sm`）
  - 标题+副标题容器：`flex min-w-0 flex-1 flex-col gap-1 self-stretch`（title/subtitle 间距 4px）
- **DialogBody**：`flex flex-col gap-0 px-5 py-5 text-base leading-normal tracking-normal` + 尺寸变体 + `as='form'` 支持 → **内边距 20px 四边**；内容间距靠子元素自己（gap-0）。
- **DialogFooter**：`flex w-full items-center justify-end gap-3`（无 padding，紧跟 Body 内容流；按钮间距 12px）。单按钮时（`expandSingleButton`，默认 true）：克隆按钮加 `w-full justify-center` → 整行居中铺满。

### 2.3 尺寸阶梯（`Lci` + `Wci`）

| size | 类 | 尺寸 |
|---|---|---|
| `narrow` | `w-[380px]` | 380 |
| `feature` | `w-[400px]` | 400 |
| `compact` | `w-[420px]` | 420 |
| `default` | `w-[520px]` | **520（默认）** |
| `wide` | `w-[600px]` | 600 |
| `xwide` | `w-[680px]` | 680 |
| `xxwide` | `w-[800px]` | 800 |
| `editor` | `w-[600px] h-[720px] max-w-full max-h-full` | 600×720 |
| `full` | `h-full min-h-0`（Body 尺寸变体） | 撑满 |
| `tall` | `min-h-[520px] max-h-[560px]`（Body 尺寸变体） | 高 520~560 |

### 2.4 内容高度动画（源码机制）

`zci` 挂 ResizeObserver 监听内容：`e.style.setProperty('--dialog-content-height', h+'px')` + `e.style.height='var(--dialog-content-height)'` + `dataset.dialogHeightReady='true'`。即**面板高度由内容驱动、动画由 height 过渡实现**；但过渡的 duration/easing 不在两个 CSS 文件的工具类里（`codex-dialog` 无 CSS 规则），**无法确定具体时长/缓动**（猜测在运行时注入样式，未在本次源码中找到）。

---

## 3. 菜单（Dropdown kit）

源码组件：`uz`（Dropdown 包装）→ `Noa`（Content）/ `lz`（Item）/ `Foa`（CheckboxItem）/ `Voa`（Separator）/ `Hoa`（SectionLabel）/ `qoa`（Title）/ `Loa`（Input）/ `Roa`（SearchInput）。基于 Radix DropdownMenu primitives（`mz` 导出 Trigger/Content/Item/CheckboxItem/Separator…）。

### 3.1 面板（Content）

```
`no-drag z-50 m-px flex select-none flex-col overflow-y-auto px-1 py-1` + surface 类
style: { maxWidth: min(var(--radix-available-width), calc(100vw - 16px)),
         maxHeight: min(var(--radix-available-height), calc(100vh - 16px)) }
sideOffset 默认 1px；collisionPadding 6px；zoom 支持
```

surface（`boa`）三档：

| surface | 类 | 效果 |
|---|---|---|
| `menu`（默认） | `bg-token-dropdown-background/90 text-token-foreground ring-token-border rounded-xl ring-[0.5px] shadow-xl-spread backdrop-blur-sm` | bg rgb(45,45,45)@90%、**圆角 15px**（rounded-xl = 0.75rem×1.25）、0.5px 白 8.4% 描边、**blur(8px)**、shadow 0 8px 16px -4px rgba(0,0,0,0.12)（shadow-xl-spread = 0 0 0 .5px border + shadow-xl） |
| `opaque` | `bg-token-dropdown-background text-token-foreground rounded-2xl shadow-xl-spread` | 不透明、**圆角 20px**（rounded-2xl = 1rem×1.25）、无描边/无 blur |
| `panel` | 上者 + `rounded-2xl p-4 shadow-2xl backdrop-blur-lg` | **内边距 16px**、shadow 0 16px 32px -8px rgba(0,0,0,0.19)、blur 16px |

- 面板内边距：`px-1 py-1` = 4px。
- `m-px` 外 margin 1px；`z-50`。
- 宽度（`Eoa`）：icon 120 / xs 160 / sm 180 / menuNarrow 208 / **menu min 220** / menuFixed 220 / menuBounded min 200 max 320 / menuWide 240 / sidebar min 172 max 240 / workspace min 260 / panel 280 / panelWide 360。
- 最大高度（`Doa` + `Soa` 钳制）：`min(Xpx, var(--radix-available-height), calc(100vh - 16px))`，X = compact 200 / list 250 / tall 350。
- modal 模式：额外渲染 `div aria-hidden pointer-events-auto fixed inset-0`（拦截点击），并订阅 window `blur` + `browser-sidebar-web-contents-pointer-down` 自动关闭（`Coa`）。

### 3.2 菜单项（Item / CheckboxItem）

```
根：`no-drag outline-hidden rounded-lg px-[var(--padding-row-x)] py-[var(--padding-row-y)] text-sm text-token-foreground group hover:bg-token-list-hover-background focus:bg-token-list-hover-background cursor-interaction`
disabled：`cursor-default opacity-50`
内容行：`flex w-full items-center gap-1.5`（icon+文字）；带 subtext 变体 gap-3（12px）
左图标：`icon-xs` + `shrink-0 opacity-75 group-focus:opacity-100 group-hover:opacity-100`
文字容器：`flex min-w-0 flex-1 flex-col` → label `truncate` + SubText `text-sm whitespace-normal text-token-text-secondary`
右区（快捷键/右图标）：快捷键 `ms-2 shrink-0 text-xs text-token-description-foreground`；右图标 `icon-xs`
子菜单箭头（moa）：`icon-xs`
```

实测（打开 model-picker 菜单，CDP 计算样式）：

| 属性 | 值 |
|---|---|
| 行高 | **29px**（内容 18.57px + 5px×2） |
| padding | **5px 8px**（`--padding-row-y` body 作用域 = calc(0.25rem×1.25) = 5px；`--padding-row-x` = calc(0.25rem×2) = 8px；:root 下 row-y=4px 是兜底） |
| 字号/行高 | 13px / 18.5714px（`--text-sm: 13px`） |
| 圆角 | **12.5px**（rounded-lg = 0.625rem×1.25） |
| hover/focus 背景 | `rgba(255,255,255,0.078)`（token list-hover） |
| 图标间距 | gap-1.5 = 6px（icon 与文字） |
| 选中态 | Radix `data-state=checked`（CheckboxItem 用同套 itemBase+itemInteractive；checked 具体高亮类未抓全，**无法确定**） |

### 3.3 其他行类型

- **Separator**：`w-full px-[var(--padding-row-x)] py-1` + 内层 `h-[1px] w-full bg-token-menu-border`（`rgba(255,255,255,0.084)`；上下各 4px）。
- **SectionLabel**：`px-[var(--padding-row-x)] py-1 text-sm text-token-description-foreground`。
- **Title（菜单标题）**：`text-token-description-foreground flex min-h-6 items-center truncate px-[var(--padding-row-x)] py-[var(--padding-row-y)] text-sm leading-4`。
- **SearchInput**：`w-full min-w-0 rounded-sm border border-none px-[var(--padding-row-x)] py-[var(--padding-row-y)] text-sm !outline-none`；内联处理 Cmd/Ctrl+A 全选。

---

## 4. 布局 / 边距汇总

| 场景 | 值 |
|---|---|
| 弹窗面板与视口 | 居中（left/top 50% + -translate-1/2），z-50，max-w 92vw |
| 弹窗 Body 内边距 | 20px 四边（px-5 py-5） |
| 弹窗 header 内部 | icon↔title 间距 gap-3（12px）；title↔subtitle gap-1（4px）；header 与 body 之间由 Body py-5 提供 |
| 关闭按钮定位 | top/right 16px；按钮 24×24；图标 16×16 |
| 弹窗 footer 按钮 | 右对齐，间距 gap-3（12px）；单按钮整行居中 |
| 菜单面板内边距 | 4px（px-1 py-1） |
| 菜单项行高 | 29px（8px 水平 padding / 5px 垂直 padding） |
| 菜单行内 gap | 图标↔文字 6px；带 subtext 12px；快捷键距文字 8px（ms-2） |
| 菜单分隔线 | 1px、行内水平 padding 8px、上下 4px |
| 下拉最大高度 | min(200/250/350px, 可用高度, 100vh-16px) |
| 下拉与触发锚 | sideOffset 1px、collisionPadding 6px、margin 1px |

---

## 5. 行为逻辑（源码）

### 5.1 打开 / 关闭

- **打开**：Radix Trigger（`asChild`）点击/Enter/Space 触发；打开瞬间 `gh()` → `window.dispatchEvent(new Event('codex:dismiss-tooltips'))`（弹窗/菜单打开时全局关闭 tooltip）。
- **关闭**：Esc（Radix 内建）、点击外部（`onPointerDownOutside`；dialog 可用 `shouldIgnoreClickOutside` 关闭该行为，menu 的 modal 模式用整屏拦截层替代）、选择菜单项后自动关（onSelect）、窗口失焦/侧栏指针按下（dropdown modal 的 `Coa` 效果）。
- 受控/非受控：`VN`/`uz` 都支持 `open`/`onOpenChange`（受控）或内部 state；`onOpenChange` 包装里先 `gh()` 再透传。

### 5.2 键盘导航（菜单）

- **↑/↓**：在 `[role=menu]` 内于 `[role=menuitem|menuitemcheckbox|menuitemradio]` 之间移动 focus，跳过 `aria-disabled`（`Boa`/`zoa`：从当前元素向后/向前找第一个可用项）。
- 搜索框（SearchInput）内 **↓** 聚焦第一项、**↑** 聚焦最后一项（`Noa` 的 onKeyDownCapture 处理 ArrowUp/Tab+Shift）。
- **Tab+Shift**（从最后一项）：回聚焦输入框。
- **Cmd/Ctrl+A**：输入框全选（SearchInput 内建）。
- Enter/Space 激活、typeahead 由 Radix 提供（源码未自定义，按 Radix 默认）。
- Esc：Radix 统一关闭。

### 5.3 悬停 / 选中

- 菜单项 hover/focus：背景 `rgba(255,255,255,0.078)`（`hover:bg-token-list-hover-background` 与 `focus:` 相同）。
- 图标：常态 opacity 0.75 → 行 hover/focus 时 1.0（`group-hover`/`group-focus`）。
- 禁用项：`opacity-50` + `cursor-default`（不响应 hover 背景）。
- 弹窗关闭按钮 hover：`bg-token-toolbar-hover-background`（白 7.8%）。

### 5.4 动画

- **通用弹窗/菜单：源码中没有任何 enter/exit 动画工具类**（两个 CSS 文件无 `animate-in/out`、无 `fade-in-*`/`zoom-in-*` 工具；`data-[state=open]` 选择器全部是颜色工具）。即默认弹窗/菜单打开是**瞬间出现**；关闭同理（Radix 无 unmount 动画配置）。
- **例外（有动画的实例）**：
  - model-picker 下拉：`.…_ModelPickerDropdownContent…[data-state=open]{animation:.32s cubic-bezier(.23,1,.32,1) 30ms both}`，keyframes `0%{opacity:0;transform:scale(.98)} → 100%{opacity:1;transform:scale(1)}`；`transform-origin: var(--radix-dropdown-menu-content-transform-origin)`；`will-change:opacity,transform`；`_ReducedMotion{animation:none}`（尊重系统减弱动态）。
  - 弹窗内容高度：ResizeObserver 驱动 `--dialog-content-height`（2.4），时长/缓动无法确定。
- 实测 model-picker 打开时 `animationName=_model-picker-dropdown-content-enter_1fm6a_1, 0.32s, cubic-bezier(.23,1,.32,1), delay 30ms` ✓（与源码一致）。

### 5.5 其它

- **zoom 支持**：弹窗/菜单都支持窗口 zoom（`zoom` 样式 + 按 zoom 缩放宽高/坐标）；dimi 单窗口场景可不实现。
- 弹窗 `onPointerDown` 内 `stopPropagation`（`q` 处理器），防止事件穿透到下层。
- 菜单 `contentMaxHeight` 与 `Xoa` 默认 maxHeight 配合，保证不超出视口 16px 边距。

---

## 6. dimi 差距（读 `Dialogs.vue` / `Dialogs.styles.ts` / `icons.ts` 逐条对比）

dimi 已按旧实测文档实现了一版弹窗/菜单；以下是与**源码级**事实的差异清单。

### 6.1 图标系统

1. **无全局尺寸阶梯类**：dimi 没有 `icon-2xs/xs/sm` 之类的 CSS 类，尺寸写死在组件样式里（`listItemIcon width:16`、`dialogClose '& svg':16`）。若要支持菜单图标 75%→100% 的 hover 提升与多尺寸，建议在 theme 中建等价类。
2. **缺 `check` 图标**：codex 菜单选中用 check（勾），dimi 用 `radio`（圆点）替代 — 选中态视觉与 codex 不同。
3. **缺 `chevronRight`**：子菜单箭头（codex `moa` 用 icon-xs 16）。dimi 只有 chevronDown。
4. **缺重命名/删除/刷新/导出等菜单图标**：pencil/edit、trash、refreshCw、externalLink、download、command、sparkles 等在 codex 资源中确认存在且用于菜单，dimi icons.ts 无。
5. **close 尺寸**：dimi close vb 24（lucide X）渲染 16×16，与 codex 弹窗关闭按钮一致（`icon-xs` 16）✓；但 codex 关闭图标本体 path 未在本次抓取比对（弹窗未开时无法确定），需保持目测复核。
6. **无 icon-tint 类**（50% 前景弱化色），菜单/徽标弱化图标场景缺。

### 6.2 弹窗结构

7. **圆角**：dimi `modalShell borderRadius: 24`；codex 源码数学 = `rounded-3xl` = 25px（`1.25rem × corner-radius-scale 1.25`）。旧实测文档记 24，两者差 1px — **建议按 25px 复核/修正**。
8. **Body 右侧 padding 不同**：dimi `dialogBody padding: '20px 32px 20px 20px'`（右 32 给关闭按钮让位）；codex 是 `px-5 py-5` = 四边 20px，关闭按钮**悬浮**在内容上方（绝对定位 top-4 right-4，不占位）。dimi 与 codex 的布局策略不同（dimi 防遮挡，codex 允许悬浮重叠）。
9. **无 size 阶梯**：dimi 只有固定 520px；codex 有 narrow/compact/default/wide/xwide/xxwide/editor/full/tall 八档（§2.3）。如需对齐不同弹窗（picker/approval/settings），应建阶梯。
10. **无 DialogHeader 组件**：dimi 直接用 dialogTitle div；codex 有 icon 瓦片（36×36、rounded-xl 15px、bg-foreground/5）、标题 `heading-dialog`（20px/28px/600/字距 -0.36px）、副标题、title↔subtitle gap-1。dimi dialogTitle 20/28/600 已接近但**缺 -0.36px letter-spacing**、缺图标瓦片。
11. **footer 间距**：dimi 内联 `gap: 8px`；codex `gap-3` = 12px；单按钮整行居中（dimi 无）。
12. **无内容高度动画**：codex 有 ResizeObserver + `--dialog-content-height`（§2.4）；dimi 无。
13. **backdrop 一致**：dimi `rgba(0,0,0,0.133)` 与 codex electron `#00000022`（13.3%）一致 ✓；均无 blur ✓。
14. **面板 shadow**：dimi `0 0 0 0.5px rgba(255,255,255,0.082) + 0 4px 8px -2px rgba(0,0,0,0.1)` = codex `ring 0.5px + shadow-lg` ✓（0.082 vs token border 0.084，可统一为 token 值）。
15. **dialogPicker（session picker）blur 不同**：dimi `blur(4px)`；codex 菜单 surface `backdrop-blur-sm` = **8px**（`--blur-sm: 8px`）。dimi 偏小，建议改 8px。
16. **关闭按钮 hover**：dimi `background: colors.hover`；codex `hover:bg-token-toolbar-hover-background`（白 7.8%）— 若 colors.hover 与其不同需对齐。

### 6.3 菜单项

17. **行高/padding/圆角/字号已对齐**：dimi listItem `padding: 5px 8px; borderRadius: 12.5; fontSize: 13; lineHeight: 18.5714px; gap: 6` 与 codex 实测完全一致 ✓。
18. **icon 透明度**：dimi `opacity: 0.75` 静态；codex 是 `opacity-75 group-hover:opacity-100 group-focus:opacity-100`（行 hover/focus 提升）。dimi 缺 group-focus 提升。
19. **选中态**：dimi `listItemSelected = colors.hoverStrong`；codex CheckboxItem 的 checked 高亮类未抓全（**无法确定**），维持 dimi 现状并目测复核（旧文档 §6 同结论）。
20. **菜单面板缺失**：dimi 只有 dialogPicker 一个近似；codex 有完整 Dropdown kit（surface 三档、width 12 档、max-height 钳制、Separator/SectionLabel/Title/SearchInput 行类型、子菜单、modal 拦截层）。dimi 若要在按钮/行内下拉，需要补齐面板原语。
21. **宽度差异**：dimi dialogPicker 440px；codex `menuBounded` min200/max320（dimi 注释表明是有意保留 440，按产品决策确认）。
22. **SearchInput 样式不同**：dimi searchInput = `border 1px solid colors.border + radius 10 + bgUnder + padding 8px 12px + focus borderColor primary`；codex SearchInput = `rounded-sm + border-none + !outline-none + px/py 行内 padding（8px/5px）+ focus 样式未指定`。两套意图不同（dimi 是普通输入框，codex 是菜单内搜索行）。

### 6.4 行为逻辑

23. **键盘**：dimi pickerKeydown 有 ↑/↓/Enter/Esc；codex 额外有：搜索框 ↓ 聚焦首项 / ↑ 聚焦末项、Tab+Shift 回输入框、Cmd/Ctrl+A 全选、typeahead、禁用项跳过。dimi 需按菜单语义补齐（至少 ↑↓ 循环/首尾聚焦）。
24. **打开时关 tooltip**：codex 打开弹窗/菜单触发 `codex:dismiss-tooltips` 事件；dimi 无此机制（dimi 无 tooltip 系统则跳过）。
25. **窗口失焦自动关闭**：codex modal 菜单监听 window blur 关闭；dimi 无。
26. **关闭交互**：dimi 用 backdrop `@mousedown.self`；codex 用 Radix `onPointerDownOutside`（且 dialog 可配 shouldIgnoreClickOutside）。行为等价，事件源不同，注意 dimi 在内容区 mousedown 冒泡时不会误关 ✓。
27. **动画**：codex 默认弹窗/菜单**无动画**（源码无动画类）→ dimi 现状无动画 ✓ 一致；若仿 model-picker 的 320ms fade+scale 动画，按 §5.4 实现（含 reduced-motion 关闭）。

---

## 7. 附：本次源码取证要点

- 弹窗 kit：`app-initial-iBPGfcXU.js` @4683525（`zci`）、@4681092（`Lci` 尺寸）、@4689400（DialogBody/Footer）、`VN` 包装 @4690000 附近、`KN` header @4689xxx。
- 菜单 kit：@6231500-6254500（`boa` surface、`Eoa`/`Doa`、`Noa` Content、`lz` Item、`Foa` CheckboxItem、`Ioa` ItemIcon、`Voa` Separator、`qoa` Title、`Xoa` 默认 max 样式、`pz`/`$oa` CSS module）。
- 图标阶梯：`app-44wrUC9v.css` `.icon-*` 规则；实时 DOM 190 svg 复核（icon-xs 46 / icon-2xs 12 / icon-sm 2）。
- 主题：`:root` `--corner-radius-scale:1.25`、`--padding-row-x:calc(.25rem*2)`、`body` `--padding-row-y:calc(.25rem*1.25)`、`--blur-sm:8px`、`--blur-xl:24px`、`--shadow-xl:0 8px 16px -4px #0000001f`、`--text-sm:13px`（electron 实测）。
- 无法确定项（已在正文标注）：弹窗内容高度动画的时长/缓动；dialog 关闭图标 path 与 dimi close 的逐像素比对；CheckboxItem checked 高亮类；弹窗面板 24 vs 25px 的实测复核（源码数学 25）。
