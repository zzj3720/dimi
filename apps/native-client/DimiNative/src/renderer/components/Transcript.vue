<script setup lang="ts">
// Codex-style thread: welcome page when empty, else 768px centered messages.
import { ref, watch, nextTick } from 'vue';
import { state } from '../store';
import type { Entry } from '../store';
import { renderMarkdown } from '../markdown';
import {
  transcript, thread, entry, bodyMuted, bodyTool, bodyThinking, bodyCompaction,
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

watch(
  () => state.entries.length,
  () => void nextTick(scrollToBottom),
);
watch(
  () => state.entries[state.entries.length - 1]?.text,
  () => void nextTick(scrollToBottom),
);

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
        <div :class="modelRow"><span :class="modelName">5.3 Codex Spark</span><span :class="modelLevel">轻度</span></div>
        <div :class="modelRow"><span :class="modelName">5.6 Sol</span><span :class="modelLevel">极高</span></div>
      </div>
    </div>

    <!-- Message thread -->
    <div v-else :class="thread">
      <div
        v-for="e in state.entries"
        :key="e"
        :class="[
          entry,
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
