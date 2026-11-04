//! MonPresso's own look: theme colors, a vector icon set and a custom-drawn toolbar.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::gdip::{Canvas, PointF, RectF};
use crate::image::Image;
use crate::util::{self, w};

/// ARGB colors.
pub mod theme {
    pub const BG: u32 = 0xFFF5F6F8;
    pub const PANEL: u32 = 0xFFFFFFFF;
    pub const BORDER: u32 = 0xFFE3E5EA;
    pub const HOVER: u32 = 0xFFEFF1F4;
    pub const PRESS: u32 = 0xFFE2E5EA;
    pub const TEXT: u32 = 0xFF1F2328;
    pub const MUTED: u32 = 0xFF6B7280;
    pub const FAINT: u32 = 0xFFA3A9B3;
    pub const ACCENT: u32 = 0xFFE8741C;
    pub const ACCENT_HOVER: u32 = 0xFFD4630F;
    pub const ACCENT_SOFT: u32 = 0xFFFDEEE1;
    pub const CANVAS: u32 = 0xFFE7E9ED;
}

/// ARGB -> COLORREF.
pub fn cref(argb: u32) -> u32 {
    ((argb >> 16) & 0xFF) | (argb & 0xFF00) | ((argb & 0xFF) << 16)
}

pub fn rf(r: &RECT) -> RectF {
    RectF { x: r.left as f32, y: r.top as f32, w: (r.right - r.left) as f32, h: (r.bottom - r.top) as f32 }
}

/// Light caption that blends into the white toolbar (Windows 11; ignored elsewhere).
pub fn style_titlebar(hwnd: HWND) {
    unsafe {
        let caption: u32 = cref(theme::PANEL);
        let border: u32 = cref(theme::BORDER);
        DwmSetWindowAttribute(hwnd, 35, &caption as *const u32 as *const _, 4);
        DwmSetWindowAttribute(hwnd, 34, &border as *const u32 as *const _, 4);
    }
}

pub struct Fonts {
    pub normal: HFONT,
    pub bold: HFONT,
    pub small: HFONT,
    pub title: HFONT,
}

impl Fonts {
    pub fn new(dpi: i32) -> Fonts {
        let px = |pt: i32| (pt * dpi + 36) / 72;
        Fonts {
            normal: util::create_font(px(9), false, "Segoe UI"),
            bold: util::create_font(px(9), true, "Segoe UI"),
            small: util::create_font((px(8) * 95) / 100, false, "Segoe UI"),
            title: util::create_font(px(12), true, "Segoe UI"),
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        unsafe {
            for f in [self.normal, self.bold, self.small, self.title] {
                DeleteObject(f as HGDIOBJ);
            }
        }
    }
}

/// An off-screen 32-bit DIB to paint into, then blit in one go (no flicker).
pub struct Surface {
    pub dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    pub px: *mut u32,
    pub w: i32,
    pub h: i32,
}

impl Surface {
    pub fn new(w: i32, h: i32) -> Surface {
        let (w, h) = (w.max(1), h.max(1));
        unsafe {
            let bmi = crate::image::bitmap_info(w, h);
            let mut bits = null_mut();
            let dc = CreateCompatibleDC(null_mut());
            let bmp = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            let old = SelectObject(dc, bmp as HGDIOBJ);
            SetBkMode(dc, TRANSPARENT as _);
            Surface { dc, bmp, old, px: bits as *mut u32, w, h }
        }
    }

    pub fn canvas(&self) -> Canvas {
        Canvas::over_raw(self.px, self.w, self.h)
    }

    /// Copy an image 1:1 at (x, y), clipped to the surface.
    pub fn blit(&self, img: &Image, x: i32, y: i32) {
        unsafe { GdiFlush() };
        for row in 0..img.h {
            let sy = y + row;
            if sy < 0 || sy >= self.h {
                continue;
            }
            let x0 = x.max(0);
            let x1 = (x + img.w).min(self.w);
            if x1 <= x0 {
                return;
            }
            let src = &img.px[(row * img.w + (x0 - x)) as usize..(row * img.w + (x1 - x)) as usize];
            unsafe {
                std::ptr::copy_nonoverlapping(src.as_ptr(), self.px.add((sy * self.w + x0) as usize), src.len());
            }
        }
    }

    pub fn text(&self, font: HFONT, text: &str, r: RECT, color: u32, flags: u32) {
        let t: Vec<u16> = text.encode_utf16().collect();
        let mut rc = r;
        unsafe {
            let old = SelectObject(self.dc, font as HGDIOBJ);
            SetTextColor(self.dc, cref(color));
            DrawTextW(self.dc, t.as_ptr(), t.len() as i32, &mut rc, flags | DT_NOPREFIX);
            SelectObject(self.dc, old);
        }
    }

    pub fn present(&self, hdc: HDC, x: i32, y: i32) {
        unsafe {
            GdiFlush();
            BitBlt(hdc, x, y, self.w, self.h, self.dc, 0, 0, SRCCOPY);
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bmp as HGDIOBJ);
            DeleteDC(self.dc);
        }
    }
}

pub fn text_extent(font: HFONT, text: &str) -> i32 {
    util::text_width(null_mut(), font, text)
}

// ---------------------------------------------------------------------------
// Icons, designed on a 24x24 grid.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Select,
    Arrow,
    Line,
    Rect,
    Ellipse,
    Pen,
    Highlight,
    Text,
    Step,
    Blur,
    Spotlight,
    Crop,
    Undo,
    Redo,
    Copy,
    Save,
    Ocr,
    Fit,
    ZoomIn,
    ZoomOut,
    FontDown,
    FontUp,
    Plus,
    Area,
    Screen,
    LastArea,
    Record,
    Folder,
    Settings,
}

pub fn draw_icon(c: &Canvas, icon: Icon, x: f32, y: f32, size: f32, color: u32) {
    let s = size / 24.0;
    let p = |px: f32, py: f32| PointF { x: x + px * s, y: y + py * s };
    let r = |rx: f32, ry: f32, rw: f32, rh: f32| RectF { x: x + rx * s, y: y + ry * s, w: rw * s, h: rh * s };
    let sw = (1.8 * s).max(1.3);
    let line = |a: (f32, f32), b: (f32, f32)| c.line(p(a.0, a.1), p(b.0, b.1), color, sw);
    let poly = |pts: &[(f32, f32)]| {
        let v: Vec<PointF> = pts.iter().map(|q| p(q.0, q.1)).collect();
        c.polyline(&v, color, sw);
    };
    match icon {
        Icon::Select => {
            let v: Vec<PointF> = [(6.0, 3.5), (6.0, 19.0), (10.0, 15.2), (12.8, 21.0), (15.4, 19.8), (12.6, 14.2), (18.0, 14.2)]
                .iter()
                .map(|q| p(q.0, q.1))
                .collect();
            c.polygon(&v, color, sw);
        }
        Icon::Arrow => {
            line((5.0, 19.0), (18.0, 6.0));
            poly(&[(10.0, 6.0), (18.0, 6.0), (18.0, 14.0)]);
        }
        Icon::Line => line((5.0, 19.0), (19.0, 5.0)),
        Icon::Rect => c.stroke_round(r(3.5, 6.0, 17.0, 12.0), 2.0 * s, color, sw),
        Icon::Ellipse => c.ellipse(r(3.0, 5.5, 18.0, 13.0), color, sw),
        Icon::Pen => c.curve(
            &[p(4.0, 18.0), p(7.0, 4.0), p(11.0, 5.0), p(12.0, 12.0), p(13.0, 19.0), p(17.0, 20.0), p(20.0, 6.0)],
            color,
            sw,
        ),
        Icon::Highlight => {
            c.fill_round(r(3.5, 8.0, 17.0, 8.0), 1.5 * s, 0xA0FDD835);
            line((4.0, 19.5), (20.0, 19.5));
        }
        Icon::Text => {
            line((5.5, 5.0), (18.5, 5.0));
            line((12.0, 5.0), (12.0, 19.5));
            line((9.0, 19.5), (15.0, 19.5));
        }
        Icon::Step => {
            c.fill_ellipse(r(3.0, 3.0, 18.0, 18.0), color);
            c.text_centered(r(3.0, 3.5, 18.0, 18.0), "1", 11.5 * s, 0xFFFFFFFF);
        }
        Icon::Blur => {
            for (i, (bx, by)) in [(4.0, 4.0), (10.0, 4.0), (16.0, 4.0), (4.0, 10.0), (10.0, 10.0), (16.0, 10.0), (4.0, 16.0), (10.0, 16.0), (16.0, 16.0)]
                .iter()
                .enumerate()
            {
                let a = if i % 2 == 0 { 0xFF00_0000 } else { 0x5500_0000 };
                c.fill_round(r(*bx, *by, 4.6, 4.6), 0.8 * s, (color & 0x00FF_FFFF) | a);
            }
        }
        Icon::Spotlight => {
            c.fill_round(r(3.0, 5.0, 18.0, 14.0), 2.0 * s, (color & 0x00FF_FFFF) | 0x3300_0000);
            c.stroke_round(r(3.0, 5.0, 18.0, 14.0), 2.0 * s, color, sw);
            c.fill_ellipse(r(8.5, 8.5, 7.0, 7.0), color);
        }
        Icon::Crop => {
            poly(&[(7.0, 3.0), (7.0, 17.0), (21.0, 17.0)]);
            poly(&[(3.0, 7.0), (17.0, 7.0), (17.0, 21.0)]);
        }
        Icon::Undo => {
            poly(&[(8.5, 4.0), (4.5, 8.0), (8.5, 12.0)]);
            line((4.5, 8.0), (14.0, 8.0));
            c.arc(r(8.0, 8.0, 12.0, 12.0), -90.0, 180.0, color, sw);
            line((14.0, 20.0), (9.0, 20.0));
        }
        Icon::Redo => {
            poly(&[(15.5, 4.0), (19.5, 8.0), (15.5, 12.0)]);
            line((19.5, 8.0), (10.0, 8.0));
            c.arc(r(4.0, 8.0, 12.0, 12.0), 270.0, -180.0, color, sw);
            line((10.0, 20.0), (15.0, 20.0));
        }
        Icon::Copy => {
            c.stroke_round(r(8.5, 8.5, 12.0, 12.0), 2.0 * s, color, sw);
            poly(&[(4.0, 15.5), (4.0, 5.5), (5.5, 4.0), (15.5, 4.0)]);
        }
        Icon::Save => {
            line((12.0, 3.5), (12.0, 15.0));
            poly(&[(7.0, 10.0), (12.0, 15.0), (17.0, 10.0)]);
            line((5.0, 20.0), (19.0, 20.0));
        }
        Icon::Ocr => {
            for (a, b, cc) in [
                ((3.5, 8.0), (3.5, 3.5), (8.0, 3.5)),
                ((16.0, 3.5), (20.5, 3.5), (20.5, 8.0)),
                ((20.5, 16.0), (20.5, 20.5), (16.0, 20.5)),
                ((8.0, 20.5), (3.5, 20.5), (3.5, 16.0)),
            ] {
                poly(&[a, b, cc]);
            }
            line((8.5, 8.5), (15.5, 8.5));
            line((12.0, 8.5), (12.0, 16.0));
        }
        Icon::Fit => {
            for (a, b, cc) in [
                ((4.0, 9.0), (4.0, 4.0), (9.0, 4.0)),
                ((15.0, 4.0), (20.0, 4.0), (20.0, 9.0)),
                ((20.0, 15.0), (20.0, 20.0), (15.0, 20.0)),
                ((9.0, 20.0), (4.0, 20.0), (4.0, 15.0)),
            ] {
                poly(&[a, b, cc]);
            }
        }
        Icon::ZoomIn | Icon::ZoomOut => {
            c.ellipse(r(4.0, 4.0, 12.5, 12.5), color, sw);
            line((14.8, 14.8), (20.0, 20.0));
            line((7.3, 10.25), (13.2, 10.25));
            if icon == Icon::ZoomIn {
                line((10.25, 7.3), (10.25, 13.2));
            }
        }
        Icon::FontDown => {
            c.text_centered(r(1.0, 4.0, 15.0, 17.0), "A", 12.0 * s, color);
            line((16.5, 12.0), (21.5, 12.0));
        }
        Icon::FontUp => {
            c.text_centered(r(0.0, 2.0, 15.0, 19.0), "A", 16.0 * s, color);
            line((15.5, 12.0), (21.5, 12.0));
            line((18.5, 9.0), (18.5, 15.0));
        }
        Icon::Plus => {
            c.ellipse(r(3.0, 3.0, 18.0, 18.0), color, sw);
            line((12.0, 8.0), (12.0, 16.0));
            line((8.0, 12.0), (16.0, 12.0));
        }
        Icon::Area => {
            for (a, b, cc) in [
                ((3.5, 8.5), (3.5, 3.5), (8.5, 3.5)),
                ((15.5, 3.5), (20.5, 3.5), (20.5, 8.5)),
                ((20.5, 15.5), (20.5, 20.5), (15.5, 20.5)),
                ((8.5, 20.5), (3.5, 20.5), (3.5, 15.5)),
            ] {
                poly(&[a, b, cc]);
            }
            line((12.0, 8.5), (12.0, 15.5));
            line((8.5, 12.0), (15.5, 12.0));
        }
        Icon::Screen => {
            c.stroke_round(r(3.0, 4.0, 18.0, 12.5), 2.0 * s, color, sw);
            line((12.0, 16.5), (12.0, 20.0));
            line((8.0, 20.0), (16.0, 20.0));
        }
        Icon::LastArea => {
            c.arc(r(4.0, 4.0, 16.0, 16.0), -20.0, -300.0, color, sw);
            poly(&[(19.6, 4.2), (19.4, 9.3), (14.4, 9.0)]);
            c.fill_round(r(9.0, 9.0, 6.0, 6.0), 1.0 * s, color);
        }
        Icon::Record => {
            c.ellipse(r(3.0, 3.0, 18.0, 18.0), color, sw);
            c.fill_ellipse(r(7.5, 7.5, 9.0, 9.0), 0xFFE5484D);
        }
        Icon::Folder => {
            let v: Vec<PointF> = [(3.0, 6.5), (9.0, 6.5), (11.0, 9.0), (21.0, 9.0), (21.0, 19.0), (3.0, 19.0)]
                .iter()
                .map(|q| p(q.0, q.1))
                .collect();
            c.polygon(&v, color, sw);
        }
        Icon::Settings => {
            for (yy, kx) in [(7.0, 9.0), (12.0, 15.5), (17.0, 7.5)] {
                line((4.0, yy), (20.0, yy));
                c.fill_ellipse(r(kx - 2.6, yy - 2.6, 5.2, 5.2), theme::PANEL);
                c.ellipse(r(kx - 2.6, yy - 2.6, 5.2, 5.2), color, sw);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Toolbar

#[derive(Clone, Copy, PartialEq)]
pub enum Look {
    /// Square icon button.
    Icon,
    /// Icon followed by a label.
    IconText,
    /// Big icon with the label underneath.
    Stacked,
    /// Accent-filled call to action.
    Primary,
    /// Color dot.
    Swatch(u32),
    /// Stroke-width preset; the value is the dot diameter in DIPs.
    Dot(f32),
    /// Clickable text.
    Text,
    /// Non-clickable text.
    Label,
    /// App mark + name.
    Brand,
    Sep,
    /// Everything after this is right-aligned.
    Spacer,
}

pub struct Item {
    pub id: i32,
    pub look: Look,
    pub icon: Option<Icon>,
    pub label: String,
    pub tip: String,
    pub checked: bool,
    pub enabled: bool,
    pub rect: RECT,
}

impl Item {
    pub fn new(id: i32, look: Look, icon: Option<Icon>, label: &str, tip: &str) -> Item {
        Item {
            id,
            look,
            icon,
            label: label.to_string(),
            tip: tip.to_string(),
            checked: false,
            enabled: true,
            rect: RECT { left: 0, top: 0, right: 0, bottom: 0 },
        }
    }

    fn clickable(&self) -> bool {
        self.enabled && !matches!(self.look, Look::Sep | Look::Spacer | Look::Label | Look::Brand)
    }
}

pub struct Bar {
    pub items: Vec<Item>,
    pub hover: Option<usize>,
    pub press: Option<usize>,
    pub height: i32,
    tip: HWND,
    tips: usize,
    tracking: bool,
    vertical: bool,
    /// Position of the bar inside the owner window (item rects are bar-local).
    origin: (i32, i32),
}

const EMPTY: RECT = RECT { left: 0, top: 0, right: 0, bottom: 0 };

impl Bar {
    pub fn new(owner: HWND, items: Vec<Item>) -> Bar {
        let class = w("tooltips_class32");
        let tip = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST,
                class.as_ptr(),
                null(),
                WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                owner,
                null_mut(),
                util::hinstance(),
                null(),
            )
        };
        unsafe { SendMessageW(tip, TTM_SETMAXTIPWIDTH, 0, 420) };
        Bar { items, hover: None, press: None, height: 0, tip, tips: 0, tracking: false, vertical: false, origin: (0, 0) }
    }

    /// A side panel: items stacked top to bottom.
    pub fn new_vertical(owner: HWND, items: Vec<Item>) -> Bar {
        let mut bar = Bar::new(owner, items);
        bar.vertical = true;
        bar
    }

    /// Lay a vertical panel out at (x, y) in the owner window.
    pub fn layout_vertical(&mut self, owner: HWND, x: i32, y: i32, width: i32, height: i32, dpi: i32) {
        let sc = |v: i32| util::sc(v, dpi);
        self.origin = (x, y);
        self.height = height;
        let pad = sc(8);
        let mut top = sc(10);
        for it in &mut self.items {
            let h = match it.look {
                Look::Primary => sc(40),
                Look::Stacked => sc(62),
                Look::Sep => sc(13),
                _ => sc(40),
            };
            it.rect = RECT { left: pad, top, right: width - pad, bottom: top + h };
            top += h + sc(4);
        }
        self.sync_tooltips(owner);
    }

    pub fn origin(&self) -> (i32, i32) {
        self.origin
    }

    pub fn find(&mut self, id: i32) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    pub fn set_checked(&mut self, id: i32, on: bool) {
        if let Some(i) = self.find(id) {
            i.checked = on;
        }
    }

    pub fn set_enabled(&mut self, id: i32, on: bool) {
        if let Some(i) = self.find(id) {
            i.enabled = on;
        }
    }

    pub fn set_label(&mut self, id: i32, text: &str) {
        if let Some(i) = self.find(id) {
            i.label = text.to_string();
        }
    }

    fn width_of(&self, it: &Item, dpi: i32, fonts: &Fonts) -> i32 {
        let sc = |v: i32| util::sc(v, dpi);
        match it.look {
            Look::Icon => sc(36),
            Look::IconText | Look::Primary => sc(12 + 20 + 7 + 14) + text_extent(if it.look == Look::Primary { fonts.bold } else { fonts.normal }, &it.label),
            Look::Stacked => sc(70).max(text_extent(fonts.normal, &it.label) + sc(20)),
            Look::Swatch(_) => sc(26),
            Look::Dot(_) => sc(28),
            Look::Text => text_extent(fonts.normal, &it.label).max(sc(40)) + sc(14),
            Look::Label => text_extent(fonts.normal, &it.label).max(sc(20)) + sc(6),
            Look::Brand => sc(28 + 10 + 18) + text_extent(fonts.title, &it.label),
            Look::Sep => sc(17),
            Look::Spacer => 0,
        }
    }

    /// Lay the items out in rows that wrap; returns the bar height.
    pub fn layout(&mut self, owner: HWND, width: i32, dpi: i32, fonts: &Fonts) -> i32 {
        let sc = |v: i32| util::sc(v, dpi);
        let tall = self.items.iter().any(|i| i.look == Look::Stacked);
        let row_h = if tall { sc(68) } else { sc(48) };
        let item_h = |look: Look| match look {
            Look::Stacked => sc(60),
            Look::Brand => sc(40),
            _ => sc(36),
        };
        let pad = sc(10);
        let gap = sc(2);
        let widths: Vec<i32> = self.items.iter().map(|i| self.width_of(i, dpi, fonts)).collect();
        let split = self.items.iter().position(|i| i.look == Look::Spacer).unwrap_or(self.items.len());

        // Wrap whole groups (items between separators) so related controls stay together.
        let (mut x, mut row) = (pad, 0);
        let mut rows: Vec<(usize, i32, i32)> = Vec::new(); // (item, x, row)
        let mut hidden = Vec::new();
        let mut i = 0;
        while i < split {
            let mut j = i + 1;
            while j < split && self.items[j].look != Look::Sep {
                j += 1;
            }
            let group: i32 = (i..j).map(|k| widths[k] + gap).sum();
            let mut start = i;
            if x + group > width - pad && x > pad {
                x = pad;
                row += 1;
                if self.items[i].look == Look::Sep {
                    hidden.push(i);
                    start = i + 1;
                }
            }
            for k in start..j {
                rows.push((k, x, row));
                x += widths[k] + gap;
            }
            i = j;
        }
        for k in hidden {
            self.items[k].rect = EMPTY;
        }
        let right: i32 = (split + 1..self.items.len()).map(|i| widths[i] + gap).sum();
        let mut rx = width - pad - right + gap;
        if rx < x + sc(16) && split > 0 {
            row += 1;
            rx = (width - pad - right + gap).max(pad);
        }
        for i in split + 1..self.items.len() {
            rows.push((i, rx, row));
            rx += widths[i] + gap;
        }
        for (i, ix, r) in rows {
            let h = item_h(self.items[i].look);
            let top = r * row_h + (row_h - h) / 2 + if r == 0 { 0 } else { -sc(4) * r };
            self.items[i].rect = RECT { left: ix, top, right: ix + widths[i], bottom: top + h };
        }
        if split < self.items.len() {
            self.items[split].rect = EMPTY;
        }
        self.height = (row + 1) * row_h - sc(4) * row;
        self.sync_tooltips(owner);
        self.height
    }

    fn sync_tooltips(&mut self, owner: HWND) {
        unsafe {
            for id in 0..self.tips {
                let mut ti: TTTOOLINFOW = std::mem::zeroed();
                ti.cbSize = std::mem::size_of::<TTTOOLINFOW>() as u32;
                ti.hwnd = owner;
                ti.uId = id;
                SendMessageW(self.tip, TTM_DELTOOLW, 0, &ti as *const _ as LPARAM);
            }
            let mut texts: Vec<Vec<u16>> = Vec::new();
            for (id, it) in self.items.iter().enumerate() {
                if it.tip.is_empty() {
                    continue;
                }
                texts.push(w(&it.tip));
                let mut ti: TTTOOLINFOW = std::mem::zeroed();
                ti.cbSize = std::mem::size_of::<TTTOOLINFOW>() as u32;
                ti.uFlags = TTF_SUBCLASS;
                ti.hwnd = owner;
                ti.uId = id;
                ti.rect = RECT {
                    left: it.rect.left + self.origin.0,
                    top: it.rect.top + self.origin.1,
                    right: it.rect.right + self.origin.0,
                    bottom: it.rect.bottom + self.origin.1,
                };
                ti.lpszText = texts.last_mut().unwrap().as_mut_ptr();
                SendMessageW(self.tip, TTM_ADDTOOLW, 0, &ti as *const _ as LPARAM);
            }
            self.tips = self.items.len();
        }
    }

    pub fn hit(&self, x: i32, y: i32) -> Option<usize> {
        self.items.iter().position(|i| {
            i.clickable() && x >= i.rect.left && x < i.rect.right && y >= i.rect.top && y < i.rect.bottom
        })
    }

    /// Returns true when the hover state changed (repaint needed).
    pub fn mouse_move(&mut self, owner: HWND, x: i32, y: i32) -> bool {
        if !self.tracking {
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: owner,
                dwHoverTime: 0,
            };
            unsafe { TrackMouseEvent(&mut tme) };
            self.tracking = true;
        }
        let h = if y < self.height { self.hit(x, y) } else { None };
        let changed = h != self.hover;
        self.hover = h;
        changed
    }

    pub fn mouse_leave(&mut self) -> bool {
        self.tracking = false;
        let changed = self.hover.is_some() || self.press.is_some();
        self.hover = None;
        changed
    }

    pub fn mouse_down(&mut self, x: i32, y: i32) -> bool {
        self.press = if y < self.height { self.hit(x, y) } else { None };
        self.press.is_some()
    }

    /// Returns the id of a completed click.
    pub fn mouse_up(&mut self, x: i32, y: i32) -> Option<i32> {
        let p = self.press.take()?;
        (self.hit(x, y) == Some(p)).then(|| self.items[p].id)
    }

    pub fn paint(&self, s: &Surface, dpi: i32, fonts: &Fonts) {
        let sc = |v: i32| util::sc(v, dpi) as f32;
        {
            let c = s.canvas();
            if self.vertical {
                c.fill_rect(RectF { x: 0.0, y: 0.0, w: s.w as f32, h: s.h as f32 }, theme::PANEL);
                c.fill_rect(RectF { x: 0.0, y: 0.0, w: 1.0, h: s.h as f32 }, theme::BORDER);
            } else {
                c.fill_rect(RectF { x: 0.0, y: 0.0, w: s.w as f32, h: self.height as f32 }, theme::PANEL);
                c.fill_rect(RectF { x: 0.0, y: self.height as f32 - 1.0, w: s.w as f32, h: 1.0 }, theme::BORDER);
            }
            for (i, it) in self.items.iter().enumerate() {
                let r = rf(&it.rect);
                let hover = self.hover == Some(i) && it.enabled;
                let pressed = self.press == Some(i) && hover;
                let radius = sc(7);
                if it.rect.right <= it.rect.left {
                    continue;
                }
                match it.look {
                    Look::Sep if self.vertical => {
                        let y = r.y + r.h / 2.0;
                        c.fill_rect(RectF { x: r.x + sc(6), y, w: r.w - sc(12), h: 1.0 }, theme::BORDER);
                    }
                    Look::Sep => {
                        let x = r.x + r.w / 2.0;
                        c.fill_rect(RectF { x, y: r.y + sc(6), w: 1.0, h: r.h - sc(12) }, theme::BORDER);
                    }
                    Look::Swatch(color) => {
                        let d = sc(18);
                        let cx = r.x + r.w / 2.0;
                        let cy = r.y + r.h / 2.0;
                        if it.checked || hover {
                            let rd = d + sc(8);
                            let ring = RectF { x: cx - rd / 2.0, y: cy - rd / 2.0, w: rd, h: rd };
                            c.ellipse(ring, if it.checked { theme::ACCENT } else { 0xFFC5CAD3 }, sc(2));
                        }
                        let dot = RectF { x: cx - d / 2.0, y: cy - d / 2.0, w: d, h: d };
                        c.fill_ellipse(dot, color);
                        if luminance(color) > 0.85 {
                            c.ellipse(dot, 0xFFC5CAD3, 1.0);
                        }
                    }
                    Look::Dot(size) => {
                        if it.checked {
                            c.fill_round(r, radius, theme::ACCENT_SOFT);
                        } else if hover {
                            c.fill_round(r, radius, if pressed { theme::PRESS } else { theme::HOVER });
                        }
                        let d = sc(size as i32).max(2.0);
                        let dot = RectF { x: r.x + (r.w - d) / 2.0, y: r.y + (r.h - d) / 2.0, w: d, h: d };
                        c.fill_ellipse(dot, if it.checked { theme::ACCENT } else { theme::TEXT });
                    }
                    Look::Primary => {
                        let bg = if !it.enabled {
                            0xFFF2B98C
                        } else if pressed {
                            0xFFC25A0C
                        } else if hover {
                            theme::ACCENT_HOVER
                        } else {
                            theme::ACCENT
                        };
                        c.fill_round(r, radius, bg);
                        if let Some(icon) = it.icon {
                            let x0 = primary_start(&it.rect, &it.label, dpi, fonts) as f32;
                            draw_icon(&c, icon, x0, r.y + (r.h - sc(20)) / 2.0, sc(20), 0xFFFFFFFF);
                        }
                    }
                    Look::Brand => {}
                    Look::Spacer | Look::Label => {}
                    _ => {
                        if it.checked {
                            c.fill_round(r, radius, theme::ACCENT_SOFT);
                        } else if hover {
                            c.fill_round(r, radius, if pressed { theme::PRESS } else { theme::HOVER });
                        }
                        let color = if !it.enabled {
                            0xFFC3C7CE
                        } else if it.checked {
                            theme::ACCENT
                        } else {
                            theme::TEXT
                        };
                        if let Some(icon) = it.icon {
                            match it.look {
                                Look::Stacked => {
                                    let size = sc(26);
                                    draw_icon(&c, icon, r.x + (r.w - size) / 2.0, r.y + sc(8), size, color);
                                }
                                Look::IconText => draw_icon(&c, icon, r.x + sc(12), r.y + (r.h - sc(20)) / 2.0, sc(20), color),
                                _ => {
                                    let size = sc(20);
                                    draw_icon(&c, icon, r.x + (r.w - size) / 2.0, r.y + (r.h - size) / 2.0, size, color);
                                }
                            }
                        }
                    }
                }
            }
        }
        // Text goes on top with GDI for crisp ClearType.
        let sci = |v: i32| util::sc(v, dpi);
        for (i, it) in self.items.iter().enumerate() {
            let r = it.rect;
            let color = if !it.enabled { 0xFFB0B5BD } else if it.checked { theme::ACCENT } else { theme::TEXT };
            let single = DT_SINGLELINE | DT_VCENTER;
            match it.look {
                Look::IconText => {
                    let tr = RECT { left: r.left + sci(12 + 20 + 7), ..r };
                    s.text(fonts.normal, &it.label, tr, color, single | DT_LEFT);
                }
                Look::Primary => {
                    let tr = RECT { left: primary_start(&r, &it.label, dpi, fonts) + sci(20 + 7), ..r };
                    s.text(fonts.bold, &it.label, tr, 0xFFFFFFFF, single | DT_LEFT);
                }
                Look::Stacked => {
                    let tr = RECT { top: r.top + sci(38), ..r };
                    let hc = if self.hover == Some(i) { theme::TEXT } else { theme::MUTED };
                    s.text(fonts.small, &it.label, tr, if it.enabled { hc } else { 0xFFB0B5BD }, DT_CENTER | DT_TOP | DT_SINGLELINE);
                }
                Look::Text => s.text(fonts.normal, &it.label, r, color, single | DT_CENTER),
                Look::Label => s.text(fonts.normal, &it.label, r, theme::MUTED, single | DT_CENTER),
                Look::Brand => {
                    unsafe {
                        let size = sci(28);
                        DrawIconEx(s.dc, r.left, r.top + (r.bottom - r.top - size) / 2, util::app_icon(size), size, size, 0, null_mut(), DI_NORMAL);
                    }
                    let tr = RECT { left: r.left + sci(28 + 10), ..r };
                    s.text(fonts.title, &it.label, tr, theme::TEXT, single | DT_LEFT);
                }
                _ => {}
            }
        }
        let _ = sc;
    }
}

impl Drop for Bar {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.tip) };
    }
}

/// Left edge of the centered icon + label group of a primary button.
fn primary_start(r: &RECT, label: &str, dpi: i32, fonts: &Fonts) -> i32 {
    let content = util::sc(20 + 7, dpi) + text_extent(fonts.bold, label);
    r.left + ((r.right - r.left) - content).max(0) / 2
}

pub fn luminance(c: u32) -> f32 {
    let (r, g, b) = (((c >> 16) & 0xFF) as f32, ((c >> 8) & 0xFF) as f32, (c & 0xFF) as f32);
    (0.299 * r + 0.587 * g + 0.114 * b) / 255.0
}
