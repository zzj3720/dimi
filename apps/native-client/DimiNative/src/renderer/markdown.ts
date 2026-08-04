// Markdown rendering for assistant messages — marked + DOMPurify.
import { marked } from 'marked';
import DOMPurify from 'dompurify';

marked.setOptions({
  gfm: true,
  breaks: false,
});

export function renderMarkdown(text: string): string {
  const raw = marked.parse(String(text ?? ''), { async: false }) as string;
  return DOMPurify.sanitize(raw, {
    USE_PROFILES: { html: true },
    ALLOWED_TAGS: [
      'p', 'br', 'strong', 'em', 's', 'u', 'code', 'pre', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
      'a', 'blockquote', 'hr', 'ul', 'ol', 'li', 'span', 'div',
    ],
    ALLOWED_ATTR: ['href', 'class', 'target', 'rel'],
  });
}
