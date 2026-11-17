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
