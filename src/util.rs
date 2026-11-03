//! Small Win32 helpers shared by every window.

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const ACCENT: u32 = 0x2B77D9; // COLORREF (BGR) for #D9772B
pub const SS_CENTERIMAGE: u32 = 0x200;

/// Null-terminated UTF-16 string.
pub fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn from_w(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

pub fn hinstance() -> HINSTANCE {
    unsafe { GetModuleHandleW(null()) }
}

pub fn dpi_of(hwnd: HWND) -> i32 {
    let d = unsafe { GetDpiForWindow(hwnd) };
    if d == 0 { 96 } else { d as i32 }
}

pub fn sc(v: i32, dpi: i32) -> i32 {
    (v * dpi + 48) / 96
}

pub fn create_font(px_height: i32, bold: bool, face: &str) -> HFONT {
    let face = w(face);
    unsafe {
        CreateFontW(
            -px_height,
            0,
            0,
            0,
            if bold { FW_SEMIBOLD as i32 } else { FW_NORMAL as i32 },
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            (DEFAULT_PITCH as u32) | (FF_DONTCARE as u32),
            face.as_ptr(),
        )
    }
}

/// Segoe UI at 9pt for the given DPI.
pub fn ui_font(dpi: i32) -> HFONT {
    create_font((9 * dpi + 36) / 72, false, "Segoe UI")
}

pub fn app_icon(size: i32) -> HICON {
    unsafe { LoadImageW(hinstance(), 1 as _, IMAGE_ICON, size, size, LR_DEFAULTCOLOR | LR_SHARED) as HICON }
}

pub fn register_class(name: &str, proc_: WNDPROC, background: HBRUSH, dblclicks: bool) {
    let name = w(name);
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW | if dblclicks { CS_DBLCLKS } else { 0 },
            lpfnWndProc: proc_,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance(),
            hIcon: app_icon(32),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: background,
            lpszMenuName: null(),
            lpszClassName: name.as_ptr(),
            hIconSm: app_icon(16),
        };
        RegisterClassExW(&wc);
    }
}

pub fn child(parent: HWND, class: &str, text: &str, style: u32, id: i32, font: HFONT) -> HWND {
    let class = w(class);
    let text = w(text);
    unsafe {
        let h = CreateWindowExW(
            0,
            class.as_ptr(),
            text.as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            10,
            10,
            parent,
            id as usize as HMENU,
            hinstance(),
            null(),
        );
        SendMessageW(h, WM_SETFONT, font as usize, 1);
        h
    }
}

pub fn set_text(hwnd: HWND, text: &str) {
    let t = w(text);
    unsafe { SetWindowTextW(hwnd, t.as_ptr()) };
}

pub fn get_text(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        let mut buf = vec![0u16; len as usize + 1];
        GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        from_w(&buf)
    }
}

pub fn move_win(hwnd: HWND, x: i32, y: i32, cx: i32, cy: i32) {
    unsafe { MoveWindow(hwnd, x, y, cx, cy, 1) };
}

pub fn msgbox(hwnd: HWND, text: &str, flags: u32) -> i32 {
    let t = w(text);
    let c = w("MonPresso");
    unsafe { MessageBoxW(hwnd, t.as_ptr(), c.as_ptr(), flags) }
}

pub fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetClientRect(hwnd, &mut r) };
    (r.right - r.left, r.bottom - r.top)
}

pub fn text_width(hwnd: HWND, font: HFONT, text: &str) -> i32 {
    let t: Vec<u16> = text.encode_utf16().collect();
    unsafe {
        let dc = GetDC(hwnd);
        let old = SelectObject(dc, font as HGDIOBJ);
        let mut sz = SIZE { cx: 0, cy: 0 };
        GetTextExtentPoint32W(dc, t.as_ptr(), t.len() as i32, &mut sz);
        SelectObject(dc, old);
        ReleaseDC(hwnd, dc);
        sz.cx
    }
}

pub fn loword(v: usize) -> u32 {
    (v & 0xFFFF) as u32
}

pub fn hiword(v: usize) -> u32 {
    ((v >> 16) & 0xFFFF) as u32
}

pub fn point_from_lparam(lp: LPARAM) -> (i32, i32) {
    ((lp & 0xFFFF) as i16 as i32, ((lp >> 16) & 0xFFFF) as i16 as i32)
}

pub fn timestamp() -> String {
    unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        format!(
            "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
        )
    }
}

pub fn shell_open(path: &str) {
    let op = w("open");
    let p = w(path);
    unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(null_mut(), op.as_ptr(), p.as_ptr(), null(), null(), SW_SHOWNORMAL);
    }
}

pub fn show_in_folder(path: &str) {
    let op = w("open");
    let exe = w("explorer.exe");
    let args = w(&format!("/select,\"{}\"", path));
    unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(null_mut(), op.as_ptr(), exe.as_ptr(), args.as_ptr(), null(), SW_SHOWNORMAL);
    }
}

pub unsafe fn state<T>(hwnd: HWND) -> Option<&'static mut T> {
    let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut T;
    if p.is_null() { None } else { Some(&mut *p) }
}

pub unsafe fn set_state<T>(hwnd: HWND, p: *mut T) {
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, p as isize);
}

pub fn bring_to_front(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        } else {
            ShowWindow(hwnd, SW_SHOW);
        }
        SetForegroundWindow(hwnd);
    }
}

#[allow(dead_code)]
pub fn null_ptr() -> *mut c_void {
    null_mut()
}

/// Center a window of the given size on the monitor holding the cursor.
pub fn centered_rect(width: i32, height: i32) -> (i32, i32) {
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(mon, &mut mi);
        let wa = mi.rcWork;
        let x = wa.left + ((wa.right - wa.left) - width).max(0) / 2;
        let y = wa.top + ((wa.bottom - wa.top) - height).max(0) / 2;
        (x, y)
    }
}

/// DPI of the monitor holding the cursor (used before a window exists).
pub fn cursor_monitor_dpi() -> i32 {
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let (mut x, mut y) = (96u32, 96u32);
        if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut x, &mut y) != 0 {
            return 96;
        }
        x as i32
    }
}
