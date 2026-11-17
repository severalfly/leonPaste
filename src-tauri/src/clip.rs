use windows_sys::Win32::System::{
    DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
        OpenClipboard, SetClipboardData,
    },
    Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
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

unsafe fn lock_bytes(h: *mut std::ffi::c_void) -> Vec<u8> {
    let p = GlobalLock(h) as *const u8;
    if p.is_null() {
        return Vec::new();
    }
    let n = GlobalSize(h);
    let out = if n > 0 && n < 256 * 1024 * 1024 {
        std::slice::from_raw_parts(p, n).to_vec()
    } else {
        Vec::new()
    };
    GlobalUnlock(h);
    out
}

/// 一次打开剪贴板，按「文件列表 > 位图 > 文本」的优先顺序读取。
pub fn read() -> Capture {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Capture::Busy;
        }
        crate::stats::note_open();

        if IsClipboardFormatAvailable(CF_HDROP) != 0 {
            let h = GetClipboardData(CF_HDROP);
            if !h.is_null() {
                let count = DragQueryFileW(h, 0xFFFFFFFF, std::ptr::null_mut(), 0);
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
            let h = GetClipboardData(CF_UNICODETEXT);
            if !h.is_null() {
                let p = GlobalLock(h) as *const u16;
                if !p.is_null() {
                    let max = GlobalSize(h) / 2;
                    let mut len = 0usize;
                    while len < max && *p.add(len) != 0 {
                        len += 1;
                    }
                    let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
                    GlobalUnlock(h);
                    if !s.trim().is_empty() {
                        CloseClipboard();
                        return Capture::Text(s);
                    }
                }
            }
        }

        if IsClipboardFormatAvailable(CF_DIB) != 0 {
            let h = GetClipboardData(CF_DIB);
            if !h.is_null() {
                let d = lock_bytes(h);
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
