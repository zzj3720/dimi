<script setup lang="ts">
// Codex-style thread: welcome page when empty, else 768px centered messages.
import { ref, watch, nextTick, onMounted } from 'vue';
import { state, Msg } from '../store';
import type { Entry } from '../store';
import { dispatch } from '../api';
import { renderMarkdown } from '../markdown';
import {
  transcript, thread, entry, entrySameTurn, entryHasActions, entryActions, entryActionBtn,
  bodyMuted, bodyTool, bodyThinking, bodyCompaction,
  toolName, toolCard, toolCardHeader, toolCardIcon, toolCardName, toolCardStatus, toolCardBody,
  clickable, entryUser, entryAssistant, entryThinking, entryTool, entryStatus,
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

// Agent-internal blocks must never surface in the UI regardless of source
// (history load, SSE stream, prompt echoes).
function cleanText(s: string): string {
  return (s ?? '')
    .replace(/<system-reminder>[\s\S]*?<\/system-reminder>/g, '')
    .replace(/<system>[\s\S]*?<\/system>/g, '')
    .replace(/\n{3,}/g, '\n\n');
}

function copyEntry(e: Entry): void {
  const text = cleanText(e.text ?? '');
  void navigator.clipboard.writeText(text).catch(() => {
    /* clipboard unavailable */
  });
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
          e.kind === 'user' || e.kind === 'assistant' ? entryHasActions : null,
          e.kind === 'user' ? entryUser : e.kind === 'assistant' ? entryAssistant : e.kind === 'thinking' ? entryThinking : e.kind === 'tool' ? entryTool : entryStatus,
        ]"
      >
        <!-- hover actions: copy -->
        <div v-if="e.kind === 'user' || e.kind === 'assistant'" :class="entryActions">
          <button :class="entryActionBtn" title="Copy" @click.stop="copyEntry(e)">
            <svg viewBox="0 0 14 14" fill="none" aria-hidden="true"><rect x="4.5" y="4.5" width="7" height="7" rx="1" stroke="currentColor" stroke-width="1.2"/><path d="M9.5 4.5V3.5a1 1 0 0 0-1-1h-5a1 1 0 0 0-1 1v5a1 1 0 0 0 1 1h1" stroke="currentColor" stroke-width="1.2"/></svg>
          </button>
        </div>

        <!-- user -->
        <div v-if="e.kind === 'user'" class="body">{{ cleanText(e.text) }}</div>

        <!-- assistant: markdown -->
        <div v-else-if="e.kind === 'assistant'" class="body md" :class="md" v-html="renderMarkdown(cleanText(e.text))"></div>

        <!-- thinking: reasoning disclosure, collapsible -->
        <div v-else-if="e.kind === 'thinking'" :class="clickable" @click="toggleThinking(e)">
          <div :class="reasoningTitle">思考</div>
          <div :class="bodyThinking">{{ cleanText(thinkingPreview(e).text) }}</div>
          <div v-if="thinkingPreview(e).hint" :class="bodyMuted">{{ thinkingPreview(e).hint }}</div>
        </div>

        <!-- tool: Codex-style card -->
        <div v-else-if="e.kind === 'tool'" :class="[toolCard, clickable]" @click="toggleTool(e.toolCallId)">
          <div :class="toolCardHeader">
            <svg :class="toolCardIcon" viewBox="0 0 14 14" fill="none" aria-hidden="true"><path d="M2 3.5 5 7 2 10.5M7 11h5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/></svg>
            <span :class="toolCardName">{{ e.toolName }}</span>
            <span :class="toolCardStatus">{{ e.text && e.text.length > 0 ? '已完成' : '进行中' }}</span>
          </div>
          <div
            v-if="expandedTools.has(e.toolCallId) && e.args"
            :class="toolCardBody"
          >{{ cleanText(shellCmd(e.args)) }}</div>
          <div
            v-if="expandedTools.has(e.toolCallId) && e.text && e.text.length > 0"
            :class="toolCardBody"
          >{{ cleanText(e.text) }}</div>
        </div>

        <!-- status / compaction -->
        <div v-else :class="bodyMuted">{{ cleanText(e.text) }}</div>
      </div>
    </div>
  </main>
</template>
