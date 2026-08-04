import { css, keyframes } from '@emotion/css';
import { colors, font, radius } from '../styles/theme';

export const dialogRoot = css({
  position: 'absolute',
  inset: 0,
  zIndex: 100,
  pointerEvents: 'none',
});

// codex-dialog-overlay (05-design §2.2): very light dim, no blur.
export const dialogBackdrop = css({
  position: 'absolute',
  inset: 0,
  background: 'rgba(0, 0, 0, 0.133)',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  pointerEvents: 'auto',
});

// Shared modal shell (codex-dialog §2.1): rgb(45,45,45) @ 90% + backdrop
// blur(24px), rounded-3xl = 1.25rem × 1.25 scale = 25px, NO border — a 0.5px
// ring (token border 0.084) + shadow-lg combined in one box-shadow. The close
// button is absolutely positioned, so the shell is position: relative and
// hides overflow (matches codex `overflow-hidden`).
const modalShell = {
  position: 'relative',
  background: 'rgba(45, 45, 45, 0.9)',
  backdropFilter: 'blur(24px)',
  WebkitBackdropFilter: 'blur(24px)',
  borderRadius: 25,
  maxWidth: '92vw',
  maxHeight: '80%',
  display: 'flex',
  flexDirection: 'column',
  boxShadow: `0 0 0 0.5px ${colors.border}, 0 4px 8px -2px rgba(0, 0, 0, 0.1)`,
  overflow: 'hidden',
} as const;

// Codex dialog width ladder (05-design §2.3). dimi dialogs currently use
// `default` (520); the rest is available for future pickers/panels. The
// `editor` / `full` / `tall` variants are height/body modifiers not needed
// by the current dialogs.
export const dialogSizes = {
  narrow: '380px',
  feature: '400px',
  compact: '420px',
  default: '520px',
  wide: '600px',
  xwide: '680px',
  xxwide: '800px',
} as const;

export const dialog = css({
  ...modalShell,
  width: dialogSizes.default,
});

// Session picker is a compact command menu (05-design §4.1 + §2.3), not a
// modal. Codex menu tokens (radius 15, #2d2d2d@90%, ring 0.5px, shadow
// 0 8px 16px -4px, padding 4px, backdrop blur-sm 8px); dimi keeps its wider
// 440px column per the design note.
// Enter animation mirrors the only animated codex menu instance — the
// model-picker dropdown (05-design §5.4): 320ms cubic-bezier(.23,1,.32,1) +
// 30ms delay, fade + scale(.98→1), disabled under prefers-reduced-motion.
const pickerEnter = keyframes({
  '0%': { opacity: 0, transform: 'scale(0.98)' },
  '100%': { opacity: 1, transform: 'scale(1)' },
});

export const dialogPicker = css({
  ...modalShell,
  width: 440,
  borderRadius: 15,
  backdropFilter: 'blur(8px)',
  WebkitBackdropFilter: 'blur(8px)',
  boxShadow: `0 0 0 0.5px ${colors.border}, 0 8px 16px -4px rgba(0, 0, 0, 0.12)`,
  padding: 4,
  transformOrigin: 'center',
  animation: `${pickerEnter} 0.32s cubic-bezier(0.23, 1, 0.32, 1) 30ms both`,
  willChange: 'opacity, transform',
  '@media (prefers-reduced-motion: reduce)': { animation: 'none' },
});

// Approval is a permission prompt (05-design §4.3): same modal shell, but
// keeps dimi's accent top border (deliberate difference from codex).
export const dialogApproval = css({
  ...modalShell,
  width: dialogSizes.default,
  borderTop: `2px solid ${colors.primary}`,
});

// Title lives in the content flow (no title bar / bottom border): 20px/28px
// weight 600, white, letter-spacing -0.36px (codex heading-dialog). The 24x24
// close button at the top-right floats above the body (codex px-5 py-5 layout;
// the body no longer reserves right padding for it).
export const dialogTitle = css({
  fontWeight: 600,
  fontSize: 20,
  lineHeight: '28px',
  letterSpacing: '-0.36px',
});

export const dialogBody = css({
  padding: 20,
  overflowY: 'auto',
});

// codex-dialog close button (§2.1): 24x24, 16px from top/right, p-1 + rounded,
// inner 16x16 x at rgba(255,255,255,0.8), hover toolbar-hover 7.8%,
// focus-visible 1px ring in token focus border.
export const dialogClose = css({
  position: 'absolute',
  top: 16,
  right: 16,
  width: 24,
  height: 24,
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  padding: 4,
  borderRadius: 4,
  border: 'none',
  background: 'transparent',
  color: 'rgba(255, 255, 255, 0.8)',
  cursor: 'pointer',
  outline: 'none',
  '&:hover': { background: colors.hoverStrong },
  '&:focus-visible': { boxShadow: `0 0 0 1px ${colors.borderFocus}` },
});

// ---- icon size ladder (05-design §1.1) — the rendered size is decided by
// the CSS class, NOT the svg width/height attributes. Apply the class to the
// svg (or its wrapper) directly.
export const icon3xs = css({ width: 10, height: 10 });
export const iconXxs = css({ width: 12, height: 12 });
export const icon2xs = css({ width: 14, height: 14 });
export const iconXs = css({ width: 16, height: 16 });
export const iconSm = css({ width: 18, height: 18 });
export const iconBase = css({ width: 20, height: 20 });
export const iconMd = css({ width: 24, height: 24 });
export const iconLg = css({ width: 28, height: 28 });
// codex .icon-tint: 50% foreground weak tint (05-design §1.1).
export const iconTint = css({
  color: 'color-mix(in oklab, var(--color-token-foreground, #ffffff) 50%, transparent)',
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

// Codex menu SearchInput (05-design §3.3): a borderless search row inside the
// menu panel — rounded-sm (4px × 1.25 = 5px), row padding 8px/5px, text-sm,
// no border, no outline. Used by the session picker (a menu surface); the
// regular `searchInput` above stays for in-dialog form fields.
export const menuSearchInput = css({
  width: '100%',
  minWidth: 0,
  border: 'none',
  borderRadius: 5,
  padding: '5px 8px',
  background: 'transparent',
  color: colors.text,
  font: 'inherit',
  fontSize: 13,
  lineHeight: '18.5714px',
  outline: 'none',
  '&:focus': { outline: 'none' },
});

// Radix menu item (§2.3): 29px high (8px 5px padding), rounded 12.5, 13px/
// 18.5714px, single row with 6px gap; hover/focus background = list-hover
// rgba(255,255,255,0.078).
export const listItem = css({
  display: 'flex',
  alignItems: 'center',
  gap: 6,
  padding: '5px 8px',
  borderRadius: 12.5,
  fontSize: 13,
  lineHeight: '18.5714px',
  cursor: 'pointer',
  '&:hover': { background: colors.hoverStrong },
  '&:focus': { background: colors.hoverStrong },
});

// Left indicator slot (icon-xs 16x16, vb 24 check -> renders 16x16), static
// opacity 75%; the row's `.group` class (added by Dialogs.vue) raises it to
// 100% on hover/focus/selected, mirroring codex `pz.icon` group-hover/focus
// (05-design §3.2 / §5.3).
export const listItemIcon = css({
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  flexShrink: 0,
  opacity: 0.75,
  transition: 'opacity 0.1s ease',
  '& svg': { width: 16, height: 16 },
  '.group:hover &, .group:focus-within &, .group.selected &': { opacity: 1 },
});

export const listItemTitle = css({
  flex: 1,
  minWidth: 0,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
  fontSize: 13,
  fontWeight: 500,
});

// Secondary hint on the right side of a menu row (session last prompt / key
// hint): 13px rgba(255,255,255,0.498).
export const listItemHint = css({
  flexShrink: 0,
  maxWidth: '45%',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
  color: colors.textTertiary,
  fontSize: 13,
  lineHeight: '18.5714px',
});

// Selected state is dimi's existing highlight; codex's selected state was
// unobservable (05-design §6), keep and re-verify visually.
export const listItemSelected = css({
  background: colors.hoverStrong,
});

export const listItemSub = css({
  color: colors.textTertiary,
  fontSize: font.sm,
  lineHeight: font.smLh,
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

// Primary action (§4.2, codex main button measured 110x32): white #fff, dark
// text rgb(45,45,45), radius 12.5, padding 6px 16px, 14px/18px, border 1px
// rgba(255,255,255,0.082). Hover shade unobservable — subtle light gray.
export const btnPrimary = css({
  background: '#fff',
  color: 'rgb(45, 45, 45)',
  borderRadius: 12.5,
  padding: '6px 16px',
  fontSize: 14,
  lineHeight: '18px',
  borderColor: 'rgba(255, 255, 255, 0.082)',
  fontWeight: 500,
  '&:hover': { background: '#ececec' },
});

// Codex DialogFooter single-button mode (expandSingleButton, 05-design §2.2):
// the lone footer button stretches full width and centers its label.
export const btnBlock = css({
  width: '100%',
  justifyContent: 'center',
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
