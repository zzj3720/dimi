import { css } from '@emotion/css';
import { colors, font, radius } from '../styles/theme';

export const dialogRoot = css({
  position: 'absolute',
  inset: 0,
  zIndex: 100,
  pointerEvents: 'none',
});

export const dialogBackdrop = css({
  position: 'absolute',
  inset: 0,
  background: 'rgba(0, 0, 0, 0.5)',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  pointerEvents: 'auto',
});

export const dialog = css({
  background: colors.surface2,
  border: `1px solid ${colors.borderHeavy}`,
  borderRadius: radius.lg,
  minWidth: 480,
  maxWidth: '90%',
  maxHeight: '80%',
  display: 'flex',
  flexDirection: 'column',
  boxShadow: '0 16px 48px rgba(0, 0, 0, 0.6)',
});

export const dialogTitle = css({
  fontWeight: 600,
  padding: '14px 16px',
  borderBottom: `1px solid ${colors.border}`,
});

export const dialogBody = css({
  padding: '14px 16px',
  overflowY: 'auto',
});

export const dialogFooter = css({
  padding: '12px 16px',
  borderTop: `1px solid ${colors.border}`,
  display: 'flex',
  gap: 8,
  justifyContent: 'flex-end',
});

export const searchInput = css({
  width: '100%',
  border: `1px solid ${colors.border}`,
  borderRadius: 10,
  background: colors.bgUnder,
  color: colors.text,
  font: 'inherit',
  fontSize: 14,
  padding: '8px 12px',
  outline: 'none',
  '&:focus': { borderColor: colors.primary },
});

export const listItem = css({
  display: 'flex',
  flexDirection: 'column',
  gap: 2,
  padding: '8px 12px',
  borderRadius: 10,
  cursor: 'pointer',
  '&:hover': { background: colors.hover },
});

const title = css({ fontWeight: 500 });

export const listItemTitle = title;

export const listItemSelected = css({
  background: colors.hoverStrong,
  [`& .${title}`]: { color: colors.text },
});

export const listItemSub = css({
  color: colors.textMuted,
  fontSize: font.sm,
});

export const toolName = css({ color: colors.textDim, fontWeight: 500 });

// Multi-line body text inside dialogs (BTW prompt/answer, approval action).
export const bodyText = css({
  whiteSpace: 'pre-wrap',
  wordBreak: 'break-word',
});

// ---- buttons ----
export const btn = css({
  fontSize: font.sm,
  padding: '5px 12px',
  borderRadius: radius.pill,
  border: '1px solid transparent',
  background: colors.hover,
  color: colors.text,
  cursor: 'pointer',
  whiteSpace: 'nowrap',
  transition: 'background 0.12s ease',
  '&:hover': { background: colors.hoverStrong },
  '&:disabled': { opacity: 0.4, cursor: 'default' },
});

export const btnGhost = css({
  borderColor: colors.border,
  background: 'transparent',
  '&:hover': { background: colors.hover },
});

export const btnPrimary = css({
  background: colors.primary,
  color: '#fff',
  fontWeight: 500,
  '&:hover': { background: '#0a8fff' },
});

// ---- badges ----
export const badge = css({
  fontSize: 12,
  padding: '2px 10px',
  borderRadius: radius.pill,
  whiteSpace: 'nowrap',
  fontWeight: 500,
});

export const badgeSecondary = css({ background: colors.hover, color: colors.textDim });
export const badgePrimary = css({ background: 'rgba(2, 133, 255, 0.15)', color: colors.primary });
export const badgeOutline = css({ border: `1px solid ${colors.border}`, color: colors.textDim });
