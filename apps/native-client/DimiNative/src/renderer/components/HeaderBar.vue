<script setup lang="ts">
// Codex-style header (46px, transparent, fixed): sessions button + session
// title + refresh. No segmented control and no status badges — Codex keeps
// the header clean; status lives in the composer area.
import { computed } from 'vue';
import { state, Msg } from '../store';
import { dispatch, loadSessions } from '../api';
import { icons } from '../icons';
import { header, headerSide, headerSideGroup, headerMain, headerTitle, iconBtn, iconBtnBordered, headerRight, shareBtn } from './HeaderBar.styles';

const current = computed(() => state.sessions.find((s) => s.id === state.currentSessionId));

function refresh(): void {
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
      <button :class="iconBtn" title="Menu" @click="toggleSidebar">
        <svg :viewBox="icons.menu.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.menu.paths" :key="i" :d="p" /></svg>
      </button>
      <div :class="headerSideGroup">
        <button :class="iconBtn" title="隐藏边栏" @click="toggleSidebar">
          <svg :viewBox="icons.hideSidebar.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.hideSidebar.paths" :key="i" :d="p" /></svg>
        </button>
        <button :class="iconBtn" title="Back" @click="navSession(-1)">
          <svg :viewBox="icons.back.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.back.paths" :key="i" :d="p" /></svg>
        </button>
        <button :class="iconBtn" title="Forward" @click="navSession(1)">
          <svg :viewBox="icons.forward.vb" fill="currentColor" aria-hidden="true" style="transform: scaleX(-1)"><path v-for="(p, i) in icons.forward.paths" :key="i" :d="p" /></svg>
        </button>
      </div>
    </div>
    <!-- Main zone: session title (Codex: plain static text, no popup) -->
    <div :class="headerMain">
      <span :class="headerTitle" title="Sessions">{{ current?.title || '' }}</span>
      <button :class="[iconBtn, iconBtnBordered]" title="More" @click="dispatch(Msg.HelpOpen())">
        <svg :viewBox="icons.ellipsis.vb" fill="currentColor" aria-hidden="true" style="width: 18px; height: 18px"><path v-for="(p, i) in icons.ellipsis.paths" :key="i" :d="p" /></svg>
      </button>
    </div>
    <div :class="headerRight">
      <button :class="shareBtn" title="Share" @click="dispatch(Msg.HelpOpen())">
        <svg :viewBox="icons.share.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.share.paths" :key="i" :d="p" /></svg>
        <span>分享</span>
      </button>
      <button :class="iconBtn" title="Refresh" @click="refresh">
        <svg :viewBox="icons.dots.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.dots.paths" :key="i" :d="p" /></svg>
      </button>
    </div>
  </header>
</template>
