<script setup lang="ts">
// Codex-style left sidebar: brand + new-chat + sessions grouped by workspace.
// Width is draggable via a resize handle on the right edge (Codex behavior).
import { computed, ref } from 'vue';
import { state } from '../store';
import { dispatch } from '../api';
import {
  sidebar, sidebarTop, brandRow, brand, brandActions, iconBtn,
  sessions, sessionGroup, sessionGroupTitle, sessionItem, sessionItemActive,
  sidebarBottom, resizeHandle, resizeHandleLine,
} from './Sidebar.styles';

const RESIZE_KEY = 'dimi.sidebarWidth';
const MIN_W = 200;
const MAX_W = 480;

const sidebarWidth = ref(Number(localStorage.getItem(RESIZE_KEY)) || 275);

const clamp = (w: number): number => Math.min(MAX_W, Math.max(MIN_W, w));

function startResize(e: MouseEvent): void {
  e.preventDefault();
  const startX = e.clientX;
  const startW = sidebarWidth.value;
  const move = (ev: MouseEvent): void => {
    sidebarWidth.value = clamp(startW + (ev.clientX - startX));
  };
  const up = (): void => {
    document.removeEventListener('mousemove', move);
    document.removeEventListener('mouseup', up);
    try {
      localStorage.setItem(RESIZE_KEY, String(sidebarWidth.value));
    } catch {
      /* non-fatal */
    }
  };
  document.addEventListener('mousemove', move);
  document.addEventListener('mouseup', up);
}

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
  <aside :class="sidebar" :style="{ width: sidebarWidth + 'px' }">
    <div :class="sidebarTop">
      <div :class="brandRow">
        <div :class="brand">
          <svg viewBox="0 0 14 14" fill="currentColor" aria-hidden="true"><rect x="1.5" y="1.5" width="11" height="11" rx="3.5" stroke="currentColor" stroke-width="1.6" fill="none" opacity="0.85"/></svg>
          <span>Dimi</span>
        </div>
        <div :class="brandActions">
          <button :class="iconBtn" title="New chat" @click="newChat">＋</button>
        </div>
      </div>
    </div>
    <div :class="sessions">
      <div v-if="state.sessions.length === 0" :class="sessionGroupTitle">
        {{ state.sessionsLoading ? '加载中…' : '暂无会话' }}
      </div>
      <div v-for="[cwd, list] in groups" :key="cwd" :class="sessionGroup">
        <div :class="sessionGroupTitle" :title="cwd">
          <svg class="chevron" viewBox="0 0 14 14" fill="none" aria-hidden="true"><path d="M3.5 5.5 7 9l3.5-3.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>
          <span>{{ groupLabel(cwd) }}</span>
        </div>
        <div
          v-for="s in list"
          :key="s.id"
          :class="[sessionItem, { [sessionItemActive]: s.id === state.currentSessionId }]"
          @click="select(s.id)"
        >
          <svg class="s-icon" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M3 3h10a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H7.5L5 13.5V11H3a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/></svg>
          <span class="s-title">{{ s.title || '(untitled)' }}</span>
        </div>
      </div>
    </div>
    <div :class="sidebarBottom">user</div>

    <!-- Codex-style resize handle on the right edge -->
    <div :class="resizeHandle" @mousedown="startResize">
      <div :class="resizeHandleLine"></div>
    </div>
  </aside>
</template>
