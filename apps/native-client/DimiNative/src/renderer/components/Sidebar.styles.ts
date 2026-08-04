import { css } from '@emotion/css';
import { colors, font, size } from '../styles/theme';

// ---- Codex sidebar tokens (measured live, see design/02-sidebar.md) ----
// Brand row prefers "OpenAI Sans"; keep the system stack as fallback.
const brandFamily =
  '"OpenAI Sans", -apple-system, "system-ui", "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif';
const cText = 'rgba(255, 255, 255, 0.85)'; // text-token-foreground
const cTertiary = 'rgba(255, 255, 255, 0.498)'; // text-token-text-tertiary
const cIconTint = 'rgba(255, 255, 255, 0.425)'; // sidebar-hover-icon-tint (50% mix)
const cHover = 'rgba(255, 255, 255, 0.08)'; // token-list-hover-background
const cActive = 'rgba(255, 255, 255, 0.15)'; // token-foreground/15

// Codex `.text-fade-truncate`: fade the right edge with a mask, no ellipsis.
const fadeTruncate = {
  whiteSpace: 'nowrap' as const,
  overflow: 'hidden' as const,
  maskImage:
    'linear-gradient(to right, #000 calc(100% - 1rem), transparent)' as const,
  WebkitMaskImage:
    'linear-gradient(to right, #000 calc(100% - 1rem), transparent)' as const,
};

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
  top: -46, // Codex: drag zone extends up over the 46px header
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

// ---- sidebar header block (padding 0 8px 1px, gap 8 → 70px total) ----
export const sidebarTop = css({
  padding: '0 8px 1px',
  display: 'flex',
  flexDirection: 'column',
  gap: 8,
  position: 'relative',
  zIndex: 10,
});

// ---- brand row (32px): mode-switch button + search + priority ----
export const brandRow = css({
  display: 'flex',
  alignItems: 'center',
  height: 32, // --height-token-mode-switch
  marginLeft: 8, // .ms-2
  paddingRight: 4, // .pe-1
});

export const brand = css({
  display: 'flex',
  alignItems: 'center',
  gap: 4,
  marginLeft: -8, // .-ms-2: cancels the row's ms-2 so the button sits at x=8
  padding: '2px 8px',
  height: 32,
  borderRadius: 15,
  border: '1px solid transparent',
  fontFamily: brandFamily,
  fontSize: 17,
  lineHeight: '24px',
  fontWeight: 500, // button 500; the text span inside is 600
  color: cText,
  whiteSpace: 'nowrap',
  cursor: 'default', // Codex: cursor-interaction resolves to default on mac
  background: 'transparent',
  '&:hover': { background: cHover },
  '&:active': { background: cActive },
  '& span': {
    fontWeight: 600,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  '& svg': { width: 14, height: 14, flexShrink: 0, color: cTertiary },
});

export const brandActions = css({
  marginLeft: 'auto',
  display: 'flex',
  alignItems: 'center',
  gap: 4, // .gap-1
});

export const brandIconBtn = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  width: 26,
  height: 26,
  padding: 4,
  border: '1px solid transparent',
  borderRadius: 10,
  background: 'transparent',
  color: cTertiary,
  cursor: 'default',
  flexShrink: 0,
  '&:hover': { background: cHover, color: cText },
  '& svg': { width: 16, height: 16, flexShrink: 0 },
});

// ---- nav rows ----
// 新对话 (header block) = 29px row; 站点/已安排/插件 (scroll block) = 30px rows.
const navItemBase = (height: number) =>
  css({
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    padding: '0 8px',
    height,
    borderRadius: size.sidebarItemRadius,
    fontSize: font.xs, // 14px
    lineHeight: '21px', // Codex nav text 14px/21px
    fontWeight: 445, // variable-font weight
    color: cText,
    cursor: 'default',
    border: 'none',
    background: 'transparent',
    textAlign: 'left',
    width: '100%',
    overflow: 'hidden',
    flexShrink: 0,
    '&:hover': { background: cHover },
    '& svg': { width: 16, height: 16, flexShrink: 0, color: cText },
    '& span': { flex: 1, minWidth: 0, ...fadeTruncate },
  });

export const navItemHeader = navItemBase(29);
export const navItemScroll = navItemBase(30);

// Fixed nav block at the top of the scroll area: 3×30 + 2×1 = 92px.
export const navBlockScroll = css({
  display: 'flex',
  flexDirection: 'column',
  gap: 1,
  flexShrink: 0,
  padding: '0 8px',
});

// ---- sessions scroll area ----
const scrollMask =
  'linear-gradient(to bottom, transparent 0, #000 1px, #000 calc(100% - 40px), transparent 100%)';

export const sessions = css({
  flex: '1 1 0%',
  overflowX: 'hidden',
  overflowY: 'auto',
  padding: '1px 0 54px', // bottom 54px = footer 46px + --padding-row-x 8px
  marginTop: -1,
  display: 'flex',
  flexDirection: 'column',
  gap: 16, // section spacing
  scrollbarWidth: 'auto',
  scrollbarColor: 'rgba(255, 255, 255, 0.082) rgba(0, 0, 0, 0)',
  maskImage: scrollMask, // top ~1px + bottom ~40px fade
  WebkitMaskImage: scrollMask,
});

// ---- sections (项目 / 最近) ----
export const section = css({
  position: 'relative',
});

export const sectionTitleRow = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  height: 25,
  padding: '0 2px 0 8px',
  gap: 8,
  // hover / keyboard focus reveals the chevron and the action buttons
  '&:hover .sb-chevron, &:focus-within .sb-chevron': { opacity: 1, color: cText },
  '&:hover .sb-title-actions, &:focus-within .sb-title-actions': {
    opacity: 1,
    pointerEvents: 'auto',
  },
});

export const sectionToggle = css({
  display: 'flex',
  minWidth: 0,
  flex: 1,
  alignItems: 'center',
  gap: 4,
  padding: '2px 4px 2px 0',
  borderRadius: 10,
  border: 'none',
  background: 'transparent',
  textAlign: 'left',
  cursor: 'default',
  fontSize: font.xs,
  lineHeight: '21px',
  fontWeight: 500,
  color: cTertiary,
  userSelect: 'none',
  '& span': { flex: 1, minWidth: 0, ...fadeTruncate },
  '& .sb-chevron': {
    width: 14,
    height: 14,
    flexShrink: 0,
    color: cIconTint,
    opacity: 0, // hidden until the title row is hovered / focused
    transition: 'transform 0.15s cubic-bezier(0.4, 0, 0.2, 1), opacity 0.15s ease',
  },
  '& .sb-chevron.collapsed': { transform: 'rotate(90deg)' },
});

export const sectionTitleActions = css({
  display: 'flex',
  alignItems: 'center',
  gap: 4,
  flexShrink: 0,
  pointerEvents: 'none',
  opacity: 0,
});

export const sectionTitleBtn = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  width: 24,
  height: 24,
  padding: 4,
  border: '1px solid transparent',
  borderRadius: 10,
  background: 'transparent',
  color: cIconTint,
  cursor: 'default',
  '&:hover': { background: cHover, color: cText },
  '& svg': { width: 14, height: 16, flexShrink: 0 }, // measured 14×16 render
});

export const emptyRow = css({
  display: 'flex',
  alignItems: 'center',
  height: 29,
  padding: '0 8px',
  fontSize: font.xs,
  lineHeight: '21px',
  color: cTertiary,
});

// ---- folder rows (cwd tree under 项目) ----
export const folderGroup = css({
  position: 'relative',
  display: 'flex',
  flexDirection: 'column',
  paddingLeft: 8, // rows sit at x=8 like the fixed nav rows
});

export const folderRow = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  height: 30,
  padding: '0 8px',
  borderRadius: size.sidebarItemRadius,
  fontSize: font.xs,
  lineHeight: '21px',
  fontWeight: 445,
  color: cText,
  cursor: 'default',
  overflowX: 'hidden',
  '&:hover': { background: cHover },
  // right-hand folder actions appear on hover (Codex w-0 → group-hover:w-auto)
  '&:hover .sb-folder-actions': { width: 'auto', opacity: 1 },
});

export const folderRowIcon = css({
  width: 30,
  height: 30,
  margin: '0 -3px', // Codex -mx-[3px]
  flexShrink: 0,
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  '& svg': { width: 16, height: 16, color: cText },
});

export const folderRowName = css({
  flex: 1,
  minWidth: 0,
  paddingRight: 4, // .pe-1
  ...fadeTruncate,
  color: cText,
});

export const folderRowActions = css({
  display: 'flex',
  alignItems: 'center',
  gap: 4, // .gap-1
  maxWidth: '50%',
  overflow: 'hidden',
  width: 0,
  opacity: 0,
});

export const folderRowBtn = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  width: 24,
  height: 24,
  padding: 4,
  border: '1px solid transparent',
  borderRadius: 10,
  background: 'transparent',
  color: cIconTint,
  cursor: 'default',
  '&:hover': { background: cHover, color: cText },
  '& svg': { width: 14, height: 16, flexShrink: 0 },
});

// ---- session items ----
export const sessionList = css({
  display: 'flex',
  flexDirection: 'column',
  gap: 1, // Codex gap-px between items
  padding: '2px 0 8px', // .pt-0.5 .pb-2
  overflow: 'hidden',
});

export const sessionItem = css({
  position: 'relative',
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  height: 30,
  padding: '0 4px 0 8px', // left 8 (--padding-row-cell-x), right 4 (.pe-1)
  borderRadius: size.sidebarItemRadius,
  fontSize: font.xs,
  lineHeight: '20px', // Codex session text 14px/20px (leading-5)
  fontWeight: 445,
  color: cText,
  cursor: 'default',
  overflow: 'hidden',
  flexShrink: 0,
  '&:hover': { background: cHover },
  // 16px reserved (empty) icon slot → title starts at x=40 (8 + 16 + 8)
  '& .sb-slot': {
    width: 16,
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
  },
  '& .s-title': { flex: 1, minWidth: 0, ...fadeTruncate },
  // hover suffix label (Codex hidden → group-hover:inline, color 0.498)
  '& .sb-suffix': { display: 'none', flexShrink: 0, color: cTertiary },
  '&:hover .sb-suffix': { display: 'inline' },
  // default right badge: 20×20 box with 14×14 icon, hidden on hover
  '& .sb-badge': {
    position: 'absolute',
    top: 0,
    bottom: 0,
    right: 0,
    zIndex: 10,
    minWidth: 52,
    paddingRight: 4, // .pe-1
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'flex-end',
  },
  '& .sb-badge-box': {
    width: 20,
    height: 20,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
  },
  '& .sb-badge-box svg': { width: 14, height: 14, color: cTertiary },
  '&:hover .sb-badge': { display: 'none' },
  // hover actions: 52px right overlay (pin + archive), instant show
  '& .sb-hover-actions': {
    position: 'absolute',
    top: 0,
    bottom: 0,
    right: 0,
    zIndex: 10,
    width: 52,
    marginRight: 2, // .me-0.5
    paddingRight: 2, // .pe-0.5
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'flex-end',
    gap: 8, // .gap-2
    opacity: 0,
  },
  '&:hover .sb-hover-actions': { opacity: 1 },
  '& .sb-hover-btn': {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: 20,
    height: 20,
    padding: 0,
    border: '1px solid transparent',
    borderRadius: 10,
    background: 'transparent',
    color: cIconTint,
    cursor: 'default',
    '&:hover': { background: cHover, color: cText },
    '& svg': { width: 16, height: 16, flexShrink: 0 },
    '& svg.sb-pin': { transform: 'translateX(1px)' }, // Codex optical offset
  },
});

export const sessionItemActive = css({
  background: cHover, // persisted selected bg = 0.08 (not hover5)
  color: '#ffffff', // pure white
});

// ---- sidebar bottom (46px: hairline + user row + help) ----
export const sidebarBottom = css({
  position: 'absolute',
  top: 'auto',
  right: 0,
  bottom: 0,
  left: 0,
  zIndex: 20,
  height: 46,
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  padding: '0 8px',
  flexShrink: 0,
  fontSize: font.xs,
  lineHeight: '21px',
  '&::before': {
    content: '""',
    position: 'absolute',
    top: 0,
    right: 0,
    left: 0,
    height: 1,
    pointerEvents: 'none',
    background: 'rgba(255, 255, 255, 0.1)', // Codex hairline
  },
});

export const userRow = css({
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  height: 29,
  padding: '0 8px',
  borderRadius: size.sidebarItemRadius,
  flex: 1,
  minWidth: 0,
  cursor: 'default',
  border: 'none',
  background: 'transparent',
  textAlign: 'left',
  fontSize: font.xs,
  lineHeight: '21px',
  fontWeight: 445,
  color: '#ffffff',
  '&:hover': { background: cHover },
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
  '& .sb-user': { flex: 1, minWidth: 0, ...fadeTruncate, color: '#ffffff' },
});

export const sidebarBottomBtn = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  width: 32,
  height: 32,
  padding: '4px 0',
  border: '1px solid transparent',
  borderRadius: 10,
  background: 'transparent',
  color: cTertiary,
  cursor: 'default',
  flexShrink: 0,
  '&:hover': { background: cHover, color: cText },
  '& svg': { width: 18, height: 18 }, // icon-sm 18×18
});
