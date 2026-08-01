/** `completion` domain (L4) — intentional Agent completion protocol constants. */

import COMPLETION_REVIEW_REMINDER from './completion-review.md?raw';

export const ALL_DONE_TOOL_NAME = 'AllDone';
export { COMPLETION_REVIEW_REMINDER };

/**
 * Turns with fewer steps than this end naturally on a text-only reply: the
 * completion reminder (and the forced AllDone review) only kicks in once a
 * turn has run this many steps, so quick answers finish without the ceremony.
 */
export const COMPLETION_REVIEW_MIN_STEPS = 10;
