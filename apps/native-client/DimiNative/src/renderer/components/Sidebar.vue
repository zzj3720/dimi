<script setup lang="ts">
// Codex-style left sidebar: brand + new-chat + sessions grouped by workspace.
import { computed } from 'vue';
import { state } from '../store';
import { dispatch } from '../api';

const groups = computed(() => {
  const map = new Map<string, typeof state.sessions>();
  for (const s of state.sessions) {
    const key = s.metadata?.cwd ?? s.cwd ?? '其他';
    if (!map.has(key)) map.set(key, []);
    map.get(key)!.push(s);
  }
  return [...map.entries()];
});

function groupLabel(cwd: string): string {
  return cwd.split('/').filter(Boolean).pop() || cwd || '其他';
}

function select(id: string): void {
  dispatch({ type: 'session_selected', id });
}

function newChat(): void {
  window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'new_chat' } }));
}
</script>

<template>
  <aside class="sidebar">
    <div class="sidebar-top">
      <div class="sidebar-brand">Dimi</div>
      <button class="btn-new-chat" @click="newChat">＋ 新对话</button>
    </div>
    <nav class="sidebar-nav">
      <div class="nav-item active">站点</div>
      <div class="nav-item">已安排</div>
      <div class="nav-item">插件</div>
      <div class="nav-item">项目</div>
    </nav>
    <div class="sidebar-sessions">
      <div v-if="state.sessions.length === 0" class="session-group-title">
        {{ state.sessionsLoading ? '加载中…' : '暂无会话' }}
      </div>
      <div v-for="[cwd, list] in groups" :key="cwd" class="session-group">
        <div class="session-group-title" :title="cwd">{{ groupLabel(cwd) }}</div>
        <div
          v-for="s in list"
          :key="s.id"
          class="session-item"
          :class="{ active: s.id === state.currentSessionId }"
          @click="select(s.id)"
        >
          <span class="s-title">{{ s.title || '(untitled)' }}</span>
          <span class="s-type">聊天</span>
        </div>
      </div>
    </div>
    <div class="sidebar-bottom">user</div>
  </aside>
</template>
