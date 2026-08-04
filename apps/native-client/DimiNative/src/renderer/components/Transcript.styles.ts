import { css } from '@emotion/css';
import { colors, font, size } from '../styles/theme';
import { md } from '../styles/global';

// ---- transcript scroll ----
export const transcript = css({
  flex: 1,
  overflowY: 'auto',
  padding: '78px 0 0',
  outline: 'none',
});

// ---- thread ----
export const thread = css({
  maxWidth: size.threadMaxW,
  margin: '0 auto',
  // Codex: 32px bottom padding; message content sits at the column edge
  padding: '20px 0 32px',
  display: 'flex',
  flexDirection: 'column',
  gap: 6,
});

const bodyBase = {
  fontSize: font.chat,
  lineHeight: font.chatLh,
  wordBreak: 'break-word',
  whiteSpace: 'pre-wrap',
} as const;

export const entry = css({
  padding: '8px 0',
  lineHeight: 1.5,
  '& .body': bodyBase,
  // `.entry .body` (pre-wrap) must not override `.md`'s white-space: normal:
  // v-html block elements separated by newline text nodes otherwise render as
  // extra 22px line boxes between paragraphs. Higher specificity wins here.
  '& .body.md': { whiteSpace: 'normal' },
});

export const bodyMuted = css({ color: colors.textMuted, fontSize: font.chat });
export const bodyTool = css({
  color: colors.textDim,
  fontSize: font.chat,
  lineHeight: font.chatLh,
  whiteSpace: 'pre-wrap',
  wordBreak: 'break-word',
  cursor: 'pointer',
});
export const bodyThinking = css({
  color: colors.text,
  fontSize: font.chat,
  lineHeight: font.chatLh,
  whiteSpace: 'pre-wrap',
  wordBreak: 'break-word',
  marginTop: 16, // Codex: expanded content starts 16px below the button
});
export const bodyCompaction = css({ color: colors.textMuted, fontSize: font.chat });
export const toolName = css({ color: colors.textDim, fontWeight: 500 });

// Codex-style tool call row: avatar-stack icon + summary text, NO card chrome.
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

// Hover action buttons on messages (Codex: 26×26 row BELOW the message, flow
// layout with gap 4 — never overlaps the message itself).
export const entryActions = css({
  display: 'flex',
  justifyContent: 'flex-end',
  gap: 4,
  marginTop: 4,
  opacity: 0,
  transition: 'opacity 0.12s ease',
  zIndex: 5,
});

export const entryHasActions = css({
  [`&:hover .${entryActions}`]: { opacity: 1 },
});

export const entryActionBtn = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  width: 26,
  height: 26,
  padding: 4,
  border: `1px solid ${colors.border}`,
  borderRadius: 10,
  background: colors.surface2,
  color: colors.textDim,
  cursor: 'pointer',
  '&:hover': { color: colors.text, background: colors.hoverStrong },
  '& svg': { width: 16, height: 16 },
});

// User messages render as right-aligned bubbles (Codex: bg 5% white,
// radius 20px, pad 8px 12px, max-width 77%, 14/21px).
export const entryUser = css({
  display: 'flex',
  justifyContent: 'flex-end',
  '& .body': {
    color: colors.text,
    lineHeight: '22px', // Codex 22px
    background: 'rgba(255, 255, 255, 0.05)',
    borderRadius: 20,
    padding: '8px 12px',
    maxWidth: '77%',
    whiteSpace: 'pre-wrap',
    wordBreak: 'break-word',
  },
});
export const entryAssistant = css({ '& .body': { color: colors.text } });
export const entryThinking = css({ '& .body': { color: colors.textDim, fontSize: font.chat } });
export const entryTool = css({ '& .body': { color: colors.textDim, fontSize: font.chat } });
export const entryStatus = css({ '& .body': { color: colors.textMuted, fontSize: font.chat } });

// Extra top margin for consecutive entries inside the same turn: the thread
// gap (6px) + this margin (6px) = the 12px inner spacing Codex uses between
// blocks of a turn. A NEW turn (next user message) gets 6 + 16 = 22px.
export const entrySameTurn = css({ marginTop: 6 });
export const entryNewTurn = css({ marginTop: 16 });

// ---- reasoning disclosure (Codex: collapsed = a "思考" button only) ----
export const reasoningTitle = css({
  display: 'inline-flex',
  alignItems: 'center',
  gap: 4,
  fontSize: font.chat,
  lineHeight: font.chatLh,
  fontWeight: 445,
  color: colors.textTertiary,
  cursor: 'pointer',
  userSelect: 'none',
});

export const reasoningChevron = css({
  width: 16,
  height: 16,
  flexShrink: 0,
  color: colors.textTertiary,
  transition: 'transform 0.12s ease',
});

export const reasoningChevronOpen = css({
  transform: 'rotate(90deg)',
});

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
