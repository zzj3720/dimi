import { injectGlobal, css } from '@emotion/css';
import { colors, font } from './theme';

// ---- reset + base ----
injectGlobal`
  * { box-sizing: border-box; margin: 0; padding: 0; }
  html, body, .app, #app { height: 100%; }
  html { background: var(--bg, #141414); }
  body {
    font-family: ${font.family};
    font-size: 16px;
    line-height: 1.5;
    font-weight: 445; /* Codex variable-font default */
    color: var(--text, #ffffff);
    background: var(--bg, #141414);
    overflow: hidden;
    -webkit-font-smoothing: antialiased;
  }
`;

export const app = css({ height: '100%' });
export const hidden = css({ display: 'none !important' });
export const spacer = css({ flex: 1 });

// ---- markdown (assistant messages, Codex-style rich rendering) ----
export const md = css({
  whiteSpace: 'normal',
  fontSize: font.chat,
  lineHeight: font.chatLh,
  color: colors.text,
  overflowWrap: 'anywhere',
  // Codex zeroes the container's first/last child margins.
  '& > :first-child': { marginTop: 0 },
  '& > :last-child': { marginBottom: 0 },
  '& p': { margin: '0 0 11px', lineHeight: font.chatLh },
  '& strong': { fontWeight: 600 },
  '& em': { fontStyle: 'italic' },
  '& s': { textDecoration: 'line-through' },
  '& u': { textDecoration: 'underline' },
  // Codex: links are plain white, no underline, default cursor (no pointer).
  '& a': { color: '#ffffff', textDecoration: 'none', cursor: 'default' },
  '& code': {
    color: colors.text,
    background: 'rgba(255, 255, 255, 0.16)',
    borderRadius: 6,
    padding: '1px 6px',
    fontFamily: font.mono,
    fontSize: '0.92em',
    wordBreak: 'break-word',
  },
  '& pre': {
    background: 'rgba(255, 255, 255, 0.052)',
    border: 'none',
    borderRadius: 12.5,
    padding: 8,
    margin: '14px 0',
    fontSize: '12px',
    lineHeight: '20px',
    overflowX: 'auto',
  },
  '& pre code': {
    background: 'transparent',
    padding: 0,
    color: colors.text,
    fontSize: 'inherit',
    whiteSpace: 'pre',
  },
  '& h1': { color: colors.text, fontWeight: 600, fontSize: '24px', lineHeight: '30px', margin: '20px 0 10px' },
  '& h2': { color: colors.text, fontWeight: 600, fontSize: '20px', lineHeight: '25px', margin: '20px 0 10px' },
  '& h3': { color: colors.text, fontWeight: 600, fontSize: '17px', lineHeight: '22px', margin: '20px 0 10px' },
  '& h4, & h5, & h6': { color: colors.text, fontWeight: 600, fontSize: '15px', lineHeight: '20px', margin: '20px 0 10px' },
  // Codex blockquote: white text, 4px rounded vertical bar (::after) inset 8px,
  // padding-block 8 + padding-left 24, margin-bottom 8.
  '& blockquote': {
    position: 'relative',
    color: colors.text,
    margin: '0 0 8px',
    padding: '8px 0 8px 24px',
    lineHeight: '24px',
    fontWeight: 400,
    border: 0,
    '&::after': {
      content: '""',
      position: 'absolute',
      left: 0,
      top: '8px',
      bottom: '8px',
      width: 4,
      borderRadius: 2,
      background: 'rgba(255, 255, 255, 0.157)',
    },
  },
  '& hr': { border: 'none', borderTop: '1px solid rgba(255, 255, 255, 0.156)', margin: '28px 0' },
  // Lists: flush margins, 21px indent, 8px between items, markers inherit #fff.
  '& ul, & ol': { margin: 0, paddingLeft: '1.3125rem', listStylePosition: 'outside' },
  '& li': { paddingLeft: '0.125rem', lineHeight: font.chatLh },
  '& li + li': { marginTop: 8 },
  '& li > p': { margin: 0 },
  '& li > p + p': { marginTop: 11 },
  '& li > ul, & li > ol': { marginTop: 8 },
  '& p:has(+ ul), & p:has(+ ol)': { marginBottom: 10 },
  '& ul:not(:last-child), & ol:not(:last-child)': { marginBottom: 10 },
  // Tables: roomy cells, header underline + row separators, 24px last-row
  // bottom padding. The table renders inside `.md-table-container` (custom
  // marked renderer in markdown.ts) which bleeds 24px into the column padding
  // (codex `._tableContainer`); the table itself stays fit-content.
  '& .md-table-container': {
    width: 'calc(100% + 48px)',
    marginInline: '-24px',
  },
  '& table': {
    borderCollapse: 'separate',
    borderSpacing: 0,
    textAlign: 'start',
    margin: 0,
    fontSize: font.chat,
    lineHeight: font.chatLh,
    width: 'fit-content',
  },
  '& th': {
    textAlign: 'left',
    fontWeight: 600,
    fontSize: '14px',
    lineHeight: '16px',
    padding: '8px 24px 8px 0',
    borderBottom: '1px solid rgba(255, 255, 255, 0.157)',
    color: colors.text,
    verticalAlign: 'top',
  },
  '& th:last-child': { paddingRight: 40 }, // Codex header column spacing
  '& td': {
    padding: '10px 24px 10px 0',
    verticalAlign: 'top',
    fontSize: '14px',
    color: colors.text,
  },
  '& td:last-child': { paddingRight: 0 },
  '& tbody tr:not(:last-child) td': { borderBottom: '1px solid rgba(255, 255, 255, 0.042)' },
  '& tbody tr:last-child td': { paddingBottom: 24 },
  // KaTeX: display math centered with 14px vertical margins, 1.21× glyphs.
  '& .katex-display': { margin: '14px 0', textAlign: 'center' },
  '& .katex': { fontSize: '16.94px', lineHeight: '20.328px' },
});
