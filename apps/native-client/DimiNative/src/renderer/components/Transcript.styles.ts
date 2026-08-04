import { css } from '@emotion/css';
import { colors, font, size, radius, elevation, spacing } from '../styles/theme';
import { md } from '../styles/global';

// Codex markdown line-height: font-size + 8px = 22px at the 14px chat font
// (design doc §6.1; theme token font.chatLh stays 21px — read-only).
const MD_LH = '22px';

// ---- transcript scroll ----
// The 46px header is position:fixed (transparent) and overlays the window top;
// mainCol (App.styles.ts) yields it with padding-top: var(--height-toolbar),
// so the scroll container starts at y=46. Codex's thread scroll container
// carries `pt-(--thread-content-top-inset)` = 32px (CDP-measured 2026-08-04,
// design doc §2: `padding: 32px 0 0`); dimi's container sits 1px higher than
// codex's (y=46 vs y=47), so 32 + 1 = 33px puts the first turn at the same
// absolute y as codex: 46/47 + 32 + 20 (thread py-5) = 99px turn top.
export const transcript = css({
  flex: 1,
  display: 'flex',
  flexDirection: 'column',
  overflowY: 'auto',
  padding: `${spacing.threadTopInset + 1}px 0 0`,
  outline: 'none',
});

// Codex column shell: 768px (48rem) outer column with 16px side padding
// (px-toolbar) and 32px bottom padding (pb-8). Width follows the same
// two-layer formula as the composer capsule: min(736px, 100% − 32px), so the
// message column keeps a 16px gutter in narrow windows (codex 717px at a
// 1024 viewport) instead of clamping to a fixed 736px.
export const threadWrap = css({
  width: 'min(736px, calc(100% - 32px))',
  margin: '0 auto',
  padding: '0 0 32px',
  display: 'flex',
  flexDirection: 'column',
  flex: '1 0 auto',
});

// Message column: 736px (768 − 16×2), gap-1.5 (6px), py-5 (20px top/bottom).
// The wrapper's 32px bottom padding + this 20px = the measured 52px bottom
// whitespace; this 20px + mainCol 46px + transcript 33px = the codex-aligned
// 99px first-turn top (codex: container 47 + 32 + 20).
export const thread = css({
  display: 'flex',
  flexDirection: 'column',
  gap: 6,
  padding: '20px 0',
  flex: '1 0 auto',
  minHeight: '100%',
});

export const bodyMuted = css({ color: colors.textMuted, fontSize: font.chat });

// ---- user message (right-aligned bubble + single copy button) ----
export const entryUser = css({
  display: 'flex',
  flexDirection: 'column',
  alignItems: 'flex-end',
  gap: 8,
});

export const userMsgGroup = css({
  display: 'flex',
  flexDirection: 'column',
  alignItems: 'flex-end',
  justifyContent: 'flex-end',
  gap: 4,
  width: '100%',
});

export const userBubble = css({
  color: colors.text,
  background: 'rgba(255, 255, 255, 0.05)', // bg-token-foreground/5
  borderRadius: 20, // rounded-2xl
  padding: '8px 12px', // px-3 py-2
  maxWidth: '77%',
  minWidth: 0,
  overflow: 'hidden',
  wordBreak: 'break-word',
  textAlign: 'start',
  // User text renders through the same markdown pipeline as assistant text
  // (codex `_markdownContent_`), but the container keeps pre-wrap so plain
  // single-newline input stays on separate lines. `.userBubble .md` (0,2,0)
  // wins over `.md`'s own `white-space: normal` (0,1,0).
  [`& .${md}`]: { whiteSpace: 'pre-wrap' },
  // Codex bubble-only markdown overrides (design doc §6.2, CDP-verified):
  // paragraphs are flush with 20px between adjacent ones (`[&_p]:!m-0
  // [&_p+p]:!mt-5`), lists indent 24px (`!ps-6`), list items get no extra
  // gap (`[&_li+li]:!mt-0 [&_li>ol]:!mt-0 [&_li>p+p]:!mt-0 [&_li>ul]:!mt-0`).
  [`& .${md} p`]: { margin: 0 },
  [`& .${md} p + p`]: { marginTop: 20 },
  [`& .${md} ul, & .${md} ol`]: { paddingLeft: 24 },
  [`& .${md} li + li`]: { marginTop: 0 },
  [`& .${md} li > ul, & .${md} li > ol`]: { marginTop: 0 },
  [`& .${md} li > p + p`]: { marginTop: 0 },
});

// ---- user message editing (codex §5.2: dblclick the bubble → a composer-like
// editor replaces it, with cancel/submit) ----
// The edit surface mirrors the composer capsule tokens (Composer.styles.ts
// capsule + input) but stays local to the thread. The commit is UI-only: the
// dimi server has no message-edit endpoint, so save replaces the local bubble
// text without a network call (see Transcript.vue commitEdit).
export const userEdit = css({
  width: '100%',
  background: colors.composerBg,
  backdropFilter: 'blur(16px)',
  WebkitBackdropFilter: 'blur(16px)',
  borderRadius: size.composerRadius, // rounded-3xl (25px)
  boxShadow: elevation.prominent,
  display: 'flex',
  flexDirection: 'column',
  gap: 8,
  padding: '12px 12px 8px',
  boxSizing: 'border-box',
});

export const userEditInput = css({
  width: '100%',
  border: 'none',
  background: 'transparent',
  color: colors.text,
  font: 'inherit',
  fontSize: 14,
  lineHeight: '20px',
  fontWeight: 445,
  padding: 0,
  outline: 'none',
  resize: 'none',
  overflowY: 'auto',
  maxHeight: '25dvh', // composer max-h-[25dvh]
  whiteSpace: 'break-spaces',
  wordBreak: 'break-word',
  caretColor: colors.text,
  userSelect: 'text',
});

export const userEditRow = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'flex-end',
  gap: 8,
});

export const userEditBtn = css({
  display: 'inline-flex',
  alignItems: 'center',
  padding: '4px 12px',
  border: '1px solid transparent',
  borderRadius: radius.pill,
  background: 'transparent',
  color: colors.text,
  fontSize: font.sm,
  lineHeight: font.smLh,
  cursor: 'pointer',
  '&:hover': { background: colors.hoverStrong },
  '&:active': { background: 'rgba(255, 255, 255, 0.15)' },
  '&:focus-visible': { outline: 'none', boxShadow: '0 0 0 2px rgba(131, 195, 255, 0.76)' },
});

export const userEditBtnPrimary = css({
  background: '#fff', // codex send button: bg-token-foreground
  color: colors.composerBg,
  '&:hover': { background: '#fff', opacity: 0.9 },
});

// Single "复制消息" button below the bubble; hidden until the turn is hovered
// or focused (codex group-hover / group-focus-within, opacity 0→1).
export const userCopyRow = css({
  display: 'flex',
  flexDirection: 'row-reverse',
  alignItems: 'center',
  gap: 8, // codex gap-2 (only one button renders, so this is inert)
  margin: '0 4px', // me-1 ms-1
  opacity: 0,
  transition: 'opacity 0.12s ease',
});

// Turn-level action row (assistant). Codex Fvl: `mt-1.5 flex h-5 items-center
// justify-start gap-0.5 electron:-translate-x-1` — margin-top 6px, height
// 20px, gap 2px, −4px electron shift — and the row is HIDDEN by default
// (`opacity-0 group-focus-within:opacity-100 group-hover:opacity-100`),
// revealed when the owning turn is hovered or focused (design doc §4.1).
export const turnActions = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'flex-start',
  gap: 2,
  marginTop: 6,
  height: 20,
  transform: 'translateX(-4px)',
  opacity: 0,
  transition: 'opacity 0.12s ease',
});

// Codex sent-time (design doc §1.7 R): `span.ms-1.5.flex.h-full.items-center`
// `[data-assistant-message-sent-time]`, `text-xs text-token-text-tertiary` —
// sits at the right end of the action row and is revealed with it on hover.
export const turnActionsTime = css({
  display: 'flex',
  alignItems: 'center',
  height: '100%',
  marginLeft: 6, // ms-1.5
  fontSize: 12, // text-xs
  lineHeight: '16px',
  color: colors.textTertiary,
  whiteSpace: 'nowrap',
  fontVariantNumeric: 'tabular-nums',
});

// User-message send time (design doc §1.3: `span` in the hover-revealed row).
export const userCopyTime = css({
  fontSize: 12, // text-xs
  lineHeight: '16px',
  color: colors.textTertiary,
  whiteSpace: 'nowrap',
  fontVariantNumeric: 'tabular-nums',
});

// ---- turn ----
// Codex: one turn = user + thinking + assistant until the next user message,
// wrapped in a single `group flex flex-col py-2` container. The thread parent
// gap (6px) plus the two 8px paddings give the 22px new-turn spacing; blocks
// inside the turn are separated by 16px dividers (codex s8c,
// `--conversation-item-gap`). The turn carries codex's per-item rendering
// hint (`content-visibility: auto` + `contain-intrinsic-size: auto 240px`,
// design doc §5.6) so long sessions skip offscreen turns.
export const turn = css({
  display: 'flex',
  flexDirection: 'column',
  padding: '8px 0', // py-2
  contentVisibility: 'auto',
  containIntrinsicSize: 'auto 240px',
  // group-hover / group-focus-within reveal for both action rows (user copy
  // row + assistant action row).
  [`&:hover .${userCopyRow}, &:focus-within .${userCopyRow}, &:hover .${turnActions}, &:focus-within .${turnActions}`]: {
    opacity: 1,
  },
});

export const turnContent = css({
  display: 'flex',
  flexDirection: 'column',
  gap: 0, // block spacing comes from the 16px itemDivider separators
});

// Codex s8c: 16px separator between turn items (`--conversation-item-gap`).
export const itemDivider = css({
  height: 16,
  width: '100%',
  flexShrink: 0,
});

// Action buttons (user copy + turn row): 26×26, radius 10, 1px transparent
// border, padding 4, tertiary color rgba(255,255,255,0.498). Hover only
// changes the background (0.078); active is 0.15; focus shows the codex ring.
export const entryActionBtn = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  width: 26,
  height: 26,
  padding: 4,
  border: '1px solid transparent',
  borderRadius: 10,
  background: 'transparent',
  color: colors.textTertiary,
  cursor: 'pointer',
  flexShrink: 0,
  '&:hover': { background: 'rgba(255, 255, 255, 0.078)' }, // list-hover-background
  '&:active': { background: 'rgba(255, 255, 255, 0.15)' }, // foreground/15
  '&:focus-visible': { outline: 'none', boxShadow: '0 0 0 2px rgba(131, 195, 255, 0.76)' },
  '&:disabled': { cursor: 'default', opacity: 0.5 },
  '& svg': { width: 16, height: 16 },
});

// "回复不佳" reuses the thumbs-up glyph rotated 180° (codex rotate-180).
export const entryActionBtnReplyBad = css({
  transform: 'rotate(180deg)',
});

// Codex thumbs (Svl): ghost icon 24×24px (CDP-measured, design doc §1.7/§9) —
// one step smaller than the 26×26 copy/fork buttons.
export const entryActionBtnRating = css({
  width: 24,
  height: 24,
});

// ---- reasoning disclosure (codex mAl: collapsed = one "思考了 …" button) ----
export const thinkingBlock = css({
  minWidth: 0,
  fontSize: font.chat,
  position: 'relative',
  overflow: 'visible',
  padding: 0,
  display: 'flex',
  flexDirection: 'column',
});

export const reasoningTitle = css({
  display: 'inline-flex',
  alignItems: 'center',
  alignSelf: 'flex-start',
  gap: 2, // gap-0.5
  maxWidth: '100%',
  minWidth: 0,
  height: 21, // codex computed button height 21px
  padding: 0, // p-0
  border: 'none',
  background: 'transparent',
  fontFamily: 'inherit',
  textAlign: 'left',
  fontSize: font.chat,
  lineHeight: '21px',
  fontWeight: 445,
  color: colors.textTertiary,
  cursor: 'pointer',
  userSelect: 'none',
  '&:hover': { color: colors.text }, // hover:text-token-text-primary
});

// Codex: 16×16 chevron (`icon-xs`, CDP-measured 2026-08-04 — the design doc's
// 20×20 was wrong), transition-transform, rotate-180 expanded.
export const reasoningChevron = css({
  width: 16,
  height: 16,
  flexShrink: 0,
  // Inherits the button's currentColor; transition 0.3s cubic-bezier(0.4,0,0.2,1).
  transition: 'transform 0.3s cubic-bezier(0.4, 0, 0.2, 1)',
});

export const reasoningChevronOpen = css({
  transform: 'rotate(180deg)',
});

// Height+opacity animation shell (codex rf.div): the shell always stays in
// the DOM, its inline height/opacity flip between 0 and the measured content
// height with a 300ms cubic-bezier(0.19,1,0.22,1) transition. The collapsed
// class delays `visibility: hidden` until the fade-out finishes and disables
// pointer events, so hidden content is neither clickable nor tabbable.
export const reasoningShell = css({
  overflow: 'hidden',
  transition:
    'height 0.3s cubic-bezier(0.19, 1, 0.22, 1), opacity 0.3s cubic-bezier(0.19, 1, 0.22, 1)',
});

export const reasoningShellCollapsed = css({
  visibility: 'hidden',
  pointerEvents: 'none',
  transition:
    'height 0.3s cubic-bezier(0.19, 1, 0.22, 1), opacity 0.3s cubic-bezier(0.19, 1, 0.22, 1), visibility 0s linear 0.3s',
});

// Capped reasoning body: max 140px (8.75rem) even when expanded, bottom edge
// fade applied via an inline mask when the content actually overflows
// (codex eAl `maxHeightByState` + `--edge-fade-distance: 1rem`).
export const reasoningBody = css({
  maxHeight: '8.75rem',
  overflow: 'hidden',
});

// Codex reasoning markdown overrides (design doc §1.5 / §3.4): tighter
// heading/paragraph spacing than the assistant body — h1/h2/h3 mt 8px, the
// element after a heading mt 4px, adjacent paragraphs mt 4px, p m-0,
// ul/ol my-0 ps-4 (16px).
export const thinkingMd = css({
  [`&.${md} p`]: { margin: 0 },
  [`&.${md} > p + p`]: { marginTop: 4 },
  [`&.${md} > h1, &.${md} > h2, &.${md} > h3`]: { marginTop: 8 },
  [`&.${md} > h1 + *, &.${md} > h2 + *, &.${md} > h3 + *`]: { marginTop: 4 },
  [`&.${md} ul, &.${md} ol`]: { margin: 0, paddingLeft: 16 },
});

// Tool activity is an independent item in codex (separate from the reasoning
// disclosure, 16px divider between them) — each card is one block of the
// column.
export const toolsCol = css({
  display: 'flex',
  flexDirection: 'column',
  gap: 16,
});

// (toolCard is declared after toolCardName/Status/Icon/Time — its hover rules
// reference those consts, and emotion's css() evaluates the object eagerly.)
export const toolCardHeader = css({
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  fontSize: font.chat,
  lineHeight: MD_LH,
  color: colors.textTertiary,
});

export const toolCardIcon = css({
  width: 16,
  height: 16,
  flexShrink: 0,
  color: colors.textTertiary,
  borderRadius: 4,
  background: 'rgba(255, 255, 255, 0.1)',
  padding: 3,
  boxSizing: 'border-box',
  opacity: 0, // codex: chevron hidden until the card is hovered (§1.6)
  transition:
    'transform 0.3s cubic-bezier(0.4, 0, 0.2, 1), opacity 0.12s ease',
});

export const toolCardIconOpen = css({
  transform: 'rotate(180deg)',
});

export const toolCardName = css({
  color: colors.textTertiary,
  fontWeight: 500,
  flexShrink: 0,
});

export const toolCardStatus = css({
  color: colors.textTertiary,
  fontSize: font.sm,
  lineHeight: font.smLh,
  marginLeft: 'auto',
  whiteSpace: 'nowrap',
});

// Tool-frame sent time (same text-xs tertiary style as the message rows);
// hidden until the card is hovered, alongside the chevron (codex §1.6).
export const toolCardTime = css({
  fontSize: 12, // text-xs
  lineHeight: '16px',
  color: colors.textTertiary,
  whiteSpace: 'nowrap',
  fontVariantNumeric: 'tabular-nums',
  opacity: 0,
  transition: 'opacity 0.12s ease',
});

export const toolCard = css({
  background: 'transparent',
  border: 'none',
  borderRadius: 0,
  padding: 0,
  margin: '2px 0',
  cursor: 'pointer',
  userSelect: 'none',
  // Codex activity header hover (design doc §4.4): summary/name text moves
  // from text-tertiary to text-primary — no whole-card dimming.
  [`&:hover .${toolCardName}, &:hover .${toolCardStatus}`]: { color: colors.text },
  // The expand chevron and the sent time are hover-only in codex (design
  // doc §1.6: `opacity-0 group-hover:opacity-100`).
  [`&:hover .${toolCardIcon}, &:hover .${toolCardTime}`]: { opacity: 1 },
});

// Shell card (codex Pbl): command + output on the code-block background,
// radius 12.5px (`--radius-lg`), 12px/20px mono; the same height+opacity
// animation as the reasoning body. The 8px padding lives on the INNER wrapper
// (toolShellInner): the animated shell itself must stay padding-free, because
// with box-sizing:border-box a `height: 0` shell that kept its padding would
// still render 8+8=16px tall in the collapsed state (measured leak).
export const toolShell = css({
  overflow: 'hidden',
  background: 'rgba(255, 255, 255, 0.052)', // text-code-block-background
  borderRadius: 12.5,
  transition:
    'height 0.3s cubic-bezier(0.19, 1, 0.22, 1), opacity 0.3s cubic-bezier(0.19, 1, 0.22, 1)',
});

export const toolShellInner = css({
  padding: 8,
});

export const toolShellCollapsed = css({
  visibility: 'hidden',
  pointerEvents: 'none',
  transition:
    'height 0.3s cubic-bezier(0.19, 1, 0.22, 1), opacity 0.3s cubic-bezier(0.19, 1, 0.22, 1), visibility 0s linear 0.3s',
});

export const toolCardBody = css({
  fontSize: 12,
  lineHeight: 20,
  color: colors.textDim,
  whiteSpace: 'pre-wrap',
  wordBreak: 'break-word',
  fontFamily: font.mono,
  overflowX: 'auto',
});

export const clickable = css({ cursor: 'pointer' });

// ---- welcome page ----
export const welcome = css({
  maxWidth: size.threadMaxW,
  margin: '0 auto',
  padding: '80px 16px 32px',
  display: 'flex',
  flexDirection: 'column',
  alignItems: 'center',
  textAlign: 'center',
});

export const welcomeH1 = css({
  fontSize: 26,
  fontWeight: 600,
  color: colors.text,
  marginBottom: 24,
});

export const suggestions = css({
  display: 'flex',
  flexDirection: 'column',
  gap: 8,
  width: '100%',
  maxWidth: 480,
  marginBottom: 40,
});

export const suggestionCard = css({
  background: colors.hover,
  borderRadius: 12,
  padding: '14px 16px',
  fontSize: 14,
  color: colors.text,
  cursor: 'pointer',
  textAlign: 'left',
  border: '1px solid transparent',
  '&:hover': { background: colors.hoverStrong },
});

export const welcomeModels = css({
  display: 'flex',
  flexDirection: 'column',
  gap: 8,
  width: '100%',
  maxWidth: 480,
});

export const welcomeModelsTitle = css({
  fontSize: font.sm,
  color: colors.textMuted,
  textAlign: 'left',
});

export const modelRow = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  background: 'transparent',
  borderRadius: 12,
  padding: '10px 16px',
  fontSize: 14,
  cursor: 'pointer',
  '&:hover': { background: colors.hover },
});

export const modelRowSelected = css({
  background: colors.hover,
  '&:hover': { background: colors.hoverStrong },
});

export const modelName = css({ color: colors.text });
export const modelLevel = css({ color: colors.textMuted, fontSize: font.sm });

// re-export markdown style for the assistant body
export { md };
