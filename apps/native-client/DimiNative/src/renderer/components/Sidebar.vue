<script setup lang="ts">
// Codex-style left sidebar: brand + new-chat + sessions grouped by workspace.
// Width is draggable via a resize handle on the right edge (Codex behavior).
import { computed, ref } from 'vue';
import { state, Msg } from '../store';
import { dispatch } from '../api';
import { icons } from '../icons';
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

function navComingSoon(name: string): void {
  state.statusMsg = `${name}（暂未实现）`;
}
</script>

<template>
  <aside :class="sidebar" :style="{ width: sidebarWidth + 'px' }">
    <div :class="sidebarTop">
      <div :class="brandRow">
        <div :class="brand">
          <span>Dimi</span>
          <svg :viewBox="icons.chevronDown.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.chevronDown.paths" :key="i" :d="p" /></svg>
        </div>
        <div :class="brandActions">
          <button :class="iconBtn" title="New chat" @click="newChat">
            <svg :viewBox="icons.newChat.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.newChat.paths" :key="i" :d="p" /></svg>
          </button>
        </div>
      </div>
      <!-- Codex sidebar nav: 新对话 stays FIXED in the header block -->
      <div :class="navBlock">
        <button :class="navItem" @click="newChat">
          <svg :viewBox="icons.newChat.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.newChat.paths" :key="i" :d="p" /></svg>
          <span>新对话</span>
        </button>
      </div>
    </div>
    <div :class="sessions">
      <!-- Codex: 站点 / 已安排 / 插件 live at the top of the SCROLLING list -->
      <div :class="navBlock" style="padding: 0; margin-bottom: 8px">
        <button :class="navItem" @click="navComingSoon('站点')"><svg :viewBox="icons.sites.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.sites.paths" :key="i" :d="p" /></svg><span>站点</span></button>
        <button :class="navItem" @click="navComingSoon('已安排')"><svg :viewBox="icons.scheduled.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.scheduled.paths" :key="i" :d="p" /></svg><span>已安排</span></button>
        <button :class="navItem" @click="navComingSoon('插件')"><svg :viewBox="icons.plugins.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.plugins.paths" :key="i" :d="p" /></svg><span>插件</span></button>
      </div>
      <div v-if="state.sessions.length === 0" :class="sessionGroupTitle">
        {{ state.sessionsLoading ? '加载中…' : '暂无会话' }}
      </div>
      <div v-for="[cwd, list] in groups" :key="cwd" :class="sessionGroup">
        <div :class="sessionGroupTitle" :title="cwd">
          <svg class="chevron" :viewBox="icons.sectionChevron.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.sectionChevron.paths" :key="i" :d="p" /></svg>
          <span>{{ groupLabel(cwd) }}</span>
        </div>
        <div
          v-for="s in list"
          :key="s.id"
          :class="[sessionItem, { [sessionItemActive]: s.id === state.currentSessionId }]"
          @click="select(s.id)"
        >
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
        <svg :viewBox="icons.gear.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.gear.paths" :key="i" :d="p" /></svg>
      </button>
    </div>

    <!-- Codex-style resize handle on the right edge -->
    <div :class="resizeHandle" @mousedown="startResize">
      <div :class="resizeHandleLine"></div>
    </div>
  </aside>
</template>
