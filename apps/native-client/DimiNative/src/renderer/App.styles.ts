import { css, injectGlobal } from '@emotion/css';
import { colors, elevation, radius, runtimeVars, size } from './styles/theme';

export const shell = css({
  display: 'flex',
  height: '100%',
});

// Codex: main content surface is #181818 over body #141414, and carries
// --elevation-prominent (0.5px hairline + double black shadow) so it floats
// one step above the canvas (design §4.2/§5.1). The surface color follows the
// runtime theme via --surface (default = colors.surface). overflow:hidden
// matches codex [data-app-shell-main-surface=default].
// position:relative anchors the future right floating panel
// (absolute right-0, top/bottom 12px, z-40, 316px — design §0).
// padding-top: the 46px header is a fixed overlay (z-30, transparent,
// pointer-events:none) covering the surface's top strip, so the content frame
// must yield --height-toolbar (codex `_MainContentFrame` margin-top:
// var(--height-toolbar), design §1.1/§4.2). Padding (not margin) keeps the
// surface background spanning the full window height, exactly like codex's
// main surface behind the transparent header. Height stays 100% (border-box):
// the transcript+composer flex column simply starts 46px lower.
export const mainCol = css({
  position: 'relative',
  display: 'flex',
  flexDirection: 'column',
  flex: 1,
  minWidth: 0,
  overflow: 'hidden',
  background: runtimeVars.surface,
  boxShadow: elevation.prominent,
  paddingTop: size.headerH,
});

// Codex win32 only: the main surface gets border-top-left-radius: radius-lg
// (design §1.1). macOS keeps square corners; applied conditionally from App.vue.
export const mainColWin = css({
  borderTopLeftRadius: radius.lg,
});

// Codex sidebar show/hide = width change (AnimatePresence spring, duration
// .5s, bounce .1 — design §4.1/§5.1), not instant DOM removal. dimi keeps
// `v-if` but animates the width through Vue <Transition name="dimi-sb">
// classes applied to the <aside> root: `width: 0 !important` overrides the
// component's inline width during enter-from/leave-to, and overflow:hidden
// clips the content mid-animation (codex wraps the panel in overflow-hidden).
// prefers-reduced-motion drops the animation (codex duration=0).
injectGlobal`
  .dimi-sb-enter-active,
  .dimi-sb-leave-active {
    transition: width 0.5s cubic-bezier(0.34, 1.3, 0.64, 1);
    overflow: hidden;
  }
  .dimi-sb-enter-from,
  .dimi-sb-leave-to {
    width: 0 !important;
  }
  @media (prefers-reduced-motion: reduce) {
    .dimi-sb-enter-active,
    .dimi-sb-leave-active {
      transition: none;
    }
  }
`;

// Codex scrollbar theming (design §3): the main scroll containers keep the
// macOS overlay scrollbar and only get `scrollbar-color` (thumb 8.4% white /
// transparent track) — exactly the property codex applies on
// .thread-scroll-container. Both scrollbar-color and scrollbar-width inherit,
// so binding this class on the app root cascades to all scroll areas inside.
// No ::-webkit-scrollbar rules on purpose: styling the webkit pseudo-elements
// converts overlay scrollbars to layout-width scrollbars, contradicting the
// measured `offsetWidth - clientWidth === 0`.
export const rootScroll = css({
  scrollbarColor: `${colors.scrollbarThumb} ${colors.scrollbarTrack}`,
  scrollbarWidth: 'auto',
});

// Global tooltip (codex Radix Tooltip: dark surface, small radius, centered
// above the target with a 6px gap).
export const tooltipStyle = css({
  position: 'fixed',
  zIndex: 200,
  padding: '4px 8px',
  background: '#1f1f1f',
  borderRadius: 6,
  color: 'rgba(255, 255, 255, 0.9)',
  fontSize: 12,
  lineHeight: '16px',
  whiteSpace: 'nowrap',
  pointerEvents: 'none',
  boxShadow: '0 4px 12px rgba(0, 0, 0, 0.4)',
});
