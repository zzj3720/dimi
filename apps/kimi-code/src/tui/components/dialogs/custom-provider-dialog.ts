import type { CustomProviderInput } from '@moonshot-ai/kimi-code-sdk';
import {
  Container,
  Input,
  Key,
  matchesKey,
  truncateToWidth,
  visibleWidth,
  type Focusable,
} from '@moonshot-ai/pi-tui';

import { currentTheme } from '#/tui/theme';

const FIELDS = [
  ['Provider id', 'example-provider'],
  ['Display name', 'Example Provider'],
  ['Base URL', 'https://api.example.test/v1'],
  ['Protocol adapter', 'openai-completions'],
  ['Model id', 'example-chat'],
  ['Context window', '128000'],
  ['Max output tokens', '8192'],
  ['Input modalities (comma separated)', 'text,image'],
  ['Thinking (off, always, or levels)', 'off'],
] as const;
const INPUTS = new Set(['text', 'image']);

export type CustomProviderDialogResult =
  | { readonly kind: 'ok'; readonly provider: CustomProviderInput }
  | { readonly kind: 'cancel' };

export class CustomProviderDialogComponent extends Container implements Focusable {
  focused = false;
  private readonly input = new Input();
  private readonly values = Array.from({ length: FIELDS.length }, () => '');
  private index = 0;
  private done = false;
  private message: string | undefined;

  constructor(
    private readonly apis: readonly string[],
    private readonly onDone: (result: CustomProviderDialogResult) => void,
  ) {
    super();
    this.input.onSubmit = (value) => this.submit(value);
  }

  handleInput(data: string): void {
    if (this.done) return;
    if (
      matchesKey(data, Key.escape) ||
      matchesKey(data, Key.ctrl('c')) ||
      matchesKey(data, Key.ctrl('d'))
    ) {
      this.done = true;
      this.onDone({ kind: 'cancel' });
      return;
    }
    if (matchesKey(data, Key.tab) || matchesKey(data, Key.down)) {
      this.advance(1);
      return;
    }
    if (matchesKey(data, Key.shift(Key.tab)) || matchesKey(data, Key.up)) {
      this.advance(-1);
      return;
    }
    this.input.handleInput(data);
  }

  override invalidate(): void {
    super.invalidate();
    this.input.invalidate();
  }

  override render(width: number): string[] {
    this.input.focused = this.focused && !this.done;
    const safeWidth = Math.max(0, width);
    if (safeWidth < 4) return [''];
    const innerWidth = safeWidth - 4;
    const content = [
      currentTheme.boldFg('textStrong', 'Add custom provider'),
      currentTheme.fg('textMuted', 'Tab/↑↓ navigate · Enter next · Esc cancel'),
      '',
      ...FIELDS.flatMap(([label, placeholder], index) => {
        const active = index === this.index;
        const value = active ? this.input.render(innerWidth)[0] ?? '> ' : this.values[index];
        return [
          currentTheme.fg(active ? 'primary' : 'textMuted', `${label}${active ? ' *' : ''}`),
          currentTheme.fg(active ? 'text' : 'textMuted', value || `  ${placeholder}`),
        ];
      }),
      ...(this.message === undefined ? [] : ['', currentTheme.fg('error', this.message)]),
      '',
      currentTheme.fg('textMuted', this.index === FIELDS.length - 1 ? 'Enter submit' : 'Enter next'),
    ];
    const border = (value: string) => currentTheme.fg('primary', value);
    const lines = [border(`╭${'─'.repeat(safeWidth - 2)}╮`)];
    for (const line of content) {
      const clipped = truncateToWidth(line, innerWidth, '…');
      lines.push(
        border('│') +
          '  ' +
          clipped +
          ' '.repeat(Math.max(0, innerWidth - visibleWidth(clipped))) +
          border('│'),
      );
    }
    lines.push(border(`╰${'─'.repeat(safeWidth - 2)}╯`));
    return lines;
  }

  private submit(value: string): void {
    const trimmed = value.trim();
    if (trimmed.length === 0) {
      this.message = `${FIELDS[this.index]![0]} is required.`;
      return;
    }
    this.values[this.index] = trimmed;
    this.message = undefined;
    if (this.index < FIELDS.length - 1) {
      this.advance(1);
      return;
    }
    const contextWindow = Number(this.values[5]);
    const maxTokens = Number(this.values[6]);
    if (!Number.isInteger(contextWindow) || contextWindow < 1 || !Number.isInteger(maxTokens) || maxTokens < 1) {
      this.message = 'Context window and max output tokens must be positive integers.';
      return;
    }
    const api = this.values[3]!;
    if (!this.apis.includes(api)) {
      this.message = `Protocol adapter must be one of: ${this.apis.join(', ')}.`;
      return;
    }
    const input = this.values[7]!
      .split(',')
      .map((value) => value.trim())
      .filter((value) => value.length > 0);
    if (input.length === 0 || input.some((value) => !INPUTS.has(value))) {
      this.message = 'Input modalities must be text or image.';
      return;
    }
    const thinking = this.values[8]!.trim().toLowerCase();
    const alwaysThinking = thinking === 'always';
    const thinkingLevels = thinking === 'off' || alwaysThinking ? [] : thinking.split(',').map((value) => value.trim()).filter(Boolean);
    if (thinking !== 'off' && !alwaysThinking && thinkingLevels.length === 0) {
      this.message = 'Thinking must be off, always, or comma-separated levels.';
      return;
    }
    this.done = true;
    this.onDone({
      kind: 'ok',
      provider: {
        id: this.values[0]!,
        name: this.values[1]!,
        baseUrl: this.values[2]!,
        api,
        models: [
          {
            id: this.values[4]!,
            name: this.values[4]!,
            reasoning: thinking !== 'off',
            input: input as ('text' | 'image')[],
            contextWindow,
            maxTokens,
            thinkingLevelMap:
              thinking === 'off'
                ? undefined
                : Object.fromEntries([
                    ...(alwaysThinking ? [['off', null] as const] : []),
                    ...thinkingLevels.map((level) => [level, level] as const),
                  ]),
          },
        ],
      },
    });
  }

  private advance(delta: number): void {
    this.values[this.index] = this.input.getValue().trim();
    this.index = Math.max(0, Math.min(FIELDS.length - 1, this.index + delta));
    this.input.setValue(this.values[this.index]!);
    this.message = undefined;
  }
}
