import { css } from '@emotion/css';
import { colors, size, font } from '../styles/theme';

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
  borderBottom: 'none',
  flexShrink: 0,
  userSelect: 'none',
});

// Left zone above the sidebar (0..275px): Codex button layout — menu at x=0,
// then a 92px-wide group (3×28 + 2×4 gap) at x=88 (collapse/back/forward).
export const headerSide = css({
  width: size.sidebarW,
  flexShrink: 0,
  display: 'flex',
  alignItems: 'center',
  gap: 4,
  paddingLeft: 0,
});

export const headerSideGroup = css({
  display: 'flex',
  alignItems: 'center',
  gap: 4,
  marginLeft: 56, // x=88 minus (28px menu + 4px gap)
});

// Main zone above the content area: session title at content left edge.
export const headerMain = css({
  flex: 1,
  minWidth: 0,
  display: 'flex',
  alignItems: 'center',
  gap: 4, // Codex gap-1
  paddingLeft: 14, // Codex title sits at main-left + 14px
});

export const headerTitle = css({
  display: 'flex',
  alignItems: 'center',
  gap: 4,
  fontSize: 14,
  lineHeight: '24px',
  fontWeight: 500,
  color: colors.text,
  background: 'transparent',
  border: 'none',
  padding: '0 6px', // Codex px-1.5
  borderRadius: 10, // Codex rounded-md
  cursor: 'pointer',
  maxWidth: 320, // Codex max-w-[320px]
  overflow: 'hidden',
  flexShrink: 1,
  minWidth: 0,
  '& span': {
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
    minWidth: 0,
    flexShrink: 1,
  },
  '&:hover': { opacity: 0.85, background: colors.hover },
});

export const iconBtn = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  gap: 4,
  width: size.iconBtn,
  height: size.iconBtn,
  padding: 0,
  border: '1px solid transparent',
  borderRadius: 10, // Codex header icon buttons use 10px
  background: 'transparent',
  color: colors.textTertiary,
  fontSize: 14,
  lineHeight: '18px',
  cursor: 'pointer',
  flexShrink: 0,
  '&:hover': { background: colors.hover, color: colors.text },
  '&:disabled': { opacity: 0.4, cursor: 'not-allowed' },
  '& svg': { width: 18, height: 18 },
});

// The "…" more button carries a visible border in Codex.
export const iconBtnBordered = css({
  borderColor: colors.border,
});

// Codex: the title chevron is a separate clickable element right after the
// title button (glyph ~12×9 inside a 14×14 viewBox).
export const headerChevron = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  width: 14,
  height: 14,
  padding: 0,
  border: 'none',
  background: 'transparent',
  color: colors.textTertiary,
  cursor: 'pointer',
  flexShrink: 0,
  borderRadius: 4,
  '&:hover': { color: colors.textDim, background: colors.hover },
  '& svg': { width: 14, height: 14 },
});

export const headerRight = css({
  display: 'flex',
  alignItems: 'center',
  gap: 6,
  flexShrink: 0,
  paddingRight: 42, // Codex keeps a 36px non-interactive spacer at the far right
});

// Codex share-style pill button on the right.
export const shareBtn = css({
  display: 'flex',
  alignItems: 'center',
  gap: 4,
  padding: '0 10px',
  height: 28,
  borderRadius: 9999,
  border: '1px solid transparent',
  background: 'transparent',
  color: colors.text,
  fontSize: font.sm,
  lineHeight: '18px',
  cursor: 'pointer',
  '&:hover': { background: colors.hover },
  '& svg': { width: 14, height: 14 },
});
