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
let version = '0.2.0'
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
  console.warn('  [portable] 警告: 未找到 WebView2Loader.dll')
}
writeFileSync(join(out, 'portable.dat'), 'leonPaste portable mode\n')
writeFileSync(join(out, 'README.txt'), `leonPaste ${version} - Windows 便携版
========================================

【启动】双击 leonPaste.exe（常驻后台，托盘图标在通知区域）
【唤起】Ctrl+Shift+V 打开屏幕底部的横向卡片面板；设置里可改热键
【浏览】鼠标滚轮 / 拖拽 / 左右方向键 / 两端箭头，横向滑动卡片
【使用】点击卡片或 Enter 复制该条；Ctrl+D 收藏；Delete 删除
【关闭】Esc，或点击其他任意应用（失焦自动隐藏）
【多格式】文本、位图、复制的文件列表都会进入历史；图片有缩略图预览并可原样写回
【诊断】设置 → 关于与诊断：版本号、开发者、监听心跳、读取计数、最近日志、数据目录；
   「检测剪贴板」按钮可即时确认程序读到了文本 / 图片 / 文件 / 被占用
【权限】读剪贴板不需要管理员或 Windows 隐私开关；若列表为空，先退出旧进程再启动
【托盘】右键可暂停记录、清空历史、退出

【便携模式】同目录存在 portable.dat 即为便携模式，
历史、设置、图片保存在同目录 data\\ 下，不写注册表、不写 %APPDATA%。
删除 portable.dat 则改为标准模式。

【运行要求】Windows 10 (1803+) / Windows 11 x64。
Win11 自带 WebView2 运行时；Win10 若未安装，首次启动会提示下载。
请勿删除同目录的 WebView2Loader.dll。

【隐私】数据纯本地保存，不联网、不上传。
「忽略关键词」可让含密码等字样的内容不进入历史；托盘可暂停记录。
诊断日志：data\\leonpaste.log

生成时间：${new Date().toLocaleString('zh-CN')}
构建目标：${target}
`)
const mb = (statSync(join(out, 'leonPaste.exe')).size / 1024 / 1024).toFixed(2)
console.log(`\n  [portable] 完成: ${out}`)
console.log(`  [portable] leonPaste.exe (${mb} MB)\n`)
