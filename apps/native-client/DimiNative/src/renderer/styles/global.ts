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

// Screen-reader-only utility (codex turns carry an `h4.sr-only` role
// heading — "你说：" / assistant role — see design doc §1.2 / gap 8.2-13).
export const srOnly = css({
  position: 'absolute',
  width: 1,
  height: 1,
  padding: 0,
  margin: -1,
  overflow: 'hidden',
  clip: 'rect(0 0 0 0)',
  clipPath: 'inset(50%)',
  whiteSpace: 'nowrap',
  border: 0,
});

// Codex markdown line-height is `calc(font-size + 8px)` = 22px at the 14px
// chat font (design doc §6.1). dimi's theme token `font.chatLh` is 21px (a
// documented deviation kept in theme.ts, which is read-only for this task);
// thread markdown follows the measured codex value.
const MD_LH = '22px';

// ---- markdown (assistant messages, Codex-style rich rendering) ----
export const md = css({
  whiteSpace: 'normal',
  fontSize: font.chat,
  lineHeight: MD_LH,
  color: colors.text,
  overflowWrap: 'anywhere',
  // Codex zeroes the container's first/last child margins.
  '& > :first-child': { marginTop: 0 },
  '& > :last-child': { marginBottom: 0 },
  '& p': { margin: '0 0 11px', lineHeight: MD_LH },
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
  '& li': { paddingLeft: '0.125rem', lineHeight: MD_LH },
  '& li + li': { marginTop: 8 },
  '& li > p': { margin: 0 },
  '& li > p + p': { marginTop: 11 },
  '& li > ul, & li > ol': { marginTop: 8 },
  '& p:has(+ ul), & p:has(+ ol)': { marginBottom: 10 },
  '& ul:not(:last-child), & ol:not(:last-child)': { marginBottom: 10 },
  // Code blocks: a copy button floats over the block's top-right corner,
  // revealed on hover (codex `_codeBlockPlaceholder` actions). The button
  // sits OUTSIDE the scrollable <pre> so it stays put when wide code
  // scrolls; `.md-code-copied` swaps the glyph for a check.
  '& .md-code-block': {
    position: 'relative',
    margin: '14px 0', // pre's margin moves to the wrapper so the button aligns with the block corner
  },
  '& .md-code-block pre': { margin: 0 },
  '& .md-code-copy': {
    position: 'absolute',
    top: 8,
    right: 8,
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
    opacity: 0,
    transition: 'opacity 0.12s ease',
    '&:hover': { background: 'rgba(255, 255, 255, 0.078)' }, // list-hover-background
    '&:active': { background: 'rgba(255, 255, 255, 0.15)' },
    '&:focus-visible': { outline: 'none', boxShadow: '0 0 0 2px rgba(131, 195, 255, 0.76)', opacity: 1 },
    '& svg': { width: 16, height: 16 },
    '& .md-code-copy-check': { display: 'none' },
    '&.md-code-copied .md-code-copy-glyph': { display: 'none' },
    '&.md-code-copied .md-code-copy-check': { display: 'block' },
  },
  '& .md-code-block:hover .md-code-copy': { opacity: 1 },
  // Tables: roomy cells, header underline + row separators, 24px last-row
  // bottom padding. markdown.ts renders
  //   .md-table-container (bleed) > .md-table-scroller > .md-table-wrapper > table
  // (codex `._tableContainer` / `._tableScroller` / `._tableWrapper`): the
  // container bleeds 24px into the column padding; the scroller scrolls
  // horizontally when the table is wider than the column (`safe center`
  // keeps narrow tables centered without hiding the overflow head); the
  // wrapper spans the text column (calc(100% − 48px) inside the bleed) and
  // the table fills it up to fit-content.
  '& .md-table-container': {
    width: 'calc(100% + 48px)',
    marginInline: '-24px',
  },
  '& .md-table-scroller': {
    display: 'flex',
    justifyContent: 'safe center',
    overflowX: 'auto',
    scrollbarWidth: 'thin',
  },
  '& .md-table-wrapper': {
    width: 'calc(100% - 48px)',
    marginInline: '24px',
    flexShrink: 0,
  },
  '& table': {
    borderCollapse: 'separate',
    borderSpacing: 0,
    textAlign: 'start',
    margin: 0,
    fontSize: font.chat,
    lineHeight: MD_LH,
    width: 'fit-content',
    minWidth: '100%',
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
