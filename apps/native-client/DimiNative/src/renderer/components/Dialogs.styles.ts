import { css } from '@emotion/css';
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
// blur(24px), rounded 24, NO border — a 0.5px ring + shadow-lg combined in one
// box-shadow. The close button is absolutely positioned, so the shell is
// position: relative and hides overflow (matches codex `overflow-hidden`).
const modalShell = {
  position: 'relative',
  background: 'rgba(45, 45, 45, 0.9)',
  backdropFilter: 'blur(24px)',
  WebkitBackdropFilter: 'blur(24px)',
  borderRadius: 24,
  maxWidth: '92vw',
  maxHeight: '80%',
  display: 'flex',
  flexDirection: 'column',
  boxShadow: '0 0 0 0.5px rgba(255, 255, 255, 0.082), 0 4px 8px -2px rgba(0, 0, 0, 0.1)',
  overflow: 'hidden',
} as const;

export const dialog = css({
  ...modalShell,
  width: 520,
});

// Session picker is a compact command menu (05-design §4.1 + §2.3), not a
// modal. Codex menu tokens (radius 15, #2d2d2d@90%, ring 0.5px, shadow
// 0 8px 16px -4px, padding 4px, backdrop blur small); dimi keeps its wider
// 440px column per the design note.
export const dialogPicker = css({
  ...modalShell,
  width: 440,
  borderRadius: 15,
  backdropFilter: 'blur(4px)',
  WebkitBackdropFilter: 'blur(4px)',
  boxShadow: '0 0 0 0.5px rgba(255, 255, 255, 0.082), 0 8px 16px -4px rgba(0, 0, 0, 0.12)',
  padding: 4,
});

// Approval is a permission prompt (05-design §4.3): same modal shell, but
// keeps dimi's accent top border (deliberate difference from codex).
export const dialogApproval = css({
  ...modalShell,
  width: 520,
  borderTop: `2px solid ${colors.primary}`,
});

// Title lives in the content flow (no title bar / bottom border): 20px/28px
// weight 600, white. The 24x24 close button at the top-right is avoided by the
// body's extra right padding (pe-8 in codex).
export const dialogTitle = css({
  fontWeight: 600,
  fontSize: 20,
  lineHeight: '28px',
});

export const dialogBody = css({
  padding: '20px 32px 20px 20px',
  overflowY: 'auto',
});

// codex-dialog close button (§2.1): 24x24, 16px from top/right, p-1 + rounded,
// inner 16x16 x at rgba(255,255,255,0.8).
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
  '& svg': { width: 16, height: 16 },
  '&:hover': { background: colors.hover },
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

// Radix menu item (§2.3): 29px high (8px 5px padding), rounded 12.5, 13px/
// 18.5714px, single row with 6px gap.
export const listItem = css({
  display: 'flex',
  alignItems: 'center',
  gap: 6,
  padding: '5px 8px',
  borderRadius: 12.5,
  fontSize: 13,
  lineHeight: '18.5714px',
  cursor: 'pointer',
  '&:hover': { background: colors.hover },
});

// Left indicator slot (icon-xs 16x16, vb 17 -> renders 16x17, opacity-75).
export const listItemIcon = css({
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  flexShrink: 0,
  width: 16,
  opacity: 0.75,
  '& svg': { width: 16, height: 17 },
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
