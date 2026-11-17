<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { Diagnostics, Entry, ProbeResult, Settings } from './types'

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
const diag = ref<Diagnostics | null>(null)
const probe = ref<ProbeResult | null>(null)

async function probeClipboard() {
  try {
    probe.value = await invoke<ProbeResult>('probe_clipboard')
    loadDiagnostics()
  } catch {
    probe.value = null
  }
}

async function loadDiagnostics() {
  try {
    diag.value = await invoke<Diagnostics>('get_diagnostics')
  } catch {
    diag.value = null
  }
}
function openSettings() {
  showSettings.value = true
  loadDiagnostics()
}
async function openDir() {
  await invoke('open_data_dir')
}
const searchEl = ref<HTMLInputElement | null>(null)
const trackEl = ref<HTMLElement | null>(null)
const unlisteners: Array<() => void> = []
let shownAt = 0
let dragging = false
let moved = 0
let startX = 0
let startScroll = 0

const filters = [
  { id: 'all', label: '全部' },
  { id: 'text', label: '文本' },
  { id: 'image', label: '图片' },
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

function ensureVisible(i: number) {
  nextTick(() => {
    const el = document.querySelector<HTMLElement>(`.card[data-i="${i}"]`)
    el?.scrollIntoView({ behavior: 'smooth', inline: 'center', block: 'nearest' })
  })
}

async function pasteAt(i: number) {
  const item = items.value[i]
  if (!item) return
  selected.value = i
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
  return (
    { text: '文本', link: '链接', code: '代码', color: '颜色', path: '路径', image: '图片', files: '文件' }[k] ??
    '文本'
  )
}
function kindGlyph(k: string) {
  return { text: 'T', link: '↗', code: '</>', color: '◧', path: '/', image: '▣', files: '≡' }[k] ?? 'T'
}
function cardMeta(item: Entry) {
  if (item.kind === 'image') return `${item.image_width}×${item.image_height}`
  if (item.kind === 'code') return `${item.text.split('\n').length} 行`
  return `${item.text.length} 字符`
}

function onWheel(e: WheelEvent) {
  const el = trackEl.value
  if (!el) return
  const d = Math.abs(e.deltaY) > Math.abs(e.deltaX) ? e.deltaY : e.deltaX
  el.scrollLeft += d * 1.15
}
function scrollBy(dir: number) {
  trackEl.value?.scrollBy({ left: dir * 470, behavior: 'smooth' })
}
function onPointerDown(e: PointerEvent) {
  if ((e.target as HTMLElement).closest('.star')) return
  dragging = true
  moved = 0
  startX = e.clientX
  startScroll = trackEl.value?.scrollLeft ?? 0
  ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
}
function onPointerMove(e: PointerEvent) {
  if (!dragging || !trackEl.value) return
  const dx = e.clientX - startX
  moved = Math.abs(dx)
  trackEl.value.scrollLeft = startScroll - dx
}
function onPointerUp() {
  dragging = false
}
function onCardClick(i: number) {
  if (moved > 6) return
  pasteAt(i)
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
  if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
    e.preventDefault()
    selected.value = Math.min(items.value.length - 1, selected.value + 1)
    ensureVisible(selected.value)
  } else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
    e.preventDefault()
    selected.value = Math.max(0, selected.value - 1)
    ensureVisible(selected.value)
  } else if (e.key === 'Enter') {
    e.preventDefault()
    pasteAt(selected.value)
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

function onBlur() {
  if (Date.now() - shownAt < 400) return
  window.setTimeout(() => {
    if (!document.hasFocus()) invoke('hide_window')
  }, 120)
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
      shownAt = Date.now()
      query.value = ''
      filter.value = 'all'
      refresh()
      focusSearch()
      nextTick(() => {
        if (trackEl.value) trackEl.value.scrollLeft = 0
      })
    }),
  )
  window.addEventListener('keydown', onKey)
  window.addEventListener('blur', onBlur)
  window.addEventListener('focus', refresh)
})

onUnmounted(() => {
  unlisteners.forEach((f) => f())
  window.removeEventListener('keydown', onKey)
  window.removeEventListener('blur', onBlur)
  window.removeEventListener('focus', refresh)
})
</script>

<template>
  <div class="dock">
    <header class="top" data-tauri-drag-region>
      <div class="search-wrap">
        <span class="search-icon">⌕</span>
        <input
          ref="searchEl"
          v-model="query"
          class="search"
          type="text"
          placeholder="搜索剪贴板历史…"
          spellcheck="false"
        />
        <span class="count">{{ total }} 项</span>
      </div>
      <nav class="filters">
        <button
          v-for="f in filters"
          :key="f.id"
          class="filter"
          :class="{ active: filter === f.id }"
          @click="filter = f.id"
        >{{ f.label }}</button>
        <button class="filter star-filter" :class="{ active: favoritesOnly }" title="只看收藏" @click="favoritesOnly = !favoritesOnly">★</button>
      </nav>
      <button class="gear" title="设置" @click="openSettings">⚙</button>
    </header>

    <div class="track-wrap">
      <button class="nav prev" title="向左" @click="scrollBy(-1)">‹</button>
      <div
        ref="trackEl"
        class="track"
        @wheel.prevent="onWheel"
        @pointerdown="onPointerDown"
        @pointermove="onPointerMove"
        @pointerup="onPointerUp"
        @pointercancel="onPointerUp"
      >
        <div
          v-for="(item, i) in items"
          :key="item.id"
          class="card"
          :class="{ sel: i === selected }"
          :data-i="i"
          @click="onCardClick(i)"
        >
          <div class="card-head">
            <span class="badge" :class="`b-${item.kind}`">{{ kindGlyph(item.kind) }}</span>
            <span class="kind-name">{{ kindLabel(item.kind) }}</span>
            <span class="time">{{ fmtTime(item.created_at) }}</span>
            <button
              class="star"
              :class="{ on: item.favorite }"
              title="收藏 (Ctrl+D)"
              @click.stop="toggleFavorite(item)"
            >{{ item.favorite ? '★' : '☆' }}</button>
          </div>
          <div class="card-body">
            <img v-if="item.thumb" :src="item.thumb" class="thumb" alt="图片预览" draggable="false" />
            <div v-else-if="item.kind === 'color'" class="color-block" :style="{ background: item.text }"></div>
            <div v-else class="text-preview" :class="{ mono: item.kind === 'code' || item.kind === 'path' }">{{ item.preview || '(空白)' }}</div>
          </div>
          <div class="card-foot">
            <span class="src">{{ item.source }}</span>
            <span>{{ cardMeta(item) }}</span>
          </div>
        </div>
        <div v-if="items.length === 0" class="empty">
          <div class="empty-mark">▤</div>
          <p>还没有剪贴板记录</p>
          <span>复制文本或截图后，会自动出现在这里</span>
        </div>
      </div>
      <button class="nav next" title="向右" @click="scrollBy(1)">›</button>
    </div>

    <footer class="hint-bar">
      <span v-if="settings.paused" class="paused">● 已暂停记录</span>
      <span v-else class="live">● 正在记录</span>
      <span class="sep">|</span>
      <span>{{ settings.hotkey }} 唤起</span>
      <span class="grow" />
      <span>滚轮 / 拖拽 / ←→ 浏览 · 点击或 Enter 复制 · Esc / 点击别处 关闭</span>
    </footer>

    <div v-if="showSettings" class="drawer">
      <div class="drawer-head">
        <h2>设置</h2>
        <button class="close" @click="showSettings = false; focusSearch()">×</button>
      </div>
      <div class="drawer-body">
        <label class="field"><span>全局热键</span>
          <select v-model="settings.hotkey"><option v-for="k in hotkeys" :key="k" :value="k">{{ k }}</option></select>
        </label>
        <label class="field"><span>主题</span>
          <select v-model="settings.theme">
            <option value="dark">深色</option><option value="light">浅色</option><option value="auto">跟随系统</option>
          </select>
        </label>
        <label class="field"><span>最多保留条数</span><input v-model.number="settings.max_items" type="number" min="10" max="5000" /></label>
        <label class="field"><span>保留天数（0 = 永久）</span><input v-model.number="settings.retention_days" type="number" min="0" max="3650" /></label>
        <label class="field checkbox"><input v-model="settings.auto_paste" type="checkbox" /><span>选中后自动粘贴（模拟 Ctrl+V）</span></label>
        <label class="field checkbox"><input v-model="settings.autostart" type="checkbox" /><span>开机自动启动</span></label>
        <label class="field checkbox"><input v-model="settings.paused" type="checkbox" /><span>暂停记录</span></label>
        <label class="field column"><span>忽略关键词（每行一个，命中则不记录）</span>
          <textarea v-model="settings.ignore_regex" rows="2" placeholder="密码 / 令牌 / 密钥"></textarea>
        </label>

        <div class="about">
          <div class="about-head">
            <span>关于与诊断</span>
            <span>
              <button class="mini" @click="probeClipboard">检测剪贴板</button>
              <button class="mini" @click="loadDiagnostics">刷新</button>
            </span>
          </div>
          <div v-if="diag" class="about-grid">
            <div class="about-title">{{ diag.product }} {{ diag.version }}</div>
            <div>开发者 {{ diag.developer }} · {{ diag.license }} 许可 · 构建 {{ diag.build_target }}</div>
            <div class="repo">{{ diag.repository }}</div>
            <div class="diag-line">
              <span class="status-dot" :class="diag.monitor_alive ? 'ok' : 'bad'"></span>
              监听{{ diag.monitor_alive ? '正常' : '异常' }}（{{ diag.last_tick_ago }}s 前心跳）
              · 已记录 {{ diag.entries }} 条
              · {{ diag.paused ? '已暂停' : '记录中' }}
              · {{ diag.portable ? '便携模式' : '标准模式' }}
              · 热键 {{ diag.hotkey }}
            </div>
            <div class="diag-counts">
              循环 {{ diag.ticks }} · 成功读取 {{ diag.opens }} · 被占用 {{ diag.busy }} · 捕获 {{ diag.captures }}
            </div>
            <div v-if="probe" class="probe-box" :class="{ bad: probe.busy }">
              <b>剪贴板探测：{{ probe.kind }}</b>
              <div>签名 {{ probe.signature }}</div>
              <div class="probe-preview">{{ probe.preview }}</div>
            </div>
            <div class="dir-row">
              <span class="dir" :title="diag.data_dir">{{ diag.data_dir }}</span>
              <button class="mini" @click="openDir">打开目录</button>
            </div>
            <details class="logbox">
              <summary>最近日志（{{ diag.log_tail.length }} 行）</summary>
              <pre><div v-for="(l, i) in diag.log_tail" :key="i">{{ l }}</div></pre>
            </details>
          </div>
          <div v-else class="about-loading">读取诊断信息…</div>
        </div>
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
