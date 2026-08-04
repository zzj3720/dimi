<script setup lang="ts">
// Codex-style thread: welcome page when empty, else 768px centered messages.
import { ref, watch, nextTick, onMounted, computed } from 'vue';
import { state, Msg } from '../store';
import type { Entry } from '../store';
import { dispatch } from '../api';
import { icons } from '../icons';
import { renderMarkdown } from '../markdown';
import {
  transcript, threadWrap, thread, turn, turnContent, turnActions, entryActionBtn, entryActionBtnReplyBad,
  userMsgGroup, userBubble, userCopyRow,
  bodyMuted, toolCard, toolCardHeader, toolCardIcon, toolCardName, toolCardStatus, toolCardBody,
  clickable, entryUser, thinkingBlock, thinkingColumn,
  reasoningTitle, reasoningChevron, reasoningChevronOpen, reasoningBody, toolsCol,
  welcome, welcomeH1, suggestions, suggestionCard, welcomeModels,
  welcomeModelsTitle, modelRow, modelName, modelLevel, md,
} from './Transcript.styles';

// Turn action-row glyphs beyond the copy icon. icons.ts is a frozen design
// token file (do not edit); the design doc only recorded the viewBoxes of the
// codex thumbs-up / continue icons, not their path data, so these are visually
// close stand-ins (Material Design paths, Apache-2.0). "回复不佳" reuses the
// thumbs-up glyph rotated 180° (codex behavior).
const ICON_REPLY_GOOD = {
  vb: '0 0 24 24',
  paths: [
    'M1 21h4V9H1v12zm22-11c0-1.1-.9-2-2-2h-6.31l.95-4.57.03-.32c0-.41-.17-.79-.44-1.06L14.17 1 7.59 7.59C7.22 7.95 7 8.45 7 9v10c0 1.1.9 2 2 2h9c.83 0 1.54-.5 1.84-1.22l3.02-7.05c.09-.23.14-.47.14-.73v-2z',
  ],
};
const ICON_CONTINUE = {
  vb: '0 0 24 24',
  paths: ['M6 6v2h8.59L5 17.59 6.41 19 16 9.41V18h2V6z'],
};

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

// Same-turn grouping (codex turn container): a turn starts at every user
// message; thinking/tool/assistant entries that follow (until the next user)
// belong to that turn. Status/compaction rows are meta and render as
// standalone rows between turns.
interface Turn {
  entries: Entry[];
  meta: boolean;
}

const turns = computed<Turn[]>(() => {
  const out: Turn[] = [];
  let cur: Turn | null = null;
  for (const e of state.entries) {
    if (e.kind === 'user') {
      cur = { entries: [e], meta: false };
      out.push(cur);
    } else if (e.kind === 'status' || e.kind === 'compaction') {
      cur = null;
      out.push({ entries: [e], meta: true });
    } else if (!cur) {
      cur = { entries: [e], meta: false };
      out.push(cur);
    } else {
      cur.entries.push(e);
    }
  }
  return out;
});

// The turn-level action row appears once the turn has assistant-side content
// (assistant / thinking / tool entries); a bare user message (still streaming)
// gets no row.
function hasTurnActions(t: Turn): boolean {
  return t.entries.some((e) => e.kind === 'assistant' || e.kind === 'thinking' || e.kind === 'tool');
}

function toggleThinking(e: Entry): void {
  const sel = window.getSelection();
  if (sel && sel.toString().length > 0) return;
  const s = expandedThinking.value;
  if (s.has(e)) s.delete(e);
  else s.add(e);
  expandedThinking.value = new Set(s);
}

function toggleTool(id: string | undefined): void {
  const sel = window.getSelection();
  if (sel && sel.toString().length > 0) return;
  if (!id) return;
  const s = expandedTools.value;
  if (s.has(id)) s.delete(id);
  else s.add(id);
  expandedTools.value = new Set(s);
}

const SUGGESTIONS = ['创建文件或搭建网站', '调研并规划后续步骤', '自动处理日常和重复性工作'];

function suggest(text: string): void {
  window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'suggestion_send', text } }));
}

function shellCmd(args: string): string {
  return '$ ' + args;
}

function fmtDuration(ms: number): string {
  const s = Math.max(1, Math.round(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const r = s % 60;
  // Codex measured "思考了 2m 0s" — seconds always shown once past a minute.
  return `${m}m ${r}s`;
}

// Codex-style disclosure label: duration when the model reasoned, otherwise
// the tool names (never a bare "思考" for a tools-only turn).
function thinkingLabel(e: Entry): string {
  if (e.durationMs) return '思考了 ' + fmtDuration(e.durationMs);
  if (e.text) return '思考';
  const names = (e.tools ?? []).map((t) => t.name).filter(Boolean);
  if (names.length > 0) {
    return names.slice(0, 3).join(' · ') + (names.length > 3 ? ` 等 ${names.length} 个工具` : '');
  }
  return '思考';
}

// Agent-internal blocks must never surface in the UI regardless of source
// (history load, SSE stream, prompt echoes).
function cleanText(s: string): string {
  return (s ?? '')
    .replace(/<system-reminder>[\s\S]*?<\/system-reminder>/g, '')
    .replace(/<system>[\s\S]*?<\/system>/g, '')
    .replace(/<notification[^>]*>[\s\S]*?<\/notification>/g, '')
    .replace(/\n{3,}/g, '\n\n');
}

function copyEntry(e: Entry): void {
  const text = cleanText(e.text ?? '');
  void navigator.clipboard.writeText(text).catch(() => {
    /* clipboard unavailable */
  });
}

function copyTurn(t: Turn): void {
  // Copy the turn's assistant response(s); fall back to the last block when a
  // turn has only thinking/tool content.
  const asst = t.entries.filter((e) => e.kind === 'assistant').map((e) => cleanText(e.text));
  const text =
    asst.length > 0 ? asst.join('\n\n') : cleanText(t.entries[t.entries.length - 1]?.text ?? '');
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

    <!-- Message thread: 768px outer column (16px side padding) wrapping the
         736px message column; each turn is one `group flex flex-col py-2`. -->
    <div v-else :class="threadWrap">
      <div :class="thread">
        <div v-for="(t, ti) in turns" :key="ti" :class="turn">
          <!-- meta rows (status / compaction) sit between turns -->
          <div v-if="t.meta" :class="bodyMuted">{{ cleanText(t.entries[0].text) }}</div>

          <template v-else>
            <div :class="turnContent">
              <div
                v-for="(e, i) in t.entries"
                :key="i"
                :class="e.kind === 'user' ? entryUser : null"
              >
                <!-- user: right-aligned bubble + single copy button -->
                <template v-if="e.kind === 'user'">
                  <div :class="userMsgGroup">
                    <div :class="userBubble">
                      <div :class="md" v-html="renderMarkdown(cleanText(e.text))"></div>
                    </div>
                    <div :class="userCopyRow">
                      <button :class="entryActionBtn" aria-label="复制消息" title="复制消息" @click="copyEntry(e)">
                        <svg :viewBox="icons.copy.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.copy.paths" :key="i" :d="p" /></svg>
                      </button>
                    </div>
                  </div>
                </template>

                <!-- assistant: markdown -->
                <div v-else-if="e.kind === 'assistant'" :class="md" v-html="renderMarkdown(cleanText(e.text))"></div>

                <!-- thinking: reasoning disclosure — collapsed shows only the
                     "思考了 …" button (Codex behavior); content appears on expand -->
                <div v-else-if="e.kind === 'thinking'" :class="thinkingBlock">
                  <div :class="thinkingColumn">
                    <button :class="reasoningTitle" @click="toggleThinking(e)">
                      {{ thinkingLabel(e) }}
                      <svg :class="[reasoningChevron, expandedThinking.has(e) ? reasoningChevronOpen : null]" :viewBox="icons.chevronDown.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.chevronDown.paths" :key="i" :d="p" /></svg>
                    </button>
                    <div v-if="expandedThinking.has(e)" :class="reasoningBody">
                      <div v-if="e.text" :class="md" v-html="renderMarkdown(cleanText(e.text))"></div>
                      <!-- Tool calls live inside the reasoning disclosure (Codex) -->
                      <div v-if="e.tools && e.tools.length > 0" :class="toolsCol">
                        <div
                          v-for="t in e.tools"
                          :key="t.id"
                          :class="[toolCard, clickable]"
                          @click.stop="toggleTool(t.id)"
                        >
                          <div :class="toolCardHeader">
                            <svg :class="toolCardIcon" :viewBox="icons.chevronDown.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.chevronDown.paths" :key="i" :d="p" /></svg>
                            <span :class="toolCardName">{{ t.name }}</span>
                            <span :class="toolCardStatus">{{ t.text && t.text.length > 0 ? '已完成' : '进行中' }}</span>
                          </div>
                          <div
                            v-if="expandedTools.has(t.id) && t.args"
                            :class="toolCardBody"
                          >{{ cleanText(shellCmd(t.args)) }}</div>
                          <div
                            v-if="expandedTools.has(t.id) && t.text && t.text.length > 0"
                            :class="toolCardBody"
                          >{{ cleanText(t.text) }}</div>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>

                <!-- tool: Codex-style card (standalone, live-stream fallback) -->
                <div v-else-if="e.kind === 'tool'" :class="[toolCard, clickable]" @click="toggleTool(e.toolCallId)">
                  <div :class="toolCardHeader">
                    <svg :class="toolCardIcon" :viewBox="icons.chevronDown.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.chevronDown.paths" :key="i" :d="p" /></svg>
                    <span :class="toolCardName">{{ e.toolName }}</span>
                    <span :class="toolCardStatus">{{ e.text && e.text.length > 0 ? '已完成' : '进行中' }}</span>
                  </div>
                  <div
                    v-if="expandedTools.has(e.toolCallId ?? '') && e.args"
                    :class="toolCardBody"
                  >{{ cleanText(shellCmd(e.args)) }}</div>
                  <div
                    v-if="expandedTools.has(e.toolCallId ?? '') && e.text && e.text.length > 0"
                    :class="toolCardBody"
                  >{{ cleanText(e.text) }}</div>
                </div>

                <!-- status / compaction (should not occur inside a turn) -->
                <div v-else :class="bodyMuted">{{ cleanText(e.text) }}</div>
              </div>
            </div>

            <!-- turn-level action row (always visible in this codex build):
                 复制 / 回复优秀 / 回复不佳 / 在新聊天中继续 -->
            <div v-if="hasTurnActions(t)" :class="turnActions">
              <button :class="entryActionBtn" aria-label="复制" title="复制" @click="copyTurn(t)">
                <svg :viewBox="icons.copy.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.copy.paths" :key="i" :d="p" /></svg>
              </button>
              <button :class="entryActionBtn" aria-label="回复优秀" title="回复优秀">
                <svg :viewBox="ICON_REPLY_GOOD.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ICON_REPLY_GOOD.paths" :key="i" :d="p" /></svg>
              </button>
              <button :class="entryActionBtn" aria-label="回复不佳" title="回复不佳">
                <svg :class="entryActionBtnReplyBad" :viewBox="ICON_REPLY_GOOD.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ICON_REPLY_GOOD.paths" :key="i" :d="p" /></svg>
              </button>
              <button :class="entryActionBtn" aria-label="在新聊天中继续" title="在新聊天中继续">
                <svg :viewBox="ICON_CONTINUE.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ICON_CONTINUE.paths" :key="i" :d="p" /></svg>
              </button>
            </div>
          </template>
        </div>
      </div>
    </div>
  </main>
</template>
