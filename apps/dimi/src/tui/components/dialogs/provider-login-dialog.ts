import type {
  AuthInteraction,
  ProviderAuthFacade,
  ProviderAuthPrompt,
} from '@dimi-agent/dimi-sdk';
import {
  Container,
  Input,
  Key,
  matchesKey,
  truncateToWidth,
  visibleWidth,
  wrapTextWithAnsi,
  type Focusable,
} from '@dimi-agent/pi-tui';

import { SELECT_POINTER } from '#/tui/constant/symbols';
import { currentTheme } from '#/tui/theme';
import { SearchableList } from '#/tui/utils/searchable-list';

type ProviderAuthEvent = Parameters<AuthInteraction['notify']>[0];
type SelectOption = Extract<ProviderAuthPrompt, { readonly type: 'select' }>['options'][number];

type PromptState =
  | {
      readonly kind: 'input';
      readonly prompt: Exclude<ProviderAuthPrompt, { readonly type: 'select' }>;
      readonly resolve: (value: string) => void;
      readonly reject: (error: Error) => void;
    }
  | {
      readonly kind: 'select';
      readonly prompt: Extract<ProviderAuthPrompt, { readonly type: 'select' }>;
      readonly list: SearchableList<SelectOption>;
      readonly resolve: (value: string) => void;
      readonly reject: (error: Error) => void;
    };

export interface ProviderLoginDialogOptions {
  readonly providerName: string;
  readonly methodLabel: string;
  readonly requestRender: () => void;
  readonly onCancel: () => void;
}

/**
 * One provider-owned authentication surface for OAuth, device-code, API-key,
 * and future prompt-driven flows. Provider events and prompts stay in this
 * dialog for the lifetime of one login attempt.
 */
export class ProviderLoginDialogComponent extends Container implements Focusable {
  private readonly options: ProviderLoginDialogOptions;
  private readonly input = new Input();
  private readonly details: string[] = [];
  private promptState: PromptState | undefined;
  private status = 'Starting authentication…';
  private done = false;
  private _focused = false;

  constructor(options: ProviderLoginDialogOptions) {
    super();
    this.options = options;
    this.input.onSubmit = (value) => {
      this.submitInput(value);
    };
  }

  get focused(): boolean {
    return this._focused;
  }

  set focused(value: boolean) {
    this._focused = value;
    this.input.focused = value && this.promptState?.kind === 'input';
  }

  notify(event: ProviderAuthEvent): void {
    switch (event.type) {
      case 'auth_url':
        this.pushDetail(event.instructions);
        this.pushDetail(`Open: ${event.url}`);
        this.status = 'Waiting for authorization…';
        break;
      case 'device_code':
        this.pushDetail(`Open: ${event.verificationUri}`);
        this.pushDetail(`Verification code: ${event.userCode}`);
        this.status = 'Waiting for authorization…';
        break;
      case 'info':
        this.pushDetail(event.message);
        for (const link of event.links ?? []) {
          this.pushDetail(link.label === undefined ? link.url : `${link.label}: ${link.url}`);
        }
        break;
      case 'progress':
        this.status = event.message;
        break;
    }
    this.options.requestRender();
  }

  prompt(prompt: ProviderAuthPrompt): Promise<string> {
    if (this.done) return Promise.reject(cancelledError());
    this.rejectActivePrompt(new Error('Authentication prompt was replaced.'));
    this.input.setValue('');
    this.status = 'Input required';
    return new Promise((resolve, reject) => {
      this.promptState =
        prompt.type === 'select'
          ? {
              kind: 'select',
              prompt,
              list: new SearchableList({
                items: prompt.options,
                toSearchText: (option) => `${option.label} ${option.description ?? ''}`.trim(),
                pageSize: 8,
                searchable: false,
              }),
              resolve,
              reject,
            }
          : { kind: 'input', prompt, resolve, reject };
      this.input.focused = this.focused && prompt.type !== 'select';
      this.options.requestRender();
    });
  }

  handleInput(data: string): void {
    if (this.done) return;
    if (
      matchesKey(data, Key.escape) ||
      matchesKey(data, Key.ctrl('c')) ||
      matchesKey(data, Key.ctrl('d'))
    ) {
      this.cancel();
      return;
    }

    const state = this.promptState;
    if (state?.kind === 'select') {
      if (matchesKey(data, Key.enter)) {
        const selected = state.list.selected();
        if (selected !== undefined) this.resolvePrompt(selected.id);
        return;
      }
      state.list.handleKey(data);
      this.options.requestRender();
      return;
    }
    if (state?.kind === 'input') {
      this.input.handleInput(data);
      this.options.requestRender();
    }
  }

  override invalidate(): void {
    super.invalidate();
    this.input.invalidate();
  }

  override render(width: number): string[] {
    const safeWidth = Math.max(0, width);
    if (safeWidth <= 0) return [''];
    const innerWidth = Math.max(1, safeWidth - 4);
    const content: string[] = [
      currentTheme.boldFg('textStrong', `Connect to ${this.options.providerName}`),
      currentTheme.fg('textMuted', this.options.methodLabel),
      '',
    ];

    for (const detail of this.details) {
      for (const line of wrapTextWithAnsi(detail, innerWidth)) {
        content.push(currentTheme.fg('text', line));
      }
    }
    if (this.details.length > 0) content.push('');
    content.push(currentTheme.fg('textDim', this.status));

    const state = this.promptState;
    if (state !== undefined) {
      content.push('', currentTheme.fg('textStrong', state.prompt.message));
      if ('placeholder' in state.prompt && state.prompt.placeholder !== undefined) {
        content.push(currentTheme.fg('textMuted', `e.g. ${state.prompt.placeholder}`));
      }
      if (state.kind === 'select') {
        const view = state.list.view();
        for (let index = view.page.start; index < view.page.end; index++) {
          const option = view.items[index]!;
          const selected = index === view.selectedIndex;
          const pointer = selected ? SELECT_POINTER : ' ';
          content.push(
            currentTheme.fg(selected ? 'primary' : 'textDim', ` ${pointer} `) +
              (selected
                ? currentTheme.boldFg('primary', option.label)
                : currentTheme.fg('text', option.label)),
          );
          if (option.description !== undefined) {
            content.push(currentTheme.fg('textMuted', `   ${option.description}`));
          }
        }
        content.push('', currentTheme.fg('textMuted', '↑↓ navigate · Enter select · Esc cancel'));
      } else {
        const rendered = this.input.render(innerWidth)[0] ?? '> ';
        content.push(
          state.prompt.type === 'secret' && this.input.getValue().length > 0
            ? maskInputLine(rendered)
            : rendered,
          '',
          currentTheme.fg('textMuted', 'Enter submit · Esc cancel'),
        );
      }
    } else {
      content.push('', currentTheme.fg('textMuted', 'Esc cancel'));
    }

    return renderBox(content, safeWidth, innerWidth);
  }

  private submitInput(value: string): void {
    const state = this.promptState;
    if (state?.kind !== 'input') return;
    const trimmed = value.trim();
    if (trimmed.length === 0) {
      this.status = 'A value is required.';
      this.options.requestRender();
      return;
    }
    this.resolvePrompt(trimmed);
  }

  private resolvePrompt(value: string): void {
    const state = this.promptState;
    if (state === undefined) return;
    this.promptState = undefined;
    this.input.focused = false;
    this.status = 'Continuing authentication…';
    state.resolve(value);
    this.options.requestRender();
  }

  private cancel(): void {
    if (this.done) return;
    this.done = true;
    this.rejectActivePrompt(cancelledError());
    this.options.onCancel();
  }

  private rejectActivePrompt(error: Error): void {
    const state = this.promptState;
    this.promptState = undefined;
    if (state !== undefined) state.reject(error);
  }

  private pushDetail(value: string | undefined): void {
    if (value === undefined || value.length === 0 || this.details.includes(value)) return;
    this.details.push(value);
  }
}

export type ProviderLoginInteraction = Parameters<ProviderAuthFacade['login']>[2];

function renderBox(content: readonly string[], width: number, innerWidth: number): string[] {
  if (width < 4) return content.map((line) => truncateToWidth(line, width, '…'));
  const border = (text: string): string => currentTheme.fg('primary', text);
  const lines = [
    '',
    border(`╭${'─'.repeat(width - 2)}╮`),
    border('│') + ' '.repeat(width - 2) + border('│'),
  ];
  for (const line of content) {
    const truncated = truncateToWidth(line, innerWidth, '…');
    lines.push(
      border('│') +
        '  ' +
        truncated +
        ' '.repeat(Math.max(0, innerWidth - visibleWidth(truncated))) +
        border('│'),
    );
  }
  lines.push(border('│') + ' '.repeat(width - 2) + border('│'));
  lines.push(border(`╰${'─'.repeat(width - 2)}╯`), '');
  return lines.map((line) => truncateToWidth(line, width, '…'));
}

function maskInputLine(raw: string): string {
  const prefix = '> ';
  if (!raw.startsWith(prefix)) return raw;
  let end = raw.length;
  while (end > prefix.length && raw[end - 1] === ' ') end--;
  const content = raw.slice(prefix.length, end);
  const padding = raw.slice(end);
  const parts = content.split(/(\u001B(?:\[[0-9;]*m|_pi:c\u0007))/);
  return (
    prefix +
    parts.map((part, index) => (index % 2 === 1 ? part : part.replaceAll(/./g, '•'))).join('') +
    padding
  );
}

function cancelledError(): Error {
  return new DOMException('Login cancelled', 'AbortError');
}
