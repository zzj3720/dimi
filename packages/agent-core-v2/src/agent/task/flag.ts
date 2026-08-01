import { type FlagDefinitionInput, registerFlagDefinition } from '#/app/flag/flagRegistry';

export const BACKGROUND_BASH_STDIN_FLAG_ID = 'background-bash-stdin';
export const BACKGROUND_BASH_STDIN_FLAG_ENV = 'KIMI_CODE_EXPERIMENTAL_BACKGROUND_BASH_STDIN';

registerFlagDefinition({
  id: BACKGROUND_BASH_STDIN_FLAG_ID,
  title: 'Background Bash stdin',
  description: 'Allow agents to keep a background Bash task stdin open and write to it later.',
  env: BACKGROUND_BASH_STDIN_FLAG_ENV,
  default: false,
  surface: 'core',
} satisfies FlagDefinitionInput);
