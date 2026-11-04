//! Workspace: a card gallery of everything in the capture folder, grouped by day.

use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::SystemTime;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_SHIFT};
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::gdip::{Canvas, PointF, RectF};
use crate::image::Image;
use crate::ui::{self, draw_icon, theme, Bar, Fonts, Icon, Item, Look, Surface};
use crate::util::{self, w};

const CLASS: &str = "MonPressoWorkspace";
const WM_THUMB: u32 = WM_APP + 30;
const WM_REFRESH: u32 = WM_APP + 31;
const WM_MOUSELEAVE: u32 = 0x02A3;

const ID_BRAND: i32 = 1;
const ID_AREA: i32 = 10;
const ID_FULL: i32 = 11;
const ID_LAST: i32 = 12;
const ID_RECORD: i32 = 13;
const ID_FOLDER: i32 = 14;
const ID_SETTINGS: i32 = 15;

const CMD_EDIT: usize = 1;
const CMD_OPEN: usize = 2;
const CMD_COPY_IMAGE: usize = 3;
const CMD_COPY_FILE: usize = 4;
const CMD_COPY_TEXT: usize = 5;
const CMD_SHOW: usize = 6;
const CMD_DELETE: usize = 7;

const EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "bmp", "gif", "mp4"];
const TILE_BG: u32 = 0xFFEEF0F3;

struct Card {
    path: PathBuf,
    name: String,
    time: String,
    size: String,
    dims: Option<(u32, u32)>,
    badge: &'static str,
    day: i64,
    day_title: String,
    editable: bool,
    thumb: Option<Image>,
    rect: RECT,
}

impl Card {
    fn meta(&self) -> String {
        let base = match self.dims {
            Some((w, h)) => format!("{}  ·  {} × {}  ·  {}", self.time, w, h, self.size),
            None => format!("{}  ·  {}", self.time, self.size),
        };
        if self.editable {
            format!("{base}  ·  editable")
        } else {
            base
        }
    }
}

struct Section {
    title: String,
    y: i32,
}

struct Workspace {
    hwnd: HWND,
    bar: Bar,
    fonts: Fonts,
    cards: Vec<Card>,
    sections: Vec<Section>,
    selected: Vec<bool>,
    anchor: Option<usize>,
    hover: Option<usize>,
    scroll: i32,
    content_h: i32,
    drag_scroll: Option<(i32, i32)>,
    generation: usize,
    dpi: i32,
    thumb: (i32, i32),
}

thread_local! {
    static OPEN: std::cell::Cell<HWND> = const { std::cell::Cell::new(null_mut()) };
}

pub fn register() {
    util::register_class(CLASS, Some(wndproc), null_mut(), true);
}

pub fn hwnd() -> HWND {
    OPEN.with(|o| o.get())
}

fn bar_items() -> Vec<Item> {
    vec![
        Item::new(ID_BRAND, Look::Brand, None, "MonPresso", ""),
        Item::new(0, Look::Sep, None, "", ""),
        Item::new(ID_AREA, Look::Stacked, Some(Icon::Area), "Capture area", "Select an area or click a window"),
        Item::new(ID_FULL, Look::Stacked, Some(Icon::Screen), "Full screen", "Capture every screen"),
        Item::new(ID_LAST, Look::Stacked, Some(Icon::LastArea), "Last area", "Capture the same area again"),
        Item::new(ID_RECORD, Look::Stacked, Some(Icon::Record), "Record video", "Record an area of the screen (needs FFmpeg)"),
        Item::new(0, Look::Spacer, None, "", ""),
        Item::new(ID_FOLDER, Look::IconText, Some(Icon::Folder), "Open folder", "Open the capture folder in Explorer"),
        Item::new(ID_SETTINGS, Look::IconText, Some(Icon::Settings), "Settings", "Hotkeys, folders and video options"),
    ]
}

pub fn show() {
    let existing = hwnd();
    if !existing.is_null() {
        util::bring_to_front(existing);
        return;
    }
    let dpi = util::cursor_monitor_dpi();
    let (ww, wh) = (util::sc(1040, dpi), util::sc(700, dpi));
    let (x, y) = util::centered_rect(ww, wh);
    let class = w(CLASS);
    let title = w("MonPresso");
    unsafe {
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            x,
            y,
            ww,
            wh,
            null_mut(),
            null_mut(),
            util::hinstance(),
            null(),
        );
        ui::style_titlebar(hwnd);
        let dpi = util::dpi_of(hwnd);
        let ws = Box::into_raw(Box::new(Workspace {
            hwnd,
            bar: Bar::new(hwnd, bar_items()),
            fonts: Fonts::new(dpi),
            cards: Vec::new(),
            sections: Vec::new(),
            selected: Vec::new(),
            anchor: None,
            hover: None,
            scroll: 0,
            content_h: 0,
            drag_scroll: None,
            generation: 0,
            dpi,
            thumb: thumb_size(dpi),
        }));
        util::set_state(hwnd, ws);
        OPEN.with(|o| o.set(hwnd));
        (*ws).reload();
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }
}

fn thumb_size(dpi: i32) -> (i32, i32) {
    (util::sc(208, dpi), util::sc(130, dpi))
}

/// Ask an open workspace to re-read the folder.
pub fn refresh() {
    let h = hwnd();
    if !h.is_null() {
        unsafe { PostMessageW(h, WM_REFRESH, 0, 0) };
    }
}

fn list_files(folder: &str) -> Vec<(PathBuf, SystemTime, u64)> {
    let mut files: Vec<(PathBuf, SystemTime, u64)> = std::fs::read_dir(folder)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .and_then(|x| x.to_str())
                        .is_some_and(|x| EXTENSIONS.iter().any(|ok| ok.eq_ignore_ascii_case(x)))
                })
                .filter_map(|e| {
                    let m = e.metadata().ok()?;
                    Some((e.path(), m.modified().ok()?, m.len()))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort_by(|a, b| b.1.cmp(&a.1));
    files
}

fn is_editable(p: &Path) -> bool {
    p.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| ["png", "jpg", "jpeg", "bmp"].iter().any(|ok| ok.eq_ignore_ascii_case(x)))
}

fn human_size(bytes: u64) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / (1u64 << 20) as f64)
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December",
];

unsafe fn day_number(st: &SYSTEMTIME) -> i64 {
    let mut day = *st;
    day.wHour = 0;
    day.wMinute = 0;
    day.wSecond = 0;
    day.wMilliseconds = 0;
    let mut ft: FILETIME = std::mem::zeroed();
    SystemTimeToFileTime(&day, &mut ft);
    ((((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64) / 864_000_000_000) as i64
}

/// Local day number, "HH:MM" and a day heading ("Today", "Yesterday", "Monday 3 November 2026").
fn local_time(t: SystemTime) -> (i64, String, String) {
    unsafe {
        let ticks = t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos() / 100).unwrap_or(0) as u64 + 116_444_736_000_000_000;
        let ft = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
        let mut utc = std::mem::zeroed();
        FileTimeToSystemTime(&ft, &mut utc);
        let mut st: SYSTEMTIME = std::mem::zeroed();
        SystemTimeToTzSpecificLocalTime(null(), &utc, &mut st);
        let day = day_number(&st);
        let mut now = std::mem::zeroed();
        GetLocalTime(&mut now);
        let title = match day_number(&now) - day {
            0 => "Today".to_string(),
            1 => "Yesterday".to_string(),
            _ => format!(
                "{} {} {} {}",
                DAYS[st.wDayOfWeek as usize % 7],
                st.wDay,
                MONTHS[(st.wMonth as usize).saturating_sub(1) % 12],
                st.wYear
            ),
        };
        (day, format!("{:02}:{:02}", st.wHour, st.wMinute), title)
    }
}

fn video_tile(tw: i32, th: i32) -> Image {
    let mut img = Image::new(tw, th);
    {
        let c = Canvas::over(&mut img);
        c.fill_rect(RectF { x: 0.0, y: 0.0, w: tw as f32, h: th as f32 }, 0xFF2A2D34);
        let d = th as f32 * 0.36;
        let (cx, cy) = (tw as f32 / 2.0, th as f32 / 2.0);
        c.fill_ellipse(RectF { x: cx - d / 2.0, y: cy - d / 2.0, w: d, h: d }, 0x33FFFFFF);
        let t = d * 0.3;
        c.fill_polygon(
            &[
                PointF { x: cx - t * 0.7, y: cy - t },
                PointF { x: cx + t, y: cy },
                PointF { x: cx - t * 0.7, y: cy + t },
            ],
            0xFFFFFFFF,
        );
    }
    img
}

impl Workspace {
    fn sc(&self, v: i32) -> i32 {
        util::sc(v, self.dpi)
    }

    fn header_h(&self) -> i32 {
        self.bar.height
    }

    fn footer_h(&self) -> i32 {
        self.sc(30)
    }

    fn view_h(&self) -> i32 {
        let (_, ch) = util::client_size(self.hwnd);
        (ch - self.header_h() - self.footer_h()).max(1)
    }

    unsafe fn layout(&mut self) {
        let (cw, _) = util::client_size(self.hwnd);
        self.bar.layout(self.hwnd, cw, self.dpi, &self.fonts);
        let (tw, th) = self.thumb;
        let margin = self.sc(24);
        let gap = self.sc(16);
        let card_w = tw + self.sc(16);
        let card_h = th + self.sc(16) + self.sc(42);
        let avail = cw - margin * 2 - self.sc(10);
        let cols = ((avail + gap) / (card_w + gap)).max(1);
        let section_h = self.sc(40);
        let mut y = self.sc(8);
        let mut col = 0;
        let mut last_day = None;
        self.sections.clear();
        for card in self.cards.iter_mut() {
            if last_day != Some(card.day) {
                if col != 0 {
                    y += card_h + gap;
                    col = 0;
                }
                self.sections.push(Section { title: card.day_title.clone(), y });
                y += section_h;
                last_day = Some(card.day);
            }
            let x = margin + col * (card_w + gap);
            card.rect = RECT { left: x, top: y, right: x + card_w, bottom: y + card_h };
            col += 1;
            if col == cols {
                col = 0;
                y += card_h + gap;
            }
        }
        if col != 0 {
            y += card_h + gap;
        }
        self.content_h = y + self.sc(8);
        self.clamp_scroll();
        InvalidateRect(self.hwnd, null(), 0);
    }

    fn clamp_scroll(&mut self) {
        let max = (self.content_h - self.view_h()).max(0);
        self.scroll = self.scroll.clamp(0, max);
    }

    unsafe fn reload(&mut self) {
        self.generation += 1;
        let folder = crate::app::settings().save_folder;
        self.cards = list_files(&folder)
            .into_iter()
            .map(|(path, modified, len)| {
                let (day, time, day_title) = local_time(modified);
                let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
                Card {
                    name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    path: path.clone(),
                    time,
                    size: human_size(len),
                    dims: None,
                    badge: match ext.as_str() {
                        "mp4" => "MP4",
                        "gif" => "GIF",
                        _ => "",
                    },
                    day,
                    day_title,
                    editable: crate::project::has(&path),
                    thumb: None,
                    rect: RECT { left: 0, top: 0, right: 0, bottom: 0 },
                }
            })
            .collect();
        self.selected = vec![false; self.cards.len()];
        self.anchor = None;
        self.hover = None;
        self.layout();

        // Decode thumbnails in the background and post them back one by one.
        let paths: Vec<PathBuf> = self.cards.iter().map(|c| c.path.clone()).collect();
        let target = self.hwnd as usize;
        let generation = self.generation;
        let (tw, th) = self.thumb;
        std::thread::spawn(move || {
            for (i, p) in paths.iter().enumerate() {
                let s = p.to_string_lossy();
                let result = if s.to_ascii_lowercase().ends_with(".mp4") {
                    Some((video_tile(tw, th), 0, 0))
                } else {
                    crate::gdip::thumbnail(&s, tw, th, TILE_BG)
                };
                let Some(result) = result else { continue };
                let payload = Box::into_raw(Box::new((i, result)));
                if unsafe { PostMessageW(target as HWND, WM_THUMB, generation, payload as isize) } == 0 {
                    drop(unsafe { Box::from_raw(payload) });
                    break;
                }
            }
        });
    }

    unsafe fn thumb_ready(&mut self, generation: usize, payload: *mut (usize, (Image, u32, u32))) {
        let data = Box::from_raw(payload);
        if generation != self.generation {
            return;
        }
        let (i, (img, iw, ih)) = *data;
        if i < self.cards.len() {
            self.cards[i].thumb = Some(img);
            if iw > 0 {
                self.cards[i].dims = Some((iw, ih));
            }
            let r = self.screen_rect(i);
            InvalidateRect(self.hwnd, &r, 0);
        }
    }

    fn screen_rect(&self, i: usize) -> RECT {
        let r = self.cards[i].rect;
        let dy = self.header_h() - self.scroll;
        RECT { left: r.left, top: r.top + dy, right: r.right, bottom: r.bottom + dy }
    }

    fn card_at(&self, x: i32, y: i32) -> Option<usize> {
        if y < self.header_h() || y >= self.header_h() + self.view_h() {
            return None;
        }
        (0..self.cards.len()).find(|&i| {
            let r = self.screen_rect(i);
            x >= r.left && x < r.right && y >= r.top && y < r.bottom
        })
    }

    fn scrollbar(&self) -> Option<RECT> {
        let vh = self.view_h();
        if self.content_h <= vh {
            return None;
        }
        let (cw, _) = util::client_size(self.hwnd);
        let th = (vh * vh / self.content_h).max(self.sc(36));
        let top = self.header_h() + (self.scroll as i64 * (vh - th) as i64 / (self.content_h - vh) as i64) as i32;
        Some(RECT { left: cw - self.sc(10), top, right: cw - self.sc(4), bottom: top + th })
    }

    unsafe fn scroll_to(&mut self, s: i32) {
        self.scroll = s;
        self.clamp_scroll();
        InvalidateRect(self.hwnd, null(), 0);
    }

    unsafe fn paint(&mut self) {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(self.hwnd, &mut ps);
        let (cw, ch) = util::client_size(self.hwnd);
        let s = Surface::new(cw, ch);
        let header = self.header_h();
        let view_h = self.view_h();
        let visible = |r: &RECT| r.bottom > header && r.top < header + view_h;
        let (tw, th) = self.thumb;
        let inset = self.sc(8);
        let radius = self.sc(10) as f32;

        // Pass 1: card shapes.
        {
            let c = s.canvas();
            c.fill_rect(RectF { x: 0.0, y: 0.0, w: cw as f32, h: ch as f32 }, theme::BG);
            for i in 0..self.cards.len() {
                let r = self.screen_rect(i);
                if !visible(&r) {
                    continue;
                }
                let sel = self.selected[i];
                let hov = self.hover == Some(i);
                let rr = ui::rf(&r);
                c.fill_round(rr, radius, if sel { theme::ACCENT_SOFT } else { theme::PANEL });
                let (bc, bw) = if sel {
                    (theme::ACCENT, self.sc(2) as f32)
                } else if hov {
                    (0xFFC2C7D0, 1.5)
                } else {
                    (theme::BORDER, 1.0)
                };
                c.stroke_round(rr, radius, bc, bw);
                let tr = RectF { x: rr.x + inset as f32, y: rr.y + inset as f32, w: tw as f32, h: th as f32 };
                c.fill_round(tr, self.sc(6) as f32, TILE_BG);
            }
        }
        // Pass 2: thumbnails.
        for i in 0..self.cards.len() {
            let r = self.screen_rect(i);
            if visible(&r) {
                if let Some(t) = &self.cards[i].thumb {
                    s.blit(t, r.left + inset, r.top + inset);
                }
            }
        }
        // Pass 3: round the thumbnail corners, badges, scrollbar, empty state.
        {
            let c = s.canvas();
            for i in 0..self.cards.len() {
                let r = self.screen_rect(i);
                if !visible(&r) {
                    continue;
                }
                let bg = if self.selected[i] { theme::ACCENT_SOFT } else { theme::PANEL };
                let tr = RectF { x: (r.left + inset) as f32, y: (r.top + inset) as f32, w: tw as f32, h: th as f32 };
                let k = self.sc(3) as f32;
                c.stroke_round(
                    RectF { x: tr.x - k, y: tr.y - k, w: tr.w + 2.0 * k, h: tr.h + 2.0 * k },
                    self.sc(6) as f32 + k,
                    bg,
                    k * 2.0,
                );
                if !self.cards[i].badge.is_empty() {
                    let bw = self.sc(38) as f32;
                    let bh = self.sc(20) as f32;
                    c.fill_round(RectF { x: tr.x + k * 2.0, y: tr.y + k * 2.0, w: bw, h: bh }, bh / 2.0, 0xD01F2328);
                }
            }
            if let Some(sb) = self.scrollbar() {
                let col = if self.drag_scroll.is_some() { 0x99000000 } else { 0x55000000 };
                c.fill_round(ui::rf(&sb), self.sc(3) as f32, col);
            }
            if self.cards.is_empty() {
                let size = self.sc(64) as f32;
                let cy = header + view_h / 2 - self.sc(70);
                draw_icon(&c, Icon::Area, (cw as f32 - size) / 2.0, cy as f32, size, theme::FAINT);
            }
        }
        // Pass 4: text.
        let single = DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_END_ELLIPSIS;
        for sec in &self.sections {
            let y = sec.y + header - self.scroll;
            let r = RECT { left: self.sc(26), top: y, right: cw - self.sc(26), bottom: y + self.sc(40) };
            if r.bottom > header && r.top < header + view_h {
                s.text(self.fonts.bold, &sec.title, r, theme::TEXT, single);
            }
        }
        for i in 0..self.cards.len() {
            let r = self.screen_rect(i);
            if !visible(&r) {
                continue;
            }
            let card = &self.cards[i];
            let ty = r.top + inset + th + self.sc(6);
            let left = r.left + self.sc(10);
            let right = r.right - self.sc(10);
            s.text(self.fonts.normal, &card.name, RECT { left, top: ty, right, bottom: ty + self.sc(18) }, theme::TEXT, single);
            s.text(self.fonts.small, &card.meta(), RECT { left, top: ty + self.sc(18), right, bottom: ty + self.sc(34) }, theme::MUTED, single);
            if !card.badge.is_empty() {
                let k = self.sc(3);
                let bx = r.left + inset + k * 2;
                let by = r.top + inset + k * 2;
                let br = RECT { left: bx, top: by, right: bx + self.sc(38), bottom: by + self.sc(20) };
                s.text(self.fonts.small, card.badge, br, 0xFFFFFFFF, DT_SINGLELINE | DT_VCENTER | DT_CENTER);
            }
        }
        if self.cards.is_empty() {
            let cy = header + view_h / 2 + self.sc(6);
            let full = |top: i32, h: i32| RECT { left: 0, top, right: cw, bottom: top + h };
            s.text(self.fonts.title, "No captures yet", full(cy, self.sc(28)), theme::TEXT, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
            let hk = crate::app::settings().hotkey_area;
            let hint = if hk.is_empty() {
                "Click Capture area to take your first screenshot.".to_string()
            } else {
                format!("Press {hk} or click Capture area to take your first screenshot.")
            };
            s.text(self.fonts.normal, &hint, full(cy + self.sc(30), self.sc(22)), theme::MUTED, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
        }

        // Header and footer go on top.
        self.bar.paint(&s, self.dpi, &self.fonts);
        let fh = self.footer_h();
        {
            let c = s.canvas();
            c.fill_rect(RectF { x: 0.0, y: (ch - fh) as f32, w: cw as f32, h: fh as f32 }, theme::PANEL);
            c.fill_rect(RectF { x: 0.0, y: (ch - fh) as f32, w: cw as f32, h: 1.0 }, theme::BORDER);
        }
        let n = self.cards.len();
        let sel = self.selected.iter().filter(|s| **s).count();
        let folder = crate::app::settings().save_folder;
        let left = if sel > 0 {
            format!("{sel} of {n} selected")
        } else {
            format!("{n} capture{}   ·   {folder}", if n == 1 { "" } else { "s" })
        };
        let fr = RECT { left: self.sc(16), top: ch - fh, right: cw - self.sc(16), bottom: ch };
        s.text(self.fonts.small, &left, fr, theme::MUTED, single);
        s.text(self.fonts.small, "Double-click to edit  ·  Right-click for more", fr, theme::FAINT, DT_SINGLELINE | DT_VCENTER | DT_RIGHT);

        s.present(hdc, 0, 0);
        EndPaint(self.hwnd, &ps);
    }

    fn selection(&self) -> Vec<String> {
        self.cards
            .iter()
            .zip(&self.selected)
            .filter(|(_, s)| **s)
            .map(|(c, _)| c.path.to_string_lossy().into_owned())
            .collect()
    }

    unsafe fn click_card(&mut self, i: usize) {
        let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
        let shift = GetKeyState(VK_SHIFT as i32) < 0;
        if shift {
            let a = self.anchor.unwrap_or(i);
            if !ctrl {
                self.selected.iter_mut().for_each(|s| *s = false);
            }
            for k in a.min(i)..=a.max(i) {
                self.selected[k] = true;
            }
        } else if ctrl {
            self.selected[i] = !self.selected[i];
            self.anchor = Some(i);
        } else {
            self.selected.iter_mut().for_each(|s| *s = false);
            self.selected[i] = true;
            self.anchor = Some(i);
        }
        InvalidateRect(self.hwnd, null(), 0);
    }

    unsafe fn run(&mut self, cmd: usize) {
        let sel = self.selection();
        let Some(first) = sel.first().cloned() else { return };
        match cmd {
            CMD_EDIT => {
                for p in &sel {
                    if is_editable(Path::new(p)) {
                        match crate::gdip::load_image(p) {
                            Some(img) => crate::editor::open(Some(p.clone()), img),
                            None => {
                                util::msgbox(self.hwnd, &format!("Could not open {p}"), MB_ICONERROR);
                            }
                        }
                    } else {
                        util::shell_open(p);
                    }
                }
            }
            CMD_OPEN => util::shell_open(&first),
            CMD_COPY_IMAGE => {
                if let Some(img) = crate::gdip::load_image(&first) {
                    crate::clipboard::set_image(self.hwnd, &img);
                }
            }
            CMD_COPY_FILE => {
                crate::clipboard::set_files(self.hwnd, &sel);
            }
            CMD_COPY_TEXT => {
                if let Some(img) = crate::gdip::load_image(&first) {
                    SetCursor(LoadCursorW(null_mut(), IDC_WAIT));
                    crate::app::show_ocr_result(self.hwnd, crate::ocr::recognize(&img));
                }
            }
            CMD_SHOW => util::show_in_folder(&first),
            CMD_DELETE => {
                let mut list: Vec<u16> = Vec::new();
                let mut targets: Vec<String> = Vec::new();
                for p in &sel {
                    targets.push(p.clone());
                    for extra in crate::project::files(Path::new(p)) {
                        targets.push(extra.to_string_lossy().into_owned());
                    }
                }
                for p in &targets {
                    list.extend(p.encode_utf16());
                    list.push(0);
                }
                list.push(0);
                let mut op: SHFILEOPSTRUCTW = std::mem::zeroed();
                op.hwnd = self.hwnd;
                op.wFunc = FO_DELETE;
                op.pFrom = list.as_ptr();
                op.fFlags = FOF_ALLOWUNDO as u16;
                SHFileOperationW(&mut op);
                self.reload();
            }
            _ => {}
        }
    }

    unsafe fn context_menu(&mut self) {
        let sel = self.selection();
        if sel.is_empty() {
            return;
        }
        let editable = sel.iter().any(|p| is_editable(Path::new(p)));
        let menu = CreatePopupMenu();
        let add = |id: usize, text: &str, enabled: bool| {
            let t = w(text);
            AppendMenuW(menu, MF_STRING | if enabled { MF_ENABLED } else { MF_GRAYED }, id, t.as_ptr());
        };
        add(CMD_EDIT, "Edit\tEnter", editable);
        add(CMD_OPEN, "Open with default app", true);
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        add(CMD_COPY_IMAGE, "Copy image\tCtrl+C", editable);
        add(CMD_COPY_FILE, "Copy file", true);
        add(CMD_COPY_TEXT, "Copy text (OCR)", editable);
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        add(CMD_SHOW, "Show in folder", true);
        add(CMD_DELETE, "Delete\tDel", true);
        SetMenuDefaultItem(menu, CMD_EDIT as u32, 0);
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, self.hwnd, null());
        DestroyMenu(menu);
        if cmd > 0 {
            self.run(cmd as usize);
        }
    }

    unsafe fn command(&mut self, id: i32) {
        match id {
            ID_AREA => crate::app::capture(crate::app::Kind::Area, true),
            ID_FULL => crate::app::capture(crate::app::Kind::FullScreen, true),
            ID_LAST => crate::app::capture(crate::app::Kind::LastArea, true),
            ID_RECORD => crate::app::toggle_recording(),
            ID_FOLDER => {
                let folder = crate::app::settings().save_folder;
                let _ = std::fs::create_dir_all(&folder);
                util::shell_open(&folder);
            }
            ID_SETTINGS => crate::settings::show(),
            _ => {}
        }
    }

    unsafe fn key(&mut self, vk: u16) {
        let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
        let page = self.view_h() - self.sc(40);
        match vk {
            0x0D => self.run(CMD_EDIT),
            0x2E => self.run(CMD_DELETE),
            0x43 if ctrl => self.run(CMD_COPY_IMAGE),
            0x41 if ctrl => {
                self.selected.iter_mut().for_each(|s| *s = true);
                InvalidateRect(self.hwnd, null(), 0);
            }
            0x1B => {
                self.selected.iter_mut().for_each(|s| *s = false);
                InvalidateRect(self.hwnd, null(), 0);
            }
            0x74 => self.reload(),
            0x21 => self.scroll_to(self.scroll - page),
            0x22 => self.scroll_to(self.scroll + page),
            0x24 => self.scroll_to(0),
            0x23 => self.scroll_to(i32::MAX / 2),
            _ => {}
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let Some(ws) = util::state::<Workspace>(hwnd) else {
        return DefWindowProcW(hwnd, msg, wp, lp);
    };
    match msg {
        WM_PAINT => {
            ws.paint();
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_SIZE => {
            ws.layout();
            return 0;
        }
        WM_GETMINMAXINFO => {
            let mmi = &mut *(lp as *mut MINMAXINFO);
            mmi.ptMinTrackSize = POINT { x: ws.sc(640), y: ws.sc(400) };
            return 0;
        }
        WM_MOUSEMOVE => {
            let (x, y) = util::point_from_lparam(lp);
            let mut dirty = ws.bar.mouse_move(hwnd, x, y);
            if let Some((start_y, start_scroll)) = ws.drag_scroll {
                let vh = ws.view_h();
                if let Some(sb) = ws.scrollbar() {
                    let track = (vh - (sb.bottom - sb.top)).max(1);
                    let s = start_scroll + ((y - start_y) as i64 * (ws.content_h - vh) as i64 / track as i64) as i32;
                    ws.scroll_to(s);
                }
            } else {
                let h = ws.card_at(x, y);
                if h != ws.hover {
                    ws.hover = h;
                    dirty = true;
                }
            }
            if dirty {
                InvalidateRect(hwnd, null(), 0);
            }
            return 0;
        }
        WM_MOUSELEAVE => {
            ws.bar.mouse_leave();
            ws.hover = None;
            InvalidateRect(hwnd, null(), 0);
            return 0;
        }
        WM_LBUTTONDOWN => {
            SetFocus(hwnd);
            let (x, y) = util::point_from_lparam(lp);
            let near_edge = x >= util::client_size(hwnd).0 - ws.sc(14);
            if ws.bar.mouse_down(x, y) {
                SetCapture(hwnd);
            } else if let Some(sb) = ws.scrollbar().filter(|_| near_edge && y > ws.header_h()) {
                if y < sb.top || y > sb.bottom {
                    let page = ws.view_h() - ws.sc(40);
                    ws.scroll_to(ws.scroll + if y < sb.top { -page } else { page });
                } else {
                    ws.drag_scroll = Some((y, ws.scroll));
                    SetCapture(hwnd);
                }
            } else if let Some(i) = ws.card_at(x, y) {
                ws.click_card(i);
            } else if y > ws.header_h() {
                ws.selected.iter_mut().for_each(|s| *s = false);
            }
            InvalidateRect(hwnd, null(), 0);
            return 0;
        }
        WM_LBUTTONDBLCLK => {
            let (x, y) = util::point_from_lparam(lp);
            if let Some(i) = ws.card_at(x, y) {
                ws.selected.iter_mut().for_each(|s| *s = false);
                ws.selected[i] = true;
                ws.run(CMD_EDIT);
            } else if ws.bar.mouse_down(x, y) {
                SetCapture(hwnd);
            }
            return 0;
        }
        WM_LBUTTONUP => {
            let (x, y) = util::point_from_lparam(lp);
            ReleaseCapture();
            ws.drag_scroll = None;
            let clicked = ws.bar.mouse_up(x, y);
            InvalidateRect(hwnd, null(), 0);
            if let Some(id) = clicked {
                ws.command(id);
            }
            return 0;
        }
        WM_RBUTTONUP => {
            let (x, y) = util::point_from_lparam(lp);
            if let Some(i) = ws.card_at(x, y) {
                if !ws.selected[i] {
                    ws.selected.iter_mut().for_each(|s| *s = false);
                    ws.selected[i] = true;
                    ws.anchor = Some(i);
                }
                InvalidateRect(hwnd, null(), 0);
                UpdateWindow(hwnd);
                ws.context_menu();
            }
            return 0;
        }
        WM_MOUSEWHEEL => {
            let delta = (util::hiword(wp) as i16) as i32;
            ws.scroll_to(ws.scroll - delta * ws.sc(90) / 120);
            return 0;
        }
        WM_KEYDOWN => {
            ws.key(wp as u16);
            return 0;
        }
        WM_THUMB => {
            ws.thumb_ready(wp, lp as *mut (usize, (Image, u32, u32)));
            return 0;
        }
        WM_REFRESH => {
            ws.reload();
            return 0;
        }
        WM_ACTIVATE => {
            // Pick up files added or removed outside MonPresso.
            if util::loword(wp) != 0 && util::hiword(wp) == 0 {
                let folder = crate::app::settings().save_folder;
                let now: Vec<PathBuf> = list_files(&folder).into_iter().map(|f| f.0).collect();
                if now.len() != ws.cards.len() || now.iter().zip(&ws.cards).any(|(a, b)| *a != b.path) {
                    ws.reload();
                }
            }
        }
        WM_DPICHANGED => {
            let r = &*(lp as *const RECT);
            ws.dpi = util::hiword(wp) as i32;
            ws.fonts = Fonts::new(ws.dpi);
            ws.thumb = thumb_size(ws.dpi);
            SetWindowPos(hwnd, null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            ws.reload();
            return 0;
        }
        WM_NCDESTROY => {
            util::set_state::<Workspace>(hwnd, null_mut());
            OPEN.with(|o| o.set(null_mut()));
            drop(Box::from_raw(ws as *mut Workspace));
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}
