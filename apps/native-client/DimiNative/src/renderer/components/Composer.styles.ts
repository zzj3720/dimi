import { css } from '@emotion/css';
import { colors, font, size, radius } from '../styles/theme';

// ---- composer shell ----
export const composer = css({
  position: 'relative',
  flexShrink: 0,
  padding: '0 16px 10px',
});

export const capsule = css({
  maxWidth: size.threadMaxW,
  margin: '0 auto',
  background: colors.composerBg,
  backdropFilter: 'blur(16px)',
  WebkitBackdropFilter: 'blur(16px)',
  border: '1px solid transparent',
  borderRadius: size.composerRadius,
  padding: 0,
  display: 'flex',
  flexDirection: 'column',
  boxShadow:
    'rgba(255, 255, 255, 0.157) 0 0 0 0.5px, rgba(0, 0, 0, 0.04) 0 3px 7.5px 0, rgba(0, 0, 0, 0.05) 0 0 20px 0',
});

export const footer = css({
  display: 'grid',
  gridTemplateColumns: 'minmax(0, auto) auto minmax(0, 1fr)',
  alignItems: 'center',
  columnGap: 5,
  padding: '0 8px',
  marginBottom: 8,
  minHeight: 76,
});

export const inputRow = css({
  gridColumn: '1 / -1',
  gridRow: 1,
  minWidth: 0,
  margin: '0 -8px',
});

export const inputWrap = css({
  marginBottom: 4,
  flexGrow: 1,
  overflowY: 'auto',
  padding: '0 12px',
  minHeight: 44,
  display: 'flex',
  alignItems: 'center',
});

export const composerLeft = css({ gridColumn: 1, gridRow: 2, minWidth: 0 });
export const composerRight = css({
  gridColumn: 3,
  gridRow: 2,
  minWidth: 0,
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'flex-end',
  gap: 2,
});

export const composerBtn = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  gap: 4,
  width: size.composerBtn,
  height: size.composerBtn,
  padding: 0,
  border: '1px solid transparent',
  borderRadius: radius.pill,
  background: 'transparent',
  color: colors.textTertiary,
  fontSize: font.sm,
  lineHeight: '18px',
  cursor: 'pointer',
  flexShrink: 0,
  '&:hover': { background: colors.hover, color: colors.text },
  '& svg': { width: 16, height: 16 },
});

export const modelPill = css({
  display: 'flex',
  alignItems: 'center',
  gap: 4,
  padding: '0 8px',
  border: '1px solid transparent',
  borderRadius: radius.pill,
  background: 'transparent',
  color: colors.textTertiary,
  fontSize: font.sm,
  lineHeight: '18px',
  cursor: 'pointer',
  whiteSpace: 'nowrap',
  height: size.composerBtn,
  '&:hover': { background: colors.hover, color: colors.text },
});

export const input = css({
  flex: 1,
  resize: 'none',
  border: 'none',
  background: 'transparent',
  color: colors.text,
  font: 'inherit',
  fontSize: 14,
  lineHeight: '20px',
  padding: 0,
  maxHeight: 160,
  outline: 'none',
  display: 'block',
  width: '100%',
  '&::placeholder': { color: colors.textMuted },
});

export const sendBtn = css({
  flexShrink: 0,
  width: size.sendBtn,
  height: size.sendBtn,
  borderRadius: radius.pill,
  border: 'none',
  background: '#fff',
  color: '#181818',
  fontSize: 15,
  lineHeight: 1,
  cursor: 'pointer',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  padding: 2,
  '&:hover': { background: '#f0f0f0' },
  '&:disabled': { background: colors.hoverStrong, color: colors.textMuted, cursor: 'default' },
  '& svg': { width: 16, height: 16 },
});

export const queuedCount = css({ color: colors.textMuted, fontSize: 12 });

export const composerToolbar = css({
  maxWidth: size.threadMaxW,
  margin: '6px auto 0',
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  padding: '0 8px',
  minHeight: 20,
});

export const hint = css({ color: colors.textMuted, fontSize: 12 });
export const footerRight = css({ color: colors.textMuted, fontSize: 12, whiteSpace: 'nowrap' });

// ---- busy-state buttons ----
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

// ---- completion popup ----
export const completion = css({
  position: 'absolute',
  left: '50%',
  transform: 'translateX(-50%)',
  bottom: 84,
  width: 'min(640px, calc(100vw - 48px))',
  background: colors.surface2,
  border: `1px solid ${colors.border}`,
  borderRadius: 12,
  maxHeight: 260,
  overflowY: 'auto',
  zIndex: 50,
  boxShadow: '0 8px 32px rgba(0, 0, 0, 0.5)',
  padding: 4,
});

export const completionItem = css({
  display: 'flex',
  gap: 10,
  alignItems: 'center',
  padding: '6px 10px',
  cursor: 'pointer',
  borderRadius: 8,
  '&:hover': { background: colors.hover },
});

export const completionPointer = css({ flexShrink: 0, width: '2ch', color: colors.textMuted });
export const completionValue = css({ color: colors.text });
export const completionDesc = css({
  color: colors.textMuted,
  fontSize: font.sm,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
});

export const completionSelected = css({
  background: colors.hoverStrong,
  [`& .${completionPointer}`]: { color: colors.primary },
  [`& .${completionValue}`]: { color: colors.text },
});
