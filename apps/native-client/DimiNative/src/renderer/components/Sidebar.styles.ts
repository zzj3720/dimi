import { css } from '@emotion/css';
import { colors, font, size } from '../styles/theme';

// ---- sidebar shell ----
export const sidebar = css({
  width: size.sidebarW,
  flexShrink: 0,
  background: colors.sidebarBg,
  display: 'flex',
  flexDirection: 'column',
  minHeight: 0,
  position: 'relative',
  paddingTop: size.headerH,
});

export const resizeHandleLine = css({
  pointerEvents: 'none',
  margin: 'auto',
  opacity: 0,
  height: '100%',
  width: 1,
  background: `linear-gradient(to bottom, transparent, ${colors.borderHeavy} 20%, ${colors.borderHeavy} 80%, transparent)`,
  transition: 'opacity 0.12s ease',
});

export const resizeHandle = css({
  position: 'absolute',
  top: 0,
  right: -8,
  bottom: 0,
  width: 16,
  cursor: 'col-resize',
  zIndex: 20,
  display: 'flex',
  alignItems: 'center',
  touchAction: 'none',
  userSelect: 'none',
  [`&:hover .${resizeHandleLine}, &:active .${resizeHandleLine}`]: { opacity: 1 },
});

// ---- sidebar header block ----
export const sidebarTop = css({
  padding: '0 0 4px',
  display: 'flex',
  flexDirection: 'column',
  gap: 8,
  position: 'relative',
  zIndex: 10,
});

export const brandRow = css({
  display: 'flex',
  alignItems: 'center',
  gap: 4,
  padding: '0 8px 0',
});

export const brand = css({
  display: 'flex',
  alignItems: 'center',
  gap: 4,
  fontSize: 17,
  lineHeight: '24px',
  fontWeight: 600,
  color: 'rgba(255, 255, 255, 0.85)',
  padding: '2px 8px',
  borderRadius: 15,
  border: '1px solid transparent',
  whiteSpace: 'nowrap',
  cursor: 'pointer',
  '&:hover': { background: colors.hover },
  '& svg': { width: 14, height: 14, flexShrink: 0, color: colors.textTertiary },
});

export const brandActions = css({
  marginLeft: 'auto',
  display: 'flex',
  alignItems: 'center',
  gap: 4,
});

// ---- sidebar nav block (Codex: 新对话 / 站点 / 已安排 / 插件) ----
export const navBlock = css({
  display: 'flex',
  flexDirection: 'column',
  gap: 1,
  padding: '0 8px',
});

export const navItem = css({
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  padding: '5px 8px',
  borderRadius: size.sidebarItemRadius,
  fontSize: font.xs,
  lineHeight: '20px', // Codex nav items are 30px tall (5px padding × 2 + 20px lh)
  color: 'rgba(255, 255, 255, 0.85)',
  cursor: 'pointer',
  border: 'none',
  background: 'transparent',
  textAlign: 'left',
  width: '100%',
  overflow: 'hidden',
  flexShrink: 0,
  '&:hover': { background: colors.hover8 },
  '& svg': { width: 16, height: 16, flexShrink: 0, color: 'rgba(255, 255, 255, 0.85)' },
  '& span': { overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
});

export const iconBtn = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  width: size.sidebarIconBtn,
  height: size.sidebarIconBtn,
  padding: 4,
  border: '1px solid transparent',
  borderRadius: size.sidebarIconBtnRadius,
  background: 'transparent',
  color: colors.textTertiary,
  fontSize: 16,
  lineHeight: '24px',
  cursor: 'pointer',
  flexShrink: 0,
  '&:hover': { background: colors.hover, color: colors.text },
  '& svg': { width: 16, height: 16 },
});

// ---- sessions scroll area ----
export const sessions = css({
  flex: 1,
  overflowY: 'auto',
  padding: '1px 8px 16px',
  marginTop: -4,
  display: 'flex',
  flexDirection: 'column',
  gap: 16,
});

export const sessionGroup = css({
  display: 'flex',
  flexDirection: 'column',
  gap: 4,
});

export const sessionGroupTitle = css({
  display: 'flex',
  alignItems: 'center',
  gap: 4,
  fontSize: font.xs,
  lineHeight: font.xsLh,
  fontWeight: 500,
  color: colors.textTertiary,
  padding: '2px 4px 2px 0',
  borderRadius: 10,
  minHeight: 25,
  userSelect: 'none',
  '& .chevron': { width: 14, height: 14, flexShrink: 0, color: colors.textTertiary },
  '& span': { flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
});

export const sessionItem = css({
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  padding: '5px 8px',
  borderRadius: size.sidebarItemRadius,
  fontSize: font.xs,
  lineHeight: font.xsLh,
  color: 'rgba(255, 255, 255, 0.85)',
  cursor: 'pointer',
  border: 'none',
  background: 'transparent',
  textAlign: 'left',
  width: '100%',
  overflow: 'hidden',
  flexShrink: 0,
  '&:hover': { background: colors.hover8 },
  '& svg': { width: 16, height: 16, flexShrink: 0 },
  '& .s-icon': { color: 'rgba(255, 255, 255, 0.85)' },
  '& .s-title': { flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
});

export const sessionItemActive = css({
  background: colors.hover5,
  color: colors.text,
  '& .s-icon': { color: colors.text },
});

// ---- sidebar bottom (Codex user/account row: clickable, avatar + name) ----
export const sidebarBottom = css({
  padding: '8px 8px 8px',
  borderTop: '0.5px solid rgba(255, 255, 255, 0.1)',
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  fontSize: font.xs,
  lineHeight: font.xsLh,
  color: 'rgba(255, 255, 255, 0.85)',
  flexShrink: 0,
});

export const userRow = css({
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  padding: '5px 8px',
  borderRadius: size.sidebarItemRadius,
  flex: 1,
  minWidth: 0,
  cursor: 'pointer',
  border: 'none',
  background: 'transparent',
  textAlign: 'left',
  fontSize: font.xs,
  lineHeight: font.xsLh,
  color: 'rgba(255, 255, 255, 0.85)',
  '&:hover': { background: colors.hover8 },
  '& .sb-avatar': {
    width: 18,
    height: 18,
    borderRadius: 9999,
    background: 'rgba(255, 255, 255, 0.85)',
    color: colors.bgUnder,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    fontSize: 11,
    fontWeight: 600,
    flexShrink: 0,
  },
  '& .sb-user': { flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', color: colors.text },
});

export const sidebarBottomBtn = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  width: 32,
  height: 32,
  padding: 7,
  border: '1px solid transparent',
  borderRadius: size.sidebarIconBtnRadius,
  background: 'transparent',
  color: colors.textTertiary,
  fontSize: 16,
  lineHeight: '24px',
  cursor: 'pointer',
  flexShrink: 0,
  '&:hover': { background: colors.hover, color: colors.text },
  '& svg': { width: 18, height: 18 },
});
