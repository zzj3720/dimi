import { Container, type Component } from '@dimi-agent/pi-tui';

import { currentTheme } from '#/tui/theme';
import { hasDispose, isExpandable } from '#/tui/utils/component-capabilities';

import { AgentGroupComponent } from './agent-group';
import { ReadGroupComponent } from './read-group';
import { ThinkingComponent } from './thinking';
import { ToolCallComponent } from './tool-call';

export type ToolDisplayMode = 'summary' | 'tools' | 'full';

export function toolCallsIn(component: Component): readonly ToolCallComponent[] | undefined {
  if (component instanceof ToolCallComponent) return [component];
  if (component instanceof ReadGroupComponent || component instanceof AgentGroupComponent) {
    return component.getToolComponents();
  }
  // A previously folded sequence (e.g. the trailing tools of an earlier
  // notification-driven turn) is still a contiguous run of tool calls: expose
  // them so the collapse walk can merge across it instead of stopping.
  if (component instanceof ToolCallSequenceComponent) {
    return component.toolCalls;
  }
  return undefined;
}

/** A completed, contiguous run of tool calls that can be shown at three detail levels. */
export class ToolCallSequenceComponent extends Container {
  readonly toolCount: number;
  readonly thinkingCount: number;
  readonly toolCalls: readonly ToolCallComponent[];
  private mode: ToolDisplayMode;
  private readonly summary: string;

  constructor(
    components: readonly Component[],
    toolCalls: readonly ToolCallComponent[],
    mode: ToolDisplayMode,
  ) {
    super();
    for (const component of components) this.addChild(component);
    this.toolCount = toolCalls.length;
    this.thinkingCount = components.filter(
      (component) => component instanceof ThinkingComponent,
    ).length;
    this.toolCalls = toolCalls;
    this.summary = buildSummary(toolCalls);
    this.mode = mode;
    this.setDisplayMode(mode);
  }

  setDisplayMode(mode: ToolDisplayMode): void {
    this.mode = mode;
    for (const child of this.children) {
      if (child instanceof ThinkingComponent) child.setHidden(mode === 'summary');
      if (isExpandable(child)) child.setExpanded(mode === 'full');
    }
    this.invalidate();
  }

  override render(width: number): string[] {
    return this.mode === 'summary' ? [currentTheme.dim(`… ${this.summary}`)] : super.render(width);
  }

  dispose(): void {
    for (const child of this.children) {
      if (hasDispose(child)) child.dispose();
    }
  }
}

function buildSummary(toolCalls: readonly ToolCallComponent[]): string {
  const total = toolCalls.length;
  let reads = 0;
  let searches = 0;
  let agents = 0;
  for (const toolCall of toolCalls) {
    switch (toolCall.toolCallView.name) {
      case 'Read':
        reads += 1;
        break;
      case 'Glob':
      case 'Grep':
        searches += 1;
        break;
      case 'Agent':
      case 'AgentSwarm':
        agents += 1;
        break;
    }
  }

  const parts = [`Used ${String(total)} ${total === 1 ? 'tool' : 'tools'}`];
  if (reads > 0) parts.push(`read ${String(reads)} ${reads === 1 ? 'file' : 'files'}`);
  if (searches > 0) parts.push(`searched ${String(searches)} ${searches === 1 ? 'time' : 'times'}`);
  if (agents > 0) parts.push(`ran ${String(agents)} ${agents === 1 ? 'agent' : 'agents'}`);
  return parts.join(' · ');
}
