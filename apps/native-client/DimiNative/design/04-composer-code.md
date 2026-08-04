# 04 · Composer（输入区）源码级设计逆向 — code

> 目标：从 codex（ChatGPT.app）webview bundle 的**源码**逆向 Composer 的组件结构、布局规则、
> 边距、透明度与行为逻辑，作为 dimi native-client 1:1 复刻的依据。
> 源码：`/tmp/codex_asar/webview/assets/app-initial-iBPGfcXU.js`（15MB 主 JS）、
> `app-initial-BSHZIbh1.css`（composer CSS module）、`app-44wrUC9v.css`（Tailwind/主题）。
> 复核：CDP 直读运行中的 codex 页面（Electron 深色，viewport 1686×960）。
> 原则：只写组件结构/布局/边距/透明度/行为，不写颜色数值表；测不到的地方明确写「无法确定」。

---

## 0. 组件清单（bundle 中的实际组件）

Composer 由 `eY = Object.assign(bds, …)`（`ComposerSurface`）及其子组件构成，
主组件 `lEs`（线程 composer）、编辑器宿主 `fU`（ProseMirror RichTextInput）：

| 标识 | 组件 | 渲染 |
|---|---|---|
| `bds` | ComposerSurface | 根 `div.relative.flex.flex-col` + 条件 chrome 类 |
| `Pds` | Body | `div.relative.z-10.flex.min-h-0.flex-1.flex-col` + 圆角类 |
| `xds` | Attachments | `div._attachmentsDefault_1xj1z_2`（附件槽） |
| `Sds` | Input | `div.mb-1.flex-grow.overflow-y-auto.px-3`（multiline）/ `min-w-0`（single-line） |
| `Tds` | Footer | grid 容器（`_footer_1xj1z_2`） |
| `Eds` | FooterAction | `div.flex.items-center`（pointerdown stopPropagation） |
| `Ods` | FooterActions | `div.flex.shrink-0.items-center.gap-2`（右区：听写+发送） |
| `kds` | FooterControls | `div.flex.min-w-0.items-center.justify-end`（+ `w-full` / `shrink-0 gap-2`） |
| `jds` | FooterExpandingControls | `div.flex.min-w-0.flex-1.justify-end`（模型 pill 弹性占位） |
| `Ads` | FooterInlineControls | gap compact/normal（`gap-[5px]` 等） |
| `Mds` | FooterDivider | `div.h-4.w-px.bg-token-border/70` |
| `Nds` | FooterLabel | `span.truncate` + `_footerLabel_1xj1z_2`（窄容器隐藏） |
| `yds` | AdaptiveFooter | 把 input/leading/trailing 摆进 footer grid 的行列 |
| `Cds` | UtilityBarSlot | 工具条插槽（home 变体带动画） |
| `fU` | RichTextInput | ProseMirror 编辑器宿主（`_root_dzscs_1`） |
| `lEs` | Composer（主） | 组装上述全部 + 逻辑 |
| `nY` | SendButton | 发送/停止圆形按钮 |
| `Dut` | DictationButton | 听写按钮（三态） |
| `Eut` | DictationRecording | 录音中波形 UI |
| `Glt` | AttachButton | 「添加文件等内容」按钮 |
| `HTs`/`sEs`/`cEs` | ModelPicker | 模型 pill + power-picker 菜单 |
| `Zys` | ContextMenu | 附件上下文菜单（文件输入 + 菜单项） |
| `bvs`/`kvs`/`Tvs` | 单行测量 | 隐藏文本测量 span + 单行判定 |
| `tm` | IconButton | 通用图标按钮（composer/ghost 等变体） |
| `vh` | Tooltip | 所有按钮的 tooltip 包装 |
| `Uds`/`Pvs`/`Hds` | 语音/Home | 语音布局与 home 工具条（dimi 不涉及） |

---

## 1. 组件结构（层级树，线程 composer，multiline+default 实测）

```
form.relative.flex.flex-col[data-thread-find-composer="true"]      ← 无背景无边框，仅布局
├─ div[data-above-composer-portal].relative.empty:hidden           ← 悬浮物 portal 锚点
├─ div.relative.flex.w-full.flex-col.gap-2                         ← 全部内容的包层
│  ├─ input.hidden                                                  ← 附件 file input（Zys 提供）
│  └─ div.composer-surface-chrome…                                 ← bds 视觉胶囊（本层即全部视觉）
│     │   class: relative flex flex-col composer-surface-chrome
│     │         bg-token-input-background/90 backdrop-blur-lg
│     │         electron:dark:bg-token-dropdown-background
│     │         _multilineSurface_1xj1z_2 overflow-y-auto
│     └─ div.relative.z-10.flex.min-h-0.flex-1.flex-col            ← Pds Body
│        ├─ div._attachmentsDefault_1xj1z_2                        ← xds 附件槽（空态高 14px）
│        │  └─ div.flex.flex-wrap.gap-2（有附件时：选择 pill + 文件卡片）
│        └─ div.contents                                            ← display:contents，无盒
│           └─ div._footer_1xj1z_2.grid.grid-cols-[minmax(0,auto)_auto_minmax(0,1fr)]
│              │        .items-center.gap-x-[5px].select-none.mb-2.px-2   ← Tds Footer
│              ├─ div.min-w-0.col-start-1.row-start-2              ← 左列（row 2）
│              │  └─ div.flex.min-w-0.items-center.gap-[5px]       ← FooterInlineControls
│              │     └─ div.flex.items-center                      ← FooterAction
│              │        └─ span.contents > button[aria-label=添加文件等内容] 28×28
│              ├─ div.min-w-0.col-span-full.row-start-1.-mx-2      ← 输入行（row 1，跨 3 列）
│              │  └─ div.mb-1.flex-grow.overflow-y-auto.px-3       ← Sds Input
│              │     └─ div._root_dzscs_1.text-size-chat…          ← fU 编辑器宿主
│              │        └─ div.ProseMirror[contenteditable]        ← ProseMirror 实例
│              └─ div.min-w-0.col-start-3.row-start-2              ← 右列（row 2）
│                 └─ div.flex.min-w-0.items-center.justify-end.w-full   ← FooterControls
│                    ├─ div.flex.min-w-0.flex-1.justify-end        ← FooterExpandingControls
│                    │  └─ div.flex.items-center > button（模型 pill）111×28
│                    └─ div.flex.shrink-0.items-center.gap-2       ← FooterActions
│                       ├─ button[aria-label=听写] 28×28           ← Dut
│                       └─ button[aria-label=发送] 28×28           ← nY
```

实测 grid 列宽：`28px 0px 682px`（左按钮 28 / 中间空列 0 / 右区 682），列间 gap 5px ×2。
胶囊（surface）与消息列对齐：form 的父链是 `div.max-w-(--container-3xl).px-16px.mx-auto`
（实测 maxWidth **768px**、左右 padding **16px**），与消息列同一容器 → 胶囊宽度
**736px = 768 − 16×2**，与消息列 1:1 对齐。

---

## 2. 布局规则（源码类 + 实测值）

### 2.1 surface（bds）— 模式条件类

```js
// bds props: utilityBarVariant='default' | 'home', layout='multiline'|'single-line',
//            radiusVariant='default'|'single-line', surfaceOverflow='auto'|'visible',
//            surfaceVariant='default'|'secondary'|'opaque', isDragActive, spacing
// 非 home 时 chrome 类（home 时无 chrome，仅背景类 + z-10）：
composer-surface-chrome
+ surfaceVariant==='default'  → bg-token-input-background/90 backdrop-blur-lg
+ surfaceVariant==='secondary'→ bg-token-bg-secondary
+ surfaceVariant==='opaque'   → bg-token-input-background
+ radiusVariant==='single-line' → _singleLineRadiusSurface_1xj1z_2   // --composer-border-radius: 22px
+ layout==='single-line'      → overflow-visible
+ layout==='single-line' && radiusVariant==='default' → rounded-full  // 9999px
+ layout==='multiline' && surfaceOverflow==='auto'    → overflow-y-auto
+ layout==='multiline' && surfaceOverflow==='visible' → overflow-visible
+ layout==='multiline' && radiusVariant==='default'   → _multilineSurface_1xj1z_2  // --composer-border-radius: var(--radius-3xl)
+ isDragActive                → bg-token-dropdown-background/50
```

- 根：`relative flex flex-col`（`bds`）+ `utilityBarVariant==='home' && 'z-10'` + 外部 className。
- **thread 默认 = `utilityBarVariant:'default'`，`layout: Rt`（'single-line'|'multiline'），`radiusVariant:'default'`，`surfaceVariant:'default'`** → 胶囊 = multilineSurface（圆角 20px×1.25=25px）+ `overflow-y-auto` + 背景 90% + blur(16px)。
- `surfaceVariant` 由外层 prop 决定；线程 composer 恒为 `default`（实测类名含 `bg-token-input-background/90 backdrop-blur-lg`）。
- chrome 的 border：`composer-surface-chrome` 在 electron/browser/chrome-extension 下
  `border-width:0!important`，**无边框**；发丝环由 `box-shadow: var(--elevation-prominent)`
  = `0 0 0 .5px border-heavy, 0 3px 7.5px #0000000a, 0 0 20px #0000000d` 承担。
  （`@media (forced-colors:active)` 时换 `outline:1px solid canvastext`。）

### 2.2 footer 网格（Tds）

```js
// multiline + 默认 spacing：
//   grid grid-cols-[minmax(0,auto)_auto_minmax(0,1fr)] items-center gap-x-[5px] select-none
//   + spacing==='flush' ? 'mb-0 px-0' : 'mb-2 px-2'（mb-2=8px，px-2=8px）
//   + responsive && _footer_1xj1z_2（container: composer-footer/inline-size）
// single-line：
//   grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2 px-2 py-1 select-none
```

- **multiline 两行布局**（AdaptiveFooter `yds` 摆位）：
  - 输入行：`col-span-full row-start-1`，且默认 spacing 加 `-mx-2`（抵消 footer `px-2`，输入行横跨整宽）
  - 左按钮：`col-start-1 row-start-2`
  - 右按钮：`col-start-3 row-start-2`
  - 中间列（col 2）在 multiline 下**为空列**（实测 `0px`），推测为 single-line 时输入列占位。
- **single-line 单行布局**：leading → input → trailing 顺序排入一行（auto / 1fr / auto）。
- `_footer_1xj1z_2` 是容器查询锚点：`@container composer-footer (width<=440px)` 隐藏 `_footerLabel`；
  electron 下 `<=475px` 隐藏。

### 2.3 附件区（xds）

```css
._attachmentsDefault_1xj1z_2 {
  --composer-attachment-inset: calc(var(--spacing) * 2);        /* 8px */
  --composer-attachment-border-radius: max(0px, calc(var(--composer-border-radius) - var(--composer-attachment-inset)));
  padding: var(--composer-attachment-inset);                    /* 8px */
  padding-bottom: calc(var(--spacing) * 1.5);                   /* 6px */
}
._attachmentsDefault_1xj1z_2 .composer-attachment-surface { border-radius: var(--composer-attachment-border-radius); }
/* flush spacing：mb-[5px] */
```

- 附件槽 = 上/左/右 8px + 下 6px = 空态 14px 高（实测 surface 顶部 14px）。
- 附件卡圆角 = 胶囊圆角 − 8px（25−8=17px，multiline default）。
- 附件内容：`div.flex.flex-wrap.gap-2`（gap 8px），子项为「选择文本 pill」（`Yus`：带 popover 计数）
  与「文件卡片」（`gEs`：`previewSrc` 时图片卡 `WGo`，否则 `qJ` 文件卡，uploading 显示进度，
  error 显示「上传失败」红字 + `_Es` 状态徽标）。
- 附件隐藏条件：`Lt` 为真时（特定模式）整个 Attachments 不渲染；有可见附件 `hasVisibleAttachments` 才渲染内容。

### 2.4 输入行（Sds Input + fU 编辑器）

```js
// Sds（eY.Input）：
//   layout==='single-line' → 'min-w-0'
//   layout==='multiline'   → 'mb-1 flex-grow overflow-y-auto' + (spacing==='flush' ? 'px-0' : 'px-3')
// fU（编辑器宿主 div._root_dzscs_1）：
//   'text-size-chat [&_.ProseMirror]:focus-visible:outline-none text-token-foreground'
//   + single-line ? 'flex h-9 max-h-none items-center overflow-hidden
//                  [&_.ProseMirror]:!h-5 [&_.ProseMirror]:!min-h-5 [&_.ProseMirror]:leading-5'
//                  + '[&_.ProseMirror]:min-w-0 [&_.ProseMirror]:flex-1 [&_.ProseMirror]:overflow-hidden
//                     [&_.ProseMirror]:whitespace-nowrap [&_.ProseMirror_p]:overflow-hidden
//                     [&_.ProseMirror_p]:text-ellipsis [&_.ProseMirror_p]:whitespace-nowrap'
//   : 'h-auto max-h-[25dvh] overflow-y-auto [&_.ProseMirror]:h-auto [&_.ProseMirror]:min-h-[2rem]'
//   + '[&_.ProseMirror]:resize-none [&_.ProseMirror_p]:m-0 [&_.ProseMirror_ul]:ps-6
//      [&_.ProseMirror_ol]:list-decimal [&_.ProseMirror_ol]:ps-6'
//   + 外层 lEs 追加 'text-base' + multiline 时 '[&_.ProseMirror]:leading-5'
```

- ProseMirror 实例内联样式：`font-size: var(--codex-chat-font-size); height: auto; resize: none; min-height: 2.75rem`。
  `min-height` 由 `lEs` 传入：single-line `1.25rem`、multiline `2.75rem`（=44px 实测）。
- **multiline 编辑器**：`max-h-[25dvh]`（实测 960 视口 → 240px）超出滚动（`overflow-y-auto`），
  行高 `leading-5`（20px 实测），`min-h-[2rem]`（32px）。
- **single-line 编辑器**：宿主 `h-9`（36px），ProseMirror `!h-5`（20px）`leading-5`，单行
  nowrap + `text-ellipsis` 截断，`items-center` 垂直居中，`overflow-hidden`。
- 代码块（`pre[data-composer-code-block]`）：圆角 `--radius-lg`(10px)、bg code-block、padding 8px、
  `font-family: var(--font-mono)`、字号 `--codex-chat-code-font-size`、line-height 20px、`white-space:pre`、
  `overflow-x:auto`；滚动时用 `clip-path: inset(0 round …)` 让代码块顶角贴合胶囊圆角（`K7a` 逻辑）。

---

## 3. 边距 / 内距（全部实测或源码推导）

| 项 | 值 | 来源 |
|---|---|---|
| 胶囊宽 | 736px（768 容器 − 16×2 px） | 实测 rect |
| 胶囊圆角 multiline | 25px（`--radius-3xl`=20px × `--corner-radius-scale`=1.25；`corner-shape: superellipse(1.5)`） | computed + 变量 |
| 胶囊圆角 single-line | 22px（`--radius-token-composer-single-line` = `.25rem×5.5`），另带 `rounded-full`(9999px) 类；二者同特异性，实际生效值由样式表加载顺序决定 → **无法确定**（未在运行中见到 single-line 态） | 源码 |
| footer 外边距 | `mb-2`=8px；flush → 0 | computed |
| footer 水平 padding | `px-2`=8px；flush → 0 | computed |
| footer 列 gap | `gap-x-[5px]`=5px（左右各一，中间列 0px） | computed |
| 输入行 | `-mx-2` 抵消 footer px → 文本列 12px（`px-3`）距胶囊边 | computed 467−455=12 |
| 输入行底部 | `mb-1`=4px（输入行 48 = 编辑器 44 + 4） | 源码 |
| 编辑器 min-height | multiline 44px（2.75rem）；single-line 20px（1.25rem） | 内联样式 |
| 编辑器 max-height | 25dvh（240px @960） | computed |
| 附件槽 padding | 8px + 下 6px（空态 14px） | computed |
| 附件 gap | `gap-2`=8px | 源码 |
| 按钮尺寸 | 左/听写/发送均 28×28（`size-token-button-composer` = `.25rem×7`） | computed |
| 按钮圆角 | `rounded-full` 9999px | computed |
| 模型 pill | h 28、`px-2`（左右 8px） | computed |
| pill ↔ 听写 | 0px（相邻） | 实测 |
| 听写 ↔ 发送 | 8px（FooterActions `gap-2`） | 实测 |
| 胶囊与视口底 | sticky wrapper `pb-4`=16px | 实测 |

---

## 4. 透明度 / 视觉态

| 状态 | 规则 | 值 |
|---|---|---|
| 胶囊背景 | `bg-token-input-background/90` + `electron:dark:bg-token-dropdown-background`；实测 computed = rgb(45,45,45)（后类覆盖 90% 混合） | 不透明 |
| 胶囊模糊 | `backdrop-blur-lg` = blur(16px) | computed |
| 胶囊阴影 | elevation-prominent：`.5px` 发丝环 + `0 3px 7.5px #0000000a` + `0 0 20px #0000000d` | computed |
| **focus-within** | **样式表扫描 0 条 focus-within 规则；实测 focus 前后胶囊 border/bg/shadow 不变** → 胶囊无焦点边框。唯一焦点反馈 = 编辑器光标（白） | 扫描 + 实测 |
| 点击胶囊任意处 | `onMouseDown=Fds`：非交互元素（a/button/input/select/textarea/[contenteditable]/role=button…）上 mousedown → `preventDefault + editor.focus()` | 源码 |
| 发送按钮 | `bg-token-foreground`（#fff 圆盘）+ 图标 `text-token-dropdown-background`（深色箭头）；`transition-opacity`；`p-0.5`(2px)；`focus-visible:outline-2 outline-token-button-background`（实测 outline 色 rgb(13,13,13)） | computed |
| 发送 disabled/loading | `opacity-50 cursor-default`；loading 时显示 spinner | 源码 |
| ghost 按钮 hover（左/听写/模型 pill） | `enabled:hover:bg-token-list-hover-background`（实测 rgba(255,255,255,.078)）；active `bg-token-foreground/15`；open `bg-token-list-hover-background` | computed + 源码 |
| ghost 按钮颜色 | `text-token-text-tertiary`（实测 rgba(255,255,255,.498)）；图标 `text-token-text-primary`（#fff） | computed |
| IconButton disabled | `disabled:opacity-40` | 源码 |
| 模型 pill 文字 | 名称 `text-token-foreground`(#fff)、模式/副标 `tertiary` | computed |
| 拖拽悬停 | surface `bg-token-dropdown-background/50` | 源码 |
| 占位符 | `.ProseMirror .placeholder`（`::after` + `data-placeholder`），`color: var(--color-token-input-placeholder-foreground)`（实测 rgba(255,255,255,.498)）+ `opacity:.5` | computed + 源码 |

---

## 5. 行为逻辑

### 5.1 single-line / multiline 切换（核心）

```js
// kvs：composerLayoutMode ∈ 'multiline' | 'auto-single-line'
// 编辑器多行判定：auto-single-line 时 doc.childCount>1 或文本含 '\n' → isEditorMultiline
// 隐藏测量 span（bvs）：
//   <span class="pointer-events-none invisible absolute h-0 w-max max-w-none
//               overflow-hidden text-size-chat whitespace-pre">{getText()}</span>
// 宽度判定：textFitsSingleLine = 测量文本宽 + 32px ≤ 胶囊可用宽（Dvs=32 缓冲）
// Tvs：
//   'multiline'        → 恒 multiline
//   'auto-single-line' → 有附件 || 编辑器多行 || 语音布局 → multiline
//                        否则 lockedLayout（用户锁定）优先，无锁定用 textFitsSingleLine
```

- 线程默认 `composerLayoutMode:'auto-single-line'`；本实测会话由外层传入 `multiline`（DOM 无测量
  span、输入行用 multiline 类）→ **auto-single-line 行为未能在运行中完整观察，细节以源码为准**。
- 切换只影响类名（无过渡动画）；surface `overflow-visible`（single-line）vs `overflow-y-auto`（multiline）。
- single-line 下右区（`FooterControls`）`shrink-0 gap-2`，multiline 下 `w-full`。
- single-line footer 是单行 grid（`auto 1fr auto`），multiline 是两行 grid。

### 5.2 模式条件类汇总

| 条件 | 效果 |
|---|---|
| `layout==='single-line'` | footer `grid-cols-[auto_minmax(0,1fr)_auto] gap-2 px-2 py-1`；Input `min-w-0`；编辑器 h-9/nowrap/ellipsis；surface `rounded-full + overflow-visible` |
| `layout==='multiline'` | footer 两行 grid `mb-2 px-2`（flush 为 0）；Input `px-3`（flush 0）；编辑器 25dvh 滚动 |
| `radiusVariant==='single-line'` | 圆角 22px（无 rounded-full） |
| `utilityBarVariant==='home'` | surface 无 chrome 类、根加 `z-10`；工具条 `Cds` 带动画（hidden 时 translateY ±100%、`opacity-0`，`relative -mb-2` 或 `absolute inset-x-0` 贴 surface 上/下沿） |
| `surfaceVariant==='secondary'/'opaque'` | 换背景（线程不用） |
| `spacing==='flush'` | footer `mb-0 px-0`、Input `px-0`、附件 `mb-[5px]` |
| `isDragActive` | surface 背景半透明 |

### 5.3 发送 / 停止

- `canSubmit = !submitDisabled && (!isStreaming || 允许停止)`；`isStreaming && !canSubmit` → 显示**停止**按钮
  （方形图标，type=button）；否则**发送**按钮（箭头图标，type=submit）。
- 发送按钮 disabled = `isStreaming ? isSubmitting : submitDisabled`；**空稿不禁用**（实测空态 enabled；
  提交时由 onSubmit 守卫：文本空且无附件 → 不提交）。
- form `onSubmit`：`preventDefault` + `canSubmit && onSubmit(getText())`。
- `data-thread-find-composer="true"` 标记表单，供查找/悬浮逻辑定位。

### 5.4 听写（Dut）

- 三态：`idle`（麦克风图标）/ `dictating`（录音图标）/ `retry`（重试，仅 `canRetryDictation && !isTranscribing`）。
- `isTranscribing` → spinner；tooltip 文案随状态（「Click to dictate or hold」等）+ 快捷键 label。
- 交互：pointerdown 按下 150ms（`Aut=150`）内抬起 = 点击 → `startDictation('tap')`；
  按住 >150ms = `startDictation('press-and-hold')`；dictating 态点击 = 插入已识别文本。
- 按钮显隐 `isDictationButtonVisible`；disabled = `!isDictationSupported || !isDictationEnabled || thread.phase!=='inactive'`。
- 录音中：右区替换为 `Eut`（波形 canvas + 时长 + 停止）。

### 5.5 附件按钮（Glt）与上下文菜单（Zys）

- 左按钮 = `tm`（IconButton）`size:composer, uniform, color: ghost|ghostActive`，纸夹图标 16px #fff；
  tooltip「添加文件等内容」+ `@` 快捷键徽标（`Nh`，`text-xs`）。
- 点击打开上下文菜单（Zys）：`Add photos & files`（触发隐藏 file input，`multiple`）、库文件、
  apps、`Create image`、`Web search` 等项；`fileAttachmentDisabled`（语音布局或不可附加）时禁用。
- `onMouseDown: preventDefault`（避免抢焦点）。

### 5.6 模型 pill

- `HTs`（默认）/ `cEs`（power-picker 开启）：触发按钮 = `tm` `size:composer, color:ghost`，
  `className: max-w-48 min-w-0`，`disabled = modelsLoading || modelsError || models==null`，
  `loading` 显示 spinner；`data-codex-intelligence-trigger` 标记。
- 内容 = `_Cs`（ModelSwitcherLabel）：`selectedValue` = 模型名 + 推理档位（如「5.6 Terra 轻度」），
  `foreground:'tertiary'`，下拉时显示 chevron（实测 14×14 tertiary）。
- 菜单含 effort 子菜单、版本选择、内部模型搜索（`SearchInput` + `max-h-[250px]` 滚动列表）。

### 5.7 placeholder 与输入属性

- 编辑器 `aria-label` = placeholder（实测「使用 ChatGPT Work」）；占位符文本来自线程上下文
  （Work 模式 → 「Work with ChatGPT」，其他 → 「使用 ChatGPT」类）。
- ProseMirror 属性：`contenteditable, aria-multiline=true, dir=auto, role=textbox,
  spellcheck=true, translate=no, data-virtualkeyboard=true`。
- placeholder 实现：ProseMirror 空态渲染 `.placeholder` 节点（`::after` + `data-placeholder`，
  `opacity:.5`，`pointer-events:none`，`user-select:none`）。

### 5.8 其它

- 工具条插槽 `Cds`（非 home）：`isVisible` 控制显隐，隐藏时 `aria-hidden + inert + pointer-events-none opacity-0`。
- footer 容器查询：宽 ≤440px（electron ≤475px）隐藏 `_footerLabel`（模型名/短标签）。
- 键盘：编辑器 Backspace 在空稿时删除最后一个 system hint；`meta/ctrl+?` 触发侧栏切换快捷键。

---

## 6. 字号 / 字重

| 元素 | 字号 | 字重/行高 | 来源 |
|---|---|---|---|
| 输入文字（ProseMirror） | `--codex-chat-font-size` = 14px（electron；主题另有 16/17px 档位） | `leading-5` 20px；字重未显式设置（继承 400；实测计算无 445） | computed |
| 编辑器宿主 | `text-base` = 14px（electron），`text-size-chat` 同值 | 行高 21px | computed |
| 左/听写按钮 | `text-sm`（electron 12px，本实例 13px） | leading-[18px]；图标 16×16 | computed |
| 模型 pill | `text-sm` | leading-[18px]；chevron 14×14 | computed |
| 发送按钮 | 无文字（图标 16×16） | — | computed |
| 按钮整体（tm） | `text-sm` | leading-[18px] | 源码 |
| attach tooltip 的 @ 徽标 | `text-xs`（11/12px） | leading-none | 源码 |
| 附件卡文件名 | 未单独指定（继承） | — | 源码 |
| 代码块 | `--codex-chat-code-font-size`（继承 vscode 编辑器字号，实测 12px 档） | 20px | 源码 |

> 字重说明：dimi 注释认为 codex 输入/按钮为 445（`--vscode-font-weight`），但**实测 computed
> 未出现 445**（继承默认 400）；此点无法确定 codex 是否在运行时设了 445，dimi 沿用 445 属
> 之前设计决策，本报告标「无法确定」。

---

## 7. 与既有设计文档的关系

- 本文件是源码级（组件/类/行为）依据；`04-composer.md` 是实测数值依据（两者一致）。
- 未覆盖：语音布局（`Uds`/`Pvs`）、Home 菜单 composer、extension mini composer —— dimi 不涉及。

---

## dimi 差距（读 `Composer.vue` / `Composer.styles.ts` 逐条对比）

参考文件：
- `apps/native-client/DimiNative/src/renderer/components/Composer.vue`
- `apps/native-client/DimiNative/src/renderer/components/Composer.styles.ts`

### A. 结构差异

1. **无 form 语义**：codex 根是 `<form data-thread-find-composer class="relative flex flex-col">`，
   发送走 `type=submit` + `onSubmit`（空稿守卫在提交处）；dimi 根是 `<footer>`，发送走
   `button @click`。dimi 缺 `data-thread-find-composer` 锚点与「点击胶囊空白聚焦输入」的
   `Fds` 行为（dimi 点胶囊空白不会聚焦）。
2. **胶囊层级少一层**：codex = form > 包层(gap-2) > surface(Body) > attachments/footer；
   dimi = footer > capsule > footer(grid)。dimi 的 `capsule` 同时承担 surface 视觉与 Body 角色，
   无 `composer-surface-chrome` 类名（因此 codex 对 chrome 的 box-shadow/无边框规则、home
   z-10 等条件类在 dimi 不存在）。
3. **附件槽是 padding 而非独立元素**：dimi `capsule { padding: 14px 0 0 }` 模拟空态附件槽高度；
   codex 是 `_attachmentsDefault_1xj1z_2` 元素（8px+6px），有附件时显示 `flex flex-wrap gap-2`
   卡片列表。dimi 无任何附件 UI（左按钮点击显示「附件（暂未实现）」），也就没有：
   - 附件卡圆角 = 胶囊圆角 − 8px 的联动
   - flush 变体 `mb-[5px]`
   - 上传进度/失败徽标
4. **footer 容器查询缺失**：codex `_footer_1xj1z_2` 有 `container-type` 查询（≤440px/electron
   ≤475px 隐藏 label）；dimi 无。
5. **中间空列**：dimi `gridTemplateColumns: minmax(0,auto) auto minmax(0,1fr)` 与 codex 相同，
   但 dimi 没有 `col-span-full row-start-1 -mx-2` 的输入行显式跨列（dimi 用 `gridColumn:1/-1,
   gridRow:1, margin:0 -8px`，等效）；dimi 也未说明 col2 空列用途。

### B. 布局 / 数值差异

6. **胶囊高度**：dimi `minHeight: 98` **固定**；codex 高度随内容（空态 98px，附件出现时增高），
   无固定 min-height。
7. **胶囊圆角**：dimi `composerRadius: 25px` 硬编码；codex 是 token 计算
   `--radius-3xl × --corner-radius-scale`（不同主题可缩放）→ dimi 在缩放/浅色主题下不跟随。
8. **输入行**：dimi `inputRow` 等效；dimi `inputWrap` `minHeight: 44` + `marginBottom: 4` 等效
   codex 编辑器内联 `min-height: 2.75rem` + `mb-1`。但 dimi 输入行高度由 inputWrap min-height
   撑起，codex 由编辑器实例 min-height 撑起（编辑器宿主本身不设 min-height）—— 行为等效，
   机制不同。
9. **single-line 模式缺失**：dimi 无 `layout` 概念，无：
   - 文本宽度测量（`bvs` 隐藏 span + 32px 缓冲）
   - 换行/附件触发 multiline
   - single-line 的 h-9 宿主 / nowrap / ellipsis / 居中
   - 两套 footer grid（单行 vs 两行）
   codex 实测会话即 multiline，dimi 始终 multiline —— 现状恰好一致，但 codex 是动态的。
10. **模型 pill 宽度**：codex `max-w-48`(192px) + `truncate` + FooterExpandingControls
    `flex-1` 弹性占位；dimi 无 max-width、无弹性占位（右区直接排列）。窄窗下 codex 先截断
    pill 文本，dimi 可能溢出。
11. **发送按钮**：
    - disabled 语义不同：dimi `:disabled = !draft.trim() || !currentSessionId`（空稿禁用，
      opacity .5）；codex 空稿**不禁用**（disabled 仅 `submitDisabled`/streaming）。
    - dimi `marginLeft: 8` 模拟听写↔发送 gap；codex 用 FooterActions `gap-2`（等效 8px，且
      pill↔听写 0px 由无 margin 实现）→ dimi 缺 FooterActions 层。
    - dimi `transition: opacity .15s`；codex `transition-opacity`（值无法确定，可能默认 150ms）。
    - focus-visible outline：dimi `2px solid rgb(13,13,13)`；codex `focus-visible:outline-2
      outline-token-button-background`（token 实测 rgb(13,13,13)）✓ 一致。
12. **字号**：codex `--codex-chat-font-size` 14px（electron 实测）；dimi 输入 `fontSize: 14` ✓。
    但 codex 按钮字号是主题 `text-sm`（electron 12px / 本实例 13px），dimi `font.sm` 需核对
    是否为同 token（若 dimi 用 13px 固定值，在 electron 主题下应 12px）。
13. **字重 445**：dimi 输入与模型 pill 均 `fontWeight: 445`；codex 源码/实测未见 445（继承 400）
    → **无法确定** codex 是否有 445，dimi 445 与 codex 可能不一致。

### C. 视觉态差异

14. **hover**：dimi `rgba(255,255,255,.078)` = codex `--vscode-list-hoverBackground` 实测值 ✓；
    dimi active `.15` = codex `bg-token-foreground/15` ✓。
15. **发送 hover**：codex 发送按钮无 hover 背景（仅 transition-opacity）✓ dimi 无 hover ✓。
16. **胶囊背景**：dimi `colors.composerBg = rgb(45,45,45)` 不透明；codex `bg-token-input-background/90`
    90% + blur(16px)，后又被 `electron:dark:bg-token-dropdown-background`（rgb(45,45,45)）覆盖 → 
    最终一致；但 dimi 未还原 90% 混合的中间态（浅色/非 electron 环境会差）。
17. **占位符颜色**：dimi placeholder `textTertiary` ✓；但 codex 占位符**额外 opacity:.5**（最终
    约 rgba(255,255,255,.249)），dimi 未加 opacity → 亮度差异。
18. **胶囊阴影**：dimi 手写三阴影 = codex elevation-prominent ✓（含 .5px 发丝环 ✓）。

### D. 行为差异

19. **占位符文案**：dimi `data-placeholder="Message…"`；codex 本地化「使用 ChatGPT Work」/ 
    「Work with ChatGPT」（随模式）。dimi 未跟随。
20. **听写**：dimi 点击显示「听写（暂未实现）」；codex 有完整三态 + 150ms 长按 + 波形录音 UI。
21. **附件**：dimi 显示「附件（暂未实现）」；codex 有上下文菜单（文件/库/apps/Web search/
    Create image）+ 附件卡 + 选择文本 pill。
22. **模型 pill**：dimi 固定「轻度」模式文案 + 点击开设置；codex 是 model+effort 组合 label +
    power-picker（服务 tier）/effort 子菜单 + 内部模型搜索。dimi 无。
23. **composerToolbar**：dimi 自创 busy 工具栏（steer/queue/Cancel）悬浮在胶囊上方；codex
    **没有**此 UI —— 这是 dimi 的 TUI 遗留功能，非 codex 设计。
24. **空稿发送守卫**：codex 提交守卫在 `onSubmit`（`onPrepareDraft` 检查文本/附件）；dimi
    用按钮 disabled 拦截 → 两者行为接近，但 codex 允许空稿下按钮可点（无效果），dimi 置灰。
25. **行高**：codex 编辑器 `leading-5`=20px、宿主 21px；dimi 输入 `lineHeight: 20px` ✓。

### E. 建议优先级（复刻差距）

- P0（视觉差异大）：single-line/multiline 动态切换 + 测量；空稿发送不禁用；占位符 opacity
  .5 + 本地化文案；胶囊高度随内容（去掉固定 98）。
- P1：附件槽独立元素与卡片；footer 容器查询；模型 pill max-w-48/弹性占位；发送/听写 gap 用
  FooterActions 结构还原；fontSize 走 `--codex-chat-font-size` token。
- P2：form 语义 + `data-thread-find-composer` + 点击胶囊聚焦；home/extension 变体；语音布局。

---

## 附录：测不到 / 无法确定清单

1. single-line 态的实际圆角（`_singleLineRadiusSurface` 22px vs `rounded-full` 9999px 的
   级联胜者）—— 运行中未能触发 single-line。
2. auto-single-line 的实时切换（本会话线程被外层固定为 multiline；切换逻辑来自源码）。
3. `transition-opacity` 发送按钮的具体时长（dimi 用 .15s 假设）。
4. 输入/按钮字重 445 是否真实存在（computed 无 445，源码未显式设置）。
5. `--codex-chat-font-size` 的最终来源链（`--vscode-chat-font-size → --vscode-font-size →
   --text-base`；16/17px 档位何时启用）。
6. 胶囊 hover 态（surface 本身无 hover 规则，未发现任何 surface:hover）。
7. flush spacing 的实际使用场景（线程 composer 不用，extension/内联场景未观察）。
