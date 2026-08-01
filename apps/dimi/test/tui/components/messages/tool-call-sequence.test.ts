import { describe, expect, it, vi } from 'vitest';

import { ToolCallComponent } from '#/tui/components/messages/tool-call';
import { ToolCallSequenceComponent } from '#/tui/components/messages/tool-call-sequence';

function tool(id: string, name: string): ToolCallComponent {
  return new ToolCallComponent(
    { id, name, args: name === 'Read' ? { path: `${id}.ts` } : {} },
    { tool_call_id: id, output: 'done' },
    undefined,
    '/tmp',
  );
}

describe('ToolCallSequenceComponent', () => {
  it('summarizes a contiguous tool run and preserves the original components', () => {
    const read = tool('read-1', 'Read');
    const grep = tool('grep-1', 'Grep');
    const bash = tool('bash-1', 'Bash');
    const readExpansion = vi.spyOn(read, 'setExpanded');
    const sequence = new ToolCallSequenceComponent(
      [read, grep, bash],
      [read, grep, bash],
      'summary',
    );

    expect(sequence.toolCount).toBe(3);
    expect(sequence.render(100).join('\n')).toContain(
      'Used 3 tools · read 1 file · searched 1 time',
    );

    sequence.setDisplayMode('tools');
    expect(sequence.children).toEqual([read, grep, bash]);
    expect(sequence.render(100).join('\n')).toContain('Used Read');
    expect(readExpansion).toHaveBeenLastCalledWith(false);

    sequence.setDisplayMode('full');
    expect(readExpansion).toHaveBeenLastCalledWith(true);
  });
});
