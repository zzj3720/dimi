import { css } from '@emotion/css';
import { colors, font, size } from '../styles/theme';
import { md } from '../styles/global';

// ---- transcript scroll ----
// The 46px header is position:fixed (transparent) and overlays the window top,
// so the scroll container keeps a 47px top padding: the first message then
// sits at 47 (container padding) + 20 (thread py-5) = 67px, matching codex's
// measured scroll-container y=47 + thread py-5.
export const transcript = css({
  flex: 1,
  display: 'flex',
  flexDirection: 'column',
  overflowY: 'auto',
  padding: '47px 0 0',
  outline: 'none',
});

// Codex column shell: 768px (48rem) outer column with 16px side padding
// (px-toolbar) and 32px bottom padding (pb-8).
export const threadWrap = css({
  width: '100%',
  maxWidth: size.threadMaxW, // 736px — the visible column (Codex 768+16 wrapper collapses to 736; direct 736 keeps capsule 1:1 alignment in narrow windows too)
  margin: '0 auto',
  padding: '0 0 32px',
  display: 'flex',
  flexDirection: 'column',
  flex: '1 0 auto',
});

// Message column: 736px (768 − 16×2), gap-1.5 (6px), py-5 (20px top/bottom).
// The wrapper's 32px bottom padding + this 20px = the measured 52px bottom
// whitespace; this 20px + the transcript's 47px = the measured 67px top.
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
});

// Single "复制消息" button below the bubble; hidden until the turn is hovered
// or focused (codex group-hover / group-focus-within, opacity 0→1).
export const userCopyRow = css({
  display: 'flex',
  flexDirection: 'row-reverse',
  alignItems: 'center',
  gap: 4,
  margin: '0 4px', // me-1 ms-1
  opacity: 0,
  transition: 'opacity 0.12s ease',
});

// ---- turn ----
// Codex: one turn = user + thinking + assistant until the next user message,
// wrapped in a single `group flex flex-col py-2` container. The thread parent
// gap (6px) plus the two 8px paddings give the 22px new-turn spacing (the old
// entrySameTurn/entryNewTurn margin hacks are gone); the content column's 12px
// gap is the intra-turn spacing (codex gap-3).
export const turn = css({
  display: 'flex',
  flexDirection: 'column',
  padding: '8px 0', // py-2
  [`&:hover .${userCopyRow}, &:focus-within .${userCopyRow}`]: { opacity: 1 },
});

export const turnContent = css({
  display: 'flex',
  flexDirection: 'column',
  gap: 12, // gap-3
});

// Turn-level action row (assistant). Codex build has NO opacity classes here —
// the row is always visible — left-aligned, mt-1.5 (6px), h-5 (20px),
// gap-0.5 (2px), translated -4px on electron (buttons sit 4px left of the
// column edge).
export const turnActions = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'flex-start',
  gap: 2,
  marginTop: 6,
  height: 20,
  transform: 'translateX(-4px)',
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
  '& svg': { width: 16, height: 16 },
});

// "回复不佳" reuses the thumbs-up glyph rotated 180° (codex rotate-180).
export const entryActionBtnReplyBad = css({
  transform: 'rotate(180deg)',
});

// ---- reasoning disclosure (codex: collapsed = one "思考了 …" button) ----
export const thinkingBlock = css({
  minWidth: 0,
  fontSize: font.chat,
  position: 'relative',
  overflow: 'visible',
  padding: 0,
});

export const thinkingColumn = css({
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
  padding: 0, // p-0
  border: 'none',
  background: 'transparent',
  fontFamily: 'inherit',
  textAlign: 'left',
  fontSize: font.chat,
  lineHeight: '21px', // codex button height 21px
  fontWeight: 445,
  color: colors.textTertiary,
  cursor: 'pointer',
  userSelect: 'none',
  '&:hover': { color: colors.text }, // hover:text-token-text-primary
});

export const reasoningChevron = css({
  width: 16,
  height: 16,
  flexShrink: 0,
  // Inherits the button's currentColor; transition 0.3s cubic-bezier(0.4,0,0.2,1).
  transition: 'transform 0.3s cubic-bezier(0.4, 0, 0.2, 1)',
});

export const reasoningChevronOpen = css({
  transform: 'rotate(90deg)',
});

// Expanded content: codex `flex flex-col gap-4 pt-4` (16px gaps, 16px top).
// The disclosure stays v-if (DOM removed when collapsed) instead of codex's
// overflow-hidden + inline height/opacity — visually equivalent, and the
// height animation is not observable in the measured build.
export const reasoningBody = css({
  display: 'flex',
  flexDirection: 'column',
  gap: 16,
  paddingTop: 16,
});

// Tool calls inside the disclosure: each card is one block of the gap-4 column.
export const toolsCol = css({
  display: 'flex',
  flexDirection: 'column',
  gap: 16,
});

// Codex-style tool call row: avatar-stack icon + summary text, NO card chrome.
// (Tool-card form is "待验证" in the design doc — kept as the existing
// expandable card inside the disclosure, per task instructions.)
export const toolCard = css({
  background: 'transparent',
  border: 'none',
  borderRadius: 0,
  padding: 0,
  margin: '2px 0',
  cursor: 'pointer',
  userSelect: 'none',
  '&:hover': { opacity: 0.85 },
});

export const toolCardHeader = css({
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  fontSize: font.chat,
  lineHeight: font.chatLh,
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

export const toolCardBody = css({
  marginTop: 4,
  paddingTop: 4,
  fontSize: font.chat,
  lineHeight: font.chatLh,
  color: colors.textDim,
  whiteSpace: 'pre-wrap',
  wordBreak: 'break-word',
  fontFamily: font.mono,
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
  background: colors.hover,
  borderRadius: 12,
  padding: '10px 16px',
  fontSize: 14,
  cursor: 'pointer',
  '&:hover': { background: colors.hoverStrong },
});

export const modelName = css({ color: colors.text });
export const modelLevel = css({ color: colors.textMuted, fontSize: font.sm });

// re-export markdown style for the assistant body
export { md };
