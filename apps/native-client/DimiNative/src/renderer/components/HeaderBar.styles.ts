import { css } from '@emotion/css';
import { colors, size } from '../styles/theme';

export const header = css({
  position: 'fixed',
  top: 0,
  left: 0,
  right: 0,
  zIndex: 30,
  display: 'flex',
  alignItems: 'center',
  gap: 6,
  height: size.headerH,
  padding: 0,
  background: 'transparent',
  borderBottom: 'none',
  flexShrink: 0,
});

export const headerLeft = css({
  display: 'flex',
  alignItems: 'center',
  gap: 6,
  minWidth: 0,
  flex: 1,
  paddingLeft: 16,
});

export const headerTitle = css({
  fontSize: 14,
  fontWeight: 500,
  color: colors.textDim,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
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

export const headerStatus = css({
  fontSize: 12,
  color: colors.textMuted,
  whiteSpace: 'nowrap',
});
