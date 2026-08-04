# 05. 弹窗 / 菜单 / 图标系统（Codex 实测逆向）

> 目标：作为 dimi native-client 像素级复刻 codex 弹窗/菜单/图标系统的**唯一设计依据**。
> 所有数值均为 2026-08-04 对 codex（ChatGPT.app，Electron，窗口 1686×960，深色主题）实际渲染的 CDP 测量，不是猜测。
> 测量方法：`node /tmp/cdp_eval.js 9223 <pageId> '<js>'`（Runtime.evaluate）。页面无法 JS 滚动，但 DOM 全量可读。
> 与 CODEX_DESIGN.md 冲突时，以本文档实测为准；实现与本文档冲突时，先改文档再改代码。

---

## 1. 图标系统（实测）

### 1.1 尺寸阶梯（CSS 实测，`app-44wrUC9v.css`）

svg 的 `width`/`height` 属性与 viewBox 同值（如 20/20、20/21、21/21、16/16），**渲染尺寸由 CSS 类决定，与属性无关**。完整阶梯：

| 类 | width | height | 实测确认 |
|---|---|---|---|
| `.icon-3xs` | 10px | 10px | 阶梯定义（页面无实例） |
| `.icon-xxs` | 12px | 12px | 阶梯定义（页面无实例） |
| `.icon-2xs` | 14px | 14px | ✔ 多次实测 |
| `.icon-xs` | 16px | 16px | ✔ 多次实测 |
| `.icon-sm` | 18px | 18px | ✔ 多次实测 |
| `.icon-base` | 20px | 20px | 阶梯定义 |
| `.icon-md` | 24px | 24px | 阶梯定义（spinner 例外，见 1.4） |
| `.icon-lg` | 28px | 28px | 阶梯定义 |

另有按钮级强制规则：`[&>svg]:!h-4 [&>svg]:!w-4` → `width/height: calc(var(--spacing)*4)`，`--spacing=.25rem` → **16×16 且 !important**（置顶/归档 hover 按钮用，见 1.3）。

### 1.2 图标渲染规律

- 绝大多数 `<path>` 带 `fill="currentColor"`；颜色跟随上下文 CSS（如 `text-token-*` 类 / 按钮前景色）。
- 少数 svg 的 path **不带 fill 属性**（颜色由 CSS 控制）：云端/bolt 徽标、固定摘要 dots、发送区大图标、模型 pill 小图标。
- sectionChevron 的分组变体同时带 `fill="currentColor"` + `stroke="currentColor"`（2 个实例）。
- 无 `fill="none"` 的 path（svg 元素自身的 computed `fill:none` 与 path 无关）。
- 图标颜色实测值：`text-token-description-foreground` = `rgba(255,255,255,0.498)`；`text-token-foreground/80` ≈ `rgba(255,255,255,0.8)`；导航图标 `oklab(0.999994 …/0.85)` ≈ `rgba(255,255,255,0.85)`。

### 1.3 每种尺寸的用途清单（页面 190 个 svg 全量枚举）

| 尺寸类 | 渲染 | 用途（实测上下文） |
|---|---|---|
| `icon-xs` | 16×16 | header 切换侧边栏（columns，`rotate-180`）、隐藏边栏、返回/前进（前进=`-scale-x-100`）、搜索按钮、优先级按钮、固定摘要 dots、分享 pill 上传图标、侧栏导航（新对话/站点/已安排/插件 4 个，vb 16×16）、思考折叠 chevron（`rotate-90`，vb 20×20）、回复优秀/回复不佳（vb 20×21，后者 `rotate-180`）、复制消息（vb 21×21）、添加文件 plus、发送箭头、消息 hover 置顶/归档 |
| `icon-sm` | 18×18 | header「ChatGPT 对话操作」（ellipsis，vb 21×21）、侧栏底部「打开帮助菜单」（?，vb 20×20） |
| `icon-2xs` | 14×14 | 模式切换 chevron（vb 20×21，`text-token-input-placeholder-foreground`）、侧栏分组 chevron（vb 20×21，hover 时显示 `opacity-0→100`）、云端聊天小徽标（vb 20×20）、表格展开（vb 20×20）/复制（vb 21×21）小按钮 |
| 无类（按钮强制） | 16×16 | 置顶聊天（`translate-x-px` + 按钮 `[&>svg]:!h-4 !w-4`）、归档聊天（无 class + 同上强制） |
| `icon-xs`（异常） | **14×16** | 「项目侧边栏选项」（vb 21×21，6 个）、「在 <项目> 中开始新聊天」（vb 16×16，6 个）、「添加新项目」（vb 20×20，1 个）——这些按钮内 `icon-xs` 渲染为 14 宽 × 16 高，**成因未进一步确认（受父级约束，无法观察完整规则），实现时按 16×16 处理并目测复核** |

**置顶/归档 hover 按钮实测**（66 组）：按钮 `!h-5 !w-5` = 20×20、`[&>svg]:!h-4 [&>svg]:!w-4` = 16×16、`sidebar-hover-icon-button-tint`、`electron:rounded-md`、hover 时 `hover:text-token-foreground`（默认 tint 色未单独解析）。

### 1.4 spinner 例外

- 1 个加载 spinner：vb `0 0 24 24`、类 `icon-xs text-token-text-secondary animate-spin`，实测渲染 **16.29×16.29**（`animate-spin` 旋转动画中的 transform scale 导致非整数值）。

### 1.5 dimi icons.ts 的 19 个图标 ↔ codex 出处对照表

| dimi key | viewBox（icons.ts） | codex 出处（实测界面元素） | codex 渲染尺寸类 | path 是否一致 |
|---|---|---|---|---|
| menu | 0 0 20 20 | 切换侧边栏按钮（columns） | icon-xs 16×16 | 一致 |
| hideSidebar | 0 0 20 20 | 隐藏边栏按钮 | icon-xs 16×16 | 一致 |
| back | 0 0 20 20 | 返回按钮 | icon-xs 16×16 | 一致 |
| forward | 0 0 20 20 | 前进按钮（同 back path，`-scale-x-100` 镜像） | icon-xs 16×16 | 一致 |
| share | 0 0 20 20 | header 分享 pill 上传图标 | icon-xs 16×16 | 一致 |
| dots | 0 0 20 20 | 固定摘要按钮（dots 列表） | icon-xs 16×16 | 一致 |
| more | 0 0 20 20 | 与 menu 同 path（菜单三线） | icon-xs 16×16 | 一致 |
| ellipsis | 0 0 21 21 | header「ChatGPT 对话操作」 | **icon-sm 18×18** | 一致 |
| newChat | 0 0 16 16 | 侧栏导航「新对话」 | icon-xs 16×16 | 一致 |
| sites | 0 0 16 16 | 侧栏导航「站点」 | icon-xs 16×16 | 一致 |
| scheduled | 0 0 16 16 | 侧栏导航「已安排」 | icon-xs 16×16 | 一致 |
| plugins | 0 0 16 16 | 侧栏导航「插件」 | icon-xs 16×16 | 一致 |
| gear | 0 0 20 20 | 优先级按钮（需要关注） | icon-xs 16×16 | 一致 |
| chevronDown | 0 0 20 20 | 思考折叠 chevron（`rotate-90`） | icon-xs 16×16 | 一致 |
| sectionChevron | 0 0 20 21 | 模式切换 chevron + 侧栏分组 chevron | **icon-2xs 14×14** | 一致 |
| copy | 0 0 21 21 | 复制消息按钮；表格复制（14×14） | icon-xs 16×16 / icon-2xs 14×14 | 一致 |
| plus | 0 0 20 20 | composer「添加文件等内容」 | icon-xs 16×16 | 一致 |
| send | 0 0 20 20 | 发送按钮（`text-token-dropdown-background` → 图标为深色 #2d2d2d，白底按钮） | icon-xs 16×16 | 一致 |
| help | 0 0 20 20 | 侧栏「打开帮助菜单」? | **icon-sm 18×18** | 一致 |

**dimi 尚未捕获的 codex 图标**（本次实测确认真实存在）：置顶 pin（vb 20×20）、归档 box（vb 20×20）、回复优秀 thumbs（vb 20×21）、回复不佳 thumbs（vb 20×21）、搜索（vb 16×16）、spinner（vb 24×24）、关闭 x（vb 20×20 附近，弹窗右上角 16×16）、菜单项 radio/check 指示器（vb 17×17）、云端/bolt 小徽标（vb 20×20，icon-2xs）。

---

## 2. 弹窗 / 菜单通用样式（实测）

### 2.1 模态弹窗 `codex-dialog`（实测：分享弹窗）

Radix Dialog 门户直挂 `<body>` 下。

| 属性 | 值 | 来源 |
|---|---|---|
| 定位 | `fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2`，`z-50` | class |
| 尺寸 | `w-[520px]`，`max-w-[92vw]`；实测 520×269（内容自适应） | class + rect |
| 圆角 | **24px**（`rounded-[24px]`/`rounded-3xl`） | computed |
| 背景 | `bg-token-dropdown-background/90` → `oklab(0.297161 0.0000135154 0.00000594556 / 0.9)` ≈ **rgb(45,45,45) @ 90%**（= #2d2d2d @ 90%） | computed |
| 边框 | **无 border**；`ring-token-border ring-[0.5px]` → `rgba(255,255,255,0.082) 0 0 0 0.5px` | computed |
| 阴影 | `shadow-lg` → `rgba(0,0,0,0.1) 0 4px 8px -2px`（与 ring 同一 box-shadow 列表） | computed |
| 玻璃 | `backdrop-filter: blur(24px)`（`backdrop-blur-xl`），`overflow: hidden` | computed |
| 内边距 | 弹窗本身 0；内容区由内部 `form` 提供 `px-5 py-5` = **20px** | class |
| 关闭按钮 | 24×24，贴右上角（距顶/右 16px）；`p-1`(4px) + `rounded`(4px)，内含 16×16 关闭 x，色 `text-token-foreground/80`（≈rgba(255,255,255,0.8)）；内容区 `pe-8`(32px) 避让 | rect + class |
| 标题 | `heading-dialog min-w-0 font-semibold` → **20px/28px，weight 600，#fff** | computed |
| 描述 | `text-token-description-foreground text-base leading-normal` → **14px/21px，rgba(255,255,255,0.498)** | computed |
| 区块间距 | 相邻区块 `pt-3` = 12px | class |
| 内容卡片 | `rounded-2xl` = 20px 圆角，bg `color(srgb 0.0784314 0.0784314 0.0784314 / 0.92)` ≈ **#141414 @ 92%**，border 1px `rgba(255,255,255,0.082)`，padding 12px | computed |
| 圆形图标容器 | `size-10` = 40×40，`rounded-full`，bg `rgba(255,255,255,0.05)`，图标色 `rgba(255,255,255,0.65)` | computed |
| 主操作 pill | `rounded-full`，bg **rgb(45,45,45)**，border 1px `rgba(255,255,255,0.157)`，padding 6px | computed |
| **主按钮（白）** | bg **#fff**，色 **rgb(45,45,45)**，`rounded-[12.5px]`，padding **6px 16px**，14px/18px，border 1px `rgba(255,255,255,0.082)`；实测 110×32 | computed + rect |

### 2.2 弹窗 backdrop `codex-dialog-overlay`

| 属性 | 值 | 来源 |
|---|---|---|
| 定位 | `fixed inset-0 z-50`（全屏 1686×960） | class + rect |
| 背景（Electron） | `electron:bg-[#00000022]` → **rgba(0,0,0,0.133)**（很浅） | computed |
| 背景（Extension） | `extension:bg-token-editor-background/80` | class |
| blur | **无**（computed `backdropFilter: none`） | computed |

### 2.3 下拉菜单（Radix，实测「项目侧边栏选项」）

| 属性 | 值 | 来源 |
|---|---|---|
| 容器 | `no-drag z-50 m-px flex select-none flex-col overflow-y-auto px-1 py-1 bg-token-dropdown-background/90 text-token-foreground ring-token-border rounded-xl ring-[0.5px] shadow-xl-spread backdrop-blur-sm min-w-[172px] max-w-[240px]` | class |
| 实测尺寸 | 172×204（`min-w-[172px]` 生效） | rect |
| 背景 | `oklab(0.297161 …/ 0.9)` ≈ **rgb(45,45,45) @ 90%**（与弹窗同 token） | computed |
| 圆角 | **15px**（`rounded-xl`） | computed |
| 边框 | 无 border；ring 0.5px `rgba(255,255,255,0.082)` | computed |
| 阴影 | `shadow-xl-spread` → `rgba(0,0,0,0.12) 0 8px 16px -4px` | computed |
| 玻璃 | `backdrop-filter: blur(…)`（`backdrop-blur-sm`，具体 px 未单独读出，标注：存在，量级小） | class |
| 内边距 | `px-1 py-1` = 4px | class |
| 锚定 | `data-side=bottom`；trigger 24×24 @(213,225)，菜单 @(214,251)（顶 = trigger 顶 + 26px，左 = trigger 左 + 1px，`m-px`） | rect |
| 字体 | 菜单 16px/24px 继承；菜单项 13px/18.5714px | computed |

**菜单项规格（实测）：**

| 部件 | 属性 | 值 |
|---|---|---|
| 分组标签 | class `px-[var(--padding-row-x)] py-1 text-sm text-token-description-foreground` | — |
| 分组标签 | 高 **27px**，padding 8px 4px，**13px**/18.5714px，色 **rgba(255,255,255,0.498)** | rect + computed |
| 菜单项 | class `no-drag outline-hidden rounded-lg px-[var(--padding-row-x)] py-[var(--padding-row-y)]` | — |
| 菜单项 | 高 **29px**，padding **8px 5px**，圆角 **12.5px**（rounded-lg），**13px**/18.5714px，色 #fff，cursor default | rect + computed |
| CSS 变量 | `--padding-row-x = calc(.25rem*2)` = **8px**；`--padding-row-y = calc(.25rem*1.25)` = **5px** | computed |
| 项内结构 | `flex w-full items-center gap-1.5`（**6px gap**）；左：指示图标 span `inline-flex items-center justify-center leading-none icon-xs shrink-0 opacity-75`；右：`flex-1 min-w-0 truncate` | class |
| 指示图标 | svg vb `0 0 17 17`，渲染 **16×17**（icon-xs 类 + vb 高 17），path 开头 `M12.8961 3.64101…`（radio/check 指示器，5 项同 path，选中态未实测区分） | rect + path |
| 菜单项 hover | **无法观察**（Input.dispatchMouseEvent 不可用，未测得；按规则不猜） | — |

---

## 3. 可触发的其他菜单/弹窗

| 目标 | 结果 |
|---|---|
| header「ChatGPT 对话操作」（More） | **无法观察**：当前 DOM 视图为项目列表页，该按钮不在文档中（首次枚举曾见其 icon-sm ellipsis 18×18，后续视图切换后消失） |
| 模式切换（「切换模式，当前模式：ChatGPT」） | 点击后**实测弹出通用 `codex-dialog`（分享弹窗）**：页面视图在查询间自动切换，点击目标与弹窗类型不可控；模式菜单本体未测得。分享弹窗的全部实测数据见 2.1/2.2 |
| 「k-3720 的项目操作」/「项目侧边栏选项」 | 「项目侧边栏选项」**成功弹出 Radix 下拉菜单**（实测见 2.3）；「项目操作」点击无菜单（可能需真实 hover/pointer 序列，不可用） |
| 搜索按钮 | 未触发成功 → **无法观察** |
| 侧栏用户行（个人资料菜单） | 当前视图无用户行 → **无法观察** |
| 消息 hover 操作行（复制/回复…） | 需真实 hover → **无法观察**（按钮规格见 CODEX_DESIGN §6：26×26、圆角 10、透明 1px 边框） |
| 会话项 hover 置顶/归档 | 图标实测 16×16（见 1.3），按钮 20×20（`!h-5 !w-5`）；菜单本体无 |
| 权限提示（approval 类） | codex 无对应物 → **无法观察**（dimi 特有，设计建议见 4.3） |

---

## 4. dimi 设计建议（基于实测样式）

### 4.1 Session picker（紧凑命令菜单）

codex 无对应物，基于 2.3 Radix 菜单实测给出：

| 属性 | 建议值 | 依据 |
|---|---|---|
| 容器 | `min-w 172 / max-w 240`（dimi 当前 440 可保留更宽，但 token 对齐） | 2.3 |
| 背景 | rgb(45,45,45) @ 90% + backdrop-blur-sm | 2.3 |
| 圆角 | 15px | 2.3 |
| 边框 | 无 border，ring 0.5px rgba(255,255,255,0.082) | 2.3 |
| 阴影 | rgba(0,0,0,0.12) 0 8px 16px -4px | 2.3 |
| 搜索框 | 顶置；dimi 现有 searchInput（radius 10、bg #141414、14px、padding 8px 12px）暂保留（codex 弹窗内无独立输入框可比对，输入框本体**无法观察**） | dimi 现状 |
| 列表项 | 高 29px、padding 8px 5px、radius 12.5px、13px 文字、行内 gap 6px | 2.3 |
| 键盘提示 | 13px、rgba(255,255,255,0.498)（同分组标签） | 2.3 |

### 4.2 Help / settings / question（通用模态）

基于 2.1 `codex-dialog` 实测：

| 属性 | 建议值 | 依据 |
|---|---|---|
| 尺寸 | 520px，max-w 92vw | 2.1 |
| 圆角 | 24px | 2.1 |
| 背景 | rgb(45,45,45) @ 90% + `backdrop-filter: blur(24px)` | 2.1 |
| 边框/阴影 | 无 border；ring 0.5px rgba(255,255,255,0.082)；shadow rgba(0,0,0,0.1) 0 4px 8px -2px | 2.1 |
| backdrop | rgba(0,0,0,0.133)，无 blur（dimi 当前 0.5 太重） | 2.2 |
| 内容 padding | 20px（px-5 py-5） | 2.1 |
| 标题 | 20px/28px weight 600，无独立标题栏/底边线（标题在内容流内，右上角 24×24 关闭按钮距边 16px） | 2.1 |
| 主按钮 | 白底 #fff、深字 rgb(45,45,45)、radius 12.5px、padding 6px 16px | 2.1 |

### 4.3 Approval 权限卡（dimi 特有）

codex 无权限提示可直接比对（**无法观察**）。设计建议：沿用 2.1 通用模态外壳（520px/24px/#2d2d2d@90%/ring 0.5px），保留 dimi 现有「顶部主色强调条」（`borderTop 2px solid #0285ff`，这是刻意差异），命令/预览区保留等宽字体卡片；选项行对齐 2.3 菜单项（29px、8px 5px、radius 12.5px、13px）；确认按钮用白色主按钮（4.2）。操作提示行用 13px rgba(255,255,255,0.498)。

---

## 5. dimi 差距（codex 实测 vs dimi 当前代码）

对照文件：`src/renderer/components/Dialogs.vue`、`Dialogs.styles.ts`、`icons.ts`。

### 5.1 弹窗/菜单样式差距

| # | 项 | codex 实测 | dimi 当前（Dialogs.styles.ts） | 差距 |
|---|---|---|---|---|
| 1 | backdrop 透明度 | rgba(0,0,0,**0.133**) | rgba(0,0,0,**0.5**) | dimi 重 ~3.8 倍，需改 0.133 |
| 2 | backdrop blur | 无 | 无 | 一致 |
| 3 | 弹窗圆角 | **24px** | `radius.lg` = **16px** | 需改 24 |
| 4 | 弹窗背景 | rgb(45,45,45) @ 90% + blur(24px) | `surface2` #212121 不透明 | 需改色值 + 加 backdrop-filter |
| 5 | 弹窗边框 | 无 border；ring 0.5px rgba(255,255,255,0.082) | 1px solid `borderHeavy` rgba(255,255,255,0.16) | 需改 ring 方案 |
| 6 | 弹窗阴影 | rgba(0,0,0,0.1) 0 4px 8px -2px | 0 16px 48px rgba(0,0,0,0.6) | 需改（dimi 更重更散） |
| 7 | 弹窗宽度 | 520px | dialog `minWidth 480`；picker 440；approval 520 | dialog 改 520（与 approval 同宽） |
| 8 | 内容 padding | 20px | `dialogBody` 14px 16px | 需改 20px |
| 9 | 标题栏 | 无独立标题栏；标题 20px/600 在内容流内，右上关闭按钮 24×24 距边 16px | `dialogTitle` 独立栏：14px 16px + **底边线** border | dimi 有独立标题栏+分隔线，与 codex 结构不同 |
| 10 | 页脚 | 无独立页脚（按钮内联在 body/输入 pill 内） | `dialogFooter` 12px 16px + 顶边线 | dimi 有独立页脚+分隔线 |
| 11 | 主按钮 | 白底 #fff、深字 #2d2d2d、radius 12.5px、padding 6px 16px | `btnPrimary` 蓝底 #0285ff 白字、radius pill、padding 5px 12px | dimi 无白色主按钮样式 |
| 12 | 菜单项行高 | 29px（8px 5px padding） | `listItem` padding 8px 12px、radius 10、**纵向 flex gap 2**（标题+副标题两行） | 结构不同（codex 单行），radius 不同（12.5 vs 10） |
| 13 | 菜单项文字 | 13px/18.5714px | `listItemTitle` 继承 14px；`listItemSub` 13px | 需 13px 主文字 |
| 14 | 菜单分组标签 | 13px rgba(255,255,255,0.498)，padding 8px 4px | 无对应（picker 直接列表） | 新增（如有分组） |
| 15 | 菜单容器 | min-w 172/max-w 240、radius 15px、#2d2d2d@90%、shadow 0 8px 16px -4px、padding 4px | picker 440、surface2、radius 16px、shadow 0 16px 48px | picker 保留宽度但样式 token 需对齐 |
| 16 | 键盘提示/描述行 | 13px rgba(255,255,255,0.498) | `listItemSub` 13px rgba(255,255,255,0.5) | 色值微调（0.5 → 0.498 可忽略或对齐） |
| 17 | 选中态 | 未完全实测（指示图标 vb 17×17，选中态**无法观察**） | `listItemSelected` bg hoverStrong rgba(255,255,255,0.08) | 保持 dimi 现状，标注待复核 |
| 18 | 菜单项 hover | **无法观察** | `listItem:hover` rgba(255,255,255,0.05) | 保持 dimi 现状，标注待复核 |

### 5.2 图标差距

| # | 项 | codex 实测 | dimi 当前 | 差距 |
|---|---|---|---|---|
| 1 | 尺寸阶梯 | icon-3xs 10 / xxs 12 / 2xs 14 / xs 16 / sm 18 / base 20 / md 24 / lg 28 | 只有注释「20x20/16x16/21x21」 | 需建完整阶梯（至少 2xs/xs/sm/base） |
| 2 | 渲染机制 | CSS 类决定尺寸，**与 svg width 属性无关**；按钮可 `[&>svg]:!h-4 !w-4` 强制 16 | 注释误导「header/buttons 20x20」 | 按类使用；header 按钮 = icon-xs 16，More/help = icon-sm 18 |
| 3 | ellipsis 尺寸 | More 按钮 **18×18** (icon-sm) | icons.ts vb 21 21，注释 ellipsis/copy 21x21 | 需 18×18 |
| 4 | help 尺寸 | 帮助按钮 **18×18** (icon-sm) | vb 20 20 | 需 18×18 |
| 5 | copy 尺寸 | 消息行 16×16 (icon-xs)；表格内 14×14 (icon-2xs) | 注释 21x21 | 需 16/14 |
| 6 | send 颜色 | 图标色 = `text-token-dropdown-background`（深 #2d2d2d，白底按钮） | send path 相同 | 确认发送按钮白底深图标 |
| 7 | 缺图标 | 置顶 pin、归档 box、thumbs up/down、search、spinner、close-x、radio 指示器、云端 bolt | icons.ts 无 | 按需补捕获 |
| 8 | sectionChevron | vb 20×21 → **14×14** (icon-2xs)；分组变体带 stroke=currentColor | vb 0 0 20 21 | 需 14×14 |
| 9 | 图标颜色 | 默认继承 currentColor；次级 = rgba(255,255,255,0.498) | dimi `textDim` 0.7 / `textMuted` 0.5 / `textTertiary` 0.498 | 0.498 token 已有，对齐 |
| 10 | spinner | vb 24×24、animate-spin、渲染 16.29×16.29 | 无 | 可选补 |

### 5.3 结构差距（Dialogs.vue）

| # | 项 | codex 实测 | dimi 当前 | 差距 |
|---|---|---|---|---|
| 1 | 弹窗 z-index | `z-50`（40 层内） | `dialogRoot zIndex: 100` | 数值不同但均最高层，可保留 |
| 2 | backdrop 关闭 | mousedown.self 关闭 | 相同 | 一致 |
| 3 | 关闭按钮 | 弹窗右上 24×24，距边 16px，内含 16×16 x | Settings/Help 无右上关闭按钮（只有页脚 Close） | 需加右上角关闭按钮 |
| 4 | 标题位置 | 内容流内 20px/600，无栏 | 独立标题栏 + 底边线 | 结构调整 |

---

## 6. 未决 / 无法观察清单

- 菜单项 hover / 选中态（Input.dispatchMouseEvent 不可用）
- header More 菜单、搜索弹窗、用户行资料菜单、消息 hover 操作行（当前 DOM 视图限制 + 无法真实 hover）
- 模式菜单本体（点击「切换模式」被视图切换干扰，弹出了通用分享弹窗）
- 弹窗内输入框/选择控件（分享弹窗无输入框可比对）
- 14×16 异常尺寸的完整成因
- 权限提示的 codex 直接对应物（不存在）
