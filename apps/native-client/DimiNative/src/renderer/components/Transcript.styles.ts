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
  // py-5: top 20px, bottom 32px (thread) + 20px (msgList) = 52px
  padding: '20px 16px 52px',
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
  color: colors.textDim,
  fontSize: font.chat,
  lineHeight: font.chatLh,
  whiteSpace: 'pre-wrap',
  wordBreak: 'break-word',
});
export const bodyCompaction = css({ color: colors.textMuted, fontSize: font.chat });
export const toolName = css({ color: colors.textDim, fontWeight: 500 });
export const clickable = css({ cursor: 'pointer' });

export const entryUser = css({ '& .body': { color: colors.text, lineHeight: '21px' } });
export const entryAssistant = css({ '& .body': { color: colors.text } });
export const entryThinking = css({ '& .body': { color: colors.textDim, fontSize: font.chat } });
export const entryTool = css({ '& .body': { color: colors.textDim, fontSize: font.chat } });
export const entryStatus = css({ '& .body': { color: colors.textMuted, fontSize: font.chat } });

// Extra top margin for consecutive entries inside the same turn: the thread
// gap (6px) + this margin (6px) = the 12px inner spacing Codex uses between
// blocks of a turn. Turn boundaries (a new user entry) keep the plain 6px gap.
export const entrySameTurn = css({ marginTop: 6 });

// ---- reasoning disclosure ----
export const reasoningTitle = css({
  display: 'flex',
  alignItems: 'center',
  gap: 2,
  fontSize: font.xs,
  lineHeight: font.xsLh,
  fontWeight: 445,
  color: colors.textTertiary,
  marginBottom: 2,
  cursor: 'pointer',
  userSelect: 'none',
  '&:hover': { color: colors.textDim },
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
