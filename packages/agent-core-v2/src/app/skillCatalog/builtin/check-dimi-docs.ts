/**
 * `skillCatalog` domain (L3) — builtin `check-dimi-docs` skill definition.
 */

import type { SkillDefinition } from '#/app/skillCatalog/types';
import { parseSkillText } from '#/app/skillCatalog/parser';
import CHECK_KIMI_CODE_DOCS_BODY from './check-dimi-docs.md?raw';

const PSEUDO_PATH = 'builtin://check-dimi-docs';

const parsed = parseSkillText({
  skillMdPath: '/builtin/skills/check-dimi-docs.md',
  skillDirName: 'check-dimi-docs',
  source: 'builtin',
  text: CHECK_KIMI_CODE_DOCS_BODY,
});

export const CHECK_DIMI_DOCS_SKILL: SkillDefinition = {
  ...parsed,
  path: PSEUDO_PATH,
  dir: PSEUDO_PATH,
  metadata: {
    ...parsed.metadata,
    type: parsed.metadata.type ?? 'inline',
  },
};
