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
import { state, Msg, type SessionSummary } from '../store';
import { dispatch } from '../api';
import { icons, type IconDef } from '../icons';
import {
  sidebar, sidebarTop, sidebarTopSearch,
  brandRow, brand, brandOpen, brandActions, brandIconBtn, brandIconBtnSearch,
  navItemHeader, navItemScroll, navBlockScroll, navBlockItems,
  sessions, sessionsSearch, section, sectionTitleRow, sectionToggle, sectionTitleActions, sectionTitleBtn, emptyRow,
  folderGroup, folderRow, folderRowIcon, folderRowName, folderRowActions, folderRowBtn, sessionList,
  sessionItem, sessionItemActive,
  resizeHandle, resizeHandleLine, sidebarBottom, userRow, userRowOpen, sidebarBottomBtn, sidebarBottomBtnOpen,
  menuAnchor, menuAnchorGrow, menu, menuWide, menuTop, menuBottomLeft, menuBottomRight, menuItem, menuCheck,
  searchView, searchInputRow, searchInput, searchClear,
} from './Sidebar.styles';

const RESIZE_KEY = 'dimi.sidebarWidth';
// Codex resize floor: widths below 240px are not applied (02-sidebar-code §9,
// gap C7). MAX is dimi's own cap (codex max not observable).
const MIN_W = 240;
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
// land in 最近.
const groups = computed(() => {
  const map = new Map<string, typeof state.sessions>();
  for (const s of state.sessions) {
    const cwd = s.metadata?.cwd ?? s.cwd ?? '';
    if (!cwd) continue;
    if (!map.has(cwd)) map.set(cwd, []);
    map.get(cwd)!.push(s);
  }
  return [...map.entries()];
});

const recent = computed(() =>
  state.sessions.filter((s) => !(s.metadata?.cwd ?? s.cwd)),
);

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
  <aside :class="sidebar" :style="{ width: sidebarWidth + 'px' }">
    <!-- header block: brand row + 新对话 (70px) -->
    <div :class="[sidebarTop, { [sidebarTopSearch]: searchOpen }]">
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
          <button :class="[brandIconBtn, brandIconBtnSearch]" type="button" aria-label="搜索" title="搜索" @click="toggleSearch">
            <svg :viewBox="ic('search').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('search').paths" :key="i" :d="p" /></svg>
          </button>
          <button :class="brandIconBtn" type="button" aria-label="优先级，需要关注" aria-pressed="false" title="优先级" @click="comingSoon('优先级')">
            <svg :viewBox="ic('priority').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('priority').paths" :key="i" :d="p" /></svg>
          </button>
        </div>
      </div>
      <!-- 新对话 stays FIXED in the header block (29px row) -->
      <button :class="navItemHeader" type="button" title="新对话" @click="newChat">
        <svg :viewBox="icons.newChat.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.newChat.paths" :key="i" :d="p" /></svg>
        <span>新对话</span>
      </button>
    </div>

    <!-- scroll area (masked top/bottom) -->
    <div :class="[sessions, { [sessionsSearch]: searchOpen }]">
      <!-- fixed nav block: 站点 / 已安排 / 插件 (92px) -->
      <div :class="navBlockScroll">
        <div :class="navBlockItems">
          <button :class="navItemScroll" type="button" title="站点" @click="comingSoon('站点')">
            <svg :viewBox="icons.sites.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.sites.paths" :key="i" :d="p" /></svg><span>站点</span>
          </button>
          <button :class="navItemScroll" type="button" title="已安排" @click="comingSoon('已安排')">
            <svg :viewBox="icons.scheduled.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.scheduled.paths" :key="i" :d="p" /></svg><span>已安排</span>
          </button>
          <button :class="navItemScroll" type="button" title="插件" @click="comingSoon('插件')">
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
            <div :class="sectionTitleActions">
              <button :class="sectionTitleBtn" type="button" aria-label="项目侧边栏选项" title="项目侧边栏选项" @click="comingSoon('项目侧边栏选项')">
                <svg :viewBox="icons.ellipsis.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.ellipsis.paths" :key="i" :d="p" /></svg>
              </button>
              <button :class="sectionTitleBtn" type="button" aria-label="添加新项目" title="添加新项目" @click="comingSoon('添加新项目')">
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
                <span :class="folderRowActions">
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
                >
                  <span class="sb-slot"></span>
                  <span class="s-title" @mouseenter="marqueeEnter($event, s.id)" @mouseleave="marqueeLeave(s.id)">
                    <span class="sb-title-inner" :class="{ 'sb-marquee': marqueeActive.has(s.id) }">{{ sessionTitle(s) }}</span>
                  </span>
                  <span class="sb-suffix">{{ groupLabel(sessionCwd(s)) }}</span>
                  <span class="sb-badge"><span class="sb-badge-box"><svg :viewBox="ic('badgeIcon').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('badgeIcon').paths" :key="i" :d="p" /></svg></span></span>
                  <span class="sb-hover-actions">
                    <button class="sb-hover-btn" type="button" aria-label="置顶聊天" title="置顶聊天" @click.stop="comingSoon('置顶聊天')">
                      <svg class="sb-pin" :viewBox="ic('pin').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('pin').paths" :key="i" :d="p" /></svg>
                    </button>
                    <button class="sb-hover-btn" type="button" aria-label="归档聊天" title="归档聊天" @click.stop="comingSoon('归档聊天')">
                      <svg :viewBox="ic('archive').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('archive').paths" :key="i" :d="p" /></svg>
                    </button>
                  </span>
                </div>
              </div>
            </div>
          </template>
        </section>

        <!-- 最近 section: chats without a cwd -->
        <section :class="section">
          <div :class="sectionTitleRow">
            <button :class="sectionToggle" type="button" :aria-expanded="!recentCollapsed" @click="recentCollapsed = !recentCollapsed">
              <svg class="sb-chevron" :class="{ collapsed: recentCollapsed }" :viewBox="icons.sectionChevron.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.sectionChevron.paths" :key="i" :d="p" /></svg>
              <span>最近</span>
            </button>
            <div :class="sectionTitleActions">
              <button :class="sectionTitleBtn" type="button" aria-label="聊天侧边栏选项" title="聊天侧边栏选项" @click="comingSoon('聊天侧边栏选项')">
                <svg :viewBox="icons.ellipsis.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.ellipsis.paths" :key="i" :d="p" /></svg>
              </button>
              <button :class="sectionTitleBtn" type="button" aria-label="筛选聊天和工作" title="筛选聊天和工作" @click="comingSoon('筛选聊天和工作')">
                <svg :viewBox="ic('filter').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('filter').paths" :key="i" :d="p" /></svg>
              </button>
              <button :class="sectionTitleBtn" type="button" aria-label="新对话" title="新对话" @click="newChat">
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
            >
              <span class="sb-slot"></span>
              <span class="s-title" @mouseenter="marqueeEnter($event, s.id)" @mouseleave="marqueeLeave(s.id)">
                <span class="sb-title-inner" :class="{ 'sb-marquee': marqueeActive.has(s.id) }">{{ sessionTitle(s) }}</span>
              </span>
              <span v-if="sessionCwd(s)" class="sb-suffix">{{ groupLabel(sessionCwd(s)) }}</span>
              <span class="sb-badge"><span class="sb-badge-box"><svg :viewBox="ic('badgeIcon').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('badgeIcon').paths" :key="i" :d="p" /></svg></span></span>
              <span class="sb-hover-actions">
                <button class="sb-hover-btn" type="button" aria-label="置顶聊天" title="置顶聊天" @click.stop="comingSoon('置顶聊天')">
                  <svg class="sb-pin" :viewBox="ic('pin').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('pin').paths" :key="i" :d="p" /></svg>
                </button>
                <button class="sb-hover-btn" type="button" aria-label="归档聊天" title="归档聊天" @click.stop="comingSoon('归档聊天')">
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
          <button :class="searchClear" type="button" aria-label="关闭搜索" title="关闭搜索" @click="closeSearch">
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
          >
            <span class="sb-slot"></span>
            <span class="s-title" @mouseenter="marqueeEnter($event, s.id)" @mouseleave="marqueeLeave(s.id)">
              <span class="sb-title-inner" :class="{ 'sb-marquee': marqueeActive.has(s.id) }">{{ sessionTitle(s) }}</span>
            </span>
            <span v-if="sessionCwd(s)" class="sb-suffix">{{ groupLabel(sessionCwd(s)) }}</span>
            <span class="sb-badge"><span class="sb-badge-box"><svg :viewBox="ic('badgeIcon').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('badgeIcon').paths" :key="i" :d="p" /></svg></span></span>
            <span class="sb-hover-actions">
              <button class="sb-hover-btn" type="button" aria-label="置顶聊天" title="置顶聊天" @click.stop="comingSoon('置顶聊天')">
                <svg class="sb-pin" :viewBox="ic('pin').vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in ic('pin').paths" :key="i" :d="p" /></svg>
              </button>
              <button class="sb-hover-btn" type="button" aria-label="归档聊天" title="归档聊天" @click.stop="comingSoon('归档聊天')">
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
  </aside>
</template>
