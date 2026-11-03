//! Pixel buffers and screen capture.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Premultiplied BGRA pixels (0xAARRGGBB as u32), top-down.
#[derive(Clone)]
pub struct Image {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn from_points(ax: i32, ay: i32, bx: i32, by: i32) -> Rect {
        Rect::new(ax.min(bx), ay.min(by), (ax - bx).abs() + 1, (ay - by).abs() + 1)
    }

    pub fn from_win(r: &RECT) -> Rect {
        Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top)
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let x1 = self.x.max(o.x);
        let y1 = self.y.max(o.y);
        let x2 = (self.x + self.w).min(o.x + o.w);
        let y2 = (self.y + self.h).min(o.y + o.h);
        (x2 > x1 && y2 > y1).then(|| Rect::new(x1, y1, x2 - x1, y2 - y1))
    }

    pub fn offset(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }
}

impl Image {
    pub fn new(w: i32, h: i32) -> Image {
        Image { w, h, px: vec![0; (w.max(0) * h.max(0)) as usize] }
    }

    pub fn crop(&self, r: Rect) -> Image {
        let r = r.intersect(&Rect::new(0, 0, self.w, self.h)).unwrap_or(Rect::new(0, 0, 1, 1));
        let mut out = Image::new(r.w, r.h);
        for y in 0..r.h {
            let src = ((r.y + y) * self.w + r.x) as usize;
            let dst = (y * r.w) as usize;
            out.px[dst..dst + r.w as usize].copy_from_slice(&self.px[src..src + r.w as usize]);
        }
        out
    }

    pub fn bitmap_info(&self) -> BITMAPINFO {
        bitmap_info(self.w, self.h)
    }

    pub fn to_hbitmap(&self) -> HBITMAP {
        unsafe {
            let bmi = self.bitmap_info();
            let mut bits = null_mut();
            let hbm = CreateDIBSection(null_mut(), &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if !hbm.is_null() && !bits.is_null() {
                std::ptr::copy_nonoverlapping(self.px.as_ptr(), bits as *mut u32, self.px.len());
            }
            hbm
        }
    }

    pub fn pixel(&self, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            0
        } else {
            self.px[(y * self.w + x) as usize]
        }
    }
}

pub fn bitmap_info(w: i32, h: i32) -> BITMAPINFO {
    let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = -h;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;
    bmi
}

pub fn virtual_screen() -> Rect {
    unsafe {
        Rect::new(
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    }
}

/// Grab a rectangle of the screen (physical pixels, absolute coordinates).
pub fn capture(r: Rect, cursor: bool) -> Image {
    let mut img = Image::new(r.w, r.h);
    unsafe {
        let screen = GetDC(null_mut());
        let mem = CreateCompatibleDC(screen);
        let bmi = bitmap_info(r.w, r.h);
        let mut bits = null_mut();
        let hbm = CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        let old = SelectObject(mem, hbm as HGDIOBJ);
        BitBlt(mem, 0, 0, r.w, r.h, screen, r.x, r.y, SRCCOPY | CAPTUREBLT);
        if cursor {
            draw_cursor(mem, r.x, r.y);
        }
        GdiFlush();
        if !bits.is_null() {
            std::ptr::copy_nonoverlapping(bits as *const u32, img.px.as_mut_ptr(), img.px.len());
        }
        SelectObject(mem, old);
        DeleteObject(hbm as HGDIOBJ);
        DeleteDC(mem);
        ReleaseDC(null_mut(), screen);
    }
    for p in img.px.iter_mut() {
        *p |= 0xFF00_0000;
    }
    img
}

unsafe fn draw_cursor(dc: HDC, ox: i32, oy: i32) {
    let mut ci: CURSORINFO = std::mem::zeroed();
    ci.cbSize = std::mem::size_of::<CURSORINFO>() as u32;
    if GetCursorInfo(&mut ci) == 0 || ci.flags & CURSOR_SHOWING == 0 {
        return;
    }
    let mut ii: ICONINFO = std::mem::zeroed();
    if GetIconInfo(ci.hCursor, &mut ii) == 0 {
        return;
    }
    DrawIconEx(
        dc,
        ci.ptScreenPos.x - ox - ii.xHotspot as i32,
        ci.ptScreenPos.y - oy - ii.yHotspot as i32,
        ci.hCursor,
        0,
        0,
        0,
        null_mut(),
        DI_NORMAL,
    );
    if !ii.hbmMask.is_null() {
        DeleteObject(ii.hbmMask as HGDIOBJ);
    }
    if !ii.hbmColor.is_null() {
        DeleteObject(ii.hbmColor as HGDIOBJ);
    }
}

pub fn window_bounds(hwnd: HWND) -> Option<Rect> {
    unsafe {
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let hr = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            &mut r as *mut RECT as *mut _,
            std::mem::size_of::<RECT>() as u32,
        );
        if hr != 0 && GetWindowRect(hwnd, &mut r) == 0 {
            return None;
        }
        let rect = Rect::from_win(&r);
        (rect.w > 0 && rect.h > 0).then_some(rect)
    }
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let out = &mut *(lparam as *mut Vec<Rect>);
    if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 || GetWindowTextLengthW(hwnd) == 0 {
        return 1;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == GetCurrentProcessId() {
        return 1;
    }
    let mut cloaked = 0u32;
    DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED as u32, &mut cloaked as *mut u32 as *mut _, 4);
    if cloaked != 0 {
        return 1;
    }
    let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if ex & WS_EX_TRANSPARENT != 0 {
        return 1;
    }
    if let Some(r) = window_bounds(hwnd) {
        out.push(r);
    }
    1
}

/// Visible top-level windows of other processes, topmost first (absolute coordinates).
pub fn visible_windows() -> Vec<Rect> {
    let mut v: Vec<Rect> = Vec::new();
    unsafe { EnumWindows(Some(enum_proc), &mut v as *mut Vec<Rect> as LPARAM) };
    v
}

pub fn monitor_at(x: i32, y: i32) -> Rect {
    unsafe {
        let mon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(mon, &mut mi);
        Rect::from_win(&mi.rcMonitor)
    }
}
