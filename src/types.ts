export type EntryKind = 'text' | 'link' | 'code' | 'color' | 'path'

export interface Entry {
  id: string
  kind: EntryKind
  text: string
  preview: string
  favorite: boolean
  created_at: number
  source: string
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
