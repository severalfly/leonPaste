<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { Entry, Settings } from './types'

const defaultSettings: Settings = {
  max_items: 500,
  retention_days: 30,
  hotkey: 'Ctrl+Shift+V',
  auto_paste: true,
  autostart: false,
  theme: 'dark',
  ignore_regex: '',
  paused: false,
}

const query = ref('')
const filter = ref('all')
const favoritesOnly = ref(false)
const items = ref<Entry[]>([])
const selected = ref(0)
const settings = ref<Settings>({ ...defaultSettings })
const showSettings = ref(false)
const searchEl = ref<HTMLInputElement | null>(null)
const unlisteners: Array<() => void> = []

const filters = [
  { id: 'all', label: '全部' },
  { id: 'text', label: '文本' },
  { id: 'link', label: '链接' },
  { id: 'code', label: '代码' },
  { id: 'color', label: '颜色' },
  { id: 'path', label: '路径' },
]

const hotkeys = ['Ctrl+Shift+V', 'Ctrl+Alt+V', 'Alt+V', 'Ctrl+`']
const total = computed(() => items.value.length)

function applyTheme() {
  document.documentElement.dataset.theme = settings.value.theme
}

async function refresh() {
  try {
    items.value = await invoke<Entry[]>('list_history', {
      query: query.value,
      filter: filter.value,
      favorites_only: favoritesOnly.value,
    })
  } catch {
    items.value = []
  }
  if (selected.value >= items.value.length) {
    selected.value = Math.max(0, items.value.length - 1)
  }
}

async function loadSettings() {
  try {
    settings.value = await invoke<Settings>('get_settings')
    applyTheme()
  } catch {
    settings.value = { ...defaultSettings }
  }
}

function focusSearch() {
  nextTick(() => {
    searchEl.value?.focus()
    searchEl.value?.select()
  })
}

async function pasteSelected() {
  const item = items.value[selected.value]
  if (!item) return
  await invoke('paste_entry', { id: item.id })
}

async function toggleFavorite(item: Entry) {
  await invoke('toggle_favorite', { id: item.id })
}

async function removeItem(item: Entry) {
  await invoke('delete_entry', { id: item.id })
}

async function saveSettings() {
  await invoke('save_settings', { settings: settings.value })
  applyTheme()
  showSettings.value = false
  focusSearch()
}

async function clearAll() {
  await invoke('clear_history')
}

function fmtTime(ts: number) {
  const d = new Date(ts * 1000)
  const now = new Date()
  const sameDay = d.toDateString() === now.toDateString()
  const hm = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
  return sameDay ? hm : `${d.getMonth() + 1}/${d.getDate()} ${hm}`
}

function kindLabel(k: string) {
  return { text: '文本', link: '链接', code: '代码', color: '颜色', path: '路径' }[k] ?? '文本'
}

function kindGlyph(k: string) {
  return { text: 'T', link: '↗', code: '</>', color: '◧', path: '/' }[k] ?? 'T'
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    e.preventDefault()
    if (showSettings.value) {
      showSettings.value = false
      focusSearch()
    } else {
      invoke('hide_window')
    }
    return
  }
  if (showSettings.value) return

  if (e.key === 'ArrowDown') {
    e.preventDefault()
    selected.value = Math.min(items.value.length - 1, selected.value + 1)
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    selected.value = Math.max(0, selected.value - 1)
  } else if (e.key === 'Enter') {
    e.preventDefault()
    pasteSelected()
  } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'd') {
    e.preventDefault()
    const item = items.value[selected.value]
    if (item) toggleFavorite(item)
  } else if (e.key === 'Delete' && !(e.target instanceof HTMLInputElement)) {
    e.preventDefault()
    const item = items.value[selected.value]
    if (item) removeItem(item)
  }
}

watch([query, filter, favoritesOnly], () => {
  selected.value = 0
  refresh()
})

onMounted(async () => {
  await loadSettings()
  await refresh()
  focusSearch()
  unlisteners.push(
    await listen('history-changed', () => refresh()),
    await listen('settings-changed', () => loadSettings()),
    await listen('window-shown', () => {
      query.value = ''
      refresh()
      focusSearch()
    }),
  )
  window.addEventListener('keydown', onKey)
  window.addEventListener('focus', refresh)
})

onUnmounted(() => {
  unlisteners.forEach((f) => f())
  window.removeEventListener('keydown', onKey)
  window.removeEventListener('focus', refresh)
})
</script>

<template>
  <div class="app" data-tauri-drag-region>
    <header class="topbar" data-tauri-drag-region>
      <div class="brand" data-tauri-drag-region>
        <span class="logo">P</span>
        <span class="name" data-tauri-drag-region>leonPaste</span>
      </div>
      <div class="top-actions">
        <button
          class="chip"
          :class="{ on: favoritesOnly }"
          title="只看收藏"
          @click="favoritesOnly = !favoritesOnly"
        >★ 收藏</button>
        <button class="chip" title="设置" @click="showSettings = !showSettings">⚙</button>
      </div>
    </header>

    <div class="search-row">
      <span class="search-icon">⌕</span>
      <input
        ref="searchEl"
        v-model="query"
        class="search"
        type="text"
        placeholder="搜索剪贴板历史…"
        spellcheck="false"
      />
      <span class="count">{{ total }} 条</span>
    </div>

    <nav class="filters">
      <button
        v-for="f in filters"
        :key="f.id"
        class="filter"
        :class="{ active: filter === f.id }"
        @click="filter = f.id"
      >{{ f.label }}</button>
    </nav>

    <main class="list" :class="{ empty: items.length === 0 }">
      <div v-if="items.length === 0" class="empty-state">
        <div class="empty-icon">▤</div>
        <p>还没有剪贴板记录</p>
        <span>复制任意文本后会自动出现在这里</span>
      </div>
      <div
        v-for="(item, i) in items"
        :key="item.id"
        class="row"
        :class="{ active: i === selected }"
        @mouseenter="selected = i"
        @click="pasteSelected"
      >
        <span class="kind" :class="`k-${item.kind}`">{{ kindGlyph(item.kind) }}</span>
        <div class="row-main">
          <div class="row-text">{{ item.preview || '(空白)' }}</div>
          <div class="row-meta">
            <span class="tag">{{ kindLabel(item.kind) }}</span>
            <span class="dot">·</span>
            <span class="src">{{ item.source }}</span>
            <span class="dot">·</span>
            <span>{{ fmtTime(item.created_at) }}</span>
          </div>
        </div>
        <button
          class="star"
          :class="{ on: item.favorite }"
          title="收藏 / 取消收藏 (Ctrl+D)"
          @click.stop="toggleFavorite(item)"
        >{{ item.favorite ? '★' : '☆' }}</button>
      </div>
    </main>

    <footer class="statusbar">
      <span v-if="settings.paused" class="paused">● 已暂停记录</span>
      <span v-else class="live">● 正在记录</span>
      <span class="sep">|</span>
      <span>{{ settings.hotkey }} 唤起</span>
      <span class="grow" />
      <span class="hint">↑↓ 选择 · Enter 复制 · Ctrl+D 收藏 · Esc 关闭</span>
    </footer>

    <div v-if="showSettings" class="drawer">
      <div class="drawer-head">
        <h2>设置</h2>
        <button class="close" @click="showSettings = false; focusSearch()">×</button>
      </div>
      <div class="drawer-body">
        <label class="field">
          <span>全局热键</span>
          <select v-model="settings.hotkey">
            <option v-for="k in hotkeys" :key="k" :value="k">{{ k }}</option>
          </select>
        </label>
        <label class="field">
          <span>主题</span>
          <select v-model="settings.theme">
            <option value="dark">深色</option>
            <option value="light">浅色</option>
            <option value="auto">跟随系统</option>
          </select>
        </label>
        <label class="field">
          <span>最多保留条数</span>
          <input v-model.number="settings.max_items" type="number" min="10" max="5000" />
        </label>
        <label class="field">
          <span>保留天数（0 = 永久）</span>
          <input v-model.number="settings.retention_days" type="number" min="0" max="3650" />
        </label>
        <label class="field checkbox">
          <input v-model="settings.auto_paste" type="checkbox" />
          <span>选中后自动粘贴（模拟 Ctrl+V）</span>
        </label>
        <label class="field checkbox">
          <input v-model="settings.autostart" type="checkbox" />
          <span>开机自动启动</span>
        </label>
        <label class="field checkbox">
          <input v-model="settings.paused" type="checkbox" />
          <span>暂停记录</span>
        </label>
        <label class="field column">
          <span>忽略关键词（每行一个，命中则不记录）</span>
          <textarea v-model="settings.ignore_regex" rows="3" placeholder="密码 / 令牌 / 密钥"></textarea>
        </label>
      </div>
      <div class="drawer-foot">
        <button class="danger" @click="clearAll">清空历史（保留收藏）</button>
        <span class="grow" />
        <button class="ghost" @click="showSettings = false; focusSearch()">取消</button>
        <button class="primary" @click="saveSettings">保存</button>
      </div>
    </div>
  </div>
</template>
