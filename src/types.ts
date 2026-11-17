export type EntryKind = 'text' | 'link' | 'code' | 'color' | 'path' | 'image' | 'files'

export interface Entry {
  id: string
  kind: EntryKind
  text: string
  preview: string
  favorite: boolean
  created_at: number
  source: string
  image: string | null
  thumb: string | null
  image_width: number
  image_height: number
}

export interface Settings {
  max_items: number
  retention_days: number
  hotkey: string
  auto_paste: boolean
  autostart: boolean
  theme: string
  ignore_regex: string
  paused: boolean
}

export interface Diagnostics {
  version: string
  product: string
  developer: string
  license: string
  repository: string
  build_target: string
  data_dir: string
  entries: number
  monitor_alive: boolean
  last_tick_ago: number
  hotkey: string
  paused: boolean
  portable: boolean
  log_tail: string[]
  ticks: number
  opens: number
  busy: number
  captures: number
}

export interface ProbeResult {
  kind: string
  signature: string
  preview: string
  busy: boolean
  opens: number
}
