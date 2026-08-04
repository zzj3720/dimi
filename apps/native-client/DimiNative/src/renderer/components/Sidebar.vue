<script setup lang="ts">
// Codex-style left sidebar: brand + new-chat + sessions grouped by workspace.
// Width is draggable via a resize handle on the right edge (Codex behavior).
import { computed, ref } from 'vue';
import { state, Msg } from '../store';
import { dispatch, loadSessions } from '../api';
import {
  sidebar, sidebarTop, brandRow, brand, brandActions, iconBtn,
  navBlock, navItem, sessions, sessionGroup, sessionGroupTitle, sessionItem, sessionItemActive,
  resizeHandle, resizeHandleLine, sidebarBottom, userRow, sidebarBottomBtn,
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

function openSessions(): void {
  dispatch(Msg.PickerOpen());
  void loadSessions();
}

function navComingSoon(name: string): void {
  state.statusMsg = `${name}（暂未实现）`;
}
</script>

<template>
  <aside :class="sidebar" :style="{ width: sidebarWidth + 'px' }">
    <div :class="sidebarTop">
      <div :class="brandRow">
        <div :class="brand" @click="openSessions">
          <span>Dimi</span>
          <svg viewBox="0 0 14 14" fill="none" aria-hidden="true"><path d="M3.5 5.5 7 9l3.5-3.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>
        </div>
        <div :class="brandActions">
          <button :class="iconBtn" title="New chat" @click="newChat">＋</button>
        </div>
      </div>
      <!-- Codex sidebar nav: 新对话 stays FIXED in the header block -->
      <div :class="navBlock">
        <button :class="navItem" @click="newChat">
          <svg viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
          <span>新对话</span>
        </button>
      </div>
    </div>
    <div :class="sessions">
      <!-- Codex: 站点 / 已安排 / 插件 live at the top of the SCROLLING list -->
      <div :class="navBlock" style="padding: 0; margin-bottom: 8px">
        <button :class="navItem" @click="navComingSoon('站点')"><svg viewBox="0 0 16 16" fill="none" aria-hidden="true"><circle cx="8" cy="8" r="6" stroke="currentColor" stroke-width="1.3"/><circle cx="8" cy="8" r="2" fill="currentColor"/></svg><span>站点</span></button>
        <button :class="navItem" @click="navComingSoon('已安排')"><svg viewBox="0 0 16 16" fill="none" aria-hidden="true"><rect x="2.5" y="5" width="11" height="8.5" rx="1.5" stroke="currentColor" stroke-width="1.3"/><path d="M5 5V4a3 3 0 0 1 6 0v1" stroke="currentColor" stroke-width="1.3"/></svg><span>已安排</span></button>
        <button :class="navItem" @click="navComingSoon('插件')"><svg viewBox="0 0 16 16" fill="none" aria-hidden="true"><rect x="2" y="2" width="12" height="12" rx="2" stroke="currentColor" stroke-width="1.3"/><path d="M4 8h8M8 4v8" stroke="currentColor" stroke-width="1.3"/></svg><span>插件</span></button>
      </div>
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
    <!-- Codex sidebar footer: user/account row -->
    <div :class="sidebarBottom">
      <button :class="userRow" @click="dispatch(Msg.SettingsOpen())">
        <span class="sb-avatar">u</span>
        <span class="sb-user">user</span>
      </button>
      <button :class="sidebarBottomBtn" title="Settings" @click="dispatch(Msg.SettingsOpen())">
        <svg viewBox="0 0 18 18" fill="none" aria-hidden="true"><path d="M9 5.5A3.5 3.5 0 1 0 9 12.5 3.5 3.5 0 0 0 9 5.5Z" stroke="currentColor" stroke-width="1.3"/><path d="M9 1.8v2M9 14.2v2M1.8 9h2M14.2 9h2M3.9 3.9l1.4 1.4M12.7 12.7l1.4 1.4M14.1 3.9l-1.4 1.4M5.3 12.7l-1.4 1.4" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>
      </button>
    </div>

    <!-- Codex-style resize handle on the right edge -->
    <div :class="resizeHandle" @mousedown="startResize">
      <div :class="resizeHandleLine"></div>
    </div>
  </aside>
</template>
