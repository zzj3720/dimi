<script setup lang="ts">
// Codex-style thread: welcome page when empty, else 768px centered messages.
import { ref, reactive, watch, nextTick, onMounted, onBeforeUnmount, computed } from 'vue';
import { state, Msg } from '../store';
import type { Entry } from '../store';
import { dispatch } from '../api';
import { icons } from '../icons';
import { renderMarkdown } from '../markdown';
import { srOnly } from '../styles/global';
import {
  transcript, threadWrap, thread, turn, turnContent, itemDivider, turnActions, entryActionBtn, entryActionBtnReplyBad,
  userMsgGroup, userBubble, userCopyRow,
  bodyMuted, toolCard, toolCardHeader, toolCardIcon, toolCardIconOpen, toolCardName, toolCardStatus,
  toolShell, toolShellCollapsed, toolCardBody,
  clickable, entryUser, thinkingBlock,
  reasoningTitle, reasoningChevron, reasoningChevronOpen,
  reasoningShell, reasoningShellCollapsed, reasoningBody, thinkingMd,
  toolsCol,
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

// ---- thinking / tool disclosure animation ----
// Codex animates the reasoning body with measured-height + opacity
// (300ms cubic-bezier(0.19,1,0.22,1)); the content stays in the DOM and the
// shell's height flips between 0 and the measured value. Bodies are capped
// at 140px (8.75rem) with a 16px bottom edge fade.
const thinkingBodyEls = new Map<Entry, HTMLElement>();
const thinkingMeta = reactive(new Map<Entry, { h: number; capped: boolean }>());
const toolBodyEls = new Map<string, HTMLElement>();
const toolMeta = reactive(new Map<string, number>());
// Entries the user explicitly toggled keep their state across streaming
// auto-expand/collapse (codex: streaming auto-expands, then "回到用户控制").
const userPinned = new Set<Entry>();

const EDGE_FADE = {
  WebkitMaskImage: 'linear-gradient(to bottom, #000 0%, #000 calc(100% - 16px), transparent 100%)',
  maskImage: 'linear-gradient(to bottom, #000 0%, #000 calc(100% - 16px), transparent 100%)',
};

function setThinkingEl(e: Entry, el: unknown): void {
  if (el instanceof HTMLElement) thinkingBodyEls.set(e, el);
  else thinkingBodyEls.delete(e);
}

function setToolEl(key: string, el: unknown): void {
  if (el instanceof HTMLElement) toolBodyEls.set(key, el);
  else toolBodyEls.delete(key);
}

function measureThinking(e: Entry): void {
  const el = thinkingBodyEls.get(e);
  if (!el) return;
  const h = el.offsetHeight;
  // scrollHeight sees the uncapped content, offsetHeight the 140px cap —
  // the edge fade only applies when the content is actually clipped.
  thinkingMeta.set(e, { h, capped: el.scrollHeight > h + 1 });
}

function remeasureAll(): void {
  for (const e of thinkingBodyEls.keys()) {
    if (expandedThinking.value.has(e)) measureThinking(e);
  }
  for (const [key, el] of toolBodyEls) {
    if (expandedTools.value.has(key)) toolMeta.set(key, el.offsetHeight);
  }
}

function toggleThinking(e: Entry): void {
  const sel = window.getSelection();
  if (sel && sel.toString().length > 0) return;
  userPinned.add(e);
  const s = expandedThinking.value;
  if (s.has(e)) s.delete(e);
  else s.add(e);
  expandedThinking.value = new Set(s);
  void nextTick(() => measureThinking(e));
}

function toggleTool(key: string | undefined): void {
  const sel = window.getSelection();
  if (sel && sel.toString().length > 0) return;
  if (!key) return;
  const s = expandedTools.value;
  if (s.has(key)) s.delete(key);
  else s.add(key);
  expandedTools.value = new Set(s);
  void nextTick(() => {
    const el = toolBodyEls.get(key);
    if (el) toolMeta.set(key, el.offsetHeight);
  });
}

// Codex: a streaming reasoning item with content auto-expands; when the turn
// finishes it falls back to the user's last explicit choice (default folded).
watch(
  () => state.entries.filter((e) => e.kind === 'thinking').map((e) => [e, e.streaming] as const),
  (list) => {
    let changed = false;
    for (const [e, streaming] of list) {
      if (streaming && !userPinned.has(e)) {
        if (!expandedThinking.value.has(e)) changed = true;
        expandedThinking.value.add(e);
      } else if (!streaming && !userPinned.has(e) && expandedThinking.value.has(e)) {
        changed = true;
        expandedThinking.value.delete(e);
      }
    }
    if (changed) {
      expandedThinking.value = new Set(expandedThinking.value);
      void nextTick(remeasureAll);
    }
  },
  { immediate: true },
);

// Tool output / args mutate card bodies without touching the entries array
// or the last-entry text — re-measure open cards when they change.
watch(
  () =>
    state.entries
      .map((e) =>
        e.kind === 'tool'
          ? e.text + '\u0001' + (e.args ?? '')
          : (e.tools ?? []).map((t) => t.text + '\u0001' + (t.args ?? '')).join('\u0002'),
      )
      .join('\u0003'),
  () => void nextTick(remeasureAll),
);

// ---- copy with success feedback (codex: icon flips to check, 1.5s) ----
const copyFeedback = ref<string | null>(null);
let copyTimer: ReturnType<typeof setTimeout> | undefined;

function flashCopy(key: string): void {
  copyFeedback.value = key;
  if (copyTimer) clearTimeout(copyTimer);
  copyTimer = setTimeout(() => {
    if (copyFeedback.value === key) copyFeedback.value = null;
  }, 1500);
}

function copyText(text: string, key: string): void {
  void navigator.clipboard
    .writeText(text)
    .then(() => flashCopy(key))
    .catch(() => {
      /* clipboard unavailable */
    });
}

function userCopyKey(ti: number, i: number): string {
  return 'u' + ti + '-' + i;
}

function turnCopyKey(ti: number): string {
  return 't' + ti;
}

function copyUser(e: Entry, ti: number, i: number): void {
  copyText(cleanText(e.text), userCopyKey(ti, i));
}

function copyTurn(t: Turn, ti: number): void {
  // Copy the turn's assistant response(s); fall back to the last block when a
  // turn has only thinking/tool content.
  const asst = t.entries.filter((x) => x.kind === 'assistant').map((x) => cleanText(x.text));
  const text =
    asst.length > 0 ? asst.join('\n\n') : cleanText(t.entries[t.entries.length - 1]?.text ?? '');
  copyText(text, turnCopyKey(ti));
}

// The code-block copy button is emitted by markdown.ts inside v-html, so it
// gets a delegated listener on the thread root + a DOM class toggle for the
// check-glyph feedback.
function onRootClick(ev: MouseEvent): void {
  const target = ev.target as HTMLElement | null;
  const btn = target?.closest?.('.md-code-copy') as HTMLElement | null;
  if (!btn) return;
  ev.preventDefault();
  const block = btn.closest('.md-code-block');
  const code = block?.querySelector('pre code');
  const text = code?.textContent ?? '';
  void navigator.clipboard
    .writeText(text)
    .then(() => {
      btn.classList.add('md-code-copied');
      setTimeout(() => btn.classList.remove('md-code-copied'), 1500);
    })
    .catch(() => {
      /* clipboard unavailable */
    });
}

onBeforeUnmount(() => {
  if (copyTimer) clearTimeout(copyTimer);
});

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
  () => void nextTick(() => {
    scrollToBottom();
    remeasureAll();
  }),
);
watch(
  () => state.entries[state.entries.length - 1]?.text,
  () => void nextTick(() => {
    scrollToBottom();
    remeasureAll();
  }),
);
// Whole-array replacement (e.g. switching sessions) can keep length + last
// text identical; watching the array reference catches those too.
watch(
  () => state.entries,
  () => void nextTick(() => {
    scrollToBottom();
    remeasureAll();
  }),
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

// The turn-level action row belongs to assistant content (codex
// AssistantMessageActions is part of the assistant item); thinking/tool-only
// turns get no row.
function hasTurnActions(t: Turn): boolean {
  return t.entries.some((e) => e.kind === 'assistant');
}

// Fork is only available once the assistant message completed (codex:
// forkDisabled while isForking / !completed).
function isTurnStreaming(t: Turn): boolean {
  return t.entries.some((e) => e.streaming);
}

// Accessible turn heading (codex `h4.sr-only` role title).
function turnLabel(t: Turn): string {
  return t.entries.some((e) => e.kind === 'user') ? '你说：' : '助手';
}

// Tool-card expansion keys: attached tools (inside a thinking entry) and
// standalone tool entries live in separate key namespaces.
function attachedToolKey(id: string | undefined): string {
  return id ? 'att:' + id : '';
}

function standaloneToolKey(e: Entry): string {
  return e.toolCallId ? 'st:' + e.toolCallId : '';
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

// Codex-style disclosure label: "Thinking" while streaming, duration when the
// model reasoned, otherwise the tool names (never a bare "思考" for a
// tools-only turn).
function thinkingLabel(e: Entry): string {
  if (e.streaming) return '思考中…';
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
</script>

<template>
  <main ref="scroller" :class="transcript" tabindex="-1" @click="onRootClick">
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
              <!-- accessible role heading (codex h4.sr-only) -->
              <h4 :class="srOnly">{{ turnLabel(t) }}</h4>

              <template v-for="(e, i) in t.entries" :key="i">
                <div :class="e.kind === 'user' ? entryUser : null">
                  <!-- user: right-aligned bubble + single copy button -->
                  <template v-if="e.kind === 'user'">
                    <div :class="userMsgGroup">
                      <div :class="userBubble">
                        <div :class="md" v-html="renderMarkdown(cleanText(e.text))"></div>
                      </div>
                      <div :class="userCopyRow">
                        <button :class="entryActionBtn" aria-label="复制消息" title="复制消息" @click="copyUser(e, ti, i)">
                          <svg v-if="copyFeedback !== userCopyKey(ti, i)" :viewBox="icons.copy.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, pi) in icons.copy.paths" :key="pi" :d="p" /></svg>
                          <svg v-else :viewBox="icons.check.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, pi) in icons.check.paths" :key="pi" :d="p" /></svg>
                        </button>
                      </div>
                    </div>
                  </template>

                  <!-- assistant: markdown -->
                  <div v-else-if="e.kind === 'assistant'" :class="md" v-html="renderMarkdown(cleanText(e.text))"></div>

                  <!-- thinking: reasoning disclosure — collapsed shows only the
                       "思考了 …" button (Codex behavior); the body animates
                       height/opacity (300ms) and caps at 140px with a fade -->
                  <div v-else-if="e.kind === 'thinking'" :class="thinkingBlock">
                    <button
                      :class="reasoningTitle"
                      :aria-expanded="expandedThinking.has(e) ? 'true' : 'false'"
                      @click="toggleThinking(e)"
                    >
                      {{ thinkingLabel(e) }}
                      <svg :class="[reasoningChevron, expandedThinking.has(e) ? reasoningChevronOpen : null]" :viewBox="icons.chevronDown.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, pi) in icons.chevronDown.paths" :key="pi" :d="p" /></svg>
                    </button>
                    <div
                      :class="[reasoningShell, expandedThinking.has(e) ? null : reasoningShellCollapsed]"
                      :style="expandedThinking.has(e) ? { height: (thinkingMeta.get(e)?.h ?? 0) + 'px', opacity: 1 } : { height: '0px', opacity: 0 }"
                      :aria-hidden="expandedThinking.has(e) ? 'false' : 'true'"
                    >
                      <div
                        :class="reasoningBody"
                        :style="thinkingMeta.get(e)?.capped ? EDGE_FADE : null"
                        :ref="(el) => setThinkingEl(e, el)"
                      >
                        <div v-if="e.text" :class="[md, thinkingMd]" v-html="renderMarkdown(cleanText(e.text))"></div>
                      </div>
                    </div>
                    <!-- Tool activity is an independent item in codex (separate
                         block after a 16px divider, visible regardless of the
                         reasoning disclosure) -->
                    <div v-if="e.tools && e.tools.length > 0" :class="itemDivider" aria-hidden="true"></div>
                    <div v-if="e.tools && e.tools.length > 0" :class="toolsCol">
                      <div
                        v-for="t in e.tools"
                        :key="t.id"
                        :class="[toolCard, clickable]"
                        @click.stop="toggleTool(attachedToolKey(t.id))"
                      >
                        <div :class="toolCardHeader">
                          <svg :class="[toolCardIcon, expandedTools.has(attachedToolKey(t.id)) ? toolCardIconOpen : null]" :viewBox="icons.chevronDown.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, pi) in icons.chevronDown.paths" :key="pi" :d="p" /></svg>
                          <span :class="toolCardName">{{ t.name }}</span>
                          <span :class="toolCardStatus">{{ t.text && t.text.length > 0 ? '已完成' : '进行中' }}</span>
                        </div>
                        <div
                          :class="[toolShell, expandedTools.has(attachedToolKey(t.id)) ? null : toolShellCollapsed]"
                          :style="expandedTools.has(attachedToolKey(t.id)) ? { height: (toolMeta.get(attachedToolKey(t.id)) ?? 0) + 'px', opacity: 1 } : { height: '0px', opacity: 0 }"
                          :aria-hidden="expandedTools.has(attachedToolKey(t.id)) ? 'false' : 'true'"
                        >
                          <div :ref="(el) => setToolEl(attachedToolKey(t.id), el)">
                            <div v-if="t.args" :class="toolCardBody">{{ cleanText(shellCmd(t.args)) }}</div>
                            <div v-if="t.text && t.text.length > 0" :class="toolCardBody">{{ cleanText(t.text) }}</div>
                          </div>
                        </div>
                      </div>
                    </div>
                  </div>

                  <!-- tool: standalone card (live-stream fallback) -->
                  <div v-else-if="e.kind === 'tool'" :class="[toolCard, clickable]" @click="toggleTool(standaloneToolKey(e))">
                    <div :class="toolCardHeader">
                      <svg :class="[toolCardIcon, expandedTools.has(standaloneToolKey(e)) ? toolCardIconOpen : null]" :viewBox="icons.chevronDown.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, pi) in icons.chevronDown.paths" :key="pi" :d="p" /></svg>
                      <span :class="toolCardName">{{ e.toolName }}</span>
                      <span :class="toolCardStatus">{{ e.text && e.text.length > 0 ? '已完成' : '进行中' }}</span>
                    </div>
                    <div
                      :class="[toolShell, expandedTools.has(standaloneToolKey(e)) ? null : toolShellCollapsed]"
                      :style="expandedTools.has(standaloneToolKey(e)) ? { height: (toolMeta.get(standaloneToolKey(e)) ?? 0) + 'px', opacity: 1 } : { height: '0px', opacity: 0 }"
                      :aria-hidden="expandedTools.has(standaloneToolKey(e)) ? 'false' : 'true'"
                    >
                      <div :ref="(el) => setToolEl(standaloneToolKey(e), el)">
                        <div v-if="e.args" :class="toolCardBody">{{ cleanText(shellCmd(e.args)) }}</div>
                        <div v-if="e.text && e.text.length > 0" :class="toolCardBody">{{ cleanText(e.text) }}</div>
                      </div>
                    </div>
                  </div>

                  <!-- status / compaction (should not occur inside a turn) -->
                  <div v-else :class="bodyMuted">{{ cleanText(e.text) }}</div>
                </div>

                <!-- 16px block separator between turn items (codex s8c) -->
                <div v-if="i < t.entries.length - 1" :class="itemDivider" aria-hidden="true"></div>
              </template>
            </div>

            <!-- turn-level action row (assistant): opacity-0, revealed on
                 turn hover / focus (codex group-hover) — 复制 / 回复优秀 /
                 回复不佳 / 在新聊天中继续 -->
            <div v-if="hasTurnActions(t)" :class="turnActions">
              <button :class="entryActionBtn" aria-label="复制" title="复制" @click="copyTurn(t, ti)">
                <svg v-if="copyFeedback !== turnCopyKey(ti)" :viewBox="icons.copy.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, pi) in icons.copy.paths" :key="pi" :d="p" /></svg>
                <svg v-else :viewBox="icons.check.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, pi) in icons.check.paths" :key="pi" :d="p" /></svg>
              </button>
              <button :class="entryActionBtn" aria-label="回复优秀" title="回复优秀">
                <svg :viewBox="ICON_REPLY_GOOD.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ICON_REPLY_GOOD.paths" :key="i" :d="p" /></svg>
              </button>
              <button :class="entryActionBtn" aria-label="回复不佳" title="回复不佳">
                <svg :class="entryActionBtnReplyBad" :viewBox="ICON_REPLY_GOOD.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ICON_REPLY_GOOD.paths" :key="i" :d="p" /></svg>
              </button>
              <button :class="entryActionBtn" aria-label="在新聊天中继续" title="在新聊天中继续" :disabled="isTurnStreaming(t)">
                <svg :viewBox="ICON_CONTINUE.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ICON_CONTINUE.paths" :key="i" :d="p" /></svg>
              </button>
            </div>
          </template>
        </div>
      </div>
    </div>
  </main>
</template>
