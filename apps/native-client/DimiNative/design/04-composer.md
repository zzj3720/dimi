# 04 · Composer（底部输入区）设计逆向

> 目标：作为 dimi native-client 1:1 复刻 codex Composer 的唯一设计依据。
> 测量方式：CDP `Runtime.evaluate` 直读 codex 页面（viewport 1686×960，深色主题，Electron），
> `getBoundingClientRect()` + `getComputedStyle()`；日期 2026-08-04。
> 原则：每个数值都有实测来源；测不到的写「无法观察」，不猜。
> 注意：`composer` 的 `form` 与「输入胶囊」（`.composer-surface-chrome`）是两层 —— form 无背景无边框，
> 所有视觉都在 surface 层。

---

## 0. 结构树（带尺寸，实测）

```
form.relative.flex.flex-col                                    455,845  736×98
└─ div.relative.flex.w-full.flex-col.gap-2                      455,845  736×98
   ├─ input.hidden（隐藏文件输入，0×0）
   └─ div.composer-surface-chrome …                             455,845  736×98   ← 视觉胶囊
      └─ div.relative.z-10.flex.min-h-0.flex-1.flex-col          455,845  736×98
         ├─ div._attachmentsDefault_*（附件槽，空态）            455,845  736×14
         └─ div.contents（display:contents，0×0）
            └─ div._footer_*（grid）                             455,859  736×76
               ├─ div.min-w-0.col-span-full.row-start-1.-mx-2    455,859  736×48   ← 输入行（row 1）
               │  └─ div.mb-1.flex-grow.overflow-y-auto.px-3     455,859  736×44
               │     └─ div._root_*（ProseMirror 容器）          467,859  712×44
               │        └─ div.ProseMirror                       467,859  712×44
               │           ├─ p.placeholder（::after 占位符）    467,859  712×20
               │           └─ br.ProseMirror-trailingBreak       467,860  0×23
               ├─ div.min-w-0.col-start-1.row-start-2            463,907   28×28   ← 左按钮（row 2）
               │  └─ … > button[aria-label="添加文件等内容"]     463,907   28×28
               │     └─ svg.icon-xs（plus）                      469,913   16×16
               └─ div.min-w-0.col-start-3.row-start-2            501,907  682×28   ← 右按钮区（row 2）
                  └─ div.flex.min-w-0.items-center.justify-end.w-full
                     ├─ div.flex.min-w-0.flex-1.justify-end      501,907  618×28   ← flex-1 弹性占位
                     │  └─ div.flex.items-center                 1008,907 111×28
                     │     └─ button（模型 pill）                1008,907 111×28
                     │        ├─ span.label（模型名+模式）       1017,912  73×18
                     │        └─ svg.h-3.5.w-3.5（chevron）      1094,915  14×14
                     ├─ button[aria-label="听写"]                1119,907  28×28   ← 听写（与 pill 右缘 0px 相接）
                     └─ button[aria-label="发送"]                1155,907  28×28   ← 发送（与听写间距 8px）
```

关键几何：
- 胶囊右缘 = 1191（form 455+736）；footer 内容右缘 = 1183（1191−8 = `px-2`）。
- 顶部 14px 是**附件槽**（`_attachmentsDefault_`，空态也有 14px 高），不是 padding。
- footer 高 76 = 输入行 48 + 按钮行 28；`mb-2` = 8px；14 + 76 + 8 = 98。
- 文本左缘 467 = form 左缘 455 + 12（输入行 `px-3`）→ 文本列 712 = 736 − 24（左右各 12px）。

---

## 1. form 整体

| 属性 | 值 | 来源 |
|---|---|---|
| 位置 | x=454.5（取整 455）, y=845 | getBoundingClientRect |
| 尺寸 | 736 × 98 | getBoundingClientRect |
| display / flex-direction | flex / column（`relative flex flex-col`） | computed |
| 背景 | transparent（无）—— 视觉背景在 surface 层 | computed |
| 边框 / 阴影 | 无（0px / none） | computed |
| 底部距视口 | **17px**（960 − 943 实测）；sticky wrapper `pb-4` = 16px | computed + rect |
| 父链 | form ← `div.max-w-(--thread-content-max-width).px-toolbar`（768×98，px 16，mx-auto）← `div.sticky.bottom-0.mt-auto.w-full.pb-4`（pb 16px） | DOM |

---

## 2. surface 胶囊（`.composer-surface-chrome`）

类名：`relative flex flex-col composer-surface-chrome bg-token-input-background/90 backdrop-blur-lg electron:dark:bg-token-dropdown-background overflow-y-auto _multilineSurface_*`

| 属性 | 值 | 来源 |
|---|---|---|
| 位置 / 尺寸 | 455,845 · 736×98（与 form 完全重合） | rect |
| 圆角 | **25px** | computed |
| 背景 | **rgb(45,45,45)**（不透明，实测 computed） | computed |
| 背景机制 | 基础类 `bg-token-input-background/90` = `--color-token-input-background`(rgba(45,45,45,0.96))×90% ≈ rgba(45,45,45,0.864)，**被** `electron:dark:bg-token-dropdown-background` = `--color-token-dropdown-background` = **rgb(45,45,45)** 覆盖（Electron 深色环境实测值） | 类 + token 解析 |
| backdrop-filter | **blur(16px)**（`backdrop-blur-lg`） | computed |
| 边框 | **0px**（无边框；发丝环由 box-shadow 承担） | computed |
| box-shadow | `rgba(255,255,255,0.157) 0 0 0 0.5px`（发丝环，≈ `--color-token-input-border` rgba(255,255,255,0.156)）+ `rgba(0,0,0,0.04) 0 3px 7.5px 0` + `rgba(0,0,0,0.05) 0 0 20px 0` | computed |
| overflow-y | auto | computed |
| transition | `all`（computed，即无实际时长 → hover/焦点切换即时） | computed |
| 焦点态 | **无变化**：focus 前后 border / bg / box-shadow 完全一致；样式表扫描 0 条 `focus-within` 规则 | 实测 + 扫描 |
| 附件槽 | 胶囊内顶部 `div._attachmentsDefault_*` 736×14（空态占位；有附件时渲染附件） | rect |

> 结论：codex 的胶囊**没有 focus-within 高亮**。聚焦时唯一反馈 = 输入区白色 caret + 常驻发丝环。

---

## 3. 输入区（ProseMirror）

结构：`div.col-span-full.row-start-1.-mx-2`（margin 0 −8px，抵消 footer `px-2`）→ `div.mb-1.flex-grow.overflow-y-auto.px-3`（margin-bottom 4px，padding 0 12px）→ `div._root_*`（max-height 25dvh）→ `div.ProseMirror`。

| 属性 | 值 | 来源 |
|---|---|---|
| 文本左缘距 form | **12px**（467 − 455；`px-3`）；右同 12px；文本列 712px | rect |
| 输入行高（row1） | 48px（44 内容 + 4 margin-bottom） | rect + computed |
| 最小高度 | **44px**（ProseMirror computed minHeight） | computed |
| 最大高度 | **25dvh = 240px**（视口 960；`max-h-[25dvh]`），超出滚动 | computed + rect |
| 字体 | **14px / line-height 20px / weight 445** / `-apple-system, "system-ui", "Segoe UI", sans-serif` | computed |
| 文字颜色 | **rgb(255,255,255)**（`text-token-foreground` = #ffffff） | computed |
| caret 颜色 | rgb(255,255,255) | computed |
| white-space | break-spaces | computed |
| 对齐 | **顶对齐**：`p.placeholder` y=859 = 输入行顶部（非垂直居中） | rect |
| 多行 | max-height 240px 后 `overflow-y:auto`；surface 层 overflow-y auto | computed |
| 占位符文字 | 「**使用 ChatGPT Work**」（随 workspace 变化的动态文案；`p.placeholder::after`） | DOM |
| 占位符颜色 | **rgba(255,255,255,0.498)**（= `--vscode-input-placeholderForeground` = text-tertiary） | computed |
| 占位符字体 | 14px / 445 / line-height 20px | computed |

---

## 4. footer 网格

| 属性 | 值 | 来源 |
|---|---|---|
| 类名 | `_footer_* grid grid-cols-[minmax(0,auto)_auto_minmax(0,1fr)] items-center gap-x-[5px] select-none mb-2 px-2` | DOM |
| grid-template-columns | `minmax(0,auto) auto minmax(0,1fr)` | 类名 |
| 实际列宽 | col1 = **28px**（463–491）；col2 = **0px**（空列，491–501 中 0 宽 + 两个 5px gap）；col3 = **682px**（501–1183） | rect |
| column-gap | 5px | computed |
| align-items | center | computed |
| padding | 0 8px（`px-2`） | computed |
| margin-bottom | **8px**（`mb-2`） | computed |
| 行高 | row1 = 48px（输入行），row2 = 28px（按钮行） | rect |

---

## 5. 左按钮（+ 添加文件）

通用按钮 base class（三个圆按钮 + pill 共用）：
`no-drag cursor-interaction items-center gap-1 border whitespace-nowrap select-none focus:outline-none disabled:cursor-not-allowed disabled:opacity-40 flex rounded-full text-token-text-tertiary enabled:hover:bg-token-list-hover-background enabled:active:bg-token-foreground/15 data-[state=open]:bg-token-list-hover-background border-transparent h-token-button-composer px-2 py-0 text-sm leading-[18px] aspect-square shrink-0 items-center justify-center !px-0`

| 属性 | 值 | 来源 |
|---|---|---|
| 位置 / 尺寸 | 463,907 · **28×28** | rect |
| 可访问名 | aria-label「**添加文件等内容**」（无 title 属性） | DOM |
| 圆角 | 9999px | computed |
| 背景 / 边框 | transparent / `1px solid transparent` | computed |
| 颜色 | rgba(255,255,255,0.498)（text-tertiary） | computed |
| 图标 | svg 渲染 **16×16**（attr 20×20, vb 0 0 20 20），plus path（见 §8） | rect + DOM |
| hover（enabled） | bg **rgba(255,255,255,0.078)**（`--vscode-list-hoverBackground`）；color 不变 | token 解析 |
| active（enabled） | bg **rgba(255,255,255,0.15)**（`--color-token-foreground` #ffffff 15%） | token 解析 |
| disabled | opacity 0.4 + cursor not-allowed（base 类；实际不触发） | 类名 |
| transition | 无（hover/active 即时切换） | computed |
| cursor | computed **default**（`cursor-interaction` 在 macOS 解析为 default，实测） | computed |

---

## 6. 模型 pill

| 属性 | 值 | 来源 |
|---|---|---|
| 位置 / 尺寸 | 1008,907 · **111×28**（宽为 auto：label + chevron + padding） | rect |
| 类名 | base class 同 §5（无 `aspect-square !px-0`，有 `min-w-0`）；padding `0 8px`，gap 4px | DOM |
| 字体 | **13px / weight 445 / line-height 18px**（`text-sm leading-[18px]`） | computed |
| 颜色 | rgba(255,255,255,0.498)（按钮级） | computed |
| 可见内容（label 73×18 @ 1017,912） | ① 模型名「5.6 Sol」43×18，color **rgb(255,255,255)**（`text-token-text`）② 模式「轻度」26×18，color rgba(255,255,255,0.498) ③ 间距 gap 4 | rect + computed |
| chevron | svg **14×14** @ 1094,915（class `me-0.5 h-3.5 w-3.5 shrink-0 text-token-text-tertiary`；attr 16×16, vb 0 0 16 16），color rgba(255,255,255,0.498) | rect + computed |
| 隐藏测量 span | 按钮内 `_ModelPickerTriggerMeasurement_*`（85×74 @ 455,845，visibility:hidden）用于撑开下拉宽度 —— 不可见 | DOM |
| hover / active | 同 §5：bg rgba(255,255,255,0.078) / rgba(255,255,255,0.15)，color 不变 | token 解析 |
| 行为 | 点击打开模型选择器（含模式选项，当前会话模式「轻度」）；无 title/aria | DOM |
| 模式文案 | 随选择变化（DOM 中见 轻度/极高/最高 等）；**「轻度」不是硬编码** | DOM |

---

## 7. 听写 / 发送按钮

### 听写（mic）
| 属性 | 值 | 来源 |
|---|---|---|
| 位置 / 尺寸 | 1119,907 · **28×28**（与 pill 右缘 0px 相接） | rect |
| 可访问名 | aria-label「**听写**」 | DOM |
| 样式 | base class 同 §5；图标 svg 渲染 16×16（attr 20×20），mic path（见 §8），color rgba(255,255,255,0.498) | DOM + computed |

### 发送
| 属性 | 值 | 来源 |
|---|---|---|
| 位置 / 尺寸 | 1155,907 · **28×28**（与听写间距 **8px**；右缘 1183 = 内容右缘） | rect |
| 类名（disabled，空输入） | `cursor-interaction size-token-button-composer flex items-center justify-center rounded-full transition-opacity focus-visible:outline-2 bg-token-foreground p-0.5 focus-visible:outline-token-button-background cursor-default opacity-50` | DOM |
| 有文字时 | 移除 `cursor-default opacity-50` | 实测（插入文本后） |
| 背景 | **rgb(255,255,255)**（`bg-token-foreground` = #ffffff，高对比白圆） | computed |
| 边框 | 0px | computed |
| 圆角 | 9999px | computed |
| padding | 2px（`p-0.5`） | computed |
| 图标 | svg 渲染 16×16（attr 20×20），上传箭头 path；class `icon-xs text-token-dropdown-background` → **fill rgb(45,45,45)**（深色箭头） | computed + DOM |
| disabled | **opacity 0.5** + cursor default | computed |
| enabled | opacity 1 | computed（过渡结束后） |
| transition | **opacity 0.15s cubic-bezier(0.4, 0, 0.2, 1)**（实测过渡中间值 0.587 → 0.15s 后 1） | computed |
| hover | **无 hover 底色规则**（类名只有 transition-opacity；悬停外观不变）——无法悬停实测，依据类名 | DOM |
| focus-visible | outline-2 + outline-color `--color-token-button-background` = rgb(13,13,13)；**无法观察实际渲染**（程序化 focus 不触发 :focus-visible） | 类名 + token |

---

## 8. 图标（实测 path，全部 `fill="currentColor"` 曲线 path）

- **plus**（vb 0 0 20 20，渲染 16×16）：
  `M9.33496 16.5V10.665H3.5C3.13273 10.665 2.83496 10.3673 2.83496 10C2.83496 9.63273 3.13273 9.33496 3.5 9.33496H9.33496V3.5C9.33496 3.13273 9.63273 2.83496 10 2.83496C10.3673 2.83496 10.665 3.13273 10.665 3.5V9.33496H16.5L16.6338 9.34863C16.9369 9.41057 17.165 9.67857 17.165 10C17.165 10.3214 16.9369 10.5894 16.6338 10.6514L16.5 10.665H10.665V16.5C10.665 16.8673 10.3673 17.165 10 17.165C9.63273 17.165 9.33496 16.8673 9.33496 16.5Z`
- **send 上传箭头**（vb 0 0 20 20，渲染 16×16）：
  `M9.33467 16.6663V4.93978L4.6374 9.63704L4.1667 9.16634L3.69599 8.69661L9.52998 2.86263L9.63447 2.77767C9.8925 2.60753 10.2433 2.63564 10.4704 2.86263L16.3034 8.69661L16.3884 8.80111C16.5588 9.05922 16.5306 9.40982 16.3034 9.63704C16.0762 9.86414 15.7255 9.89242 15.4675 9.722L15.363 9.63704L10.6647 4.9388V16.6663C10.6647 17.0336 10.367 17.3314 9.99971 17.3314C9.63259 17.3312 9.33467 17.0335 9.33467 16.6663ZM4.6374 9.63704C4.3777 9.89674 3.95569 9.89674 3.69599 9.63704C3.43657 9.37744 3.43668 8.95628 3.69599 8.69661L4.6374 9.63704Z`
- **chevron**（vb 0 0 16 16，渲染 14×14）：
  `M12.1338 5.94433C12.3919 5.77382 12.7434 5.80202 12.9707 6.02929C13.1979 6.25656 13.2261 6.60807 13.0556 6.8662L12.9707 6.9707L8.47067 11.4707C8.21097 11.7304 7.78896 11.7304 7.52926 11.4707L3.02926 6.9707L2.9443 6.8662C2.77379 6.60807 2.80199 6.25656 3.02926 6.02929C3.25653 5.80202 3.60804 5.77382 3.86617 5.94433L3.97067 6.02929L7.99996 10.0586L12.0293 6.02929L12.1338 5.94433Z`
- **mic 听写**（vb 0 0 20 20，渲染 16×16）：
  `M15.7806 10.1963C16.1326 10.3011 16.3336 10.6714 16.2288 11.0234L16.1487 11.2725C15.3429 13.6262 13.2236 15.3697 10.6644 15.6299L10.6653 16.835H12.0833L12.2171 16.8486C12.5202 16.9106 12.7484 17.1786 12.7484 17.5C12.7484 17.8214 12.5202 18.0894 12.2171 18.1514L12.0833 18.165H7.91632C7.5492 18.1649 7.25128 17.8672 7.25128 17.5C7.25128 17.1328 7.5492 16.8351 7.91632 16.835H9.33527L9.33429 15.6299C6.775 15.3697 4.6558 13.6262 3.84992 11.2725L3.76984 11.0234L3.74445 10.8906C3.71751 10.5825 3.91011 10.2879 4.21808 10.1963C4.52615 10.1047 4.84769 10.2466 4.99347 10.5195L5.04523 10.6436L5.10871 10.8418C5.8047 12.8745 7.73211 14.335 9.99933 14.335C12.3396 14.3349 14.3179 12.7789 14.9534 10.6436L15.0052 10.5195C15.151 10.2466 15.4725 10.1046 15.7806 10.1963ZM12.2513 5.41699C12.2513 4.17354 11.2437 3.16521 10.0003 3.16504C8.75675 3.16504 7.74835 4.17343 7.74835 5.41699V9.16699C7.74853 10.4104 8.75685 11.418 10.0003 11.418C11.2436 11.4178 12.2511 10.4103 12.2513 9.16699V5.41699ZM13.5814 9.16699C13.5812 11.1448 11.9781 12.7479 10.0003 12.748C8.02232 12.748 6.41845 11.1449 6.41828 9.16699V5.41699C6.41828 3.43889 8.02221 1.83496 10.0003 1.83496C11.9783 1.83514 13.5814 3.439 13.5814 5.41699V9.16699Z`

---

## 9. 状态 / transition 汇总

| 控件 | default | hover | active | disabled | transition |
|---|---|---|---|---|---|
| + 添加文件 / 听写 / 模型 pill | bg transparent, color rgba(255,255,255,0.498), border 1px transparent | bg rgba(255,255,255,0.078), color 不变 | bg rgba(255,255,255,0.15) | opacity 0.4, cursor not-allowed | 无（即时） |
| 发送 | bg #fff, 箭头 rgb(45,45,45) | 无底色变化（只有 opacity 过渡） | 无 | **opacity 0.5**, cursor default | **opacity 0.15s cubic-bezier(0.4,0,0.2,1)** |

实测 token：
| token | 值 |
|---|---|
| --color-token-dropdown-background | rgb(45,45,45) |
| --color-token-input-background | rgba(45,45,45,0.96) |
| --color-token-input-border | rgba(255,255,255,0.156) |
| --color-token-foreground | #ffffff |
| --color-token-text-tertiary | rgba(255,255,255,0.498) |
| --color-token-text-secondary | color-mix(#ffffff 65%, transparent) ≈ rgba(255,255,255,0.65) |
| --vscode-list-hoverBackground | rgba(255,255,255,0.078) |
| --vscode-input-placeholderForeground | rgba(255,255,255,0.498) |
| --vscode-button-background | rgb(13,13,13)（发送 focus-visible outline 色） |

---

## 10. 忙碌 / 队列状态

- **无法观察**：需要正在运行的会话（busy）才能看到 codex 忙碌态 UI；当前会话空闲。
- dimi 的 `composerToolbar`（steer / queue / Cancel / queued count，浮在胶囊上方 `bottom: calc(100%+8px)`）是 **dimi 自有设计**，与 codex 无对应 —— 已确认并保留差异。

---

## 11. dimi 差距（codex 实测 vs dimi 当前代码）

对照文件：`src/renderer/components/Composer.vue`、`Composer.styles.ts`、`styles/theme.ts`、`icons.ts`。

| # | 项 | codex 实测 | dimi 当前 | 差距 |
|---|---|---|---|---|
| 1 | 左按钮 | **+ 添加文件**（aria「添加文件等内容」，plus 图标） | **gear 设置**（title Settings） | 图标/位置/行为全错；codex 的 plus 在**左**，dimi 的 plus 在**右** |
| 2 | 听写按钮 | 右区有 mic 听写（28×28，aria「听写」） | 无 | **缺失**，需新增 + mic path（§8） |
| 3 | 模型 pill 结构 | button：模型名(#fff) + 模式(tertiary) + **chevron 14×14** | span：无 chevron；modelPill 全 tertiary，mode 加 opacity 0.85 | chevron 缺失；模型名颜色应 #fff（`text-token-text`）；opacity 0.85 是错误做法 |
| 4 | 模型 pill 文字 | 13px / **weight 445** / 18px | 13px，weight 未设（继承） | 需加 weight 445 |
| 5 | 模型 pill hover | 只改 bg rgba(255,255,255,0.078)，color 不变 | hover 改 bg + **color 变 text** | dimi 多改了颜色 |
| 6 | 模式文案 | 随选择变化（当前「轻度」） | 硬编码「轻度」 | 行为差异（dimi 无模型选择器时至少标注） |
| 7 | 发送按钮 hover | **无** hover 底色 | `&:hover:not(:disabled) { background:#f0f0f0 }` | dimi 多了 hover 变灰，codex 无 |
| 8 | 发送图标色 | rgb(45,45,45)（=dropdown-background） | #181818 | 数值不同 |
| 9 | 发送 transition | opacity 0.15s cubic-bezier(0.4,0,0.2,1) | 无 | 缺失 |
| 10 | 发送 disabled | opacity 0.5 + cursor default | opacity 0.5 + cursor default | ✅ 一致 |
| 11 | 发送 focus-visible | outline-2，色 rgb(13,13,13) | 无 outline | 缺失（可后补） |
| 12 | 胶囊焦点态 | **无** focus-within 变化（border 0，环常驻） | `&:focus-within { borderColor: rgba(2,133,255,0.7) }` + border 1px transparent | dimi 聚焦出现**蓝边框**，codex 没有 —— dimi 自有设计，需移除或降级 |
| 13 | 胶囊边框 | border 0（发丝环 = box-shadow 0.5px） | border 1px transparent + 相同 box-shadow | 几何等价，机制不同；焦点态差异见 #12 |
| 14 | 输入对齐 | 顶对齐（p y=859 = 输入行顶部） | `inputWrap` flex + `align-items:center` 垂直居中 | dimi 垂直居中，codex 顶对齐 |
| 15 | 输入 max-height | 25dvh = 240px（@960） | 160px | 数值不同（dimi 需随视口 25dvh） |
| 16 | 输入字体 | 14px / 20px / **445** | 14px / 20px，weight 未设 | 需加 445 |
| 17 | 占位符 | 「使用 ChatGPT Work」（workspace 动态） | 「Message…」（data-placeholder 固定） | 文案机制不同（dimi 可保留或接入 workspace 名） |
| 18 | 按钮 hover 底色 | rgba(255,255,255,0.078) | colors.hover rgba(255,255,255,0.05) | 数值不对，需 0.078 |
| 19 | 按钮 active | bg rgba(255,255,255,0.15) | 无 active 态 | 缺失 |
| 20 | 按钮 disabled | base 类 opacity 0.4 + cursor not-allowed | composerBtn 无 disabled 样式 | 缺失 |
| 21 | 右区按钮间距 | 听写↔发送 **8px**；pill↔听写 0px | composerRight `gap: 2` | 需 8px（pill 侧 0） |
| 22 | 按钮 cursor | computed **default**（cursor-interaction） | cursor: pointer | dimi 手型 vs codex default |
| 23 | 胶囊顶部 14px | 附件槽（`_attachmentsDefault_`） | capsule `padding-top:14px` | 视觉等价；dimi 无附件 UI 槽 |
| 24 | footer margin-bottom | 8px（mb-2） | 6px（+2px border 补偿） | 几何结果同为 98px，机制不同 |
| 25 | 图标 plus/send | 实测 path | icons.plus / icons.send | ✅ path 一致 |
| 26 | 图标 chevron(16×16)/mic | 见 §8 | 缺失 | 需新增 |
| 27 | 胶囊背景 | rgb(45,45,45)（token-dropdown-background） | colors.composerBg rgb(45,45,45) | ✅ 一致 |
| 28 | 忙碌/队列工具栏 | codex 无对应（无法观察忙碌态） | composerToolbar 浮层 | dimi 自有设计，保留并标注 |

### 优先修复建议（实现 agent）
1. 左按钮换 plus（复用 icons.plus）、右区加听写（新增 mic icon）、pill 加 chevron（新增 16×16 chevron）+ 模型名 #fff。
2. 删除胶囊 `&:focus-within` 蓝边框；hover 底色 0.05→0.078；补 active 0.15；按钮 hover 不再改 color。
3. 发送按钮：去 hover #f0f0f0、图标色 #181818→#2d2d2d、补 opacity 过渡 0.15s、补 focus-visible outline。
4. 输入区：去垂直居中（顶对齐）、max-height 160→25dvh、weight 445。
5. 右区间距：pill/听写/发送 布局改 0px + 8px。
