//! Full-screen area picker drawn over a frozen screenshot.
//! Drag to select an area, click to take a window, Space for everything, Esc to cancel.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::image::{self, Image, Rect};
use crate::util::{self, w};

const CLASS: &str = "MonPressoOverlay";

struct Overlay {
    screen: Rect,
    shot: *const Image,
    windows: Vec<Rect>,
    bright_dc: HDC,
    bright_bmp: HBITMAP,
    dim_dc: HDC,
    dim_bmp: HBITMAP,
    back_dc: HDC,
    back_bmp: HBITMAP,
    font: HFONT,
    hint: String,
    note: Option<String>,
    cursor: (i32, i32),
    drag_start: Option<(i32, i32)>,
    dragging: bool,
    target: Option<Rect>,
    done: bool,
    result: Option<Rect>,
}

pub fn register() {
    util::register_class(CLASS, Some(wndproc), null_mut(), false);
}

/// Returns the chosen rectangle in absolute screen coordinates.
pub fn select_area(shot: &Image, screen: Rect, hint: &str) -> Option<Rect> {
    let windows: Vec<Rect> = image::visible_windows()
        .into_iter()
        .filter_map(|r| r.offset(-screen.x, -screen.y).intersect(&Rect::new(0, 0, screen.w, screen.h)))
        .collect();

    let mut dim = shot.clone();
    for p in dim.px.iter_mut() {
        let v = *p;
        let r = ((v >> 16) & 0xFF) * 45 / 100;
        let g = ((v >> 8) & 0xFF) * 45 / 100;
        let b = (v & 0xFF) * 45 / 100;
        *p = 0xFF00_0000 | (r << 16) | (g << 8) | b;
    }

    unsafe {
        let screen_dc = GetDC(null_mut());
        let bright_dc = CreateCompatibleDC(screen_dc);
        let bright_bmp = shot.to_hbitmap();
        SelectObject(bright_dc, bright_bmp as HGDIOBJ);
        let dim_dc = CreateCompatibleDC(screen_dc);
        let dim_bmp = dim.to_hbitmap();
        SelectObject(dim_dc, dim_bmp as HGDIOBJ);
        let back_dc = CreateCompatibleDC(screen_dc);
        let back_bmp = CreateCompatibleBitmap(screen_dc, screen.w, screen.h);
        SelectObject(back_dc, back_bmp as HGDIOBJ);
        ReleaseDC(null_mut(), screen_dc);
        drop(dim);

        let dpi = util::cursor_monitor_dpi();
        let mut st = Box::new(Overlay {
            screen,
            shot,
            windows,
            bright_dc,
            bright_bmp,
            dim_dc,
            dim_bmp,
            back_dc,
            back_bmp,
            font: util::create_font(util::sc(14, dpi), false, "Segoe UI"),
            hint: hint.to_string(),
            note: None,
            cursor: (0, 0),
            drag_start: None,
            dragging: false,
            target: None,
            done: false,
            result: None,
        });
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        st.cursor = (pt.x - screen.x, pt.y - screen.y);
        st.target = st.hover_target();

        let class = w(CLASS);
        let title = w("MonPresso capture");
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            screen.x,
            screen.y,
            screen.w,
            screen.h,
            null_mut(),
            null_mut(),
            util::hinstance(),
            null(),
        );
        let raw = Box::into_raw(st);
        util::set_state(hwnd, raw);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);

        let mut msg: MSG = std::mem::zeroed();
        while !(*raw).done {
            let r = GetMessageW(&mut msg, null_mut(), 0, 0);
            if r == 0 {
                PostQuitMessage(msg.wParam as i32);
                break;
            }
            if r == -1 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        util::set_state::<Overlay>(hwnd, null_mut());
        DestroyWindow(hwnd);
        let st = Box::from_raw(raw);
        DeleteDC(st.bright_dc);
        DeleteObject(st.bright_bmp as HGDIOBJ);
        DeleteDC(st.dim_dc);
        DeleteObject(st.dim_bmp as HGDIOBJ);
        DeleteDC(st.back_dc);
        DeleteObject(st.back_bmp as HGDIOBJ);
        DeleteObject(st.font as HGDIOBJ);
        st.result.map(|r| r.offset(screen.x, screen.y))
    }
}

impl Overlay {
    fn shot(&self) -> &Image {
        unsafe { &*self.shot }
    }

    fn hover_target(&self) -> Option<Rect> {
        let (x, y) = self.cursor;
        if let Some(r) = self.windows.iter().find(|r| r.contains(x, y)) {
            return Some(*r);
        }
        let m = image::monitor_at(x + self.screen.x, y + self.screen.y);
        m.offset(-self.screen.x, -self.screen.y).intersect(&Rect::new(0, 0, self.screen.w, self.screen.h))
    }

    fn finish(&mut self, r: Option<Rect>) {
        self.result = r.filter(|r| r.w > 0 && r.h > 0);
        self.done = true;
    }

    unsafe fn paint(&self, hwnd: HWND) {
        let dc = self.back_dc;
        let (sw, sh) = (self.screen.w, self.screen.h);
        BitBlt(dc, 0, 0, sw, sh, self.dim_dc, 0, 0, SRCCOPY);
        SetBkMode(dc, TRANSPARENT as _);
        let old_font = SelectObject(dc, self.font as HGDIOBJ);
        let dpi = util::dpi_of(hwnd);

        if let Some(r) = self.target {
            BitBlt(dc, r.x, r.y, r.w, r.h, self.bright_dc, r.x, r.y, SRCCOPY);
            let pen = CreatePen(PS_SOLID, util::sc(2, dpi), util::ACCENT);
            let old_pen = SelectObject(dc, pen as HGDIOBJ);
            let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
            Rectangle(dc, r.x, r.y, r.x + r.w, r.y + r.h);
            SelectObject(dc, old_brush);
            SelectObject(dc, old_pen);
            DeleteObject(pen as HGDIOBJ);

            // Corner and edge handles.
            let hs = util::sc(4, dpi);
            let white = CreateSolidBrush(0xFFFFFF);
            let accent = CreateSolidBrush(util::ACCENT);
            for (hx, hy) in [
                (r.x, r.y),
                (r.x + r.w / 2, r.y),
                (r.x + r.w, r.y),
                (r.x, r.y + r.h / 2),
                (r.x + r.w, r.y + r.h / 2),
                (r.x, r.y + r.h),
                (r.x + r.w / 2, r.y + r.h),
                (r.x + r.w, r.y + r.h),
            ] {
                FillRect(dc, &RECT { left: hx - hs - 1, top: hy - hs - 1, right: hx + hs + 1, bottom: hy + hs + 1 }, accent);
                FillRect(dc, &RECT { left: hx - hs + 1, top: hy - hs + 1, right: hx + hs - 1, bottom: hy + hs - 1 }, white);
            }
            DeleteObject(white as HGDIOBJ);
            DeleteObject(accent as HGDIOBJ);

            let label = format!("{} × {}", r.w, r.h);
            let lh = util::sc(22, dpi);
            let ly = if r.y - lh - 4 >= 0 { r.y - lh - 4 } else { r.y + 4 };
            self.label(dc, r.x, ly, &label, dpi);
        }

        self.magnifier(dc, dpi);

        // Hint at the top of the monitor under the cursor.
        let m = image::monitor_at(self.cursor.0 + self.screen.x, self.cursor.1 + self.screen.y)
            .offset(-self.screen.x, -self.screen.y);
        let text = self.note.clone().unwrap_or_else(|| self.hint.clone());
        let tw = util::text_width(hwnd, self.font, &text) + util::sc(24, dpi);
        self.label(dc, m.x + (m.w - tw) / 2, m.y + util::sc(16, dpi), &text, dpi);

        SelectObject(dc, old_font);
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);
        BitBlt(hdc, 0, 0, sw, sh, dc, 0, 0, SRCCOPY);
        EndPaint(hwnd, &ps);
    }

    unsafe fn label(&self, dc: HDC, x: i32, y: i32, text: &str, dpi: i32) {
        let t: Vec<u16> = text.encode_utf16().collect();
        let mut sz = SIZE { cx: 0, cy: 0 };
        GetTextExtentPoint32W(dc, t.as_ptr(), t.len() as i32, &mut sz);
        let pad = util::sc(8, dpi);
        let mut rc = RECT { left: x, top: y, right: x + sz.cx + pad * 2, bottom: y + sz.cy + pad };
        let bg = CreateSolidBrush(0x2B2320);
        let ob = SelectObject(dc, bg as HGDIOBJ);
        let op = SelectObject(dc, GetStockObject(NULL_PEN));
        let rad = util::sc(10, dpi);
        RoundRect(dc, rc.left, rc.top, rc.right + 1, rc.bottom + 1, rad, rad);
        SelectObject(dc, op);
        SelectObject(dc, ob);
        DeleteObject(bg as HGDIOBJ);
        SetTextColor(dc, 0xFFFFFF);
        rc.left += pad;
        rc.top += pad / 2;
        DrawTextW(dc, t.as_ptr(), t.len() as i32, &mut rc, DT_LEFT | DT_TOP | DT_NOPREFIX | DT_SINGLELINE);
    }

    unsafe fn magnifier(&self, dc: HDC, dpi: i32) {
        let cells = 15;
        let cell = util::sc(8, dpi).max(6);
        let size = cells * cell;
        let (cx, cy) = self.cursor;
        let info_h = util::sc(44, dpi);
        let gap = util::sc(20, dpi);
        let mon = image::monitor_at(cx + self.screen.x, cy + self.screen.y).offset(-self.screen.x, -self.screen.y);
        let mut x = cx + gap;
        let mut y = cy + gap;
        if x + size > mon.x + mon.w {
            x = cx - gap - size;
        }
        if y + size + info_h > mon.y + mon.h {
            y = cy - gap - size - info_h;
        }

        SetStretchBltMode(dc, COLORONCOLOR);
        let half = cells / 2;
        StretchBlt(dc, x, y, size, size, self.bright_dc, cx - half, cy - half, cells, cells, SRCCOPY);

        let border = CreatePen(PS_SOLID, 2, 0xFFFFFF);
        let old_pen = SelectObject(dc, border as HGDIOBJ);
        let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
        Rectangle(dc, x - 1, y - 1, x + size + 1, y + size + 1);
        let accent = CreatePen(PS_SOLID, 1, util::ACCENT);
        SelectObject(dc, accent as HGDIOBJ);
        Rectangle(dc, x + half * cell, y + half * cell, x + (half + 1) * cell + 1, y + (half + 1) * cell + 1);
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        DeleteObject(border as HGDIOBJ);
        DeleteObject(accent as HGDIOBJ);

        let px = self.shot().pixel(cx, cy);
        let (r, g, b) = ((px >> 16) & 0xFF, (px >> 8) & 0xFF, px & 0xFF);
        let mut rc = RECT { left: x - 1, top: y + size + 1, right: x + size + 1, bottom: y + size + info_h };
        let bg = CreateSolidBrush(0x2B2320);
        FillRect(dc, &rc, bg);
        DeleteObject(bg as HGDIOBJ);
        let sw = util::sc(14, dpi);
        let swatch_rc = RECT {
            left: rc.left + util::sc(6, dpi),
            top: rc.top + (info_h - sw) / 2,
            right: rc.left + util::sc(6, dpi) + sw,
            bottom: rc.top + (info_h - sw) / 2 + sw,
        };
        let swatch = CreateSolidBrush(r | (g << 8) | (b << 16));
        FillRect(dc, &swatch_rc, swatch);
        DeleteObject(swatch as HGDIOBJ);
        let text = format!("{}, {}\n#{:02X}{:02X}{:02X}", cx + self.screen.x, cy + self.screen.y, r, g, b);
        let t: Vec<u16> = text.encode_utf16().collect();
        rc.left = swatch_rc.right + util::sc(6, dpi);
        rc.top += util::sc(4, dpi);
        SetTextColor(dc, 0xFFFFFF);
        DrawTextW(dc, t.as_ptr(), t.len() as i32, &mut rc, DT_LEFT | DT_NOPREFIX);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let Some(st) = util::state::<Overlay>(hwnd) else {
        return DefWindowProcW(hwnd, msg, wp, lp);
    };
    match msg {
        WM_PAINT => {
            st.paint(hwnd);
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_SETCURSOR => {
            SetCursor(LoadCursorW(null_mut(), IDC_CROSS));
            return 1;
        }
        WM_LBUTTONDOWN => {
            SetCapture(hwnd);
            st.drag_start = Some(util::point_from_lparam(lp));
            st.dragging = false;
        }
        WM_MOUSEMOVE => {
            st.cursor = util::point_from_lparam(lp);
            if let Some((sx, sy)) = st.drag_start {
                let (x, y) = st.cursor;
                if !st.dragging && ((x - sx).abs() > 3 || (y - sy).abs() > 3) {
                    st.dragging = true;
                }
                if st.dragging {
                    st.target = Some(Rect::from_points(sx, sy, x, y));
                }
            } else {
                st.target = st.hover_target();
            }
            InvalidateRect(hwnd, null(), 0);
        }
        WM_LBUTTONUP => {
            ReleaseCapture();
            if st.drag_start.take().is_some() {
                if !st.dragging {
                    st.cursor = util::point_from_lparam(lp);
                    st.target = st.hover_target();
                }
                let t = st.target;
                st.finish(t);
            }
        }
        WM_RBUTTONUP => st.finish(None),
        WM_KEYDOWN => {
            let step = if GetKeyState(VK_SHIFT as i32) < 0 { 10 } else { 1 };
            match wp as u16 {
                VK_ESCAPE => st.finish(None),
                VK_RETURN => {
                    let t = st.target;
                    st.finish(t);
                }
                VK_SPACE => {
                    let all = Rect::new(0, 0, st.screen.w, st.screen.h);
                    st.finish(Some(all));
                }
                VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN => {
                    let mut pt = POINT { x: 0, y: 0 };
                    GetCursorPos(&mut pt);
                    match wp as u16 {
                        VK_LEFT => pt.x -= step,
                        VK_RIGHT => pt.x += step,
                        VK_UP => pt.y -= step,
                        _ => pt.y += step,
                    }
                    SetCursorPos(pt.x, pt.y);
                }
                k if k == b'C' as u16 => {
                    let px = st.shot().pixel(st.cursor.0, st.cursor.1);
                    let hex = format!("#{:02X}{:02X}{:02X}", (px >> 16) & 0xFF, (px >> 8) & 0xFF, px & 0xFF);
                    crate::clipboard::set_text(hwnd, &hex);
                    st.note = Some(format!("Copied {hex} to the clipboard"));
                    InvalidateRect(hwnd, null(), 0);
                }
                _ => {}
            }
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}
