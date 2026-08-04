<script setup lang="ts">
// Codex-style thread: welcome page when empty, else 768px centered messages.
import { ref, watch, nextTick, onMounted } from 'vue';
import { state, Msg } from '../store';
import type { Entry } from '../store';
import { dispatch } from '../api';
import { renderMarkdown } from '../markdown';
import {
  transcript, thread, entry, entrySameTurn, bodyMuted, bodyTool, bodyThinking, bodyCompaction,
  toolName, clickable, entryUser, entryAssistant, entryThinking, entryTool, entryStatus,
  reasoningTitle, welcome, welcomeH1, suggestions, suggestionCard, welcomeModels,
  welcomeModelsTitle, modelRow, modelName, modelLevel, md,
} from './Transcript.styles';

const scroller = ref<HTMLElement | null>(null);
const expandedThinking = ref<Set<Entry>>(new Set());
const expandedTools = ref<Set<string>>(new Set());

// Auto-follow only when already near the bottom.
function scrollToBottom(): void {
  const el = scroller.value;
  if (!el) return;
  const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  if (nearBottom) el.scrollTop = el.scrollHeight;
}

onMounted(() => void nextTick(scrollToBottom));

watch(
  () => state.entries.length,
  () => void nextTick(scrollToBottom),
);
watch(
  () => state.entries[state.entries.length - 1]?.text,
  () => void nextTick(scrollToBottom),
);
// Whole-array replacement (e.g. switching sessions) can keep length + last
// text identical; watching the array reference catches those too.
watch(
  () => state.entries,
  () => void nextTick(scrollToBottom),
);

// Same-turn grouping: a new turn starts at every user message; thinking/tool/
// assistant entries that follow (until the next user) belong to that turn.
// Status/compaction rows are meta and never join a group.
function isSameTurn(prev: Entry | undefined, e: Entry): boolean {
  if (!prev) return false;
  if (e.kind === 'user' || e.kind === 'status' || e.kind === 'compaction') return false;
  if (prev.kind === 'status' || prev.kind === 'compaction') return false;
  return true;
}

function toggleThinking(e: Entry): void {
  if (window.getSelection && window.getSelection().toString().length > 0) return;
  const s = expandedThinking.value;
  if (s.has(e)) s.delete(e);
  else s.add(e);
  expandedThinking.value = new Set(s);
}

function toggleTool(id: string): void {
  if (window.getSelection && window.getSelection().toString().length > 0) return;
  const s = expandedTools.value;
  if (s.has(id)) s.delete(id);
  else s.add(id);
  expandedTools.value = new Set(s);
}

const SUGGESTIONS = ['创建文件或搭建网站', '调研并规划后续步骤', '自动处理日常和重复性工作'];

function suggest(text: string): void {
  window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'suggestion_send', text } }));
}

function thinkingPreview(e: Entry): { text: string; hint: string | null } {
  const lines = String(e.text ?? '').split('\n');
  if (expandedThinking.value.has(e) || lines.length <= 2) return { text: e.text, hint: null };
  return {
    text: lines.slice(0, 2).join('\n'),
    hint: `... (${lines.length - 2} more lines, click to expand)`,
  };
}

function shellCmd(args: string): string {
  return '$ ' + args;
}
</script>

<template>
  <main ref="scroller" :class="transcript" tabindex="-1">
    <!-- Welcome page (Codex-style) -->
    <div v-if="state.entries.length === 0" :class="welcome">
      <h1 :class="welcomeH1">我们该处理什么工作？</h1>
      <div :class="suggestions">
        <div v-for="s in SUGGESTIONS" :key="s" :class="suggestionCard" @click="suggest(s)">{{ s }}</div>
      </div>
      <div :class="welcomeModels">
        <div :class="welcomeModelsTitle">模型</div>
        <div :class="modelRow" @click="dispatch(Msg.SettingsOpen())"><span :class="modelName">{{ state.modelName || '模型' }}</span><span :class="modelLevel">轻度</span></div>
        <div :class="modelRow"><span :class="modelName">5.6 Sol</span><span :class="modelLevel">极高</span></div>
      </div>
    </div>

    <!-- Message thread -->
    <div v-else :class="thread">
      <div
        v-for="(e, i) in state.entries"
        :key="e"
        :class="[
          entry,
          i > 0 && isSameTurn(state.entries[i - 1], e) ? entrySameTurn : null,
          e.kind === 'user' ? entryUser : e.kind === 'assistant' ? entryAssistant : e.kind === 'thinking' ? entryThinking : e.kind === 'tool' ? entryTool : entryStatus,
        ]"
      >
        <!-- user -->
        <div v-if="e.kind === 'user'" class="body">{{ e.text }}</div>

        <!-- assistant: markdown -->
        <div v-else-if="e.kind === 'assistant'" class="body md" :class="md" v-html="renderMarkdown(e.text)"></div>

        <!-- thinking: reasoning disclosure, collapsible -->
        <div v-else-if="e.kind === 'thinking'" :class="clickable" @click="toggleThinking(e)">
          <div :class="reasoningTitle">思考</div>
          <div :class="bodyThinking">{{ thinkingPreview(e).text }}</div>
          <div v-if="thinkingPreview(e).hint" :class="bodyMuted">{{ thinkingPreview(e).hint }}</div>
        </div>

        <!-- tool -->
        <div v-else-if="e.kind === 'tool'" :class="clickable" @click="toggleTool(e.toolCallId)">
          <div :class="bodyTool">
            <span :class="toolName">{{ e.text && e.text.length > 0 ? 'Used' : 'Using' }} {{ e.toolName }}</span>
          </div>
          <div v-if="e.args" :class="bodyTool" style="font-family: ui-monospace, 'SF Mono', Menlo, monospace">{{ shellCmd(e.args) }}</div>
          <div
            v-if="e.text && e.text.length > 0"
            :class="bodyTool"
            :style="{ maxHeight: expandedTools.has(e.toolCallId) ? 'none' : '4.2em', overflow: 'hidden', fontFamily: 'ui-monospace, SF Mono, Menlo, monospace' }"
          >{{ e.text }}</div>
        </div>

        <!-- status / compaction -->
        <div v-else :class="bodyMuted">{{ e.text }}</div>
      </div>
    </div>
  </main>
</template>
