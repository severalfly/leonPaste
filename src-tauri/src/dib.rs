use windows_sys::Win32::System::{
    DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
        OpenClipboard, SetClipboardData,
    },
    Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
};

pub const CF_DIB: u32 = 8;
const GMEM_MOVEABLE: u32 = 0x0002;

#[derive(Debug, Clone, Copy)]
pub struct Info {
    pub width: u32,
    pub height: u32,
    pub bpp: u16,
    pub pixels_offset: usize,
    pub stride: usize,
    pub top_down: bool,
}

fn u16le(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn u32le(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn i32le(b: &[u8], o: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

pub fn parse(dib: &[u8]) -> Option<Info> {
    if dib.len() < 40 {
        return None;
    }
    let header_size = u32le(dib, 0)? as usize;
    let w = i32le(dib, 4)?;
    let h = i32le(dib, 8)?;
    if header_size < 40 || header_size > dib.len() || w <= 0 || h == 0 {
        return None;
    }
    let width = w as u32;
    let top_down = h < 0;
    let height = h.unsigned_abs();
    let bpp = u16le(dib, 14)?;
    if bpp != 24 && bpp != 32 {
        return None;
    }
    let compression = u32le(dib, 16)?;
    if compression != 0 && compression != 3 {
        return None;
    }
    let clr_used = u32le(dib, 32)? as usize;
    let palette = if bpp <= 8 {
        (if clr_used > 0 { clr_used } else { 1usize << bpp }) * 4
    } else {
        0
    };
    let stride = ((width as usize * bpp as usize + 31) / 32) * 4;
    let pixels_offset = header_size + palette;
    if pixels_offset.saturating_add(stride.saturating_mul(height as usize)) > dib.len() {
        return None;
    }
    Some(Info {
        width,
        height,
        bpp,
        pixels_offset,
        stride,
        top_down,
    })
}

pub fn summary(dib: &[u8]) -> String {
    if dib.len() < 40 {
        return format!("len={}", dib.len());
    }
    format!(
        "len={} header={} w={} h={} bpp={} compression={}",
        dib.len(),
        u32le(dib, 0).unwrap_or(0),
        i32le(dib, 4).unwrap_or(0),
        i32le(dib, 8).unwrap_or(0),
        u16le(dib, 14).unwrap_or(0),
        u32le(dib, 16).unwrap_or(0),
    )
}

fn pixel(dib: &[u8], i: &Info, x: u32, y: u32) -> [u8; 4] {
    let row = if i.top_down { y } else { i.height - 1 - y };
    let off = i.pixels_offset + row as usize * i.stride + x as usize * (i.bpp as usize / 8);
    if off + 2 >= dib.len() {
        return [0, 0, 0, 255];
    }
    let a = if i.bpp == 32 && off + 3 < dib.len() {
        dib[off + 3]
    } else {
        255
    };
    [dib[off], dib[off + 1], dib[off + 2], a]
}

/// 生成一张 32bpp bottom-up 的缩小 DIB，供卡片预览用。
pub fn thumbnail(dib: &[u8], max_w: u32, max_h: u32) -> Option<(Vec<u8>, u32, u32)> {
    let i = parse(dib)?;
    let scale = (max_w as f64 / i.width as f64)
        .min(max_h as f64 / i.height as f64)
        .min(1.0);
    let tw = ((i.width as f64 * scale).round() as u32).max(1);
    let th = ((i.height as f64 * scale).round() as u32).max(1);
    let stride = tw as usize * 4;
    let mut out = vec![0u8; 40 + stride * th as usize];
    out[0..4].copy_from_slice(&40u32.to_le_bytes());
    out[4..8].copy_from_slice(&(tw as i32).to_le_bytes());
    out[8..12].copy_from_slice(&(th as i32).to_le_bytes());
    out[12..14].copy_from_slice(&1u16.to_le_bytes());
    out[14..16].copy_from_slice(&32u16.to_le_bytes());
    for ty in 0..th {
        let y_top = th - 1 - ty;
        let sy = ((y_top as u64 * i.height as u64) / th as u64).min(i.height as u64 - 1) as u32;
        for tx in 0..tw {
            let sx = ((tx as u64 * i.width as u64) / tw as u64).min(i.width as u64 - 1) as u32;
            let p = pixel(dib, &i, sx, sy);
            let o = 40 + ty as usize * stride + tx as usize * 4;
            out[o..o + 4].copy_from_slice(&p);
        }
    }
    Some((out, tw, th))
}

/// 把 DIB 包成浏览器可直接显示的 BMP 文件字节。
pub fn to_bmp(dib: &[u8]) -> Option<Vec<u8>> {
    let i = parse(dib)?;
    let off = 14 + i.pixels_offset;
    let mut bmp = Vec::with_capacity(14 + dib.len());
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&((14 + dib.len()) as u32).to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&(off as u32).to_le_bytes());
    bmp.extend_from_slice(dib);
    Some(bmp)
}

#[allow(dead_code)]
pub fn read_clipboard_dib() -> Option<Vec<u8>> {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        if IsClipboardFormatAvailable(CF_DIB) == 0 {
            CloseClipboard();
            return None;
        }
        let h = GetClipboardData(CF_DIB);
        if h.is_null() {
            CloseClipboard();
            return None;
        }
        let p = GlobalLock(h) as *const u8;
        let mut out = Vec::new();
        if !p.is_null() {
            let n = GlobalSize(h);
            if n > 0 && n < 32 * 1024 * 1024 {
                out = std::slice::from_raw_parts(p, n).to_vec();
            }
            GlobalUnlock(h);
        }
        CloseClipboard();
        if out.is_empty() { None } else { Some(out) }
    }
}

pub fn write_clipboard_dib(dib: &[u8]) -> bool {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        let _ = EmptyClipboard();
        let h = GlobalAlloc(GMEM_MOVEABLE, dib.len());
        if h.is_null() {
            CloseClipboard();
            return false;
        }
        let p = GlobalLock(h) as *mut u8;
        if p.is_null() {
            CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(dib.as_ptr(), p, dib.len());
        GlobalUnlock(h);
        let r = SetClipboardData(CF_DIB, h);
        CloseClipboard();
        !r.is_null()
    }
}

pub fn hash(dib: &[u8]) -> u64 {
    let mut h = 14695981039346656037u64;
    for (n, b) in dib.iter().enumerate() {
        if n % 13 == 0 {
            h ^= *b as u64;
            h = h.wrapping_mul(1099511628211);
        }
    }
    h ^ dib.len() as u64
}
