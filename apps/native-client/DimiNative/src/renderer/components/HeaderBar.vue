<script setup lang="ts">
// Codex-style header (46px): sidebar toggle + session title + segmented
// control (chat/work) + minimal actions. No status badges — Codex keeps
// the header clean; status lives in the composer area.
import { computed } from 'vue';
import { state } from '../store';
import { loadSessions } from '../api';

const current = computed(() => state.sessions.find((s) => s.id === state.currentSessionId));

function refresh(): void {
  void loadSessions();
}
</script>

<template>
  <header class="header">
    <div class="header-left">
      <button class="icon-btn" title="Sessions" @click="state.pickerOpen = true">☰</button>
      <span class="header-title" :title="state.currentCwd">{{ current?.title || '' }}</span>
    </div>
    <div class="segmented">
      <button class="seg active" data-seg="chat">聊天</button>
      <button class="seg" data-seg="work">工作</button>
    </div>
    <div class="header-right">
      <span v-if="state.busy" class="header-status">{{ state.phase === 'compacting' ? 'compacting' : 'working' }}</span>
      <button class="icon-btn" title="Refresh" @click="refresh">↻</button>
    </div>
  </header>
</template>
