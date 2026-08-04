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

// Left zone above the sidebar (0..275px): icon buttons like Codex.
export const headerSide = css({
  width: size.sidebarW,
  flexShrink: 0,
  display: 'flex',
  alignItems: 'center',
  gap: 6,
  paddingLeft: 16,
});

// Main zone above the content area: session title at content left edge.
export const headerMain = css({
  flex: 1,
  minWidth: 0,
  display: 'flex',
  alignItems: 'center',
  gap: 6,
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
  padding: 0,
  cursor: 'pointer',
  maxWidth: '60%',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
  '&:hover': { opacity: 0.85 },
  '& svg': { width: 14, height: 14, flexShrink: 0, color: colors.textTertiary },
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
  borderRadius: size.iconBtnRadius,
  background: 'transparent',
  color: colors.textTertiary,
  fontSize: 14,
  lineHeight: '18px',
  cursor: 'pointer',
  flexShrink: 0,
  '&:hover': { background: colors.hover, color: colors.text },
  '&:disabled': { opacity: 0.4, cursor: 'not-allowed' },
  '& svg': { width: 16, height: 16 },
});

export const headerRight = css({
  display: 'flex',
  alignItems: 'center',
  gap: 6,
  flexShrink: 0,
  paddingRight: 8,
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
