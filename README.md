# leonPaste

一个受 macOS [Paste](https://pasteapp.io/) 启发的**剪贴板历史管理器**，为 Windows 10 / 11 设计。

复制过的文字不再消失：后台自动记录、随时用全局热键唤起悬浮面板，搜索、收藏、一键写回剪贴板。
数据全部存在本地，不联网、不上传、无账号。

![平台](https://img.shields.io/badge/platform-Windows%2010%2F11-blue)
![许可](https://img.shields.io/badge/license-MIT-green)

## 功能

- **自动记录**：每 180ms 用 Win32 直接读取剪贴板内容签名，文本 / 图片 / **文件列表** 任一变化即捕获，去重并记录来源窗口与时间戳
- **智能分类**：文本 / 链接 / 代码 / 颜色 / 路径，可一键筛选
- **底部横条**：无边框置顶窗口横在屏幕下方，毛玻璃卡片流，每项都有真实预览
- **横向滑动**：滚轮、拖拽、方向键、两端箭头均可浏览；点击卡片或 `Enter` 即写回剪贴板
- **失焦即退**：点击其他应用自动隐藏，等同 `Esc`
- **首次启动可见**：第一次运行自动弹出一次面板，避免「双击后什么都没有」
- **全局热键**：默认 `Ctrl+Shift+V`，设置里可改为 `Ctrl+Alt+V` / `Alt+V` / `` Ctrl+` ``
- **即时搜索**：输入即过滤，`←` `→` 选择，`Enter` 写回剪贴板，`Esc` 关闭
- **图片与文件**：位图生成缩略图预览并原样写回；复制的文件列表也能被记录
- **关于与诊断**：设置页显示版本号、开发者、许可证、仓库、构建目标、数据目录、已记录条数、监听心跳与最近日志，可一键打开数据目录
- **自动粘贴**：选中后可模拟 `Ctrl+V` 直接粘到当前应用（可关）
- **收藏与清理**：收藏置顶、单条删除、清空历史（保留收藏）
- **系统托盘**：显示 / 隐藏、暂停记录、清空、退出
- **设置**：容量上限、保留天数、深浅主题、忽略关键词、开机自启、暂停记录
- **便携模式**：数据写在 exe 同目录 `data/`，不写注册表、不写 `%APPDATA%`
- **单实例**、无边框置顶窗口、纯本地 JSON 存储

## 快捷键

| 按键 | 作用 |
|---|---|
| `Ctrl+Shift+V` | 唤起 / 隐藏面板（可改） |
| `←` / `→` / `↑` / `↓` | 移动选中项 |
| 滚轮 / 拖拽 | 横向浏览卡片 |
| `Enter` | 写回剪贴板并关闭面板 |
| `Ctrl+D` | 收藏 / 取消收藏 |
| `Delete` | 删除选中项 |
| `Esc` / 点击其他应用 | 关闭面板 / 退出设置 |

## 安装与使用

从 [Releases](https://github.com/severalfly/leonPaste/releases) 下载最新版 `leonPaste-<版本>-portable.zip`，
解压到任意目录，双击 `leonPaste.exe` 即可。

- **Windows 11**：WebView2 运行时已内置，开箱即用。
- **Windows 10 (1803+)**：若未安装 WebView2 Evergreen Runtime，首次启动会提示下载；
  也可从微软官网手动安装。
- 请勿删除同目录的 `WebView2Loader.dll`。
- `portable.dat` 存在即便携模式；删掉它则改为标准模式（配置写入 `%APPDATA%`）。

## 技术栈

| 层 | 选型 |
|---|---|
| 外壳 | Tauri 2（Rust） |
| 界面 | Vue 3 + TypeScript + Vite |
| 剪贴板 | `clipboard-win`（Win32 Clipboard API） |
| 全局热键 / 自启 / 单实例 | Tauri 官方插件 |
| 存储 | 本地 JSON（`history.json` / `settings.json`） |

目标是单个约 10 MB 的 exe，不依赖 .NET、不装 Visual Studio。

## 本地开发

Windows 上需要：Node.js 18+、Rust（`stable-x86_64-pc-windows-gnu`）、
MSYS2 mingw-w64（`mingw-w64-x86_64-gcc` + `binutils`），并把 `C:/msys64/mingw64/bin` 加入 PATH。
`.cargo/config.toml` 已配好 GNU linker；若 MSYS2 不在默认路径，可用环境变量
`CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER` 覆盖。

```bat
npm install
npm run dev                    :: 仅前端调试
npm run build                  :: vue-tsc 类型检查 + vite 构建
npm run tauri:build            :: tauri build --no-bundle --target x86_64-pc-windows-gnu
npm run portable               :: 生成 dist-portable 便携目录
```

`tauri build` 的 `beforeBuildCommand` 会自动执行前端构建。

## 路线图

- [x] 文本与图片历史、搜索、收藏、热键、托盘、便携模式
- [x] 底部横向卡片流与失焦自动隐藏
- [x] 文件列表历史
- [ ] HTML 富文本历史
- [ ] 剪贴板片段（Snippets）与固定分组
- [ ] 历史条目预览大窗与语法高亮
- [ ] 导出 / 导入历史、可选加密
- [ ] i18n（英文界面）

## 隐私

所有数据只保存在本机 `data/` 目录，**不会联网、不会上传**。
「忽略关键词」可让包含密码、令牌等字样的内容不进入历史；托盘可随时暂停记录。

## 许可

[MIT](LICENSE) © 2026 severalfly
