import { injectGlobal, css } from '@emotion/css';
import { colors, font } from './theme';

// ---- reset + base ----
injectGlobal`
  * { box-sizing: border-box; margin: 0; padding: 0; }
  html, body, .app, #app { height: 100%; }
  html { background: ${colors.bgUnder}; }
  body {
    font-family: ${font.family};
    font-size: 16px;
    line-height: 1.5;
    color: ${colors.text};
    background: ${colors.bgUnder};
    overflow: hidden;
    -webkit-font-smoothing: antialiased;
  }
`;

export const app = css({ height: '100%' });
export const hidden = css({ display: 'none !important' });
export const spacer = css({ flex: 1 });

// ---- markdown (assistant messages) ----
export const md = css({
  whiteSpace: 'normal',
  fontSize: font.chat,
  lineHeight: font.chatLh,
  '& p': { margin: '0 0 8px' },
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
  '& h1, & h2, & h3, & h4, & h5, & h6': {
    color: colors.text,
    fontWeight: 600,
    fontSize: '1em',
    margin: '10px 0 4px',
  },
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
  '& li': { margin: '2px 0' },
});
