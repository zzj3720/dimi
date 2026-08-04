<script setup lang="ts">
// Codex-style header (46px, transparent, fixed): sidebar-trigger/back/forward
// + session title (click → inline rename) + More + share pill + pinned
// summary + toggle-sidebar. No segmented control and no status badges —
// Codex keeps the header clean; status lives in the composer area.
import { computed, nextTick, ref, watch, onMounted, onUnmounted } from 'vue';
import { state } from '../store';
import { api, dispatch, createSession } from '../api';
import { icons } from '../icons';
import {
  header, headerSide, headerSideOpen, headerSideClosed, headerSideGroup,
  headerMain, headerTitle, headerTitleInput, iconBtn, moreBtn, headerRight,
  shareBtn, pinnedBtn, pinnedBtnOn, headerCtxMenu, ctxMenuItem,
  projectBtn, projectPicker, projectPickerItem, projectPickerItemActive, projectPickerEmpty,
} from './HeaderBar.styles';

const current = computed(() => state.sessions.find((s) => s.id === state.currentSessionId));

// Codex disables back/forward when there is no history in that direction
// (S14). dimi simulates history with sidebar-list order, so the ends of the
// list are the disabled states.
const navIndex = computed(() => state.sessions.findIndex((s) => s.id === state.currentSessionId));
const canBack = computed(() => navIndex.value > 0);
const canForward = computed(() => navIndex.value >= 0 && navIndex.value < state.sessions.length - 1);

// Codex left slot width = spring(sidebar width, default 275); dimi's sidebar
// is v-if so the zone snaps between the open width and the natural 180px
// when it is hidden (§1.2 / §2). The open width is live — it tracks
// state.sidebarWidth, which Sidebar.vue's drag updates (store sidebar_resize).
const sideWidthClass = computed(() =>
  state.sidebarVisible ? headerSideOpen(state.sidebarWidth) : headerSideClosed,
);

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

// ---- inline title rename (S12/A5) ----
// Codex: clicking the title swaps it for an inline <input>; Enter/Blur commit
// via onRename, Esc cancels, and the text is auto-selected. dimi's server
// rename endpoint is `POST /sessions/:id/profile {title}` — the same call the
// `/title` slash command makes — so the full interaction is doable in-place.
const editing = ref(false);
const draftTitle = ref('');
const editWidth = ref(189);
const titleSpan = ref<HTMLSpanElement | null>(null);
const titleInput = ref<HTMLInputElement | null>(null);

function startRename(): void {
  if (!state.currentSessionId) return;
  editing.value = true;
  draftTitle.value = current.value?.title || '(untitled)';
  // Size the input to the rendered title (codex's input falls back to its
  // ~20ch ≈ 189px default width); the 189px floor keeps short titles at the
  // codex default instead of shrinking to the text.
  editWidth.value = Math.min(320, Math.max(189, titleSpan.value?.offsetWidth ?? 189));
  void nextTick(() => titleInput.value?.select());
}

function commitRename(): void {
  if (!editing.value) return;
  const id = state.currentSessionId;
  const title = draftTitle.value.trim();
  editing.value = false;
  if (!id || !title) return; // empty → cancel (server keeps the auto title)
  const sess = state.sessions.find((s) => s.id === id);
  api('POST', `/api/v1/sessions/${id}/profile`, { title })
    .then(() => {
      // Local mirror; the session.meta.updated SSE also refreshes the title.
      if (sess) sess.title = title;
    })
    .catch((e) => {
      state.statusMsg = `rename failed: ${(e as Error).message}`;
    });
}

function cancelRename(): void {
  if (!editing.value) return;
  editing.value = false;
  draftTitle.value = '';
}

// Switching sessions while editing discards the draft (no commit).
watch(() => state.currentSessionId, () => cancelRename());

// ---- session context menu on the title (codex HeaderContextMenuItem) ----
const headerMenu = ref<{ x: number; y: number } | null>(null);
const viewport = computed(() => ({ w: window.innerWidth, h: window.innerHeight }));

function openHeaderMenu(e: MouseEvent): void {
  e.preventDefault();
  headerMenu.value = { x: e.clientX, y: e.clientY };
}

function closeHeaderMenu(): void {
  headerMenu.value = null;
}

function headerCopyId(): void {
  const id = state.currentSessionId;
  headerMenu.value = null;
  if (!id) return;
  void navigator.clipboard.writeText(id).then(() => {
    state.statusMsg = '会话 ID 已复制';
  });
}

function headerCopyCwd(): void {
  const cwd = state.currentCwd;
  headerMenu.value = null;
  if (!cwd) {
    state.statusMsg = '该会话无工作目录';
    return;
  }
  void navigator.clipboard.writeText(cwd).then(() => {
    state.statusMsg = '工作目录已复制';
  });
}

function headerFork(): void {
  const id = state.currentSessionId;
  headerMenu.value = null;
  if (!id) return;
  api('POST', `/api/v1/sessions/${id}:fork`, {})
    .then((data) => {
      const fid = (data?.data?.id as string) ?? '';
      state.statusMsg = fid ? `forked ${fid}` : 'forked';
    })
    .catch((e) => {
      state.statusMsg = `fork failed: ${(e as Error).message}`;
    });
}

function headerExport(): void {
  const id = state.currentSessionId;
  headerMenu.value = null;
  if (!id) return;
  api('POST', `/api/v1/sessions/${id}/export`, {})
    .then((data) => {
      const text = typeof data?.data === 'string' ? data.data : JSON.stringify(data?.data ?? {});
      void navigator.clipboard.writeText(text).then(() => {
        state.statusMsg = '导出已复制';
      });
    })
    .catch((e) => {
      state.statusMsg = `export failed: ${(e as Error).message}`;
    });
}

function onHeaderMenuDown(): void {
  if (headerMenu.value) headerMenu.value = null;
  if (projectPickerOpen.value) projectPickerOpen.value = false;
}

function onHeaderMenuEsc(e: KeyboardEvent): void {
  if (e.key === 'Escape' && headerMenu.value) {
    headerMenu.value = null;
    e.stopPropagation();
  }
  if (e.key === 'Escape' && projectPickerOpen.value) {
    projectPickerOpen.value = false;
    e.stopPropagation();
  }
}

// ---- project button (codex col1 env icon: `项目：{cwd}` popover trigger) ----
// Clicking the 28px folder button opens a project list; selecting one starts
// a new session in that cwd (createSession supports a cwd override).
const projectPickerOpen = ref(false);
const projects = computed(() => {
  const seen = new Map<string, string>();
  for (const s of state.sessions) {
    const cwd = s.metadata?.cwd ?? s.cwd ?? '';
    if (cwd && !seen.has(cwd)) seen.set(cwd, cwd);
  }
  return [...seen.values()];
});

const currentProject = computed(() => {
  const cwd = state.currentCwd || current.value?.metadata?.cwd || '';
  return cwd.split('/').filter(Boolean).pop() || cwd || '';
});

function toggleProjectPicker(): void {
  projectPickerOpen.value = !projectPickerOpen.value;
}

function pickProject(cwd: string): void {
  projectPickerOpen.value = false;
  void createSession(cwd).then((id) => {
    if (id) dispatch({ type: 'session_selected', id });
  });
}

onMounted(() => {
  document.addEventListener('mousedown', onHeaderMenuDown);
  document.addEventListener('keydown', onHeaderMenuEsc);
});

onUnmounted(() => {
  document.removeEventListener('mousedown', onHeaderMenuDown);
  document.removeEventListener('keydown', onHeaderMenuEsc);
});

// 固定摘要 (pinned summary): Codex toggles a summary panel (pressed → white
// 5% bg + aria-pressed). dimi has no summary panel yet, so this is a local
// visual toggle only — needs product wiring when the feature lands (A2).
const pinnedSummaryOn = ref(true);
function togglePinnedSummary(): void {
  pinnedSummaryOn.value = !pinnedSummaryOn.value;
}

// More / Share are behavior placeholders in dimi (no conversation-actions or
// share menu yet, A3/A4): keep the previous intent — open the help dialog.
function openHelp(): void {
  state.helpDialogOpen = true;
}
</script>

<template>
  <header :class="header">
    <!-- Left zone (sidebar-width slot, safe-left 88px): sidebar-trigger / back / forward -->
    <div :class="[headerSide, sideWidthClass]">
      <div :class="headerSideGroup">
        <!-- The first button IS the sidebar trigger in codex (L2): aria-label
             and icon follow sidebar visibility (A7/§6.2). icons.ts lacks the
             wRr (show-sidebar) glyph — codex's wRr is DRr's mirror (§8), so
             hideSidebar is mirrored with scaleX(-1). -->
        <button :class="iconBtn" type="button" data-app-shell-sidebar-trigger
                :aria-label="state.sidebarVisible ? '隐藏边栏' : '显示边栏'"
                @click="toggleSidebar">
          <svg :viewBox="icons.hideSidebar.vb" fill="currentColor" aria-hidden="true"
               :style="state.sidebarVisible ? undefined : { transform: 'scaleX(-1)' }">
            <path v-for="(p, i) in icons.hideSidebar.paths" :key="i" :d="p" />
          </svg>
        </button>
        <button :class="iconBtn" type="button" aria-label="返回" data-tooltip="返回" :disabled="!canBack" @click="navSession(-1)">
          <svg :viewBox="icons.back.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.back.paths" :key="i" :d="p" /></svg>
        </button>
        <button :class="iconBtn" type="button" aria-label="前进" data-tooltip="前进" :disabled="!canForward" @click="navSession(1)">
          <svg :viewBox="icons.forward.vb" fill="currentColor" aria-hidden="true" style="transform: scaleX(-1)"><path v-for="(p, i) in icons.forward.paths" :key="i" :d="p" /></svg>
        </button>
      </div>
    </div>
    <!-- Main zone: project button + session title (inline rename) + More -->
    <div :class="headerMain">
      <button :class="projectBtn" type="button" :aria-label="`项目：${currentProject}`" :aria-expanded="projectPickerOpen" data-tooltip="切换项目" @click="toggleProjectPicker">
        <svg :viewBox="icons.folder.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.folder.paths" :key="i" :d="p" /></svg>
      </button>
      <input v-if="editing" ref="titleInput" v-model="draftTitle" :class="headerTitleInput"
             :style="{ width: `${editWidth}px` }" aria-label="会话标题"
             @keydown.enter.prevent="commitRename"
             @keydown.esc.prevent="cancelRename"
             @blur="commitRename" />
      <button v-else :class="headerTitle" type="button" @click="startRename" @contextmenu.prevent="openHeaderMenu($event)">
        <span ref="titleSpan">{{ current?.title || '(untitled)' }}</span>
      </button>
      <button :class="[iconBtn, moreBtn]" type="button" aria-label="ChatGPT 对话操作" @click="openHelp">
        <svg :viewBox="icons.ellipsis.vb" fill="currentColor" aria-hidden="true" style="width: 18px; height: 18px"><path v-for="(p, i) in icons.ellipsis.paths" :key="i" :d="p" /></svg>
      </button>
    </div>
    <!-- Right zone: share pill + pinned summary + toggle sidebar -->
    <div :class="headerRight">
      <button :class="shareBtn" type="button" aria-label="分享" data-tooltip="分享" @click="openHelp">
        <svg :viewBox="icons.share.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.share.paths" :key="i" :d="p" /></svg>
        <span>分享</span>
      </button>
      <button :class="[pinnedBtn, { [pinnedBtnOn]: pinnedSummaryOn }]" type="button"
              :aria-pressed="pinnedSummaryOn" aria-label="切换固定摘要" data-tooltip="切换固定摘要"
              @click="togglePinnedSummary">
        <svg :viewBox="icons.dots.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.dots.paths" :key="i" :d="p" /></svg>
      </button>
      <!-- dimi maps codex's right-panel toggle (HeaderButton pressed=isOpen)
           onto the left sidebar until a right panel exists; the pressed
           secondary state is left off to match codex's default ghost look. -->
      <button :class="iconBtn" type="button" aria-label="切换侧边栏" data-tooltip="切换侧边栏" :aria-pressed="state.sidebarVisible" @click="toggleSidebar">
        <svg :viewBox="icons.menu.vb" fill="currentColor" aria-hidden="true" style="transform: rotate(180deg)"><path v-for="(p, i) in icons.menu.paths" :key="i" :d="p" /></svg>
      </button>
    </div>

    <!-- Project picker (codex col1 env popover) -->
    <div
      v-if="projectPickerOpen"
      :class="projectPicker"
      :style="{ left: (state.sidebarWidth + 8) + 'px', top: 'calc(100% - 4px)' }"
      role="menu"
      @mousedown.stop
    >
      <button v-for="p in projects" :key="p" :class="[projectPickerItem, { [projectPickerItemActive]: p === (current?.metadata?.cwd ?? state.currentCwd) }]" type="button" role="menuitem" @click="pickProject(p)">
        <svg :viewBox="icons.folder.vb" fill="currentColor" aria-hidden="true" style="width: 14px; height: 14px; flex-shrink: 0"><path v-for="(pi, i) in icons.folder.paths" :key="i" :d="pi" /></svg>
        <span style="overflow: hidden; text-overflow: ellipsis; white-space: nowrap">{{ p }}</span>
      </button>
      <div v-if="projects.length === 0" :class="projectPickerEmpty">无项目</div>
    </div>

    <!-- Session context menu (codex HeaderContextMenuItem subset) -->
    <div
      v-if="headerMenu"
      :class="headerCtxMenu"
      :style="{ left: Math.min(headerMenu.x, viewport.w - 200) + 'px', top: Math.min(headerMenu.y, viewport.h - 220) + 'px' }"
      role="menu"
      @mousedown.stop
    >
      <button :class="ctxMenuItem" type="button" role="menuitem" @click="startRename">
        <span>重命名会话</span>
      </button>
      <button :class="ctxMenuItem" type="button" role="menuitem" @click="headerCopyId">
        <span>复制会话 ID</span>
      </button>
      <button :class="ctxMenuItem" type="button" role="menuitem" @click="headerCopyCwd">
        <span>复制工作目录</span>
      </button>
      <button :class="ctxMenuItem" type="button" role="menuitem" @click="headerFork">
        <span>Fork 会话</span>
      </button>
      <button :class="ctxMenuItem" type="button" role="menuitem" @click="headerExport">
        <span>导出复制</span>
      </button>
    </div>
  </header>
</template>
