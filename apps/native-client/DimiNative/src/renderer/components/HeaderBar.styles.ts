import { css } from '@emotion/css';
import { colors, font, size, elevation } from '../styles/theme';

// Codex header: 46px fixed transparent bar. The whole header is
// pointer-events:none — only the buttons re-enable it (S17), so empty header
// space passes clicks through (window drag / sidebar resize zone). The bar
// itself is -webkit-app-region:drag (S17) so it doubles as the macOS window
// drag strip; every interactive child opts out with no-drag.
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
  WebkitAppRegion: 'drag', // S17: window drag strip
});

// Left zone above the sidebar: Codex leaves an 88px safe-left gap
// (--spacing-token-safe-header-left = windowControlsOverlay.left/zoom + 6)
// before the 92px-wide button group (3×28 + 2×4 gap) — hide-sidebar / back /
// forward. dimi hardcodes 88 (L1; theme.ts is read-only and Windows has no
// traffic lights, so the WCO-driven value would collapse to ~8px there — a
// future variable can swap this once the bridge exposes window controls
// overlay geometry). No menu button at x=0 — the first button IS the sidebar
// trigger (L2).
export const headerSide = css({
  flexShrink: 0,
  display: 'flex',
  alignItems: 'center',
  minWidth: 180, // L9: codex measured natural width (88 safe + 92 buttons)
  paddingLeft: 88, // L1: safe-left (see comment above)
});

// Codex left-slot width = spring(sidebar width, default 275, clamp 240–520);
// dimi's sidebar is v-if (no spring), so the zone snaps between the open
// width and the natural 180px when the sidebar is hidden (§1.2 / §2). The
// open width is DYNAMIC: it tracks state.sidebarWidth (Sidebar.vue's drag
// writes it via the store), bound here by HeaderBar.vue as
// headerSideOpen(width) — never the hardcoded 275.
export const headerSideOpen = (w: number): string => css({ width: `${w}px` });
export const headerSideClosed = css({ width: '180px' });

export const headerSideGroup = css({
  display: 'flex',
  alignItems: 'center',
  gap: 4, // Codex gap-1
});

// Main zone (flex-1): ms-2 (8px margin-left) + title-row ps-2 (8px padding);
// the title offsets itself -2px (Codex -ms-0.5) so its text sits at x=289
// (L6). More lives INSIDE the title row's actions group (col1, gap-1 = 4px);
// codex's grid gap-x-4 (16px) separates col1 from col2, which dimi doesn't
// have (L7). pe-1.5 (6px) right padding applies when right entries exist
// (§3).
export const headerMain = css({
  flex: 1,
  minWidth: 0,
  display: 'flex',
  alignItems: 'center',
  gap: 4, // L7: Codex col1 actions gap-1
  marginLeft: 8, // Codex ms-2
  paddingRight: 6, // Codex pe-1.5
});

// Codex col1 env icon: 28×28 ghost button (project popover trigger), folder
// glyph 16px — `aria-label="项目：{cwd}"` (CDP-measured 2026-08-05). This is
// the element that pushes the session title to x≈311 in codex.
export const projectBtn = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  width: 28,
  height: 28,
  flexShrink: 0,
  padding: 0,
  border: '1px solid transparent',
  borderRadius: 12.5,
  background: 'transparent',
  color: colors.textTertiary,
  cursor: 'default',
  '& svg': { width: 16, height: 16, color: colors.text },
  '&:hover': { background: colors.hover },
  '&:active': { background: colors.hoverStrong },
});

// Project picker popover (below the header, left-aligned to the main zone).
// absolute — the header is position:fixed, so this anchors to the header's
// padding box; `top: calc(100% + 4px)` then means just below the 46px bar.
export const projectPicker = css({
  position: 'absolute',
  zIndex: 80,
  minWidth: 240,
  maxWidth: 420,
  padding: 6,
  background: colors.surface2,
  borderRadius: 12,
  boxShadow: elevation.prominent,
  display: 'flex',
  flexDirection: 'column',
  gap: 1,
});

export const projectPickerItem = css({
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  height: 32,
  padding: '0 8px',
  borderRadius: 8,
  border: 'none',
  background: 'transparent',
  color: colors.text,
  fontSize: 13,
  lineHeight: '18.5714px',
  fontWeight: 445,
  textAlign: 'left',
  cursor: 'default',
  '&:hover': { background: colors.hover },
});

export const projectPickerItemActive = css({
  background: colors.hover,
  '&:hover': { background: colors.hoverStrong },
});

export const projectPickerEmpty = css({
  padding: '8px 12px',
  color: colors.textMuted,
  fontSize: font.sm,
});

// Session title is a BUTTON in Codex (S12): hover white-8% bg, radius 10px,
// padding 0 6px, margin-left -2px, max-width 320px, truncating text. Codex
// gives it no border class (unlike the toolbar buttons) so none here either —
// with the global box-sizing:border-box that keeps the 24px height exact.
export const headerTitle = css({
  display: 'flex',
  alignItems: 'center',
  height: 24,
  marginLeft: -2, // Codex -ms-0.5
  padding: '0 6px', // Codex px-1.5
  fontSize: 14,
  lineHeight: '24px', // Codex leading-6
  fontWeight: 500, // Codex font-medium
  fontFamily: 'inherit',
  color: colors.text,
  borderRadius: 10, // Codex rounded-md
  background: 'transparent',
  border: '1px solid transparent', // Codex border-transparent — kills the UA button border
  maxWidth: 320, // Codex max-w-[320px]
  minWidth: 0,
  flexShrink: 1,
  cursor: 'default', // S6: cursor-interaction → default in electron
  pointerEvents: 'auto',
  outline: 'none', // Codex focus-visible:outline-none
  WebkitAppRegion: 'no-drag',
  '&:hover': { background: colors.hover8 }, // hover:bg-token-list-hover-background
  '&:focus-visible': { background: colors.hover8 },
  '& span': {
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
    minWidth: 0,
  },
});

// Generic 28×28 icon button (sidebar-trigger/back/forward/toggle-sidebar):
// radius 12.5px (S1), hover white 8% (S2), color stays tertiary on hover
// (S3), weight 445 from the theme token (S5), cursor default (S6), no
// transition (S15).
export const iconBtn = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  width: size.iconBtn,
  height: size.iconBtn,
  padding: 0,
  border: '1px solid transparent', // tm base `border` + ghost `border-transparent`
  borderRadius: size.iconBtnRadius, // Codex rounded-lg = 12.5px
  background: 'transparent',
  color: colors.textTertiary, // ghost default
  fontSize: 14,
  fontWeight: font.weight, // 445
  lineHeight: '18px',
  cursor: 'default',
  flexShrink: 0,
  pointerEvents: 'auto',
  outline: 'none', // Codex focus:outline-none
  transition: 'all 0s ease',
  WebkitAppRegion: 'no-drag',
  '& svg': { width: 16, height: 16 }, // icon-xs
  '&:hover:not(:disabled)': { background: colors.hover8 }, // 白 8%
  '&:active:not(:disabled)': { background: 'rgba(255, 255, 255, 0.15)' }, // 白 15%
  '&:disabled': { opacity: 0.4, cursor: 'not-allowed' }, // S14
});

// More button: transparent border (S10), electron p-1 4px padding (S11),
// rounded-md 10px, 18×18 ellipsis icon (icon-sm, set inline in the template).
export const moreBtn = css({
  padding: 4,
  borderRadius: 10,
  fontSize: 14,
  fontWeight: font.weight,
  lineHeight: '18px',
});

// Right zone: share pill + pinned summary + toggle-sidebar, gap 6px, 8px
// right padding (pe-2); no opaque 36px spacer (L3/L5).
export const headerRight = css({
  marginLeft: 'auto', // Codex ms-auto
  display: 'flex',
  alignItems: 'center',
  gap: 6, // Codex gap-1.5
  flexShrink: 0,
  paddingRight: 8, // Codex pe-2
});

// Codex share pill: rounded-lg 12.5px (S7, NOT pill), padding 0 8px (S8),
// white 14px/445/18px text (S9), 16×16 icon, 4px icon-text gap.
export const shareBtn = css({
  display: 'flex',
  alignItems: 'center',
  gap: 4,
  height: 28, // h-token-button-composer
  padding: '0 8px', // px-2
  borderRadius: size.iconBtnRadius, // rounded-lg
  border: '1px solid transparent',
  background: 'transparent',
  color: colors.text, // enabled:text-token-text-primary
  fontSize: 14,
  fontWeight: font.weight,
  lineHeight: '18px',
  cursor: 'default',
  flexShrink: 0,
  pointerEvents: 'auto',
  outline: 'none',
  transition: 'all 0s ease',
  WebkitAppRegion: 'no-drag',
  '& svg': { width: 16, height: 16 },
  '&:hover': { background: colors.hover8 },
  '&:active': { background: 'rgba(255, 255, 255, 0.15)' },
});

// Pinned summary (固定摘要) toggle — Codex HeaderButton: pressed ?
// 'secondary' : 'ghost' (§1.7). OFF = ghost: tertiary icon, hover 8%, active
// 15%. ON (pinnedBtnOn) = secondary: white icon, 5% bg, hover 10%, active
// 15% (§4). Icon: dimi `dots` (= codex w5o, §8).
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
  color: colors.textTertiary, // ghost default
  fontSize: 14,
  fontWeight: font.weight,
  lineHeight: '18px',
  cursor: 'default',
  flexShrink: 0,
  pointerEvents: 'auto',
  outline: 'none',
  transition: 'all 0s ease',
  WebkitAppRegion: 'no-drag',
  '& svg': { width: 16, height: 16 },
  '&:hover:not(:disabled)': { background: colors.hover8 }, // ghost hover
  '&:active:not(:disabled)': { background: 'rgba(255, 255, 255, 0.15)' },
});

export const pinnedBtnOn = css({
  background: 'rgba(255, 255, 255, 0.05)', // secondary default (白 5%)
  color: colors.text, // secondary → foreground
  '&:hover:not(:disabled)': { background: 'rgba(255, 255, 255, 0.10)' }, // secondary hover
});

// Inline rename input (S12/A5): replaces the title button while editing —
// Codex: `h-6 rounded-md border border-token-focus-border
// bg-token-input-background px-1.5 text-base leading-6 font-medium`.
// `bg-token-input-background` has no dimi token — surface2 (#2d2d2d, the
// dropdown/panel surface) is the closest approximation.
export const headerTitleInput = css({
  height: 24, // h-6
  minWidth: 0,
  maxWidth: 320, // same clamp as the title button
  marginLeft: -2, // Codex -ms-0.5
  padding: '0 6px', // px-1.5
  borderRadius: 10, // rounded-md
  border: `1px solid ${colors.borderFocus}`, // border-token-focus-border
  background: colors.surface2, // bg-token-input-background (approximated)
  color: colors.text, // text-token-input-foreground (approximated)
  fontSize: 14, // text-base
  lineHeight: '24px', // leading-6
  fontWeight: 500, // font-medium
  fontFamily: 'inherit',
  outline: 'none',
  boxSizing: 'border-box',
  pointerEvents: 'auto',
  WebkitAppRegion: 'no-drag',
});

// ---- session context menu (fixed overlay, codex HeaderContextMenuItem) ----
export const headerCtxMenu = css({
  position: 'fixed',
  zIndex: 80,
  minWidth: 180,
  padding: 6,
  background: colors.surface2,
  borderRadius: 12,
  boxShadow: elevation.prominent,
  display: 'flex',
  flexDirection: 'column',
});

export const ctxMenuItem = css({
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  height: 32,
  padding: '0 8px',
  borderRadius: 8,
  border: 'none',
  background: 'transparent',
  fontSize: 13,
  lineHeight: '18.5714px',
  fontWeight: 445,
  color: colors.text,
  textAlign: 'left',
  cursor: 'default',
  whiteSpace: 'nowrap',
  '&:hover': { background: colors.hover },
});
