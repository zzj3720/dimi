// Codex desktop client design tokens (extracted live via CDP).
export const colors = {
  bgUnder: '#141414',
  surface: '#181818',
  surface2: '#212121',
  surface3: '#2d2d2d',
  sidebarBg: 'rgba(40, 40, 40, 0.7)',
  composerBg: 'rgb(45, 45, 45)',
  border: 'rgba(255, 255, 255, 0.08)',
  borderHeavy: 'rgba(255, 255, 255, 0.16)',
  borderFocus: 'rgba(2, 133, 255, 0.7)',
  text: '#ffffff',
  textDim: 'rgba(255, 255, 255, 0.7)',
  textMuted: 'rgba(255, 255, 255, 0.5)',
  textTertiary: 'rgba(255, 255, 255, 0.498)',
  primary: '#0285ff',
  success: '#00a240',
  warning: '#e25507',
  error: '#e02e2a',
  hover: 'rgba(255, 255, 255, 0.05)',
  hoverStrong: 'rgba(255, 255, 255, 0.08)',
  hover8: 'rgba(255, 255, 255, 0.08)',
  hover5: 'rgba(255, 255, 255, 0.05)',
};

export const font = {
  family: '-apple-system, "system-ui", "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif',
  mono: 'ui-monospace, "SF Mono", Menlo, monospace',
  chat: '14px',
  chatLh: '22px',
  sm: '13px',
  smLh: '18.57px',
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
};

export const radius = {
  pill: '9999px',
  md: '12px',
  lg: '16px',
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
