// Codex desktop client design tokens (extracted live via CDP).
// Source of truth: design/06-app-theme.md (2026-08-04 codex measurements).
// NOTE: other module agents read this file — do not delete keys, only change values.

// ---- 4px spacing base (codex --spacing: .25rem) ----
// Every named spacing is a multiple (or half-grid 1.5/2.5/3.25×) of the base,
// so any two elements land on the same grid (design §2.2/§2.3).
export const spacing = {
  base: 4, // --spacing .25rem
  row: 8, // spacing×2 --padding-row-x
  rowY: 5, // spacing×1.25 --padding-row-y (electron)
  toolbar: 16, // spacing×4 --padding-toolbar
  panel: 20, // spacing×5 --padding-panel-base (electron)
  overhang: 24, // spacing×6 --composer-inline-overhang
  composerInset: 13, // spacing×3.25 --home-composer-inline-inset
  threadTopInset: 32, // spacing×8 --thread-content-top-inset
  floatingInset: 12, // spacing×3 --thread-floating-content-top/bottom-inset
};
const s = (n: number): string => `${spacing.base * n}px`;

// ---- corner radius: base grid × 1.25 scale (design §3.3) ----
// codex splits the system into a 2px-step base ladder + one global scale knob
// (--codex-corner-radius-scale: 1.25) + a superellipse shape. Runtime values:
// md=10, lg=12.5, xl=15, 2xl=20, 3xl=25, 4xl=30.
export const cornerRadiusScale = 1.25;
export const cornerShape = 'superellipse(1.5)'; // --codex-corner-shape (design §3.3; no consumer yet — reserved)
export const radiusBase = {
  xs: 4,
  sm: 6,
  md: 8,
  lg: 10,
  xl: 12,
  '2xl': 16,
  '3xl': 20,
  '4xl': 24,
};

export const colors = {
  bgUnder: '#141414', // body bg (html/body)
  surface: '#181818', // main content surface (sidebar right)
  surface2: '#2d2d2d', // dialogs / dropdown layers (codex --color-token-dropdown-background)
  surface3: '#232323', // alias of panel bg (was #2d2d2d — that tier is control/dropdown, see surface2)
  panel: '#232323', // --color-background-panel (design §3.1 面板层; no consumer yet — reserved)
  sidebarBg: 'rgba(40, 40, 40, 0.7)',
  composerBg: 'rgb(45, 45, 45)', // electron: solid #2d2d2d (+blur(16px) in Composer)
  borderLight: 'rgba(255, 255, 255, 0.042)', // --color-token-border-light (4%)
  border: 'rgba(255, 255, 255, 0.084)', // --color-token-border-default
  borderHeavy: 'rgba(255, 255, 255, 0.156)', // --color-token-border-heavy
  borderFocus: 'rgba(131, 195, 255, 0.76)', // --color-token-focus-border (#83C3FF 76%)
  text: '#ffffff', // --color-token-text-primary
  textDim: 'rgba(255, 255, 255, 0.65)', // --color-token-text-secondary (codex 实时实测 0.65; 曾误按旧文档改为 0.71)
  textMuted: 'rgba(255, 255, 255, 0.498)', // 与 textTertiary 同值（0.5/0.498 重复档合并；key 保留）
  textTertiary: 'rgba(255, 255, 255, 0.498)', // --color-token-text-tertiary
  primary: '#83C3FF', // link / accent (dark theme light blue, codex --color-token-link)
  // Dark-theme 300-level accents (design §3.1); light theme uses the 500 level.
  success: '#40c977', // green-300 (was #00a240)
  warning: '#ff8549', // orange-300 (was #e25507)
  error: '#ff6764', // red-300 (was #e02e2a)
  // Elevated surfaces (design §3.1): primary = floating-layer main bg (96%
  // translucent gray), secondary = universal 3% white lift. No consumers yet.
  elevatedPrimary: 'rgba(54, 54, 54, 0.96)', // --color-background-elevated-primary
  elevatedSecondary: 'rgba(255, 255, 255, 0.032)', // --color-background-elevated-secondary
  // Icon scale reuses the foreground opacity ladder (90/70/50%, design §3.1).
  iconPrimary: 'rgba(255, 255, 255, 0.9)', // --color-icon-primary
  iconSecondary: 'rgba(255, 255, 255, 0.7)', // --color-icon-secondary
  iconTertiary: 'rgba(255, 255, 255, 0.5)', // --color-icon-tertiary
  // Status borders: same hue as the status colors at 40% (dark, design §3.1).
  borderWarning: 'rgba(255, 133, 73, 0.4)', // --color-border-warning (orange-300 @40%)
  borderError: 'rgba(255, 103, 100, 0.4)', // --color-border-error (red-300 @40%)
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
  chatLh: '22px', // chat markdown 段落行高（实测 calc(14px + 8px) = 22px，见 03-thread.md；21 → 22）
  listLh: '22px', // markdown ul/ol 行高（与 chatLh 同值；key 保留，global.ts li 已改用 chatLh）
  sm: '13px',
  smLh: '18px', // 按钮文字行高（设计 18.57→18）
  xs: '14px', // 侧栏导航/区块标题文字（codex 侧栏实测 14px/21px，见 02-sidebar.md。注意：codex 全局 --text-xs=12px 用于徽标、侧栏不用 xs 档——名称沿用但值按侧栏实测保留）
  xsLh: '21px',
  // Large display tiers (design §3.2): --text-xl hero/empty-state, --text-2xl. No consumers yet.
  xl: '28px',
  '2xl': '36px',
};

export const radius = {
  pill: '9999px',
  md: `${radiusBase.md * cornerRadiusScale}px`, // 10px (was 12px)
  lg: `${radiusBase.lg * cornerRadiusScale}px`, // 12.5px (was 16px)
  xl: `${radiusBase.xl * cornerRadiusScale}px`, // 15px
  '2xl': `${radiusBase['2xl'] * cornerRadiusScale}px`, // 20px
  '3xl': `${radiusBase['3xl'] * cornerRadiusScale}px`, // 25px
  '4xl': `${radiusBase['4xl'] * cornerRadiusScale}px`, // 30px
};

export const size = {
  headerH: '46px', // --height-toolbar (fixed, not spacing-derived)
  sidebarW: '275px', // sidebar default width (Sidebar.vue clamps 200..480)
  threadMaxW: '736px', // 48rem (768px) 内容盒 = 768 − 2×16 px-toolbar（Transcript/Composer 共用）
  iconBtn: s(7), // 28px — spacing×7
  iconBtnRadius: radius.lg, // 12.5px — rounded-lg
  sidebarIconBtn: '26px',
  sidebarIconBtnRadius: radius.md, // 10px — rounded-md
  sidebarItemH: '29px',
  sidebarItemRadius: radius.lg, // 12.5px
  composerRadius: radius['3xl'], // 25px — rounded-3xl
  composerBtn: s(7), // 28px — --spacing-token-button-composer
  sendBtn: s(7), // 28px
  // Design §0 extra geometry (tokens for consumers in other files):
  composerBottomGap: '17px', // composer form 底距视口（04-composer.md 实测 960−943=17px；sticky wrapper pb-4=16px）
  headerLeftPad: s(22), // 88px --spacing-token-safe-header-left
  sidebarResizeW: s(2), // 8px — codex w-2 拖拽命中区（Sidebar.styles 现用 16px 硬编码，待其收敛）
  panelW: '316px', // 右浮动面板容器宽（right-0）
  panelCardW: '300px', // 面板卡片宽（316 − pe-4 16px）
  panelGap: s(3), // 12px — spacing×3
};

// Codex elevation: 0.5px hairline stroke + double black shadow (design §3.1).
// prominent = main surface / composer capsule / right panel; sidebar is the
// weaker floating-panel variant (7.5px α0.03 + 20px α0.02).
// NOTE: Composer.styles.ts hardcodes the prominent value — mirror changes there.
export const elevation = {
  stroke: 'rgba(255, 255, 255, 0.157) 0 0 0 0.5px', // --elevation-stroke
  prominent:
    'rgba(255, 255, 255, 0.157) 0 0 0 0.5px, rgba(0, 0, 0, 0.04) 0 3px 7.5px 0, rgba(0, 0, 0, 0.05) 0 0 20px 0',
  sidebar:
    'rgba(255, 255, 255, 0.157) 0 0 0 0.5px, rgba(0, 0, 0, 0.03) 0 3px 7.5px 0, rgba(0, 0, 0, 0.02) 0 0 20px 0',
};

// Runtime theme CSS variables — names must match the ones api.ts
// `applyTheme()` sets via the `/theme` command (`--bg`, `--surface`, `--text`).
// Static `colors` above stay the default palette; styles that should follow
// the runtime theme must consume these vars instead of a static token, e.g.
//   background: ${runtimeVars.bg}   ->  var(--bg, #141414)
//   color: ${runtimeVars.text}      ->  var(--text, #ffffff)
// NOTE (cross-file): global.ts consumes `--bg` / `--text` for the html/body
// background and body text color; the main column (App.styles.ts mainCol)
// consumes `--surface` (fallback = colors.surface #181818). The remaining
// component stylesheets (Dialogs, Composer, Sidebar, Transcript, HeaderBar)
// still use static colors — a known limitation of the runtime theme.
export const runtimeVars = {
  bg: 'var(--bg, #141414)',
  surface: 'var(--surface, #181818)', // fallback was #212121 — now colors.surface
  text: 'var(--text, #ffffff)',
};
