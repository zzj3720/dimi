<script setup lang="ts">
// Codex-style header (46px, transparent, fixed): sessions button + session
// title + refresh. No segmented control and no status badges — Codex keeps
// the header clean; status lives in the composer area.
import { computed } from 'vue';
import { state, Msg } from '../store';
import { dispatch, loadSessions } from '../api';
import { header, headerSide, headerMain, headerTitle, iconBtn, headerRight, shareBtn } from './HeaderBar.styles';

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
</script>

<template>
  <header :class="header">
    <!-- Sidebar zone: collapse + sessions -->
    <div :class="headerSide">
      <button :class="iconBtn" title="隐藏边栏" @click="toggleSidebar">
        <svg viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M2 3h12M2 8h12M2 13h12" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>
      </button>
    </div>
    <!-- Main zone: session title (Codex: clickable, at content left) -->
    <div :class="headerMain">
      <button :class="headerTitle" title="Sessions" @click="openSessions">
        <span>{{ current?.title || '' }}</span>
        <svg viewBox="0 0 14 14" fill="none" aria-hidden="true"><path d="M3.5 5.5 7 9l3.5-3.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>
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
