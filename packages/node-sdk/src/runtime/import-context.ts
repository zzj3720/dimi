/**
 * Public SDK import-context construction, including validation and overflow
 * rejection.
 *
 * The runtime has no import-context capability of its
 * own (nothing under `agent-core-v2` builds this message), but all of its
 * primitives — `context.append_message`, the token estimator, and model
 * capabilities — are available, so the SDK composes the operation directly.
 */
import type { ContextMessage } from '@moonshot-ai/agent-core-v2';
import { estimateTokensForMessages } from '@moonshot-ai/agent-core-v2/kosong/contract/tokens';

import { ErrorCodes, KimiError } from '#/errors';

/** Guidance embedded in imported-context messages. */
const IMPORT_CONTEXT_GUIDANCE =
  'This is a prior conversation history that may be relevant to the current session. ' +
  'Please review this context and use it to inform your responses.';

/** Escape XML text content. */
function escapeXml(input: string): string {
  return input
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;');
}

/** Escape XML attribute content. */
function escapeXmlAttr(input: string): string {
  return input.replaceAll('&', '&amp;').replaceAll('"', '&quot;');
}

/**
 * The message appended for an import, including its rejections:
 * blank content (`import_content_empty`) and blank source
 * (`import_source_empty`) fail with v1's `request.invalid` shapes before any
 * token math runs.
 */
export function buildImportContextMessage(content: string, source: string): ContextMessage {
  if (content.trim().length === 0) {
    throw new KimiError(ErrorCodes.REQUEST_INVALID, 'Imported context cannot be empty', {
      details: { reason: 'import_content_empty' },
    });
  }
  const normalizedSource = source.trim();
  if (normalizedSource.length === 0) {
    throw new KimiError(ErrorCodes.REQUEST_INVALID, 'Imported context source cannot be empty', {
      details: { reason: 'import_source_empty' },
    });
  }
  return {
    role: 'user',
    content: [
      {
        type: 'text',
        text:
          `<system>The user has imported context from ${escapeXml(normalizedSource)}. ` +
          `${IMPORT_CONTEXT_GUIDANCE}</system>`,
      },
      {
        type: 'text',
        text:
          `<imported_context source="${escapeXmlAttr(normalizedSource)}">\n` +
          `${content}\n</imported_context>`,
      },
    ],
    toolCalls: [],
    origin: { kind: 'user' },
  };
}

/**
 * The import estimate plus the current context must fit
 * the model window (unknown window = `0` skips the check on both engines).
 * The estimator is the same character heuristic on both sides, so the counts
 * — and therefore the rejection — agree.
 */
export function assertImportFits(
  message: ContextMessage,
  currentTokenCount: number,
  maxContextTokens: number,
): void {
  const importTokenCount = estimateTokensForMessages([message]);
  const totalTokenCount = currentTokenCount + importTokenCount;
  if (maxContextTokens > 0 && totalTokenCount > maxContextTokens) {
    throw new KimiError(
      ErrorCodes.CONTEXT_OVERFLOW,
      'Imported content is too large for the current model context ' +
        `(~${String(importTokenCount)} import tokens + ~${String(currentTokenCount)} existing ` +
        `= ~${String(totalTokenCount)} total > ${String(maxContextTokens)} token limit). ` +
        'Please import a smaller file or session.',
      {
        details: {
          reason: 'import_context_overflow',
          importTokenCount,
          currentTokenCount,
          totalTokenCount,
          maxContextTokens,
        },
      },
    );
  }
}
