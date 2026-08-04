<script setup lang="ts">
// Codex-style left sidebar: brand row (mode switch + search + priority),
// fixed nav (新对话 / 站点 / 已安排 / 插件), 项目 folder tree + 最近, and a
// footer user row. Width is draggable via a resize handle (Codex behavior).
// Measurements: design/02-sidebar.md + design/02-sidebar-code.md (codex 实测).
import {
  computed,
  nextTick,
  onMounted,
  onUnmounted,
  reactive,
  ref,
  watch,
} from 'vue';
import { state, Msg, SIDEBAR_WIDTH_KEY, type SessionSummary } from '../store';
import { dispatch, api } from '../api';
import { icons, type IconDef } from '../icons';
import {
  sidebar, sidebarTop, sidebarTopSearch, sidebarTopScrolled,
  brandRow, brand, brandOpen, brandActions, brandIconBtn, brandIconBtnSearch,
  navItemHeader, navItemScroll, navBlockScroll, navBlockItems,
  sessions, sessionsSearch, sessionsScrolled, section, sectionTitleRow, sectionToggle, sectionTitleActions, sectionTitleBtn, emptyRow,
  folderGroup, folderRow, folderRowIcon, folderRowName, folderRowActions, folderRowBtn, sessionList,
  sessionItem, sessionItemActive,
  resizeHandle, resizeHandleLine, sidebarBottom, userRow, userRowOpen, sidebarBottomBtn, sidebarBottomBtnOpen,
  menuAnchor, menuAnchorGrow, menu, menuWide, menuTop, menuBottomLeft, menuBottomRight, menuItem, menuCheck, ctxMenuStyle, editInput,
  searchView, searchInputRow, searchInput, searchClear,
} from './Sidebar.styles';

// Sidebar width is SHARED state (store.ts): the drag writes state.sidebarWidth
// via Msg.SidebarResize (clamped by the reducer, 240–520), HeaderBar reads the
// same value for its left slot, and mouseup persists it under SIDEBAR_WIDTH_KEY.
function startResize(e: MouseEvent): void {
  e.preventDefault();
  const startX = e.clientX;
  const startW = state.sidebarWidth;
  const move = (ev: MouseEvent): void => {
    dispatch(Msg.SidebarResize(startW + (ev.clientX - startX)));
  };
  const up = (): void => {
    document.removeEventListener('mousemove', move);
    document.removeEventListener('mouseup', up);
    try {
      localStorage.setItem(SIDEBAR_WIDTH_KEY, String(state.sidebarWidth));
    } catch {
      /* non-fatal */
    }
  };
  document.addEventListener('mousemove', move);
  document.addEventListener('mouseup', up);
}

// Missing icons (priority / filter / badgeIcon) are not in icons.ts yet —
// render the correct box with an empty glyph so the layout stays pixel-correct.
const FALLBACK_VB: Record<string, string> = {
  priority: '0 0 20 20',
  filter: '0 0 16 16',
  badgeIcon: '0 0 20 20',
};
function ic(name: string): IconDef {
  const def = icons[name];
  return def ?? { vb: FALLBACK_VB[name] ?? '0 0 20 20', paths: [] };
}

// Section / folder fold state.
const projectsCollapsed = ref(false);
const recentCollapsed = ref(false);
const collapsedFolders = reactive<Record<string, boolean>>({});

function toggleFolder(cwd: string): void {
  collapsedFolders[cwd] = !collapsedFolders[cwd];
}

// Sessions with a cwd become folder rows under 项目; sessions without one
// land in 最近. Archived sessions are hidden; pinned ones move to the
// Pinned section.
const groups = computed(() => {
  const map = new Map<string, typeof state.sessions>();
  for (const s of state.sessions) {
    const cwd = s.metadata?.cwd ?? s.cwd ?? '';
    if (!cwd) continue;
    if (state.archivedIds.includes(s.id)) continue;
    if (!map.has(cwd)) map.set(cwd, []);
    map.get(cwd)!.push(s);
  }
  return [...map.entries()];
});

const recent = computed(() =>
  state.sessions.filter(
    (s) => !(s.metadata?.cwd ?? s.cwd) && !state.pinnedIds.includes(s.id) && !state.archivedIds.includes(s.id),
  ),
);

// Codex Pinned section (local simulation: the server has no pin API, so the
// pinned set lives in localStorage via the pin_toggle reducer).
const pinned = computed(() =>
  state.sessions
    .filter((s) => state.pinnedIds.includes(s.id))
    .sort((a, b) => (b.updated_at ?? 0) - (a.updated_at ?? 0)),
);

function togglePin(s: SessionSummary): void {
  const pin = !state.pinnedIds.includes(s.id);
  dispatch(Msg.PinToggle(s.id));
  state.statusMsg = pin ? '已置顶' : '已取消置顶';
}

function toggleArchive(s: SessionSummary): void {
  const arch = !state.archivedIds.includes(s.id);
  dispatch(Msg.ArchiveToggle(s.id));
  state.statusMsg = arch ? '已归档' : '已取消归档';
}

function groupLabel(cwd: string): string {
  return cwd.split('/').filter(Boolean).pop() || cwd || '其他';
}

function sessionCwd(s: SessionSummary): string {
  return s.metadata?.cwd ?? s.cwd ?? '';
}

function sessionTitle(s: SessionSummary): string {
  return s.title || '(untitled)';
}

function select(id: string): void {
  dispatch({ type: 'session_selected', id });
}

function newChat(): void {
  window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'new_chat' } }));
}

function comingSoon(name: string): void {
  state.statusMsg = `${name}（暂未实现）`;
}

// ---- session row context menu (codex Work Ztu: right-click thread row) ----
// Codex shows a 15-item menu (pin / rename / archive / read-state / fork /
// folder / copy cwd / copy id / copy link / open in new window). dimi
// implements the subset its server supports: rename (inline edit), fork,
// export-to-clipboard, copy id, copy cwd.
const ctxMenu = ref<{ x: number; y: number; session: SessionSummary } | null>(null);
const editingSessionId = ref<string | null>(null);
const editDraft = ref('');
const editInputEl = ref<HTMLInputElement | null>(null);
const viewport = computed(() => ({ w: window.innerWidth, h: window.innerHeight }));

function openCtxMenu(e: MouseEvent, s: SessionSummary): void {
  e.preventDefault();
  e.stopPropagation();
  editingSessionId.value = null;
  ctxMenu.value = { x: e.clientX, y: e.clientY, session: s };
}

function closeCtxMenu(): void {
  ctxMenu.value = null;
  editingSessionId.value = null;
}

function ctxRename(): void {
  const s = ctxMenu.value?.session;
  if (!s) return;
  editDraft.value = sessionTitle(s);
  editingSessionId.value = s.id;
  ctxMenu.value = null;
  void nextTick(() => editInputEl.value?.focus());
}

function commitRename(id: string): void {
  const t = editDraft.value.trim();
  editingSessionId.value = null;
  if (!t) return;
  api('POST', `/api/v1/sessions/${id}/profile`, { title: t })
    .then(() => {
      state.statusMsg = `renamed to ${t}`;
    })
    .catch((e) => {
      state.statusMsg = `rename failed: ${(e as Error).message}`;
    });
}

function ctxCopyId(): void {
  const s = ctxMenu.value?.session;
  ctxMenu.value = null;
  if (!s) return;
  void navigator.clipboard.writeText(s.id).then(() => {
    state.statusMsg = '会话 ID 已复制';
  });
}

function ctxCopyCwd(): void {
  const s = ctxMenu.value?.session;
  ctxMenu.value = null;
  if (!s) return;
  const cwd = sessionCwd(s);
  if (!cwd) {
    state.statusMsg = '该会话无工作目录';
    return;
  }
  void navigator.clipboard.writeText(cwd).then(() => {
    state.statusMsg = '工作目录已复制';
  });
}

function ctxFork(): void {
  const s = ctxMenu.value?.session;
  ctxMenu.value = null;
  if (!s) return;
  api('POST', `/api/v1/sessions/${s.id}:fork`, {})
    .then((data) => {
      const id = (data?.data?.id as string) ?? '';
      state.statusMsg = id ? `forked ${id}` : 'forked';
    })
    .catch((e) => {
      state.statusMsg = `fork failed: ${(e as Error).message}`;
    });
}

function ctxExport(): void {
  const s = ctxMenu.value?.session;
  ctxMenu.value = null;
  if (!s) return;
  api('POST', `/api/v1/sessions/${s.id}/export`, {})
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

function onGlobalDown(): void {
  if (ctxMenu.value) closeCtxMenu();
}

function onGlobalEsc(e: KeyboardEvent): void {
  if (e.key === 'Escape' && (ctxMenu.value || editingSessionId.value)) {
    closeCtxMenu();
    e.stopPropagation();
  }
}

onMounted(() => {
  document.addEventListener('mousedown', onGlobalDown);
  document.addEventListener('keydown', onGlobalEsc);
});

onUnmounted(() => {
  document.removeEventListener('mousedown', onGlobalDown);
  document.removeEventListener('keydown', onGlobalEsc);
});

// ---- scroll-linked header state (02-sidebar-code §2.5, C state) ----
// scrolledContentUnderHeader: once the session list actually scrolls
// (scrollTop > 0), the header block's pb grows 1px → 4px with a 0.5px divider
// (--sidebar-scroll-header-spacing) and the scroll area's top fade widens to
// 4px→16px. Search mode keeps its own spacing approximation (see styles).
const scrolled = ref(false);
const sessionsEl = ref<HTMLElement | null>(null);
function onSessionsScroll(e: Event): void {
  scrolled.value = (e.target as HTMLElement).scrollTop > 0;
}

// ---- search mode (02-sidebar-code C4 / A1) ----
// Codex swaps the scroll content for a search view (idu) and switches the
// header/scroll spacing vars; idu's internals are 无法确定, so the view is a
// client-side title filter over state.sessions (dimi's own approximation).
const searchOpen = ref(false);
const searchQuery = ref('');
const searchInputEl = ref<HTMLInputElement | null>(null);

const searchResults = computed(() => {
  const q = searchQuery.value.trim().toLowerCase();
  if (!q) return state.sessions;
  const tokens = q.split(/[\s/]+/).filter((t) => t.length > 0);
  return state.sessions.filter((s) => {
    const text = `${sessionTitle(s)} ${sessionCwd(s)}`.toLowerCase();
    return tokens.every((t) => text.includes(t));
  });
});

function openSearch(): void {
  closeMenus();
  searchOpen.value = true;
}

function closeSearch(): void {
  searchOpen.value = false;
  searchQuery.value = '';
}

function toggleSearch(): void {
  if (searchOpen.value) closeSearch();
  else openSearch();
}

watch(searchOpen, (open) => {
  if (open) void nextTick(() => searchInputEl.value?.focus());
});

// ---- dropdown menus (mode / profile / help, 02-sidebar-code A5/C1/C8) ----
// Single-open Radix-style menus; outside click / Escape closes them.
type MenuName = 'mode' | 'profile' | 'help';
const openMenu = ref<MenuName | null>(null);
const modeAnchor = ref<HTMLElement | null>(null);
const profileAnchor = ref<HTMLElement | null>(null);
const helpAnchor = ref<HTMLElement | null>(null);

function toggleMenu(name: MenuName): void {
  openMenu.value = openMenu.value === name ? null : name;
}

function closeMenus(): void {
  openMenu.value = null;
}

function onDocMousedown(e: MouseEvent): void {
  if (!openMenu.value) return;
  const t = e.target as Node;
  const anchors = [modeAnchor.value, profileAnchor.value, helpAnchor.value];
  if (anchors.some((a) => a?.contains(t))) return;
  closeMenus();
}

function onDocKeydown(e: KeyboardEvent): void {
  if (e.key !== 'Escape') return;
  closeMenus();
  closeSearch();
}

onMounted(() => {
  document.addEventListener('mousedown', onDocMousedown);
  document.addEventListener('keydown', onDocKeydown);
  // Restore the scroll-linked header state if the list starts scrolled.
  scrolled.value = (sessionsEl.value?.scrollTop ?? 0) > 0;
});
onUnmounted(() => {
  document.removeEventListener('mousedown', onDocMousedown);
  document.removeEventListener('keydown', onDocKeydown);
});

function openSettings(): void {
  dispatch(Msg.SettingsOpen());
  closeMenus();
}

function openHelpDialog(): void {
  state.helpDialogOpen = true;
  closeMenus();
}

// ---- session title marquee (02-sidebar-code A6) ----
// Codex keyframes are 无法确定; the title scrolls by its measured overflow
// while the row is hovered (--sb-marquee-dx injected per row).
const marqueeActive = reactive(new Set<string>());

function marqueeEnter(e: MouseEvent, id: string): void {
  const outer = e.currentTarget as HTMLElement;
  requestAnimationFrame(() => {
    const inner = outer.querySelector<HTMLElement>('.sb-title-inner');
    if (!inner) return;
    const dx = inner.scrollWidth - outer.clientWidth;
    if (dx > 0) {
      inner.style.setProperty('--sb-marquee-dx', `-${dx}px`);
      marqueeActive.add(id);
    }
  });
}

function marqueeLeave(id: string): void {
  marqueeActive.delete(id);
}
</script>

<template>
  <aside :class="sidebar" :style="{ width: state.sidebarWidth + 'px' }">
    <!-- header block: brand row + 新对话 (70px) -->
    <div :class="[sidebarTop, { [sidebarTopSearch]: searchOpen, [sidebarTopScrolled]: scrolled && !searchOpen }]">
      <div :class="brandRow">
        <div :class="menuAnchor" ref="modeAnchor">
          <button
            :class="[brand, { [brandOpen]: openMenu === 'mode' }]"
            type="button"
            aria-label="切换模式，当前模式：Dimi"
            aria-haspopup="menu"
            :aria-expanded="openMenu === 'mode'"
            @click="toggleMenu('mode')"
          >
            <span>Dimi</span>
            <svg :viewBox="icons.sectionChevron.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.sectionChevron.paths" :key="i" :d="p" /></svg>
          </button>
          <!-- Radix-style mode menu (p-1.5 / menuWide); dimi has one mode -->
          <div v-if="openMenu === 'mode'" :class="[menu, menuWide, menuTop]" role="menu">
            <button :class="menuItem" type="button" role="menuitemradio" aria-checked="true" @click="closeMenus">
              <span :class="menuCheck"><svg :viewBox="icons.radio.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.radio.paths" :key="i" :d="p" /></svg></span>
              <span>Dimi</span>
            </button>
          </div>
        </div>
        <div :class="brandActions">
          <button :class="[brandIconBtn, brandIconBtnSearch]" type="button" aria-label="搜索" data-tooltip="搜索" @click="toggleSearch">
            <svg :viewBox="ic('search').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('search').paths" :key="i" :d="p" /></svg>
          </button>
          <button :class="brandIconBtn" type="button" aria-label="优先级，需要关注" aria-pressed="false" data-tooltip="优先级" @click="comingSoon('优先级')">
            <svg :viewBox="ic('priority').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('priority').paths" :key="i" :d="p" /></svg>
          </button>
        </div>
      </div>
      <!-- 新对话 stays FIXED in the header block (29px row) -->
      <button :class="navItemHeader" type="button" data-tooltip="新对话" @click="newChat">
        <svg :viewBox="icons.newChat.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.newChat.paths" :key="i" :d="p" /></svg>
        <span>新对话</span>
      </button>
    </div>

    <!-- scroll area (masked top/bottom) -->
    <div
      ref="sessionsEl"
      :class="[sessions, { [sessionsSearch]: searchOpen, [sessionsScrolled]: scrolled && !searchOpen }]"
      @scroll="onSessionsScroll"
    >
      <!-- fixed nav block: 站点 / 已安排 / 插件 (92px) -->
      <div :class="navBlockScroll">
        <div :class="navBlockItems">
          <button :class="navItemScroll" type="button" data-tooltip="站点" @click="comingSoon('站点')">
            <svg :viewBox="icons.sites.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.sites.paths" :key="i" :d="p" /></svg><span>站点</span>
          </button>
          <button :class="navItemScroll" type="button" data-tooltip="已安排" @click="comingSoon('已安排')">
            <svg :viewBox="icons.scheduled.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.scheduled.paths" :key="i" :d="p" /></svg><span>已安排</span>
          </button>
          <button :class="navItemScroll" type="button" data-tooltip="插件" @click="comingSoon('插件')">
            <svg :viewBox="icons.plugins.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.plugins.paths" :key="i" :d="p" /></svg><span>插件</span>
          </button>
        </div>
      </div>

      <!-- search mode replaces the 项目 / 最近 sections (idu) -->
      <template v-if="!searchOpen">
        <!-- 项目 section: cwd folder tree -->
        <section :class="section">
          <div :class="sectionTitleRow">
            <button :class="sectionToggle" type="button" :aria-expanded="!projectsCollapsed" @click="projectsCollapsed = !projectsCollapsed">
              <svg class="sb-chevron" :class="{ collapsed: projectsCollapsed }" :viewBox="icons.sectionChevron.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.sectionChevron.paths" :key="i" :d="p" /></svg>
              <span>项目</span>
            </button>
            <div class="sb-title-actions" :class="sectionTitleActions">
              <button :class="sectionTitleBtn" type="button" aria-label="项目侧边栏选项" data-tooltip="项目侧边栏选项" @click="comingSoon('项目侧边栏选项')">
                <svg :viewBox="icons.ellipsis.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.ellipsis.paths" :key="i" :d="p" /></svg>
              </button>
              <button :class="sectionTitleBtn" type="button" aria-label="添加新项目" data-tooltip="添加新项目" @click="comingSoon('添加新项目')">
                <svg :viewBox="icons.plus.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.plus.paths" :key="i" :d="p" /></svg>
              </button>
            </div>
          </div>
          <template v-if="!projectsCollapsed">
            <div v-if="groups.length === 0" :class="emptyRow">
              {{ state.sessionsLoading ? '加载中…' : '暂无会话' }}
            </div>
            <div v-for="[cwd, list] in groups" :key="cwd" :class="folderGroup" role="listitem" :aria-label="cwd">
              <div
                :class="folderRow"
                role="button"
                tabindex="0"
                :title="cwd"
                :aria-expanded="!collapsedFolders[cwd]"
                @click="toggleFolder(cwd)"
                @keydown.enter.prevent="toggleFolder(cwd)"
                @keydown.space.prevent="toggleFolder(cwd)"
              >
                <span :class="folderRowIcon">
                  <svg :viewBox="ic('folder').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('folder').paths" :key="i" :d="p" /></svg>
                </span>
                <span :class="folderRowName">{{ groupLabel(cwd) }}</span>
                <span class="sb-folder-actions" :class="folderRowActions">
                  <button :class="folderRowBtn" type="button" :aria-label="`${groupLabel(cwd)} 的项目操作`" :title="`${groupLabel(cwd)} 的项目操作`" @click.stop="comingSoon('项目操作')">
                    <svg :viewBox="icons.ellipsis.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.ellipsis.paths" :key="i" :d="p" /></svg>
                  </button>
                  <button :class="folderRowBtn" type="button" :aria-label="`在 ${groupLabel(cwd)} 中开始新聊天`" :title="`在 ${groupLabel(cwd)} 中开始新聊天`" @click.stop="newChat">
                    <svg :viewBox="icons.newChat.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.newChat.paths" :key="i" :d="p" /></svg>
                  </button>
                </span>
              </div>
              <div v-if="!collapsedFolders[cwd]" :class="sessionList">
                <div
                  v-for="s in list"
                  :key="s.id"
                  :class="[sessionItem, { [sessionItemActive]: s.id === state.currentSessionId }]"
                  role="listitem"
                  :aria-label="sessionTitle(s)"
                  :aria-current="s.id === state.currentSessionId ? 'page' : undefined"
                  :title="sessionTitle(s)"
                  @click="select(s.id)"
                  @contextmenu.prevent="openCtxMenu($event, s)"
                >
                  <span class="sb-slot"></span>
                  <span class="s-title" @mouseenter="marqueeEnter($event, s.id)" @mouseleave="marqueeLeave(s.id)">
                    <template v-if="editingSessionId === s.id">
                      <input
                        v-model="editDraft"
                        :class="editInput"
                        type="text"
                        :aria-label="`重命名 ${sessionTitle(s)}`"
                        @click.stop
                        @keydown.enter.prevent="commitRename(s.id)"
                        @keydown.esc.stop.prevent="editingSessionId = null"
                      />
                    </template>
                    <span v-else class="sb-title-inner" :class="{ 'sb-marquee': marqueeActive.has(s.id) }">{{ sessionTitle(s) }}</span>
                  </span>
                  <span class="sb-row-spacer"></span>
                  <span class="sb-badge"><span class="sb-badge-box"><svg :viewBox="ic('badgeIcon').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('badgeIcon').paths" :key="i" :d="p" /></svg></span></span>
                  <span class="sb-hover-actions">
                    <button class="sb-hover-btn" type="button" :aria-label="state.pinnedIds.includes(s.id) ? '取消置顶' : '置顶聊天'" :data-tooltip="state.pinnedIds.includes(s.id) ? '取消置顶' : '置顶聊天'" @click.stop="togglePin(s)">
                      <svg class="sb-pin" :viewBox="ic('pin').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('pin').paths" :key="i" :d="p" /></svg>
                    </button>
                    <button class="sb-hover-btn" type="button" :aria-label="state.archivedIds.includes(s.id) ? '取消归档' : '归档聊天'" :data-tooltip="state.archivedIds.includes(s.id) ? '取消归档' : '归档聊天'" @click.stop="toggleArchive(s)">
                      <svg :viewBox="ic('archive').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('archive').paths" :key="i" :d="p" /></svg>
                    </button>
                  </span>
                </div>
              </div>
            </div>
          </template>
        </section>

        <!-- 置顶 section (codex Pinned; local simulation) -->
        <section v-if="pinned.length > 0" :class="section">
          <div :class="sectionTitleRow">
            <button :class="sectionToggle" type="button" :aria-expanded="!recentCollapsed" @click="recentCollapsed = !recentCollapsed">
              <svg class="sb-chevron" :class="{ collapsed: recentCollapsed }" :viewBox="icons.sectionChevron.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.sectionChevron.paths" :key="i" :d="p" /></svg>
              <span>置顶</span>
            </button>
          </div>
          <div v-if="!recentCollapsed" :class="sessionList">
            <div
              v-for="s in pinned"
              :key="s.id"
              :class="[sessionItem, { [sessionItemActive]: s.id === state.currentSessionId }]"
              role="listitem"
              :aria-label="sessionTitle(s)"
              :aria-current="s.id === state.currentSessionId ? 'page' : undefined"
              :title="sessionTitle(s)"
              @click="select(s.id)"
              @contextmenu.prevent="openCtxMenu($event, s)"
            >
              <span class="sb-slot"></span>
              <span class="s-title" @mouseenter="marqueeEnter($event, s.id)" @mouseleave="marqueeLeave(s.id)">
                <span class="sb-title-inner" :class="{ 'sb-marquee': marqueeActive.has(s.id) }">{{ sessionTitle(s) }}</span>
              </span>
              <span class="sb-row-spacer"></span>
              <span class="sb-badge"><span class="sb-badge-box"><svg :viewBox="ic('badgeIcon').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('badgeIcon').paths" :key="i" :d="p" /></svg></span></span>
              <span class="sb-hover-actions">
                <button class="sb-hover-btn" type="button" aria-label="取消置顶" data-tooltip="取消置顶" @click.stop="togglePin(s)">
                  <svg class="sb-pin" :viewBox="ic('pin').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('pin').paths" :key="i" :d="p" /></svg>
                </button>
                <button class="sb-hover-btn" type="button" aria-label="归档聊天" data-tooltip="归档聊天" @click.stop="toggleArchive(s)">
                  <svg :viewBox="ic('archive').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('archive').paths" :key="i" :d="p" /></svg>
                </button>
              </span>
            </div>
          </div>
        </section>

        <!-- 最近 section: chats without a cwd -->
        <section :class="section">
          <div :class="sectionTitleRow">
            <button :class="sectionToggle" type="button" :aria-expanded="!recentCollapsed" @click="recentCollapsed = !recentCollapsed">
              <svg class="sb-chevron" :class="{ collapsed: recentCollapsed }" :viewBox="icons.sectionChevron.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.sectionChevron.paths" :key="i" :d="p" /></svg>
              <span>最近</span>
            </button>
            <div class="sb-title-actions" :class="sectionTitleActions">
              <button :class="sectionTitleBtn" type="button" aria-label="聊天侧边栏选项" data-tooltip="聊天侧边栏选项" @click="comingSoon('聊天侧边栏选项')">
                <svg :viewBox="icons.ellipsis.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.ellipsis.paths" :key="i" :d="p" /></svg>
              </button>
              <button :class="sectionTitleBtn" type="button" aria-label="筛选聊天和工作" data-tooltip="筛选聊天和工作" @click="comingSoon('筛选聊天和工作')">
                <svg :viewBox="ic('filter').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('filter').paths" :key="i" :d="p" /></svg>
              </button>
              <button :class="sectionTitleBtn" type="button" aria-label="新对话" data-tooltip="新对话" @click="newChat">
                <svg :viewBox="icons.newChat.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.newChat.paths" :key="i" :d="p" /></svg>
              </button>
            </div>
          </div>
          <div v-if="!recentCollapsed && recent.length > 0" :class="sessionList">
            <div
              v-for="s in recent"
              :key="s.id"
              :class="[sessionItem, { [sessionItemActive]: s.id === state.currentSessionId }]"
              role="listitem"
              :aria-label="sessionTitle(s)"
              :aria-current="s.id === state.currentSessionId ? 'page' : undefined"
              :title="sessionTitle(s)"
              @click="select(s.id)"
              @contextmenu.prevent="openCtxMenu($event, s)"
            >
              <span class="sb-slot"></span>
              <span class="s-title" @mouseenter="marqueeEnter($event, s.id)" @mouseleave="marqueeLeave(s.id)">
                <template v-if="editingSessionId === s.id">
                  <input
                    ref="editInputEl"
                    v-model="editDraft"
                    :class="editInput"
                    type="text"
                    :aria-label="`重命名 ${sessionTitle(s)}`"
                    @click.stop
                    @keydown.enter.prevent="commitRename(s.id)"
                    @keydown.esc.stop.prevent="editingSessionId = null"
                  />
                </template>
                <span v-else class="sb-title-inner" :class="{ 'sb-marquee': marqueeActive.has(s.id) }">{{ sessionTitle(s) }}</span>
              </span>
              <span class="sb-row-spacer"></span>
              <span class="sb-badge"><span class="sb-badge-box"><svg :viewBox="ic('badgeIcon').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('badgeIcon').paths" :key="i" :d="p" /></svg></span></span>
              <span class="sb-hover-actions">
                <button class="sb-hover-btn" type="button" :aria-label="state.pinnedIds.includes(s.id) ? '取消置顶' : '置顶聊天'" :data-tooltip="state.pinnedIds.includes(s.id) ? '取消置顶' : '置顶聊天'" @click.stop="togglePin(s)">
                  <svg class="sb-pin" :viewBox="ic('pin').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('pin').paths" :key="i" :d="p" /></svg>
                </button>
                <button class="sb-hover-btn" type="button" :aria-label="state.archivedIds.includes(s.id) ? '取消归档' : '归档聊天'" :data-tooltip="state.archivedIds.includes(s.id) ? '取消归档' : '归档聊天'" @click.stop="toggleArchive(s)">
                  <svg :viewBox="ic('archive').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('archive').paths" :key="i" :d="p" /></svg>
                </button>
              </span>
            </div>
          </div>
        </section>
      </template>

      <!-- search mode content (codex idu equivalent) -->
      <div v-else :class="searchView">
        <div :class="searchInputRow">
          <svg :viewBox="ic('search').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('search').paths" :key="i" :d="p" /></svg>
          <input
            ref="searchInputEl"
            v-model="searchQuery"
            :class="searchInput"
            type="text"
            placeholder="搜索会话"
            aria-label="搜索会话"
            @keydown.esc.prevent="closeSearch"
          />
          <button :class="searchClear" type="button" aria-label="关闭搜索" data-tooltip="关闭搜索" @click="closeSearch">
            <svg :viewBox="icons.close.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.close.paths" :key="i" :d="p" /></svg>
          </button>
        </div>
        <div :class="sessionList">
          <div
            v-for="s in searchResults"
            :key="s.id"
            :class="[sessionItem, { [sessionItemActive]: s.id === state.currentSessionId }]"
            role="listitem"
            :aria-label="sessionTitle(s)"
            :aria-current="s.id === state.currentSessionId ? 'page' : undefined"
            :title="sessionTitle(s)"
            @click="select(s.id)"
            @contextmenu.prevent="openCtxMenu($event, s)"
          >
            <span class="sb-slot"></span>
            <span class="s-title" @mouseenter="marqueeEnter($event, s.id)" @mouseleave="marqueeLeave(s.id)">
              <template v-if="editingSessionId === s.id">
                <input
                  v-model="editDraft"
                  :class="editInput"
                  type="text"
                  :aria-label="`重命名 ${sessionTitle(s)}`"
                  @click.stop
                  @keydown.enter.prevent="commitRename(s.id)"
                  @keydown.esc.stop.prevent="editingSessionId = null"
                />
              </template>
              <span v-else class="sb-title-inner" :class="{ 'sb-marquee': marqueeActive.has(s.id) }">{{ sessionTitle(s) }}</span>
            </span>
            <span class="sb-row-spacer"></span>
            <span class="sb-badge"><span class="sb-badge-box"><svg :viewBox="ic('badgeIcon').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('badgeIcon').paths" :key="i" :d="p" /></svg></span></span>
            <span class="sb-hover-actions">
              <button class="sb-hover-btn" type="button" :aria-label="state.pinnedIds.includes(s.id) ? '取消置顶' : '置顶聊天'" :data-tooltip="state.pinnedIds.includes(s.id) ? '取消置顶' : '置顶聊天'" @click.stop="togglePin(s)">
                <svg class="sb-pin" :viewBox="ic('pin').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('pin').paths" :key="i" :d="p" /></svg>
              </button>
              <button class="sb-hover-btn" type="button" :aria-label="state.archivedIds.includes(s.id) ? '取消归档' : '归档聊天'" :data-tooltip="state.archivedIds.includes(s.id) ? '取消归档' : '归档聊天'" @click.stop="toggleArchive(s)">
                <svg :viewBox="ic('archive').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('archive').paths" :key="i" :d="p" /></svg>
              </button>
            </span>
          </div>
        </div>
        <div v-if="searchQuery.trim() && searchResults.length === 0" :class="emptyRow">无匹配会话</div>
      </div>
    </div>

    <!-- footer: user row + help (46px, hairline on top) -->
    <div :class="sidebarBottom">
      <div :class="[menuAnchor, menuAnchorGrow]" ref="profileAnchor">
        <button
          :class="[userRow, { [userRowOpen]: openMenu === 'profile' }]"
          type="button"
          aria-label="打开个人资料菜单"
          aria-haspopup="menu"
          :aria-expanded="openMenu === 'profile'"
          @click="toggleMenu('profile')"
        >
          <span class="sb-avatar">u</span>
          <span class="sb-user">user</span>
        </button>
        <div v-if="openMenu === 'profile'" :class="[menu, menuBottomLeft]" role="menu">
          <button :class="menuItem" type="button" role="menuitem" @click="openSettings">
            <span :class="menuCheck"></span>
            <span>设置</span>
          </button>
        </div>
      </div>
      <div :class="menuAnchor" ref="helpAnchor">
        <button
          :class="[sidebarBottomBtn, { [sidebarBottomBtnOpen]: openMenu === 'help' }]"
          type="button"
          aria-label="打开帮助菜单"
          aria-haspopup="menu"
          :aria-expanded="openMenu === 'help'"
          @click="toggleMenu('help')"
        >
          <svg :viewBox="icons.help.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.help.paths" :key="i" :d="p" /></svg>
        </button>
        <div v-if="openMenu === 'help'" :class="[menu, menuBottomRight]" role="menu">
          <button :class="menuItem" type="button" role="menuitem" @click="openHelpDialog">
            <span :class="menuCheck"></span>
            <span>帮助</span>
          </button>
        </div>
      </div>
    </div>

    <!-- Codex-style resize handle on the right edge -->
    <div :class="resizeHandle" role="separator" aria-orientation="vertical" @mousedown="startResize">
      <div :class="resizeHandleLine"></div>
    </div>

    <!-- Session row context menu (codex Work Ztu subset) -->
    <div
      v-if="ctxMenu"
      :class="ctxMenuStyle"
      :style="{ left: Math.min(ctxMenu.x, viewport.w - 200) + 'px', top: Math.min(ctxMenu.y, viewport.h - 220) + 'px' }"
      role="menu"
      @mousedown.stop
    >
      <button :class="menuItem" type="button" role="menuitem" @click="ctxRename">
        <span :class="menuCheck"></span>
        <span>重命名会话</span>
      </button>
      <button :class="menuItem" type="button" role="menuitem" @click="ctxCopyId">
        <span :class="menuCheck"></span>
        <span>复制会话 ID</span>
      </button>
      <button :class="menuItem" type="button" role="menuitem" @click="ctxCopyCwd">
        <span :class="menuCheck"></span>
        <span>复制工作目录</span>
      </button>
      <button :class="menuItem" type="button" role="menuitem" @click="ctxFork">
        <span :class="menuCheck"></span>
        <span>Fork 会话</span>
      </button>
      <button :class="menuItem" type="button" role="menuitem" @click="ctxExport">
        <span :class="menuCheck"></span>
        <span>导出复制</span>
      </button>
    </div>
  </aside>
</template>
