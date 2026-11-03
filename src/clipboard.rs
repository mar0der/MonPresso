//! Clipboard: images (DIB + PNG), text and file lists.

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::DataExchange::*;
use windows_sys::Win32::System::Memory::*;

use crate::image::Image;
use crate::util::w;

const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;

unsafe fn open(hwnd: HWND) -> bool {
    for _ in 0..10 {
        if OpenClipboard(hwnd) != 0 {
            EmptyClipboard();
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    false
}

unsafe fn put(format: u32, bytes: &[u8]) {
    let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
    if h.is_null() {
        return;
    }
    let p = GlobalLock(h) as *mut u8;
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
    GlobalUnlock(h);
    if SetClipboardData(format, h as HANDLE).is_null() {
        GlobalFree(h);
    }
}

pub fn set_image(hwnd: HWND, img: &Image) -> bool {
    // CF_DIB: BITMAPINFOHEADER + bottom-up 32-bit rows.
    let mut dib = Vec::with_capacity(40 + img.px.len() * 4);
    let header: [i32; 10] = [40, img.w, img.h, 1 | (32 << 16), 0, img.w * img.h * 4, 0, 0, 0, 0];
    for v in header {
        dib.extend_from_slice(&v.to_le_bytes());
    }
    for y in (0..img.h).rev() {
        let row = &img.px[(y * img.w) as usize..((y + 1) * img.w) as usize];
        for p in row {
            dib.extend_from_slice(&p.to_le_bytes());
        }
    }
    // PNG for apps that prefer it (keeps quality and transparency).
    let tmp = std::env::temp_dir().join(format!("monpresso-clip-{}.png", std::process::id()));
    let png = crate::gdip::save_image(img, &tmp.to_string_lossy()).then(|| std::fs::read(&tmp).ok()).flatten();
    let _ = std::fs::remove_file(&tmp);

    unsafe {
        if !open(hwnd) {
            return false;
        }
        put(CF_DIB, &dib);
        if let Some(png) = png {
            let name = w("PNG");
            let fmt = RegisterClipboardFormatW(name.as_ptr());
            if fmt != 0 {
                put(fmt, &png);
            }
        }
        CloseClipboard();
    }
    true
}

pub fn set_text(hwnd: HWND, text: &str) -> bool {
    let wide = w(text);
    let bytes: Vec<u8> = wide.iter().flat_map(|c| c.to_le_bytes()).collect();
    unsafe {
        if !open(hwnd) {
            return false;
        }
        put(CF_UNICODETEXT, &bytes);
        CloseClipboard();
    }
    true
}

pub fn set_files(hwnd: HWND, paths: &[String]) -> bool {
    // DROPFILES { pFiles, pt.x, pt.y, fNC, fWide } followed by a double-null list.
    let mut bytes = Vec::new();
    for v in [20u32, 0, 0, 0, 1] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    for p in paths {
        for c in w(p) {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&[0, 0]);
    unsafe {
        if !open(hwnd) {
            return false;
        }
        put(CF_HDROP, &bytes);
        CloseClipboard();
    }
    true
}
