import { css } from '@emotion/css';
import { colors, size } from '../styles/theme';

// Codex header: 46px fixed transparent bar. The whole header is
// pointer-events:none — only the buttons re-enable it (S17), so empty header
// space passes clicks through (window drag / sidebar resize zone).
export const header = css({
  position: 'fixed',
  top: 0,
  left: 0,
  right: 0,
  zIndex: 30,
  display: 'flex',
  alignItems: 'center',
  height: size.headerH,
  padding: 0,
  background: 'transparent',
  border: 'none',
  boxShadow: 'none',
  flexShrink: 0,
  userSelect: 'none',
  pointerEvents: 'none',
});

// Left zone above the sidebar (0..275px): Codex leaves an 88px safe-left gap
// (--spacing-token-safe-header-left, hardcoded here since theme.ts is
// read-only) before the 92px-wide button group (3×28 + 2×4 gap) at x=88 —
// hide-sidebar / back / forward. No menu button at x=0 (L1).
export const headerSide = css({
  width: size.sidebarW,
  minWidth: 180,
  flexShrink: 0,
  display: 'flex',
  alignItems: 'center',
  paddingLeft: 88,
});

export const headerSideGroup = css({
  display: 'flex',
  alignItems: 'center',
  gap: 4, // Codex gap-1
});

// Main zone (flex-1): ms-2 (8px margin-left) + ps-2 (8px padding-left); the
// title button offsets itself -2px (Codex -ms-0.5) so its text sits at x=289
// (L5).
export const headerMain = css({
  flex: 1,
  minWidth: 0,
  display: 'flex',
  alignItems: 'center',
  gap: 16, // Codex grid gap-x-4 between title and More
  marginLeft: 8, // Codex ms-2
  paddingLeft: 8, // Codex ps-2
});

// Session title is a BUTTON in Codex (S11): hover white-8% bg, radius 10px,
// padding 0 6px, margin-left -2px, max-width 320px, truncating text.
export const headerTitle = css({
  display: 'flex',
  alignItems: 'center',
  height: 24,
  marginLeft: -2, // Codex -ms-0.5
  padding: '0 6px', // Codex px-1.5
  fontSize: 14,
  lineHeight: '24px', // Codex leading-6
  fontWeight: 500, // Codex font-medium
  color: colors.text,
  borderRadius: 10, // Codex rounded-md
  border: '1px solid transparent',
  background: 'transparent',
  maxWidth: 320, // Codex max-w-[320px]
  minWidth: 0,
  flexShrink: 1,
  cursor: 'default',
  pointerEvents: 'auto',
  '&:hover': { background: colors.hover8 }, // Codex hover:bg-token-list-hover-background
  '& span': {
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
    minWidth: 0,
  },
});

// Generic 28×28 icon button (hide-sidebar/back/forward/toggle-sidebar):
// radius 12.5px (S1), hover white 8% (S2), color stays tertiary on hover
// (S3), weight 445 (S4), cursor default (S5), no transition (S15).
export const iconBtn = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  width: size.iconBtn,
  height: size.iconBtn,
  padding: 0,
  border: '1px solid transparent',
  borderRadius: size.iconBtnRadius, // Codex rounded-lg = 12.5px
  background: 'transparent',
  color: colors.textTertiary,
  fontSize: 14,
  fontWeight: 445,
  lineHeight: '18px',
  cursor: 'default',
  flexShrink: 0,
  pointerEvents: 'auto',
  transition: 'all 0s ease',
  '& svg': { width: 16, height: 16 },
  '&:hover:not(:disabled)': { background: colors.hover8 },
  '&:active:not(:disabled)': { background: 'rgba(255, 255, 255, 0.15)' },
  '&:disabled': { opacity: 0.4, cursor: 'not-allowed' }, // Codex S14
});

// More button: transparent border (S9), electron 4px padding (S10), radius
// 10px (Codex electron:rounded-md), 18×18 ellipsis icon (set inline).
export const moreBtn = css({
  padding: 4,
  borderRadius: 10,
  fontSize: 14,
  fontWeight: 500,
  lineHeight: '21px',
});

// Right zone: share pill + pinned summary + toggle-sidebar, gap 6px, 8px
// right padding (pe-2); no opaque 36px spacer (L2/L4).
export const headerRight = css({
  marginLeft: 'auto', // Codex ms-auto
  display: 'flex',
  alignItems: 'center',
  gap: 6, // Codex gap-1.5
  flexShrink: 0,
  paddingRight: 8, // Codex pe-2
});

// Codex share pill: rounded-lg 12.5px (S6, NOT pill), padding 0 8px (S7),
// white 14px/445/18px text (S8), 16×16 icon, 4px icon-text gap.
export const shareBtn = css({
  display: 'flex',
  alignItems: 'center',
  gap: 4,
  height: 28,
  padding: '0 8px',
  borderRadius: size.iconBtnRadius,
  border: '1px solid transparent',
  background: 'transparent',
  color: colors.text,
  fontSize: 14,
  fontWeight: 445,
  lineHeight: '18px',
  cursor: 'default',
  flexShrink: 0,
  pointerEvents: 'auto',
  transition: 'all 0s ease',
  '& svg': { width: 16, height: 16 },
  '&:hover': { background: colors.hover8 },
  '&:active': { background: 'rgba(255, 255, 255, 0.15)' },
});

// Pinned summary (固定摘要) toggle: white icon, ON state carries a white 5%
// background (measured), hover 10%, active 15%. Icon: dimi `dots` (bulleted
// list) as a stand-in — the exact Codex summary path is not captured yet.
export const pinnedBtn = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  width: size.iconBtn,
  height: size.iconBtn,
  padding: 0,
  border: '1px solid transparent',
  borderRadius: size.iconBtnRadius,
  background: 'transparent',
  color: colors.text,
  fontSize: 14,
  fontWeight: 445,
  lineHeight: '18px',
  cursor: 'default',
  flexShrink: 0,
  pointerEvents: 'auto',
  transition: 'all 0s ease',
  '& svg': { width: 16, height: 16 },
  '&:hover:not(:disabled)': { background: 'rgba(255, 255, 255, 0.10)' },
  '&:active:not(:disabled)': { background: 'rgba(255, 255, 255, 0.15)' },
});

export const pinnedBtnOn = css({
  background: 'rgba(255, 255, 255, 0.05)', // Codex bg-token-foreground/5
});
