use std::sync::atomic::Ordering;
use windows_sys::Win32::System::{
    DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
        OpenClipboard, SetClipboardData,
    },
    Diagnostics::Debug::ReadProcessMemory,
    Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
    Threading::GetCurrentProcess,
};
use windows_sys::Win32::UI::Shell::DragQueryFileW;

const CF_UNICODETEXT: u32 = 13;
const CF_DIB: u32 = 8;
const CF_HDROP: u32 = 15;
const GMEM_MOVEABLE: u32 = 0x0002;

pub enum Capture {
    Text(String),
    Dib(Vec<u8>),
    Files(Vec<String>),
    Empty,
    Busy,
}

impl Capture {
    /// 内容签名：只要剪贴板内容变了就会变，不依赖序列号或单一格式。
    pub fn signature(&self) -> String {
        match self {
            Capture::Text(s) => format!("t{}:{}", s.len(), crate::dib::hash(s.as_bytes())),
            Capture::Dib(d) => format!("i{}:{}", d.len(), crate::dib::hash(d)),
            Capture::Files(f) => format!("f{}:{}", f.len(), crate::dib::hash(f.join("|").as_bytes())),
            Capture::Empty => "empty".into(),
            Capture::Busy => "busy".into(),
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Capture::Text(_) => "text",
            Capture::Dib(_) => "image",
            Capture::Files(_) => "files",
            Capture::Empty => "empty",
            Capture::Busy => "busy",
        }
    }
}

/// 剪贴板全局内存一律通过 ReadProcessMemory 从自身进程复制，绝不对外部指针
/// 建裸切片。延迟渲染 / 句柄失效 / GlobalSize 异常时内核只返回 FALSE，
/// 不会触发 0xC0000005（access violation）。
pub(crate) unsafe fn copy_global(h: *mut std::ffi::c_void, limit: usize) -> Vec<u8> {
    let p = GlobalLock(h) as *const std::ffi::c_void;
    if p.is_null() {
        return Vec::new();
    }
    let n = GlobalSize(h).min(limit);
    let mut out = Vec::new();
    if n > 0 {
        let mut buf = vec![0u8; n];
        let mut read = 0usize;
        let ok = ReadProcessMemory(
            GetCurrentProcess(),
            p,
            buf.as_mut_ptr() as *mut std::ffi::c_void,
            n,
            &mut read,
        );
        if ok != 0 && read > 0 {
            buf.truncate(read.min(n));
            out = buf;
        }
    }
    GlobalUnlock(h);
    out
}

fn decode_utf16z(bytes: &[u8]) -> String {
    let units = bytes.len() / 2;
    let mut wide = Vec::with_capacity(units);
    for i in 0..units {
        wide.push(u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]));
    }
    let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..len])
}

/// 退出 read() 时把操作码归零，供崩溃日志判断崩溃发生在哪个阶段。
struct OpGuard;
impl Drop for OpGuard {
    fn drop(&mut self) {
        crate::LAST_OP.store(0, Ordering::Relaxed);
    }
}

/// 一次打开剪贴板，按「文件列表 > 位图 > 文本」的优先顺序读取。
pub fn read() -> Capture {
    let _serial = crate::CLIP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Capture::Busy;
        }
        let _guard = OpGuard;
        crate::LAST_OP.store(1, Ordering::Relaxed);
        crate::stats::note_open();

        if IsClipboardFormatAvailable(CF_HDROP) != 0 {
            crate::LAST_OP.store(2, Ordering::Relaxed);
            let h = GetClipboardData(CF_HDROP);
            // 先确认这块内存可读，避免失效 HDROP 让 DragQueryFileW 越界
            if !h.is_null() && !copy_global(h, 64 * 1024).is_empty() {
                let count = DragQueryFileW(h, 0xFFFFFFFF, std::ptr::null_mut(), 0).min(4096);
                let mut files = Vec::new();
                for i in 0..count {
                    let mut buf = [0u16; 1024];
                    let n = DragQueryFileW(h, i, buf.as_mut_ptr(), buf.len() as u32);
                    if n > 0 && (n as usize) < buf.len() {
                        files.push(String::from_utf16_lossy(&buf[..n as usize]));
                    }
                }
                if !files.is_empty() {
                    CloseClipboard();
                    return Capture::Files(files);
                }
            }
        }

        if IsClipboardFormatAvailable(CF_UNICODETEXT) != 0 {
            crate::LAST_OP.store(3, Ordering::Relaxed);
            let h = GetClipboardData(CF_UNICODETEXT);
            if !h.is_null() {
                let s = decode_utf16z(&copy_global(h, 16 * 1024 * 1024));
                if !s.trim().is_empty() {
                    CloseClipboard();
                    return Capture::Text(s);
                }
            }
        }

        if IsClipboardFormatAvailable(CF_DIB) != 0 {
            crate::LAST_OP.store(4, Ordering::Relaxed);
            let h = GetClipboardData(CF_DIB);
            if !h.is_null() {
                let d = copy_global(h, 32 * 1024 * 1024);
                if !d.is_empty() {
                    CloseClipboard();
                    return Capture::Dib(d);
                }
            }
        }
        CloseClipboard();
        Capture::Empty
    }
}

pub fn write_text(s: &str) -> bool {
    let _serial = crate::CLIP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        let _ = EmptyClipboard();
        let mut wide: Vec<u16> = s.encode_utf16().collect();
        wide.push(0);
        let h = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2);
        if h.is_null() {
            CloseClipboard();
            return false;
        }
        let p = GlobalLock(h) as *mut u16;
        if p.is_null() {
            CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
        GlobalUnlock(h);
        let r = SetClipboardData(CF_UNICODETEXT, h);
        CloseClipboard();
        !r.is_null()
    }
}
