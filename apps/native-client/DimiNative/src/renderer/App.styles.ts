import { css } from '@emotion/css';
import { colors } from './styles/theme';

export const shell = css({
  display: 'flex',
  height: '100%',
});

// Codex: main content surface is #181818 over body #141414.
// position:relative anchors the future right floating panel
// (absolute right-0, top/bottom 12px, z-40, 316px — design §0).
export const mainCol = css({
  position: 'relative',
  display: 'flex',
  flexDirection: 'column',
  flex: 1,
  minWidth: 0,
  background: colors.surface,
});

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
