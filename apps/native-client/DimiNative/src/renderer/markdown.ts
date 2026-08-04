// Markdown rendering for assistant messages — marked + DOMPurify + KaTeX.
import { marked, Renderer, Tokens } from 'marked';
import DOMPurify from 'dompurify';
import katex from 'katex';
import { icons } from './icons';

marked.setOptions({
  gfm: true,
  breaks: false,
});

// Codex-aligned renderer tweaks:
// - links carry target="_blank" (the Electron main process routes
//   window.open / navigation to shell.openExternal, so they open in the
//   system browser);
// - tables are wrapped in `.md-table-container > .md-table-scroller >
//   .md-table-wrapper` (styled in styles/global.ts): the container bleeds
//   24px into the column padding, the scroller scrolls horizontally when
//   the table is wider than the column, and the wrapper keeps the table
//   pinned to the text edge (codex `._tableContainer` / `._tableScroller` /
//   `._tableWrapper`).
marked.use({
  renderer: {
    link(this: Renderer, { href, title, tokens }: Tokens.Link) {
      const text = this.parser.parseInline(tokens);
      const t = title ? ` title="${title}"` : '';
      return `<a href="${href}"${t} target="_blank">${text}</a>`;
    },
    table(this: Renderer, token: Tokens.Table) {
      const header = token.header.map((cell) => this.tablecell(cell)).join('');
      const rows = token.rows
        .map((row) => `<tr>${row.map((cell) => this.tablecell(cell)).join('')}</tr>`)
        .join('');
      return `<div class="md-table-container"><div class="md-table-scroller"><div class="md-table-wrapper"><table><thead><tr>${header}</tr></thead><tbody>${rows}</tbody></table></div></div></div>`;
    },
  },
});

// Code blocks get a copy button (codex `_codeBlockPlaceholder` actions).
// The button carries both glyphs (copy + check); the check is swapped in via
// the `.md-code-copied` class after a successful copy (see global.ts).
// `</pre>` cannot appear inside the escaped code text, so the post-processing
// wrap is safe.
const CODE_COPY_BUTTON =
  `<button type="button" class="md-code-copy" aria-label="复制代码" title="复制代码">` +
  `<svg class="md-code-copy-glyph" viewBox="${icons.copy.vb}" fill="currentColor" aria-hidden="true">` +
  icons.copy.paths.map((p) => `<path d="${p}"/>`).join('') +
  `</svg>` +
  `<svg class="md-code-copy-check" viewBox="${icons.check.vb}" fill="currentColor" aria-hidden="true">` +
  icons.check.paths.map((p) => `<path d="${p}"/>`).join('') +
  `</svg>` +
  `</button>`;

function wrapCodeBlocks(html: string): string {
  return html
    .replace(/<pre>/g, '<div class="md-code-block"><pre>')
    .replace(/<\/pre>/g, `</pre>${CODE_COPY_BUTTON}</div>`);
}

// Render inline `$...$` and block `$$...$$` LaTeX before markdown parsing so
// marked never mangles math tokens. The placeholders are unique hex markers
// that DOMPurify's ALLOWED_TAGS would otherwise strip, so we restore them
// AFTER sanitization.
const MATH_PLACEHOLDER = (i: number, block: boolean): string =>
  `\u0000KATEX${block ? 'B' : 'I'}${i}\u0000`;

interface MathSpan {
  html: string;
  placeholder: string;
}

function extractMath(text: string): { out: string; spans: MathSpan[] } {
  const spans: MathSpan[] = [];
  let out = '';
  let i = 0;
  let idx = 0;
  // block $$...$$ first, then inline $...$ (non-greedy, no space right after $)
  while (idx < text.length) {
    if (text.startsWith('$$', idx)) {
      const end = text.indexOf('$$', idx + 2);
      if (end > idx) {
        const raw = text.slice(idx + 2, end);
        let html = '';
        try {
          html = katex.renderToString(raw, { displayMode: true, throwOnError: false });
        } catch {
          html = raw;
        }
        const placeholder = MATH_PLACEHOLDER(i++, true);
        spans.push({ html, placeholder });
        out += placeholder;
        idx = end + 2;
        continue;
      }
    }
    if (text[idx] === '$') {
      const end = text.indexOf('$', idx + 1);
      if (end > idx) {
        const raw = text.slice(idx + 1, end);
        // `$ ` (space after $) is not math
        if (raw.length > 0 && !raw.startsWith(' ') && !raw.endsWith(' ')) {
          let html = '';
          try {
            html = katex.renderToString(raw, { displayMode: false, throwOnError: false });
          } catch {
            html = raw;
          }
          const placeholder = MATH_PLACEHOLDER(i++, false);
          spans.push({ html, placeholder });
          out += placeholder;
          idx = end + 1;
          continue;
        }
      }
    }
    out += text[idx];
    idx++;
  }
  return { out, spans };
}

function restoreMath(html: string, spans: MathSpan[]): string {
  let out = html;
  for (const s of spans) {
    out = out.split(s.placeholder).join(s.html);
  }
  return out;
}

export function renderMarkdown(text: string): string {
  const { out: withPlaceholders, spans } = extractMath(String(text ?? ''));
  const raw = marked.parse(withPlaceholders, { async: false }) as string;
  const wrapped = wrapCodeBlocks(raw);
  const sanitized = DOMPurify.sanitize(wrapped, {
    USE_PROFILES: { html: true },
    ALLOWED_TAGS: [
      'p', 'br', 'strong', 'em', 's', 'u', 'code', 'pre', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
      'a', 'blockquote', 'hr', 'ul', 'ol', 'li', 'span', 'div', 'button', 'svg', 'path',
    ],
    ALLOWED_ATTR: [
      'href', 'class', 'target', 'rel', 'title',
      'type', 'aria-label', 'aria-hidden',
      'viewBox', 'd', 'fill',
    ],
  });
  return restoreMath(sanitized, spans);
}
