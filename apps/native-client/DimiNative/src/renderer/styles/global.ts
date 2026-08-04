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
  '& p': { margin: '0 0 8px', lineHeight: font.chatLh },
  '& strong': { fontWeight: 600 },
  '& em': { fontStyle: 'italic' },
  '& s': { textDecoration: 'line-through' },
  '& u': { textDecoration: 'underline' },
  '& code': {
    color: colors.text,
    background: colors.hover,
    borderRadius: 4,
    padding: '1px 4px',
    fontFamily: font.mono,
    fontSize: '0.9em',
  },
  '& pre': {
    background: 'rgba(255, 255, 255, 0.04)',
    border: `1px solid ${colors.border}`,
    borderRadius: 8,
    padding: '10px 12px',
    margin: '8px 0',
    overflowX: 'auto',
  },
  '& pre code': {
    background: 'transparent',
    padding: 0,
    color: colors.text,
    fontSize: '0.9em',
    whiteSpace: 'pre-wrap',
    wordBreak: 'break-word',
  },
  '& h1': { color: colors.text, fontWeight: 600, fontSize: '24px', lineHeight: '30px', margin: '24px 0 8px' },
  '& h2': { color: colors.text, fontWeight: 600, fontSize: '20px', lineHeight: '25px', margin: '20px 0 8px' },
  '& h3': { color: colors.text, fontWeight: 600, fontSize: '17px', lineHeight: '22px', margin: '16px 0 6px' },
  '& h4, & h5, & h6': { color: colors.text, fontWeight: 600, fontSize: '15px', lineHeight: '20px', margin: '12px 0 4px' },
  '& a': { color: colors.primary, textDecoration: 'none' },
  '& a:hover': { textDecoration: 'underline' },
  '& blockquote': {
    color: colors.textDim,
    margin: '6px 0',
    borderLeft: `2px solid ${colors.borderHeavy}`,
    paddingLeft: 10,
  },
  '& hr': { border: 'none', borderTop: `1px solid ${colors.border}`, margin: '8px 0' },
  '& ul, & ol': { margin: '4px 0', paddingLeft: '1.5em' },
  '& li': { margin: '2px 0', lineHeight: font.chatLh },
  // Codex-style tables
  '& table': {
    borderCollapse: 'collapse',
    margin: '8px 0',
    width: '100%',
    fontSize: font.chat,
    lineHeight: font.chatLh,
  },
  '& thead': { borderBottom: `1px solid ${colors.borderHeavy}` },
  '& th': {
    textAlign: 'left',
    fontWeight: 600,
    padding: '6px 10px',
    borderBottom: `1px solid ${colors.border}`,
    whiteSpace: 'nowrap',
  },
  '& td': { padding: '6px 10px', borderBottom: `1px solid ${colors.border}`, verticalAlign: 'top' },
  '& tr:last-child td': { borderBottom: 'none' },
});
