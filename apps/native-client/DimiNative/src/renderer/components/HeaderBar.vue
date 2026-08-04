<script setup lang="ts">
// Codex-style header (46px): sidebar toggle + session title + segmented
// control (chat/work) + minimal actions. No status badges — Codex keeps
// the header clean; status lives in the composer area.
import { computed } from 'vue';
import { state } from '../store';
import { loadSessions } from '../api';
import { header, headerLeft, headerTitle, iconBtn, headerRight, headerStatus } from './HeaderBar.styles';

const current = computed(() => state.sessions.find((s) => s.id === state.currentSessionId));

function refresh(): void {
  void loadSessions();
}
</script>

<template>
  <header :class="header">
    <div :class="headerLeft">
      <button :class="iconBtn" title="Sessions" @click="state.pickerOpen = true">☰</button>
      <span :class="headerTitle" :title="state.currentCwd">{{ current?.title || '' }}</span>
    </div>
    <div :class="headerRight">
      <span v-if="state.busy" :class="headerStatus">{{ state.phase === 'compacting' ? 'compacting' : 'working' }}</span>
      <button :class="iconBtn" title="Refresh" @click="refresh">↻</button>
    </div>
  </header>
</template>
