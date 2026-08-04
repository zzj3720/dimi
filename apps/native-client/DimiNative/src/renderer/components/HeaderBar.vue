<script setup lang="ts">
// Codex-style header (46px, transparent, fixed): hide-sidebar/back/forward +
// session title button + More + share pill + pinned summary + toggle-sidebar.
// No segmented control and no status badges — Codex keeps the header clean;
// status lives in the composer area.
import { computed, ref } from 'vue';
import { state, Msg } from '../store';
import { dispatch, loadSessions } from '../api';
import { icons } from '../icons';
import {
  header, headerSide, headerSideGroup, headerMain, headerTitle, iconBtn,
  moreBtn, headerRight, shareBtn, pinnedBtn, pinnedBtnOn,
} from './HeaderBar.styles';

const current = computed(() => state.sessions.find((s) => s.id === state.currentSessionId));

// Codex disables back/forward when there is no history in that direction
// (S14). dimi simulates history with sidebar-list order, so the ends of the
// list are the disabled states.
const navIndex = computed(() => state.sessions.findIndex((s) => s.id === state.currentSessionId));
const canBack = computed(() => navIndex.value > 0);
const canForward = computed(() => navIndex.value >= 0 && navIndex.value < state.sessions.length - 1);

function toggleSidebar(): void {
  dispatch({ type: 'sidebar_toggle' });
}

// Codex back/forward: navigate to the previous/next session in the sidebar list.
function navSession(delta: number): void {
  const list = state.sessions;
  if (list.length === 0) return;
  const idx = list.findIndex((s) => s.id === state.currentSessionId);
  const base = idx === -1 ? 0 : idx;
  const next = list[Math.min(list.length - 1, Math.max(0, base + delta))];
  if (next && next.id !== state.currentSessionId) {
    dispatch({ type: 'session_selected', id: next.id });
  }
}

// Codex title is a button that opens a title menu / jumps — dimi has no title
// menu, so open the session picker (closest equivalent; A6).
function openTitleMenu(): void {
  dispatch(Msg.PickerOpen());
  void loadSessions();
}

// 固定摘要 (pinned summary): Codex toggles a summary panel (measured ON, white
// 5% bg). dimi has no summary panel yet, so this is a local visual toggle only
// — needs product wiring when the feature lands (A4). OFF style (transparent)
// is inferred from the generic icon-button default (unobservable in Codex).
const pinnedSummaryOn = ref(true);
function togglePinnedSummary(): void {
  pinnedSummaryOn.value = !pinnedSummaryOn.value;
}

// More / Share are behavior placeholders in dimi (no conversation-actions or
// share menu yet, A2/A3): keep the previous intent — open the help dialog.
// The old Msg.HelpOpen() message does not exist in store.ts (would throw), so
// set the dialog flag directly.
function openHelp(): void {
  state.helpDialogOpen = true;
}
</script>

<template>
  <header :class="header">
    <!-- Left zone (0..275px, safe-left 88px): hide-sidebar / back / forward -->
    <div :class="headerSide">
      <div :class="headerSideGroup">
        <button :class="iconBtn" aria-label="隐藏边栏" @click="toggleSidebar">
          <svg :viewBox="icons.hideSidebar.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.hideSidebar.paths" :key="i" :d="p" /></svg>
        </button>
        <button :class="iconBtn" aria-label="返回" :disabled="!canBack" @click="navSession(-1)">
          <svg :viewBox="icons.back.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.back.paths" :key="i" :d="p" /></svg>
        </button>
        <button :class="iconBtn" aria-label="前进" :disabled="!canForward" @click="navSession(1)">
          <svg :viewBox="icons.forward.vb" fill="currentColor" aria-hidden="true" style="transform: scaleX(-1)"><path v-for="(p, i) in icons.forward.paths" :key="i" :d="p" /></svg>
        </button>
      </div>
    </div>
    <!-- Main zone: session title button + More -->
    <div :class="headerMain">
      <button :class="headerTitle" @click="openTitleMenu">
        <span>{{ current?.title || '' }}</span>
      </button>
      <button :class="[iconBtn, moreBtn]" aria-label="ChatGPT 对话操作" @click="openHelp">
        <svg :viewBox="icons.ellipsis.vb" fill="currentColor" aria-hidden="true" style="width: 18px; height: 18px"><path v-for="(p, i) in icons.ellipsis.paths" :key="i" :d="p" /></svg>
      </button>
    </div>
    <!-- Right zone: share pill + pinned summary + toggle sidebar -->
    <div :class="headerRight">
      <button :class="shareBtn" aria-label="分享" @click="openHelp">
        <svg :viewBox="icons.share.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.share.paths" :key="i" :d="p" /></svg>
        <span>分享</span>
      </button>
      <button :class="[pinnedBtn, { [pinnedBtnOn]: pinnedSummaryOn }]" aria-label="切换固定摘要" title="切换固定摘要" @click="togglePinnedSummary">
        <svg :viewBox="icons.dots.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.dots.paths" :key="i" :d="p" /></svg>
      </button>
      <button :class="iconBtn" aria-label="切换侧边栏" @click="toggleSidebar">
        <svg :viewBox="icons.menu.vb" fill="currentColor" aria-hidden="true" style="transform: rotate(180deg)"><path v-for="(p, i) in icons.menu.paths" :key="i" :d="p" /></svg>
      </button>
    </div>
  </header>
</template>
