import { css, keyframes } from '@emotion/css';
import { colors, font, size, elevation } from '../styles/theme';

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

// ---- sidebar header block ----
// pb is 1px in normal mode and 8px in search mode
// (--sidebar-scroll-header-spacing, 02-sidebar-code §2.5 / gap A1).
export const sidebarTop = css({
  padding: '0 8px 1px',
  display: 'flex',
  flexDirection: 'column',
  gap: 8,
  position: 'relative',
  zIndex: 10,
});

export const sidebarTopSearch = css({
  padding: '0 8px 8px', // search mode: --sidebar-scroll-header-spacing 8px
});

// Scrolled state (02-sidebar-code §2.5, C = scrolledContentUnderHeader): the
// header block's pb grows 1px → 4px (--sidebar-scroll-header-spacing) and a
// 0.5px hairline appears under it — codex
// `after:pointer-events-none after:absolute after:inset-x-0 after:bottom-0
// after:h-[0.5px] after:bg-token-foreground/10` (rgba(255,255,255,0.1)).
export const sidebarTopScrolled = css({
  padding: '0 8px 4px',
  '&::after': {
    content: '""',
    position: 'absolute',
    left: 0,
    right: 0,
    bottom: 0,
    height: '0.5px',
    background: 'rgba(255, 255, 255, 0.1)', // bg-token-foreground/10
    pointerEvents: 'none',
  },
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

// data-[state=open]:bg-token-list-hover-background (Radix open highlight)
export const brandOpen = css({
  background: cHover,
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

// Codex search button carries `ms-auto translate-x-0.5` (2px optical shift).
export const brandIconBtnSearch = css({
  transform: 'translateX(2px)',
});

// ---- nav rows ----
// 新对话 (header block) = 29px row; 站点/已安排/插件 (scroll block) = 30px rows.
// Codex nav rows use text-base: CDP-measured 14px / 21px line-height.
const navItemBase = (height: number) =>
  css({
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    padding: '0 8px',
    height,
    borderRadius: size.sidebarItemRadius,
    fontSize: 14,
    lineHeight: '21px',
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
// Codex nests two layers: outer .gap-1 (4px) + inner .gap-px (1px)
// (02-sidebar-code §2.4 / gap A3).
export const navBlockScroll = css({
  display: 'flex',
  flexDirection: 'column',
  gap: 4, // outer .gap-1
  flexShrink: 0,
  padding: '0 8px',
});

export const navBlockItems = css({
  display: 'flex',
  flexDirection: 'column',
  gap: 1, // inner .gap-px
});

// ---- sessions scroll area ----
const scrollMask =
  'linear-gradient(to bottom, transparent 0, #000 1px, #000 calc(100% - 40px), transparent 100%)';
// search mode (--sidebar-scroll-header-fade-start 8px / -distance 16px):
const scrollMaskSearch =
  'linear-gradient(to bottom, transparent 0, transparent 8px, #000 24px, #000 calc(100% - 40px), transparent 100%)';
// scrolled state (--sidebar-scroll-header-fade-start 4px / -distance 16px):
const scrollMaskScrolled =
  'linear-gradient(to bottom, transparent 0, transparent 4px, #000 16px, #000 calc(100% - 40px), transparent 100%)';

export const sessions = css({
  flex: '1 1 0%',
  overflowX: 'hidden',
  overflowY: 'auto',
  padding: '1px 0 54px', // bottom 54px = footer 46px + --padding-row-x 8px
  marginTop: -1, // -mt-[--sidebar-scroll-header-spacing] (1px normal)
  display: 'flex',
  flexDirection: 'column',
  gap: 16, // section spacing
  scrollbarWidth: 'auto',
  scrollbarColor: 'rgba(255, 255, 255, 0.082) rgba(0, 0, 0, 0)',
  maskImage: scrollMask, // top ~1px + bottom ~40px fade
  WebkitMaskImage: scrollMask,
});

// Search mode: -mt 8px (--sidebar-scroll-header-spacing 8px), pt stays 1px;
// top mask fade moves to 8px→24px.
export const sessionsSearch = css({
  marginTop: -8,
  maskImage: scrollMaskSearch,
  WebkitMaskImage: scrollMaskSearch,
});

// Scrolled state (C = scrolledContentUnderHeader, 02-sidebar-code §2.5): the
// scroll area pulls up to -mt 4px (--sidebar-scroll-header-spacing 4px) and
// the top mask fade widens to 4px→16px (--sidebar-scroll-header-fade-start
// 4px / -distance 16px). pt stays 1px (--sidebar-scroll-content-top-padding).
export const sessionsScrolled = css({
  marginTop: -4,
  maskImage: scrollMaskScrolled,
  WebkitMaskImage: scrollMaskScrolled,
});

// ---- sections (项目 / 最近) ----
export const section = css({
  position: 'relative',
  padding: '0 8px', // Codex px-row-x (rows sit 8px off both sidebar edges)
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
  fontSize: font.xs, // section titles stay 14px (text-base)
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
  // rows sit at x=8 (section px-row-x provides both 8px gutters)
});

export const folderRow = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  gap: 4, // Codex gap-1 between chevron and name (name lands at x=40)
  height: 30,
  padding: '0 8px',
  borderRadius: size.sidebarItemRadius,
  fontSize: 14, // Codex folder row text-base (CDP-measured 14px/21px)
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
  // Codex chevron sits at outer-8px + ps-1(4px) − mx(3px) = x9; dimi's row
  // padding-left is 8px so the chevron needs an extra −4px to match (CDP: svg
  // center codex 24px vs dimi 28px). Keep name at x=40 via the row gap-1.
  margin: '0 -3px 0 -7px',
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

// Marquee (02-sidebar-code A6): the exact codex keyframes are 无法确定, so the
// title scrolls by its measured overflow while the row is hovered. The
// distance is injected as --sb-marquee-dx per row from Sidebar.vue.
const marqueeKeyframes = keyframes`
  from { transform: translateX(0); }
  to { transform: translateX(var(--sb-marquee-dx, -40px)); }
`;

export const sessionItem = css({
  position: 'relative',
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  height: 30,
  padding: '0 8px', // Codex py-row-y + px-2 (5px 8px) — CDP-measured 2026-08-05; right was 4px
  borderRadius: size.sidebarItemRadius,
  fontSize: 14, // Codex text-sm = 14px (was 13px)
  lineHeight: '20px', // Codex text-sm leading (CDP-measured 20px)
  fontWeight: 445,
  color: cText,
  cursor: 'default',
  userSelect: 'none', // Codex select-none
  overflow: 'visible',
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
  // 24px right spacer (Codex shrink-0 group-hover:hidden): non-hover rows
  // reserve space that the hover actions replace on hover.
  '& .sb-row-spacer': { width: 24, flexShrink: 0 },
  '&:hover .sb-row-spacer': { display: 'none' },
  // inner span is what marquees inside the masked viewport
  '& .sb-title-inner': {
    display: 'inline-block',
    maxWidth: '100%',
    whiteSpace: 'nowrap',
  },
  '& .sb-title-inner.sb-marquee': {
    animation: `${marqueeKeyframes} 0.9s ease-in-out forwards`,
  },
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
  fontSize: font.xs, // user row stays 14px (measured)
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

export const userRowOpen = css({ background: cHover });

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

export const sidebarBottomBtnOpen = css({ background: cHover });

// ---- dropdown menus (mode switch / profile / help) ----
// Codex renders these as Radix DropdownMenus. CDP-measured 2026-08-05:
// content bg oklab(0.297/0.9) = rgba(45,45,45,0.9), radius 15px, padding 4px,
// shadow = 0.5px white 8.2% ring + 0 8px 16px -4px black 12%; menu items are
// 29px (5px 8px padding), radius 12.5px, 13px text.
export const menuAnchor = css({
  position: 'relative',
  display: 'flex',
  minWidth: 0,
});

export const menuAnchorGrow = css({
  flex: 1,
  minWidth: 0,
});

export const menu = css({
  position: 'absolute',
  zIndex: 40,
  minWidth: 160,
  margin: 1, // Codex m-px
  padding: 4, // Radix px-1 py-1
  background: 'rgba(45, 45, 45, 0.9)', // --color-token-dropdown-background @ 90%
  backdropFilter: 'blur(8px)', // Codex backdrop-blur-sm
  WebkitBackdropFilter: 'blur(8px)',
  borderRadius: 15,
  boxShadow: elevation.dropdown,
  display: 'flex',
  flexDirection: 'column',
});

// codex `menuWide`
export const menuWide = css({ minWidth: 180 });

// anchors
export const menuTop = css({
  top: 'calc(100% + 4px)',
  left: -8, // cancels the brand button's -ms-2
});

export const menuBottomLeft = css({
  bottom: 'calc(100% + 7px)', // Codex profile menu gap 8px incl. 1px m-px margin
  left: 0,
});

export const menuBottomRight = css({
  bottom: 'calc(100% + 7px)',
  right: 0,
});

export const menuItem = css({
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  height: 29, // Codex CDP-measured 29px (pad 5px 8px + 19px content)
  padding: '5px 8px',
  borderRadius: 12.5,
  border: 'none',
  background: 'transparent',
  fontSize: 13,
  lineHeight: '18.5714px',
  fontWeight: 445,
  color: cText,
  textAlign: 'left',
  cursor: 'default',
  whiteSpace: 'nowrap',
  flexShrink: 0,
  '&:hover': { background: cHover },
});

// fixed 16px slot: check icon for radio items, empty spacer otherwise
export const menuCheck = css({
  width: 16,
  flexShrink: 0,
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  '& svg': { width: 14, height: 14, color: cText },
});

// ---- search mode view (02-sidebar-code C4 / A1) ----
// The idu search-view internals are 无法确定 (待补全), so the input row
// follows the sidebar-item scale; the rest is dimi's own approximation.
export const searchView = css({
  display: 'flex',
  flexDirection: 'column',
  gap: 4,
  flexShrink: 0,
  padding: '0 8px', // Codex px-row-x (8px gutters like the sections)
});

export const searchInputRow = css({
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  height: 30,
  padding: '0 8px',
  borderRadius: size.sidebarItemRadius,
  '& svg': { width: 16, height: 16, flexShrink: 0, color: cTertiary },
});

export const searchInput = css({
  flex: 1,
  minWidth: 0,
  border: 'none',
  outline: 'none',
  background: 'transparent',
  fontSize: 13,
  lineHeight: '18.5714px',
  fontWeight: 445,
  color: cText,
  '&::placeholder': { color: cTertiary },
});

export const searchClear = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  width: 20,
  height: 20,
  padding: 0,
  border: '1px solid transparent',
  borderRadius: 10,
  background: 'transparent',
  color: cTertiary,
  cursor: 'default',
  '&:hover': { background: cHover, color: cText },
  '& svg': { width: 14, height: 14, flexShrink: 0 },
});

// ---- session row context menu (fixed overlay, codex Ztu subset) ----
export const ctxMenuStyle = css({
  position: 'fixed',
  zIndex: 80,
  minWidth: 180,
  margin: 1, // Codex m-px
  padding: 4,
  background: 'rgba(45, 45, 45, 0.9)', // Radix dropdown (CDP-measured)
  backdropFilter: 'blur(8px)', // Codex backdrop-blur-sm
  WebkitBackdropFilter: 'blur(8px)',
  borderRadius: 15,
  boxShadow: elevation.dropdown,
  display: 'flex',
  flexDirection: 'column',
});

// Inline rename input inside a session row (codex thread-title edit).
export const editInput = css({
  flex: 1,
  minWidth: 0,
  height: 22,
  padding: '0 6px',
  borderRadius: 8,
  border: `1px solid ${colors.borderFocus}`,
  background: colors.surface2,
  color: cText,
  fontSize: 13,
  lineHeight: '18.5714px',
  outline: 'none',
});
