<script setup lang="ts">
// Codex-style header (46px, transparent, fixed): sessions button + session
// title + refresh. No segmented control and no status badges — Codex keeps
// the header clean; status lives in the composer area.
import { computed } from 'vue';
import { state, Msg } from '../store';
import { dispatch, loadSessions } from '../api';
import { header, headerSide, headerSideGroup, headerMain, headerTitle, iconBtn, iconBtnBordered, headerRight, shareBtn } from './HeaderBar.styles';

const current = computed(() => state.sessions.find((s) => s.id === state.currentSessionId));

function refresh(): void {
  void loadSessions();
}

function openSessions(): void {
  dispatch(Msg.PickerOpen());
  void loadSessions();
}

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
</script>

<template>
  <header :class="header">
    <!-- Sidebar zone: Codex layout — menu (x=0), then collapse/back/forward group -->
    <div :class="headerSide">
      <button :class="iconBtn" title="Menu" @click="openSessions">
        <svg viewBox="0 0 18 18" fill="none" aria-hidden="true"><path d="M2 4.5h14M2 9h14M2 13.5h14" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>
      </button>
      <div :class="headerSideGroup">
        <button :class="iconBtn" title="隐藏边栏" @click="toggleSidebar">
          <svg viewBox="0 0 18 18" fill="none" aria-hidden="true"><path d="M3 3h12v12H3z" stroke="currentColor" stroke-width="1.3"/><path d="M12 3v12" stroke="currentColor" stroke-width="1.3"/><rect x="13" y="6" width="1" height="6" fill="currentColor"/></svg>
        </button>
        <button :class="iconBtn" title="Back" @click="navSession(-1)">
          <svg viewBox="0 0 18 18" fill="none" aria-hidden="true"><path d="M11 4 6 9l5 5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>
        </button>
        <button :class="iconBtn" title="Forward" @click="navSession(1)">
          <svg viewBox="0 0 18 18" fill="none" aria-hidden="true"><path d="M7 4l5 5-5 5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>
        </button>
      </div>
    </div>
    <!-- Main zone: session title (Codex: clickable, at content left) -->
    <div :class="headerMain">
      <button :class="headerTitle" title="Sessions" @click="openSessions">
        <span>{{ current?.title || '' }}</span>
        <svg viewBox="0 0 14 14" fill="none" aria-hidden="true"><path d="M3.5 5.5 7 9l3.5-3.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>
      </button>
      <button :class="[iconBtn, iconBtnBordered]" title="More" @click="dispatch(Msg.HelpOpen())">
        <svg viewBox="0 0 18 18" fill="none" aria-hidden="true"><circle cx="3" cy="9" r="1.4" fill="currentColor"/><circle cx="9" cy="9" r="1.4" fill="currentColor"/><circle cx="15" cy="9" r="1.4" fill="currentColor"/></svg>
      </button>
    </div>
    <div :class="headerRight">
      <button :class="shareBtn" title="Share" @click="dispatch(Msg.HelpOpen())">
        <svg viewBox="0 0 14 14" fill="none" aria-hidden="true"><path d="M8.5 3.5 10.5 5.5M10.5 5.5 8.5 7.5M10.5 5.5H6a2.5 2.5 0 0 0-2.5 2.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>
        <span>分享</span>
      </button>
      <button :class="iconBtn" title="Refresh" @click="refresh">
        <svg viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M15.33 2.67v4h-4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/><path d="M13.66 10a6 6 0 1 1-1.41-6.24L15.33 6.67" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>
      </button>
    </div>
  </header>
</template>
