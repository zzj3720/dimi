import { describe, expect, it, vi } from 'vitest';

import { PermissionSelectorComponent } from '#/tui/components/dialogs/permission-selector';

const ANSI = /\[[0-9;]*m/g;
const strip = (s: string): string => s.replaceAll(ANSI, '');
const ESC = String.fromCodePoint(27);

function text(component: PermissionSelectorComponent, width = 120): string {
  return component.render(width).map(strip).join('\n');
}

describe('PermissionSelectorComponent', () => {
  it('renders the three permission modes with the active one marked', () => {
    const picker = new PermissionSelectorComponent({
      currentValue: 'yolo',
      onSelect: vi.fn(),
      onCancel: vi.fn(),
    });
    const out = text(picker);
    expect(out).toContain('Manual');
    expect(out).toContain('YOLO');
    expect(out).toContain('Auto');
    expect(out).toContain('← current');
    const yoloLine = out.split('\n').find((line) => line.includes('YOLO'));
    expect(yoloLine).toContain('← current');
  });

  it('invokes onSelect with the chosen mode on Enter', () => {
    const onSelect = vi.fn();
    const picker = new PermissionSelectorComponent({
      currentValue: 'manual',
      onSelect,
      onCancel: vi.fn(),
    });
    picker.handleInput('\r');
    expect(onSelect).toHaveBeenCalledWith('manual');
  });

  it('invokes onSessionOnlySelect on Alt+S instead of onSelect', () => {
    const onSelect = vi.fn();
    const onSessionOnlySelect = vi.fn();
    const picker = new PermissionSelectorComponent({
      currentValue: 'manual',
      onSelect,
      onSessionOnlySelect,
      onCancel: vi.fn(),
    });
    picker.handleInput(`${ESC}s`);
    expect(onSessionOnlySelect).toHaveBeenCalledWith('manual');
    expect(onSelect).not.toHaveBeenCalled();
  });

  it('advertises the Alt+S session-only key in the hint when supported', () => {
    const picker = new PermissionSelectorComponent({
      currentValue: 'manual',
      onSelect: vi.fn(),
      onSessionOnlySelect: vi.fn(),
      onCancel: vi.fn(),
    });
    expect(text(picker)).toContain('Alt+S session-only');
  });

  it('omits the Alt+S hint when session-only is not supported', () => {
    const picker = new PermissionSelectorComponent({
      currentValue: 'manual',
      onSelect: vi.fn(),
      onCancel: vi.fn(),
    });
    expect(text(picker)).not.toContain('Alt+S session-only');
  });

  it('cancels on Escape', () => {
    const onCancel = vi.fn();
    const picker = new PermissionSelectorComponent({
      currentValue: 'manual',
      onSelect: vi.fn(),
      onCancel,
    });
    picker.handleInput(ESC);
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});
