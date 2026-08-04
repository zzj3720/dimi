// Codex desktop client design tokens (extracted live via CDP).
// Source of truth: design/06-app-theme.md (2026-08-04 codex measurements).
// NOTE: other module agents read this file — do not delete keys, only change values.
export const colors = {
  bgUnder: '#141414', // body bg (html/body)
  surface: '#181818', // main content surface (sidebar right)
  surface2: '#2d2d2d', // dialogs / dropdown layers (codex --color-token-dropdown-background)
  surface3: '#2d2d2d', // alias of dropdown / panel bg
  sidebarBg: 'rgba(40, 40, 40, 0.7)',
  composerBg: 'rgb(45, 45, 45)', // electron: solid #2d2d2d (+blur(16px) in Composer)
  border: 'rgba(255, 255, 255, 0.084)', // --color-token-border-default
  borderHeavy: 'rgba(255, 255, 255, 0.156)', // --color-token-border-heavy
  borderFocus: 'rgba(131, 195, 255, 0.76)', // --color-token-focus-border (#83C3FF 76%)
  text: '#ffffff', // --color-token-text-primary
  textDim: 'rgba(255, 255, 255, 0.65)', // --color-token-text-secondary
  textMuted: 'rgba(255, 255, 255, 0.5)',
  textTertiary: 'rgba(255, 255, 255, 0.498)', // --color-token-text-tertiary
  primary: '#83C3FF', // link / accent (dark theme light blue, codex --color-token-link)
  success: '#00a240',
  warning: '#e25507',
  error: '#e02e2a',
  hover: 'rgba(255, 255, 255, 0.05)', // hover5 / 固定摘要按钮底 / 用户气泡底
  hoverStrong: 'rgba(255, 255, 255, 0.078)', // --color-token-list-hover-background
  hover8: 'rgba(255, 255, 255, 0.078)', // alias of list hover
  hover5: 'rgba(255, 255, 255, 0.05)',
  // Scrollbar (design §3): overlay (no layout width), thumb 8.4% / hover+active 15.6% / track transparent.
  scrollbarThumb: 'rgba(255, 255, 255, 0.084)', // --color-token-scrollbar-slider-background
  scrollbarThumbHover: 'rgba(255, 255, 255, 0.156)', // --color-token-scrollbar-slider-hover/active-background
  scrollbarTrack: 'rgba(0, 0, 0, 0)',
};

export const font = {
  family: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif', // codex --font-sans
  mono: 'ui-monospace, "SFMono-Regular", "SF Mono", Menlo, Consolas, "Liberation Mono", monospace', // codex --font-mono
  weight: 445, // codex --vscode-font-weight (variable font default)
  chat: '14px',
  chatLh: '21px', // chat 正文行高（设计修正 22→21）
  listLh: '22px', // markdown ul/ol 行高（与 chat 正文 21px 不同；global.ts li 尚未改用）
  sm: '13px',
  smLh: '18px', // 按钮文字行高（设计 18.57→18）
  xs: '14px',
  xsLh: '21px',
};

export const size = {
  headerH: '46px',
  sidebarW: '275px',
  threadMaxW: '736px', // Codex message column width (form aligns 1:1 with it)
  iconBtn: '28px',
  iconBtnRadius: '12.5px',
  sidebarIconBtn: '26px',
  sidebarIconBtnRadius: '10px',
  sidebarItemH: '29px',
  sidebarItemRadius: '12.5px',
  composerRadius: '25px',
  composerBtn: '28px',
  sendBtn: '28px',
  // Design §0 extra geometry (tokens for consumers in other files):
  composerBottomGap: '17px', // composer form 底距窗口（设计 16→17）
  headerLeftPad: '88px', // header 左区按钮组起点 x=88（实测）
  sidebarResizeW: '16px', // sidebar resize handle 宽（延伸到 header 上方）
  panelW: '316px', // 右浮动面板容器宽（right-0）
  panelCardW: '300px', // 面板卡片宽（316 − pe-4 16px）
  panelGap: '12px', // 面板容器 top/bottom 距内容区
};

export const radius = {
  pill: '9999px',
  md: '12px',
  lg: '16px',
};

// Codex elevation-prominent (composer capsule & right panel): 0.5px white
// hairline + double black shadow. Composer.styles.ts hardcodes this exact value.
export const elevation = {
  prominent:
    'rgba(255, 255, 255, 0.157) 0 0 0 0.5px, rgba(0, 0, 0, 0.04) 0 3px 7.5px 0, rgba(0, 0, 0, 0.05) 0 0 20px 0',
};

// Runtime theme CSS variables — names must match the ones api.ts
// `applyTheme()` sets via the `/theme` command (`--bg`, `--surface`, `--text`).
// Static `colors` above stay the default palette; styles that should follow
// the runtime theme must consume these vars instead of a static token, e.g.
//   background: ${runtimeVars.bg}   ->  var(--bg, #141414)
//   color: ${runtimeVars.text}      ->  var(--text, #ffffff)
// NOTE (cross-file): global.ts consumes `--bg` / `--text` for the html/body
// background and body text color, so `/theme` visibly switches the window
// background + text. `--surface` is not yet consumed — the main column
// background (App.styles.ts mainCol) and the per-component stylesheets
// (Dialogs, Composer, Sidebar, Transcript, HeaderBar) still use static
// colors, which is a known limitation of the runtime theme.
export const runtimeVars = {
  bg: 'var(--bg, #141414)',
  surface: 'var(--surface, #212121)',
  text: 'var(--text, #ffffff)',
};
