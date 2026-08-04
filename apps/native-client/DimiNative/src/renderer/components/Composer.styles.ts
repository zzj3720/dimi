import { css } from '@emotion/css';
import { colors, font, size, radius } from '../styles/theme';

// ---- composer shell ----
export const composer = css({
  position: 'relative',
  flexShrink: 0,
  padding: '0 0 16px',
});

export const capsule = css({
  maxWidth: size.threadMaxW,
  minHeight: 98, // border-box: 14 (attachment slot) + 76 (footer) + 8 (mb-2) = 98 outer
  margin: '0 auto',
  background: colors.composerBg,
  backdropFilter: 'blur(16px)',
  WebkitBackdropFilter: 'blur(16px)',
  borderRadius: size.composerRadius,
  padding: '14px 0 0', // Codex: inner grid starts 14px below the capsule top
  display: 'flex',
  flexDirection: 'column',
  boxSizing: 'border-box',
  // Codex has NO border on the capsule: the hairline ring is the 0.5px
  // box-shadow spread, and focus does NOT change the capsule (no
  // focus-within rule; the only focus feedback is the white caret).
  boxShadow:
    'rgba(255, 255, 255, 0.157) 0 0 0 0.5px, rgba(0, 0, 0, 0.04) 0 3px 7.5px 0, rgba(0, 0, 0, 0.05) 0 0 20px 0',
});

export const footer = css({
  display: 'grid',
  gridTemplateColumns: 'minmax(0, auto) auto minmax(0, 1fr)',
  alignItems: 'center',
  columnGap: 5, // Codex template "28px 0px 682px" leaves 5px gaps each side
  padding: '0 8px',
  marginBottom: 8, // Codex mb-2; capsule: 14 + 76 + 8 = 98 outer
  minHeight: 76,
});

export const inputRow = css({
  gridColumn: '1 / -1',
  gridRow: 1,
  minWidth: 0,
  margin: '0 -8px',
});

export const inputWrap = css({
  flexGrow: 1,
  overflowY: 'auto',
  padding: '0 12px', // Codex px-3: text column starts 12px from the capsule edge
  minHeight: 44, // Codex ProseMirror min-height 44
  marginBottom: 4, // Codex mb-1: input row 48 = 44 content + 4 below
  // Top-aligned (Codex): no align-items, so the editable stretches to the
  // 44px min-height and the text starts at the top of the row.
  display: 'flex',
});

export const composerLeft = css({ gridColumn: 1, gridRow: 2, minWidth: 0 });
export const composerRight = css({
  gridColumn: 3,
  gridRow: 2,
  minWidth: 0,
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'flex-end',
  // Codex gaps: pill ↔ 听写 0px; 听写 ↔ 发送 8px (margin-left on sendBtn).
});

export const composerBtn = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  gap: 4,
  width: size.composerBtn,
  height: size.composerBtn,
  padding: 0,
  border: '1px solid transparent', // Codex: 1px but transparent
  borderRadius: radius.pill,
  background: 'transparent',
  color: colors.textTertiary,
  fontSize: font.sm,
  lineHeight: '18px',
  cursor: 'default', // Codex cursor-interaction resolves to `default` on macOS
  flexShrink: 0,
  // Codex: hover only changes bg (--vscode-list-hoverBackground), color stays.
  '&:hover': { background: 'rgba(255, 255, 255, 0.078)' },
  '&:active': { background: 'rgba(255, 255, 255, 0.15)' }, // token-foreground @ 15%
  '&:disabled': { opacity: 0.4, cursor: 'not-allowed' },
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
  fontWeight: 445, // Codex text-sm weight 445
  cursor: 'default',
  whiteSpace: 'nowrap',
  height: size.composerBtn,
  minWidth: 0,
  // Codex: hover/active only change bg, color stays tertiary.
  '&:hover': { background: 'rgba(255, 255, 255, 0.078)' },
  '&:active': { background: 'rgba(255, 255, 255, 0.15)' },
  '&:disabled': { opacity: 0.4, cursor: 'not-allowed' },
  '& svg': { width: 14, height: 14, flexShrink: 0 }, // chevron 14×14
});

export const modelPillName = css({ color: colors.text }); // model name is token-text (#fff)

export const modelPillMode = css({ color: colors.textTertiary });

export const input = css({
  flex: 1,
  border: 'none',
  background: 'transparent',
  color: colors.text,
  font: 'inherit',
  fontSize: 14,
  lineHeight: '20px',
  fontWeight: 445, // Codex input weight 445
  padding: 0,
  maxHeight: '25dvh', // Codex max-h-[25dvh] (240px @ 960 viewport), then scrolls
  outline: 'none',
  overflowY: 'auto',
  display: 'block',
  width: '100%',
  whiteSpace: 'break-spaces',
  wordBreak: 'break-word',
  userSelect: 'text',
  caretColor: colors.text,
  '&:empty::before': {
    content: 'attr(data-placeholder)',
    color: colors.textTertiary, // Codex placeholder = text-tertiary rgba(255,255,255,0.498)
    pointerEvents: 'none',
  },
});

export const sendBtn = css({
  flexShrink: 0,
  width: size.sendBtn,
  height: size.sendBtn,
  borderRadius: radius.pill,
  border: 'none',
  background: '#fff', // Codex bg-token-foreground
  color: colors.composerBg, // Codex arrow fill = token-dropdown-background rgb(45,45,45)
  fontSize: 15,
  lineHeight: 1,
  cursor: 'default',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  padding: 2,
  marginLeft: 8, // Codex: 听写 ↔ 发送 gap 8px (pill ↔ 听写 gap 0)
  transition: 'opacity 0.15s cubic-bezier(0.4, 0, 0.2, 1)',
  // Codex has NO hover background (class carries only transition-opacity).
  '&:focus-visible': { outline: '2px solid rgb(13, 13, 13)' }, // Codex outline-2 token-button-background
  '&:disabled': { opacity: 0.5, cursor: 'default' },
  '& svg': { width: 16, height: 16 },
});

export const queuedCount = css({ color: colors.textMuted, fontSize: 12 });

// Busy-state controls float ABOVE the capsule (TUI functionality preserved
// without pushing the capsule off Codex's 98px composer geometry).
export const composerToolbar = css({
  position: 'absolute',
  bottom: 'calc(100% + 8px)',
  left: '50%',
  transform: 'translateX(-50%)',
  width: 'min(768px, calc(100vw - 32px))',
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  padding: '6px 10px',
  minHeight: 26,
  background: 'rgba(24, 24, 24, 0.92)',
  backdropFilter: 'blur(10px)',
  WebkitBackdropFilter: 'blur(10px)',
  borderRadius: 12,
  border: `1px solid ${colors.border}`,
  boxShadow: '0 8px 24px rgba(0, 0, 0, 0.45)',
  zIndex: 15,
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

// ---- completion popup (floats ABOVE the 98px capsule + 16px bottom margin)
export const completion = css({
  position: 'absolute',
  left: '50%',
  transform: 'translateX(-50%)',
  bottom: 128,
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
