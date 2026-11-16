#!/usr/bin/env node
import { existsSync, mkdirSync, copyFileSync, writeFileSync, statSync, unlinkSync, readFileSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = dirname(dirname(fileURLToPath(import.meta.url)))
const target = process.env.TAURI_TARGET || 'x86_64-pc-windows-gnu'
const candidates = [
  join(root, 'src-tauri', 'target', target, 'release'),
  join(root, 'src-tauri', 'target', 'release'),
]

let exePath = null
for (const d of candidates) {
  const p = join(d, 'leonpaste.exe')
  if (existsSync(p)) { exePath = p; break }
}
if (!exePath) {
  console.error('[portable] 找不到 leonpaste.exe，请先运行 npm run tauri:build')
  process.exit(1)
}

let version = '0.1.0'
try {
  version = JSON.parse(readFileSync(join(root, 'src-tauri', 'tauri.conf.json'), 'utf8')).version || version
} catch { /* ignore */ }

const out = join(root, 'dist-portable')
mkdirSync(out, { recursive: true })
for (const name of ['leonPaste.exe', 'WebView2Loader.dll', 'portable.dat', 'README.txt']) {
  const p = join(out, name)
  if (existsSync(p)) unlinkSync(p)
}

copyFileSync(exePath, join(out, 'leonPaste.exe'))
const dll = join(dirname(exePath), 'WebView2Loader.dll')
if (existsSync(dll)) {
  copyFileSync(dll, join(out, 'WebView2Loader.dll'))
  console.log('  [portable] 已包含 WebView2Loader.dll')
} else {
  console.warn('  [portable] 警告: 未找到 WebView2Loader.dll，GNU 构建可能无法启动')
}

writeFileSync(join(out, 'portable.dat'), 'leonPaste portable mode\n')
writeFileSync(join(out, 'README.txt'), `leonPaste ${version} - Windows 便携版
========================================

【启动】双击 leonPaste.exe
【快捷键】Ctrl+Shift+V 唤起 / 隐藏面板；↑↓ 选择；Enter 复制；Ctrl+D 收藏；Esc 关闭
【托盘】右键图标可暂停记录、清空历史、退出

【便携模式】同目录存在 portable.dat 即为便携模式，
历史与设置保存在同目录 data\\ 下，不写注册表、不写 %APPDATA%。
删除 portable.dat 则改为标准模式。

【运行要求】Windows 10 (1803+) / Windows 11 x64。
Win11 自带 WebView2 运行时；Win10 若未安装，
首次启动会提示下载 Evergreen Runtime。
请勿删除同目录的 WebView2Loader.dll。

【数据】history.json / settings.json 均为明文 JSON，纯本地存储，不联网、不上传。

生成时间：${new Date().toLocaleString('zh-CN')}
构建目标：${target}
`)

const mb = (statSync(join(out, 'leonPaste.exe')).size / 1024 / 1024).toFixed(2)
console.log(`\n  [portable] 完成: ${out}`)
console.log(`  [portable] leonPaste.exe (${mb} MB)\n`)
