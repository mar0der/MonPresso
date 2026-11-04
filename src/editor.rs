//! Annotation editor: vector annotations over a capture, undo/redo, zoom, crop, export.

use std::ptr::{null, null_mut};
use std::rc::Rc;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::Controls::Dialogs::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::gdip::{self, Canvas, PointF, RectF};
use crate::image::{Image, Rect};
use crate::ui::{self, theme, Bar, Fonts, Icon, Item, Look, Surface};
use crate::util::{self, w};

const CLASS: &str = "MonPressoEditor";
const CANVAS_CLASS: &str = "MonPressoCanvas";
const WM_COMMIT_TEXT: u32 = WM_APP + 20;

const ID_TOOL: i32 = 100;
const ID_SWATCH: i32 = 200;
const ID_MORE_COLORS: i32 = 220;
const ID_WIDTH: i32 = 300;
const ID_FONT_DOWN: i32 = 310;
const ID_FONT_LABEL: i32 = 311;
const ID_FONT_UP: i32 = 312;
const ID_UNDO: i32 = 400;
const ID_REDO: i32 = 401;
const ID_FIT: i32 = 402;
const ID_ACTUAL: i32 = 403;
const ID_COPY: i32 = 404;
const ID_OCR: i32 = 405;
const ID_SAVE_AS: i32 = 406;
const ID_SAVE: i32 = 407;
const ID_ZOOM_OUT: i32 = 408;
const ID_ZOOM_IN: i32 = 409;
const ID_TEXT_EDIT: i32 = 600;

const SWATCHES: [u32; 8] = [0xFFE53935, 0xFFFB8C00, 0xFFFDD835, 0xFF43A047, 0xFF1E88E5, 0xFF8E24AA, 0xFF212121, 0xFFFFFFFF];
/// (stroke width, dot size in the toolbar)
const WIDTHS: [(f32, f32); 4] = [(2.0, 4.0), (4.0, 7.0), (8.0, 10.0), (14.0, 14.0)];
const FONT_SIZES: [f32; 9] = [12.0, 16.0, 20.0, 24.0, 32.0, 40.0, 48.0, 64.0, 96.0];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Select,
    Arrow,
    Line,
    Rect,
    Ellipse,
    Pen,
    Marker,
    Text,
    Step,
    Blur,
    Spot,
    Crop,
}

const TOOLS: [(Tool, &str, u8, &str, Icon); 12] = [
    (Tool::Select, "Select", b'V', "Click to select, drag to move, Delete removes, double-click text to edit", Icon::Select),
    (Tool::Arrow, "Arrow", b'A', "Drag to draw an arrow (Shift snaps to 45°)", Icon::Arrow),
    (Tool::Line, "Line", b'L', "Drag to draw a line (Shift snaps to 45°)", Icon::Line),
    (Tool::Rect, "Rectangle", b'R', "Drag to draw a rectangle (Shift for a square)", Icon::Rect),
    (Tool::Ellipse, "Ellipse", b'E', "Drag to draw an ellipse (Shift for a circle)", Icon::Ellipse),
    (Tool::Pen, "Pen", b'P', "Drag to draw freehand", Icon::Pen),
    (Tool::Marker, "Highlight", b'H', "Drag over text to highlight it", Icon::Highlight),
    (Tool::Text, "Text", b'T', "Click to type; click outside or press Esc to finish", Icon::Text),
    (Tool::Step, "Step", b'N', "Click to drop the next numbered step", Icon::Step),
    (Tool::Blur, "Blur", b'B', "Drag over sensitive content to pixelate it", Icon::Blur),
    (Tool::Spot, "Spotlight", b'S', "Drag to keep an area bright and dim the rest", Icon::Spotlight),
    (Tool::Crop, "Crop", b'C', "Drag the area to keep", Icon::Crop),
];

#[derive(Clone, Copy, PartialEq)]
struct P {
    x: f32,
    y: f32,
}

fn pf(p: P) -> PointF {
    PointF { x: p.x, y: p.y }
}

#[derive(Clone)]
struct Ann {
    tool: Tool,
    a: P,
    b: P,
    pts: Vec<P>,
    text: String,
    n: u32,
    color: u32,
    width: f32,
    size: f32,
}

impl Ann {
    fn rect(&self) -> RectF {
        RectF {
            x: self.a.x.min(self.b.x),
            y: self.a.y.min(self.b.y),
            w: (self.a.x - self.b.x).abs(),
            h: (self.a.y - self.b.y).abs(),
        }
    }

    fn pixel_rect(&self, img: &Image) -> Option<Rect> {
        let r = self.rect();
        let x = r.x.floor() as i32;
        let y = r.y.floor() as i32;
        Rect::new(x, y, (r.x + r.w).ceil() as i32 - x, (r.y + r.h).ceil() as i32 - y).intersect(&Rect::new(0, 0, img.w, img.h))
    }

    fn step_radius(&self) -> f32 {
        (self.size * 0.7).max(10.0)
    }

    fn bounds(&self) -> RectF {
        let pad = self.width / 2.0 + 2.0;
        let r = match self.tool {
            Tool::Pen => {
                let (mut x1, mut y1, mut x2, mut y2) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for p in &self.pts {
                    x1 = x1.min(p.x);
                    y1 = y1.min(p.y);
                    x2 = x2.max(p.x);
                    y2 = y2.max(p.y);
                }
                RectF { x: x1, y: y1, w: x2 - x1, h: y2 - y1 }
            }
            Tool::Text => return inflate(gdip::text_bounds(&self.text, self.size, self.a.x, self.a.y), 4.0),
            Tool::Step => {
                let r = self.step_radius();
                return RectF { x: self.a.x - r - 2.0, y: self.a.y - r - 2.0, w: 2.0 * r + 4.0, h: 2.0 * r + 4.0 };
            }
            Tool::Arrow => return inflate(self.rect(), arrow_head(self.width).max(pad)),
            _ => self.rect(),
        };
        inflate(r, pad)
    }

    fn hit(&self, p: P, tol: f32) -> bool {
        let reach = tol + self.width / 2.0;
        match self.tool {
            Tool::Line => seg_dist(p, self.a, self.b) <= reach,
            Tool::Arrow => seg_dist(p, self.a, self.b) <= reach.max(arrow_head(self.width) * 0.5),
            Tool::Pen => self.pts.windows(2).any(|s| seg_dist(p, s[0], s[1]) <= reach),
            _ => {
                let b = inflate(self.bounds(), tol);
                p.x >= b.x && p.y >= b.y && p.x <= b.x + b.w && p.y <= b.y + b.h
            }
        }
    }

    fn translate(&mut self, dx: f32, dy: f32) {
        self.a.x += dx;
        self.a.y += dy;
        self.b.x += dx;
        self.b.y += dy;
        for p in &mut self.pts {
            p.x += dx;
            p.y += dy;
        }
    }

    fn is_meaningful(&self) -> bool {
        match self.tool {
            Tool::Pen => self.pts.len() >= 2,
            _ => (self.a.x - self.b.x).hypot(self.a.y - self.b.y) >= 3.0,
        }
    }
}

fn inflate(r: RectF, d: f32) -> RectF {
    RectF { x: r.x - d, y: r.y - d, w: r.w + 2.0 * d, h: r.h + 2.0 * d }
}

fn seg_dist(p: P, a: P, b: P) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0) };
    (p.x - (a.x + t * dx)).hypot(p.y - (a.y + t * dy))
}

fn arrow_head(width: f32) -> f32 {
    (width * 4.0).max(16.0)
}

fn snap45(a: P, b: P) -> P {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = dx.hypot(dy);
    let step = std::f32::consts::FRAC_PI_4;
    let ang = (dy.atan2(dx) / step).round() * step;
    P { x: a.x + len * ang.cos(), y: a.y + len * ang.sin() }
}

fn square(a: P, b: P) -> P {
    let s = (b.x - a.x).abs().max((b.y - a.y).abs());
    P { x: a.x + s * (b.x - a.x).signum(), y: a.y + s * (b.y - a.y).signum() }
}

fn luminance(c: u32) -> f32 {
    let (r, g, b) = (((c >> 16) & 0xFF) as f32, ((c >> 8) & 0xFF) as f32, (c & 0xFF) as f32);
    (0.299 * r + 0.587 * g + 0.114 * b) / 255.0
}

fn colorref(argb: u32) -> u32 {
    ((argb >> 16) & 0xFF) | (argb & 0xFF00) | ((argb & 0xFF) << 16)
}

// ---------------------------------------------------------------------------
// Rendering

fn draw_arrow(c: &Canvas, a: P, b: P, color: u32, width: f32) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = dx.hypot(dy);
    if len < 1.0 {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    let head = arrow_head(width).min(len);
    let half = head * 0.55;
    let base = P { x: b.x - ux * head, y: b.y - uy * head };
    c.line(pf(a), PointF { x: base.x + ux, y: base.y + uy }, color, width);
    c.fill_polygon(
        &[
            pf(b),
            PointF { x: base.x - uy * half, y: base.y + ux * half },
            PointF { x: base.x + uy * half, y: base.y - ux * half },
        ],
        color,
    );
}

fn multiply(img: &mut Image, r: Rect, color: u32) {
    let (cr, cg, cb) = ((color >> 16) & 0xFF, (color >> 8) & 0xFF, color & 0xFF);
    for y in r.y..r.y + r.h {
        for x in r.x..r.x + r.w {
            let p = &mut img.px[(y * img.w + x) as usize];
            let v = *p;
            let nr = ((v >> 16) & 0xFF) * cr / 255;
            let ng = ((v >> 8) & 0xFF) * cg / 255;
            let nb = (v & 0xFF) * cb / 255;
            *p = (v & 0xFF00_0000) | (nr << 16) | (ng << 8) | nb;
        }
    }
}

fn pixelate(img: &mut Image, r: Rect, block: i32) {
    let mut by = r.y;
    while by < r.y + r.h {
        let bh = block.min(r.y + r.h - by);
        let mut bx = r.x;
        while bx < r.x + r.w {
            let bw = block.min(r.x + r.w - bx);
            let mut sum = [0u64; 4];
            for y in by..by + bh {
                for x in bx..bx + bw {
                    let v = img.px[(y * img.w + x) as usize];
                    for (i, s) in sum.iter_mut().enumerate() {
                        *s += ((v >> (i * 8)) & 0xFF) as u64;
                    }
                }
            }
            let n = (bw * bh) as u64;
            let avg = sum.iter().enumerate().fold(0u32, |acc, (i, s)| acc | (((s / n) as u32) << (i * 8)));
            for y in by..by + bh {
                for x in bx..bx + bw {
                    img.px[(y * img.w + x) as usize] = avg;
                }
            }
            bx += block;
        }
        by += block;
    }
}

fn draw(c: &Canvas, img: &mut Image, a: &Ann) {
    let col = a.color;
    match a.tool {
        Tool::Arrow => draw_arrow(c, a.a, a.b, col, a.width),
        Tool::Line => c.line(pf(a.a), pf(a.b), col, a.width),
        Tool::Rect => c.rect(a.rect(), col, a.width),
        Tool::Ellipse => c.ellipse(a.rect(), col, a.width),
        Tool::Pen => {
            let pts: Vec<PointF> = a.pts.iter().map(|p| pf(*p)).collect();
            c.polyline(&pts, col, a.width);
        }
        Tool::Text => {
            let halo = if luminance(col) > 0.55 { 0xC0000000 } else { 0xF0FFFFFF };
            c.text(a.a.x, a.a.y, &a.text, a.size, col, halo);
        }
        Tool::Step => {
            let r = a.step_radius();
            let rc = RectF { x: a.a.x - r, y: a.a.y - r, w: 2.0 * r, h: 2.0 * r };
            c.fill_ellipse(rc, col);
            c.ellipse(rc, 0xFFFFFFFF, (r / 7.0).max(1.5));
            let fg = if luminance(col) > 0.6 { 0xFF000000 } else { 0xFFFFFFFF };
            c.text_centered(rc, &a.n.to_string(), r * 1.05, fg);
        }
        Tool::Spot => {
            let all = RectF { x: 0.0, y: 0.0, w: img.w as f32, h: img.h as f32 };
            c.fill_except(all, a.rect(), 0x99000000);
        }
        Tool::Marker => {
            if let Some(r) = a.pixel_rect(img) {
                c.flush();
                multiply(img, r, col);
            }
        }
        Tool::Blur => {
            if let Some(r) = a.pixel_rect(img) {
                c.flush();
                pixelate(img, r, (a.width * 2.5).max(6.0) as i32);
            }
        }
        Tool::Select | Tool::Crop => {}
    }
}

fn render(base: &Image, items: &[Ann], hidden: Option<usize>, preview: Option<&Ann>) -> Image {
    let mut img = base.clone();
    if items.is_empty() && preview.is_none() {
        return img;
    }
    let canvas = Canvas::over(&mut img);
    for (i, a) in items.iter().enumerate() {
        if Some(i) != hidden {
            draw(&canvas, &mut img, a);
        }
    }
    if let Some(a) = preview {
        draw(&canvas, &mut img, a);
    }
    drop(canvas);
    img
}

// ---------------------------------------------------------------------------
// Editor window

#[derive(Clone)]
struct Doc {
    base: Rc<Image>,
    items: Vec<Ann>,
}

enum Drag {
    None,
    Draw(Ann),
    Move { last: P, moved: bool, was_dirty: bool },
    Crop { a: P, b: P },
}

struct TextEdit {
    hwnd: HWND,
    font: HFONT,
    at: P,
    replacing: Option<usize>,
    color: u32,
    size: f32,
}

struct Editor {
    hwnd: HWND,
    canvas: HWND,
    path: Option<String>,
    doc: Doc,
    undo: Vec<Doc>,
    redo: Vec<Doc>,
    dirty: bool,
    tool: Tool,
    color: u32,
    width: f32,
    font_size: f32,
    custom_colors: [u32; 16],
    zoom: f32,
    scroll: (i32, i32),
    composite: Image,
    stale: bool,
    drag: Drag,
    selected: Option<usize>,
    hidden: Option<usize>,
    text: Option<TextEdit>,
    bar: Bar,
    side: Bar,
    toolbar_h: i32,
    fonts: Fonts,
    dpi: i32,
    back: (HDC, HBITMAP, i32, i32),
    note: String,
}

pub fn register() {
    util::register_class(CLASS, Some(wndproc), null_mut(), true);
    util::register_class(CANVAS_CLASS, Some(canvas_proc), null_mut(), true);
}

pub fn open(path: Option<String>, img: Image) {
    let dpi = util::cursor_monitor_dpi();
    let wa = work_area();
    let ww = (img.w + util::sc(80, dpi)).clamp(util::sc(1120, dpi).min(wa.w * 9 / 10), (wa.w * 9 / 10).max(400));
    let wh = (img.h + util::sc(170, dpi)).clamp(util::sc(620, dpi).min(wa.h), (wa.h * 9 / 10).max(300));
    let x = wa.x + (wa.w - ww) / 2;
    let y = wa.y + (wa.h - wh) / 2;
    let class = w(CLASS);
    let title = w("MonPresso editor");
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
        let dpi = util::dpi_of(hwnd);
        // Reopen saved annotations as editable objects when MonPresso kept them.
        let (img, items) = match path.as_deref().and_then(|p| crate::project::load(std::path::Path::new(p))) {
            Some((base, text)) => (base, parse_annotations(&text)),
            None => (img, Vec::new()),
        };
        let composite = render(&img, &items, None, None);
        let ed = Box::into_raw(Box::new(Editor {
            hwnd,
            canvas: null_mut(),
            path,
            doc: Doc { base: Rc::new(img), items },
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
            tool: Tool::Arrow,
            color: SWATCHES[0],
            width: WIDTHS[1].0,
            font_size: 24.0,
            custom_colors: [0xFFFFFF; 16],
            zoom: 1.0,
            scroll: (0, 0),
            composite,
            stale: false,
            drag: Drag::None,
            selected: None,
            hidden: None,
            text: None,
            bar: Bar::new(hwnd, toolbar_items()),
            side: Bar::new_vertical(hwnd, side_items()),
            toolbar_h: 0,
            fonts: Fonts::new(dpi),
            dpi,
            back: (null_mut(), null_mut(), 0, 0),
            note: String::new(),
        }));
        util::set_state(hwnd, ed);
        ui::style_titlebar(hwnd);
        let e = &mut *ed;
        e.build();
        e.layout();
        e.zoom_fit();
        e.set_tool(Tool::Arrow);
        e.update_title();
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(e.canvas);
    }
}

fn work_area() -> Rect {
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(mon, &mut mi);
        Rect::from_win(&mi.rcWork)
    }
}

fn toolbar_items() -> Vec<Item> {
    let sep = || Item::new(0, Look::Sep, None, "", "");
    let mut v = Vec::new();
    for (i, t) in TOOLS.iter().enumerate() {
        v.push(Item::new(ID_TOOL + i as i32, Look::Icon, Some(t.4), "", &format!("{}  ({})", t.1, t.2 as char)));
    }
    v.push(sep());
    for (i, c) in SWATCHES.iter().enumerate() {
        v.push(Item::new(ID_SWATCH + i as i32, Look::Swatch(*c), None, "", "Color"));
    }
    v.push(Item::new(ID_MORE_COLORS, Look::Icon, Some(Icon::Plus), "", "More colors…"));
    v.push(sep());
    for (i, (wd, dot)) in WIDTHS.iter().enumerate() {
        v.push(Item::new(ID_WIDTH + i as i32, Look::Dot(*dot), None, "", &format!("Line width {wd}")));
    }
    v.push(sep());
    v.push(Item::new(ID_FONT_DOWN, Look::Icon, Some(Icon::FontDown), "", "Smaller text and steps"));
    v.push(Item::new(ID_FONT_LABEL, Look::Label, None, "24", ""));
    v.push(Item::new(ID_FONT_UP, Look::Icon, Some(Icon::FontUp), "", "Larger text and steps"));
    v.push(Item::new(0, Look::Spacer, None, "", ""));
    v.push(Item::new(ID_UNDO, Look::Icon, Some(Icon::Undo), "", "Undo  (Ctrl+Z)"));
    v.push(Item::new(ID_REDO, Look::Icon, Some(Icon::Redo), "", "Redo  (Ctrl+Y)"));
    v.push(sep());
    v.push(Item::new(ID_ZOOM_OUT, Look::Icon, Some(Icon::ZoomOut), "", "Zoom out  (Ctrl+-)"));
    v.push(Item::new(ID_ACTUAL, Look::Text, None, "100%", "Actual size  (Ctrl+1)"));
    v.push(Item::new(ID_ZOOM_IN, Look::Icon, Some(Icon::ZoomIn), "", "Zoom in  (Ctrl++)"));
    v.push(Item::new(ID_FIT, Look::Icon, Some(Icon::Fit), "", "Fit to window  (Ctrl+0)"));
    v
}

/// Output actions live in a panel on the right.
fn side_items() -> Vec<Item> {
    vec![
        Item::new(ID_SAVE, Look::Primary, Some(Icon::Save), "Save", "Save  (Ctrl+S)"),
        Item::new(ID_COPY, Look::Stacked, Some(Icon::Copy), "Copy", "Copy image  (Ctrl+C)"),
        Item::new(0, Look::Sep, None, "", ""),
        Item::new(ID_SAVE_AS, Look::Stacked, Some(Icon::Folder), "Save as", "Save as  (Ctrl+Shift+S)"),
        Item::new(ID_OCR, Look::Stacked, Some(Icon::Ocr), "Copy text", "Copy the text in this image (OCR)"),
    ]
}

fn side_width(dpi: i32) -> i32 {
    util::sc(100, dpi)
}

// ---- editable annotations ---------------------------------------------------

const TOOL_NAMES: [(Tool, &str); 10] = [
    (Tool::Arrow, "arrow"),
    (Tool::Line, "line"),
    (Tool::Rect, "rect"),
    (Tool::Ellipse, "ellipse"),
    (Tool::Pen, "pen"),
    (Tool::Marker, "highlight"),
    (Tool::Text, "text"),
    (Tool::Step, "step"),
    (Tool::Blur, "blur"),
    (Tool::Spot, "spotlight"),
];

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// One annotation per line, tab separated, so the files stay readable and diffable.
fn serialize_annotations(items: &[Ann]) -> String {
    let mut out = String::from("MonPresso annotations 1\n");
    for a in items {
        let Some((_, name)) = TOOL_NAMES.iter().find(|(t, _)| *t == a.tool) else { continue };
        let pts: Vec<String> = a.pts.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
        out.push_str(&format!(
            "{name}\t{}\t{}\t{}\t{}\t{:08X}\t{}\t{}\t{}\t{}\t{}\n",
            a.a.x,
            a.a.y,
            a.b.x,
            a.b.y,
            a.color,
            a.width,
            a.size,
            a.n,
            pts.join(";"),
            escape(&a.text)
        ));
    }
    out
}

fn parse_annotations(text: &str) -> Vec<Ann> {
    let mut lines = text.lines();
    if !lines.next().is_some_and(|h| h.starts_with("MonPresso annotations")) {
        return Vec::new();
    }
    let num = |s: &str| s.trim().parse::<f32>().ok();
    lines
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 11 {
                return None;
            }
            let tool = TOOL_NAMES.iter().find(|(_, n)| *n == f[0])?.0;
            let pts = f[9]
                .split(';')
                .filter_map(|p| {
                    let (x, y) = p.split_once(',')?;
                    Some(P { x: num(x)?, y: num(y)? })
                })
                .collect();
            Some(Ann {
                tool,
                a: P { x: num(f[1])?, y: num(f[2])? },
                b: P { x: num(f[3])?, y: num(f[4])? },
                pts,
                text: unescape(f[10]),
                n: f[8].trim().parse().ok()?,
                color: u32::from_str_radix(f[5].trim(), 16).ok()?,
                width: num(f[6])?,
                size: num(f[7])?,
            })
        })
        .collect()
}

impl Editor {
    fn sc(&self, v: i32) -> i32 {
        util::sc(v, self.dpi)
    }

    unsafe fn build(&mut self) {
        let cc = w(CANVAS_CLASS);
        self.canvas = CreateWindowExW(
            0,
            cc.as_ptr(),
            null(),
            WS_CHILD | WS_VISIBLE | WS_HSCROLL | WS_VSCROLL | WS_CLIPCHILDREN,
            0,
            0,
            10,
            10,
            self.hwnd,
            null_mut(),
            util::hinstance(),
            null(),
        );
        util::set_state(self.canvas, self as *mut Editor);
    }

    unsafe fn layout(&mut self) {
        let (cw, ch) = util::client_size(self.hwnd);
        self.toolbar_h = self.bar.layout(self.hwnd, cw, self.dpi, &self.fonts);
        let status_h = self.sc(28);
        let side_w = side_width(self.dpi);
        let body_h = (ch - self.toolbar_h - status_h).max(1);
        self.side.layout_vertical(self.hwnd, cw - side_w, self.toolbar_h, side_w, body_h, self.dpi);
        util::move_win(self.canvas, 0, self.toolbar_h, (cw - side_w).max(1), body_h);
        self.update_scrollbars();
        InvalidateRect(self.hwnd, null(), 0);
    }

    fn canvas_size(&self) -> (i32, i32) {
        util::client_size(self.canvas)
    }

    fn img(&self) -> &Image {
        &self.doc.base
    }

    fn view(&self) -> (i32, i32, i32, i32) {
        let (cw, ch) = self.canvas_size();
        let dw = ((self.img().w as f32 * self.zoom).round() as i32).max(1);
        let dh = ((self.img().h as f32 * self.zoom).round() as i32).max(1);
        let ox = if dw <= cw { (cw - dw) / 2 } else { -self.scroll.0 };
        let oy = if dh <= ch { (ch - dh) / 2 } else { -self.scroll.1 };
        (ox, oy, dw, dh)
    }

    fn to_image(&self, mx: i32, my: i32) -> P {
        let (ox, oy, _, _) = self.view();
        P { x: (mx - ox) as f32 / self.zoom, y: (my - oy) as f32 / self.zoom }
    }

    fn to_screen(&self, p: P) -> (i32, i32) {
        let (ox, oy, _, _) = self.view();
        (ox + (p.x * self.zoom).round() as i32, oy + (p.y * self.zoom).round() as i32)
    }

    unsafe fn update_scrollbars(&mut self) {
        let (cw, ch) = self.canvas_size();
        let dw = (self.img().w as f32 * self.zoom).round() as i32;
        let dh = (self.img().h as f32 * self.zoom).round() as i32;
        self.scroll.0 = self.scroll.0.clamp(0, (dw - cw).max(0));
        self.scroll.1 = self.scroll.1.clamp(0, (dh - ch).max(0));
        for (bar, total, page, pos) in [(SB_HORZ, dw, cw, self.scroll.0), (SB_VERT, dh, ch, self.scroll.1)] {
            let si = SCROLLINFO {
                cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
                nMin: 0,
                nMax: (total - 1).max(0),
                nPage: page.max(0) as u32,
                nPos: pos,
                nTrackPos: 0,
            };
            SetScrollInfo(self.canvas, bar, &si, 1);
        }
    }

    unsafe fn refresh(&mut self) {
        InvalidateRect(self.canvas, null(), 0);
    }

    unsafe fn changed(&mut self) {
        self.stale = true;
        self.refresh();
        self.update_title();
        self.update_status();
    }

    unsafe fn update_title(&mut self) {
        let name = self
            .path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        util::set_text(self.hwnd, &format!("{}{} - MonPresso", if self.dirty { "● " } else { "" }, name));
        self.sync_bar();
    }

    unsafe fn update_status(&self) {
        InvalidateRect(self.hwnd, null(), 0);
    }

    unsafe fn set_note(&mut self, note: &str) {
        self.note = note.to_string();
        self.update_status();
    }

    unsafe fn set_tool(&mut self, tool: Tool) {
        self.commit_text();
        self.tool = tool;
        if tool != Tool::Select {
            self.selected = None;
        }
        self.note.clear();
        self.sync_bar();
        self.refresh();
    }

    unsafe fn set_zoom(&mut self, z: f32) {
        self.commit_text();
        self.zoom = z.clamp(0.05, 16.0);
        self.update_scrollbars();
        self.sync_bar();
        self.refresh();
    }

    unsafe fn zoom_fit(&mut self) {
        let (cw, ch) = self.canvas_size();
        let m = self.sc(40) as f32;
        let z = ((cw as f32 - m) / self.img().w as f32).min((ch as f32 - m) / self.img().h as f32).min(1.0);
        self.scroll = (0, 0);
        self.set_zoom(if z.is_finite() && z > 0.0 { z } else { 1.0 });
    }

    unsafe fn zoom_at(&mut self, z: f32, mx: i32, my: i32) {
        self.commit_text();
        let p = self.to_image(mx, my);
        self.zoom = z.clamp(0.05, 16.0);
        self.scroll = ((p.x * self.zoom) as i32 - mx, (p.y * self.zoom) as i32 - my);
        self.update_scrollbars();
        self.sync_bar();
        self.refresh();
    }

    unsafe fn paint_chrome(&mut self) {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(self.hwnd, &mut ps);
        let (cw, ch) = util::client_size(self.hwnd);
        let bar = Surface::new(cw, self.toolbar_h);
        self.bar.paint(&bar, self.dpi, &self.fonts);
        bar.present(hdc, 0, 0);
        let (sx, sy) = self.side.origin();
        let side = Surface::new(cw - sx, self.side.height);
        self.side.paint(&side, self.dpi, &self.fonts);
        side.present(hdc, sx, sy);

        let sh = self.sc(28);
        let st = Surface::new(cw, sh);
        {
            let c = st.canvas();
            c.fill_rect(RectF { x: 0.0, y: 0.0, w: cw as f32, h: sh as f32 }, theme::PANEL);
            c.fill_rect(RectF { x: 0.0, y: 0.0, w: cw as f32, h: 1.0 }, theme::BORDER);
        }
        let pad = self.sc(14);
        let dims = format!("{} × {} px", self.img().w, self.img().h);
        let dims_w = ui::text_extent(self.fonts.bold, &dims);
        let flags = DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_END_ELLIPSIS;
        st.text(self.fonts.bold, &dims, RECT { left: pad, top: 0, right: pad + dims_w + self.sc(10), bottom: sh }, theme::TEXT, flags);
        let hint = TOOLS.iter().find(|t| t.0 == self.tool).map(|t| t.3).unwrap_or("");
        let text = if self.note.is_empty() { hint.to_string() } else { self.note.clone() };
        let color = if self.note.is_empty() { theme::MUTED } else { theme::ACCENT };
        st.text(self.fonts.normal, &text, RECT { left: pad + dims_w + self.sc(24), top: 0, right: cw - pad, bottom: sh }, color, flags);
        st.present(hdc, 0, ch - sh);
        EndPaint(self.hwnd, &ps);
    }

    fn push_undo(&mut self) {
        self.undo.push(self.doc.clone());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    unsafe fn undo(&mut self) {
        self.commit_text();
        if let Some(d) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.doc, d));
            self.after_history();
        }
    }

    unsafe fn redo(&mut self) {
        self.commit_text();
        if let Some(d) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.doc, d));
            self.after_history();
        }
    }

    unsafe fn after_history(&mut self) {
        self.selected = None;
        self.dirty = true;
        self.update_scrollbars();
        self.changed();
    }

    fn export(&self) -> Image {
        render(&self.doc.base, &self.doc.items, None, None)
    }

    unsafe fn crop(&mut self, r: Rect) {
        self.push_undo();
        let img = self.doc.base.crop(r);
        self.doc.base = Rc::new(img);
        for a in &mut self.doc.items {
            a.translate(-r.x as f32, -r.y as f32);
        }
        self.selected = None;
        self.zoom_fit();
        self.changed();
    }

    fn hit_test(&self, p: P) -> Option<usize> {
        let tol = 4.0 / self.zoom;
        (0..self.doc.items.len()).rev().find(|&i| self.doc.items[i].hit(p, tol))
    }

    unsafe fn delete_selected(&mut self) {
        if let Some(i) = self.selected.take() {
            if i < self.doc.items.len() {
                self.push_undo();
                self.doc.items.remove(i);
                self.changed();
            }
        }
    }

    fn new_ann(&self, tool: Tool, p: P) -> Ann {
        Ann {
            tool,
            a: p,
            b: p,
            pts: Vec::new(),
            text: String::new(),
            n: 0,
            color: self.color,
            width: self.width,
            size: self.font_size,
        }
    }

    // ---- mouse -------------------------------------------------------------

    unsafe fn mouse_down(&mut self, mx: i32, my: i32, double: bool) {
        if self.text.is_some() {
            self.commit_text();
            return;
        }
        SetFocus(self.canvas);
        let p = self.to_image(mx, my);
        match self.tool {
            Tool::Select => {
                self.selected = self.hit_test(p);
                if let Some(i) = self.selected {
                    if double && self.doc.items[i].tool == Tool::Text {
                        let a = self.doc.items[i].clone();
                        self.begin_text(a.a, &a.text, Some(i), a.color, a.size);
                        return;
                    }
                    let was_dirty = self.dirty;
                    self.push_undo();
                    self.drag = Drag::Move { last: p, moved: false, was_dirty };
                    SetCapture(self.canvas);
                }
                self.refresh();
            }
            Tool::Text => {
                if let Some(i) = self.hit_test(p).filter(|&i| self.doc.items[i].tool == Tool::Text) {
                    let a = self.doc.items[i].clone();
                    self.begin_text(a.a, &a.text, Some(i), a.color, a.size);
                } else {
                    self.begin_text(p, "", None, self.color, self.font_size);
                }
            }
            Tool::Step => {
                let n = self.doc.items.iter().filter(|a| a.tool == Tool::Step).map(|a| a.n).max().unwrap_or(0) + 1;
                let mut a = self.new_ann(Tool::Step, p);
                a.n = n;
                self.push_undo();
                self.doc.items.push(a);
                self.changed();
            }
            Tool::Crop => {
                self.drag = Drag::Crop { a: p, b: p };
                SetCapture(self.canvas);
            }
            t => {
                let mut a = self.new_ann(t, p);
                if t == Tool::Pen {
                    a.pts.push(p);
                }
                self.drag = Drag::Draw(a);
                SetCapture(self.canvas);
            }
        }
    }

    unsafe fn mouse_move(&mut self, mx: i32, my: i32) {
        let p = self.to_image(mx, my);
        let shift = GetKeyState(VK_SHIFT as i32) < 0;
        match &mut self.drag {
            Drag::None => return,
            Drag::Draw(a) => {
                match a.tool {
                    Tool::Pen => {
                        if a.pts.last().map_or(true, |l| (l.x - p.x).hypot(l.y - p.y) >= 0.75) {
                            a.pts.push(p);
                        }
                    }
                    Tool::Arrow | Tool::Line => a.b = if shift { snap45(a.a, p) } else { p },
                    Tool::Rect | Tool::Ellipse => a.b = if shift { square(a.a, p) } else { p },
                    _ => a.b = p,
                }
                self.stale = true;
            }
            Drag::Move { last, moved, .. } => {
                let (dx, dy) = (p.x - last.x, p.y - last.y);
                if let Some(i) = self.selected {
                    self.doc.items[i].translate(dx, dy);
                }
                *last = p;
                *moved = true;
                self.stale = true;
            }
            Drag::Crop { b, .. } => *b = p,
        }
        self.refresh();
    }

    unsafe fn mouse_up(&mut self) {
        let drag = std::mem::replace(&mut self.drag, Drag::None);
        ReleaseCapture();
        match drag {
            Drag::None => return,
            Drag::Draw(a) => {
                if a.is_meaningful() {
                    self.push_undo();
                    self.doc.items.push(a);
                }
            }
            Drag::Move { moved, was_dirty, .. } => {
                if !moved {
                    self.undo.pop();
                    self.dirty = was_dirty;
                }
            }
            Drag::Crop { a, b } => {
                let x1 = a.x.min(b.x).max(0.0).floor() as i32;
                let y1 = a.y.min(b.y).max(0.0).floor() as i32;
                let x2 = (a.x.max(b.x).ceil() as i32).min(self.img().w);
                let y2 = (a.y.max(b.y).ceil() as i32).min(self.img().h);
                if x2 - x1 >= 4 && y2 - y1 >= 4 {
                    self.crop(Rect::new(x1, y1, x2 - x1, y2 - y1));
                }
            }
        }
        self.changed();
    }

    // ---- text --------------------------------------------------------------

    unsafe fn begin_text(&mut self, at: P, initial: &str, replacing: Option<usize>, color: u32, size: f32) {
        self.hidden = replacing;
        self.selected = None;
        self.stale = true;
        let (sx, sy) = self.to_screen(at);
        let font = util::create_font((size * self.zoom).round().max(6.0) as i32, true, "Segoe UI");
        let class = w("EDIT");
        let text = w(&initial.replace('\n', "\r\n"));
        let edit = CreateWindowExW(
            0,
            class.as_ptr(),
            text.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_BORDER | (ES_MULTILINE | ES_AUTOHSCROLL | ES_AUTOVSCROLL | ES_WANTRETURN) as u32,
            sx - 2,
            sy - 2,
            50,
            30,
            self.canvas,
            ID_TEXT_EDIT as usize as HMENU,
            util::hinstance(),
            null(),
        );
        SendMessageW(edit, WM_SETFONT, font as usize, 1);
        SendMessageW(edit, EM_SETMARGINS, (EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize, 0);
        SetWindowSubclass(edit, Some(edit_subclass), 1, self.canvas as usize);
        self.text = Some(TextEdit { hwnd: edit, font, at, replacing, color, size });
        self.fit_text_edit();
        SetFocus(edit);
        let len = initial.encode_utf16().count() + initial.matches('\n').count();
        SendMessageW(edit, EM_SETSEL, len, len as isize);
        self.refresh();
    }

    unsafe fn fit_text_edit(&self) {
        let Some(te) = &self.text else { return };
        let text = util::get_text(te.hwnd);
        let sample: Vec<u16> = format!("{text}W").encode_utf16().collect();
        let dc = GetDC(te.hwnd);
        let old = SelectObject(dc, te.font as HGDIOBJ);
        let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        DrawTextW(dc, sample.as_ptr(), sample.len() as i32, &mut rc, DT_CALCRECT | DT_NOPREFIX);
        SelectObject(dc, old);
        ReleaseDC(te.hwnd, dc);
        let wdt = (rc.right + self.sc(12)).max(self.sc(60));
        SetWindowPos(te.hwnd, null_mut(), 0, 0, wdt, rc.bottom + self.sc(8), SWP_NOMOVE | SWP_NOZORDER);
    }

    unsafe fn commit_text(&mut self) {
        let Some(te) = self.text.take() else { return };
        let text = util::get_text(te.hwnd).replace("\r\n", "\n").trim_end().to_string();
        DestroyWindow(te.hwnd);
        DeleteObject(te.font as HGDIOBJ);
        self.hidden = None;
        match te.replacing {
            Some(i) if i < self.doc.items.len() => {
                if text.is_empty() {
                    self.push_undo();
                    self.doc.items.remove(i);
                } else if text != self.doc.items[i].text {
                    self.push_undo();
                    self.doc.items[i].text = text;
                }
            }
            Some(_) => {}
            None => {
                if !text.is_empty() {
                    let mut a = self.new_ann(Tool::Text, te.at);
                    a.text = text;
                    a.color = te.color;
                    a.size = te.size;
                    self.push_undo();
                    self.doc.items.push(a);
                }
            }
        }
        SetFocus(self.canvas);
        self.changed();
    }

    // ---- painting ----------------------------------------------------------

    unsafe fn paint(&mut self) {
        if self.stale {
            let preview = match &self.drag {
                Drag::Draw(a) => Some(a),
                _ => None,
            };
            self.composite = render(&self.doc.base, &self.doc.items, self.hidden, preview);
            self.stale = false;
        }
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(self.canvas, &mut ps);
        let (cw, ch) = self.canvas_size();
        if self.back.2 != cw || self.back.3 != ch || self.back.0.is_null() {
            if !self.back.0.is_null() {
                DeleteDC(self.back.0);
                DeleteObject(self.back.1 as HGDIOBJ);
            }
            let dc = CreateCompatibleDC(hdc);
            let bmp = CreateCompatibleBitmap(hdc, cw.max(1), ch.max(1));
            SelectObject(dc, bmp as HGDIOBJ);
            self.back = (dc, bmp, cw, ch);
        }
        let dc = self.back.0;
        let bg = CreateSolidBrush(ui::cref(theme::CANVAS));
        FillRect(dc, &RECT { left: 0, top: 0, right: cw, bottom: ch }, bg);
        DeleteObject(bg as HGDIOBJ);

        let (ox, oy, dw, dh) = self.view();
        // Soft drop shadow and hairline border around the page.
        for (i, shade) in [0xDCDEE2u32, 0xD5D7DC, 0xCDD0D5].iter().enumerate() {
            let d = 3 - i as i32;
            let b = CreateSolidBrush(*shade);
            FillRect(dc, &RECT { left: ox - d + 1, top: oy - d + 2, right: ox + dw + d - 1, bottom: oy + dh + d + 1 }, b);
            DeleteObject(b as HGDIOBJ);
        }
        let img = &self.composite;
        SetStretchBltMode(dc, if self.zoom < 1.0 { HALFTONE } else { COLORONCOLOR });
        SetBrushOrgEx(dc, 0, 0, null_mut());
        let bmi = img.bitmap_info();
        StretchDIBits(dc, ox, oy, dw, dh, 0, 0, img.w, img.h, img.px.as_ptr() as *const _, &bmi, DIB_RGB_COLORS, SRCCOPY);

        let outline = |r: RectF, style: i32, width: i32| {
            let (x1, y1) = self.to_screen(P { x: r.x, y: r.y });
            let (x2, y2) = self.to_screen(P { x: r.x + r.w, y: r.y + r.h });
            let pen = CreatePen(style, width, util::ACCENT);
            let op = SelectObject(dc, pen as HGDIOBJ);
            let ob = SelectObject(dc, GetStockObject(NULL_BRUSH));
            SetBkMode(dc, TRANSPARENT as _);
            Rectangle(dc, x1, y1, x2 + 1, y2 + 1);
            SelectObject(dc, ob);
            SelectObject(dc, op);
            DeleteObject(pen as HGDIOBJ);
        };
        if let Some(i) = self.selected.filter(|&i| i < self.doc.items.len()) {
            outline(self.doc.items[i].bounds(), PS_DOT, 1);
        }
        if let Drag::Crop { a, b } = &self.drag {
            let r = Ann { a: *a, b: *b, ..self.new_ann(Tool::Crop, *a) }.rect();
            outline(r, PS_SOLID, self.sc(2));
        }
        BitBlt(hdc, 0, 0, cw, ch, dc, 0, 0, SRCCOPY);
        EndPaint(self.canvas, &ps);
    }

    // ---- commands ----------------------------------------------------------

    unsafe fn save(&mut self, save_as: bool) -> bool {
        self.commit_text();
        let mut path = self.path.clone();
        if save_as || path.is_none() {
            match save_dialog(self.hwnd, path.as_deref()) {
                Some(p) => path = Some(p),
                None => return false,
            }
        }
        let path = path.unwrap();
        if !gdip::save_image(&self.export(), &path) {
            util::msgbox(self.hwnd, &format!("Could not save {path}"), MB_ICONERROR);
            return false;
        }
        // Keep the original capture and the annotation list so they stay editable.
        let image_path = std::path::Path::new(&path);
        if self.doc.items.is_empty() {
            crate::project::remove(image_path);
        } else if !crate::project::save(image_path, &self.doc.base, &serialize_annotations(&self.doc.items)) {
            util::msgbox(self.hwnd, "The image was saved, but its annotations could not be kept editable.", MB_ICONWARNING);
        }
        self.path = Some(path.clone());
        self.dirty = false;
        self.update_title();
        let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.set_note(&format!("Saved {name}"));
        crate::workspace::refresh();
        true
    }

    unsafe fn copy(&mut self) {
        self.commit_text();
        if crate::clipboard::set_image(self.hwnd, &self.export()) {
            self.set_note("Copied to the clipboard");
        }
    }

    unsafe fn ocr(&mut self) {
        self.commit_text();
        self.set_note("Reading text…");
        SetCursor(LoadCursorW(null_mut(), IDC_WAIT));
        let result = crate::ocr::recognize(&self.export());
        self.note.clear();
        self.update_status();
        crate::app::show_ocr_result(self.hwnd, result);
    }

    unsafe fn pick_color(&mut self) {
        let mut cc: CHOOSECOLORW = std::mem::zeroed();
        cc.lStructSize = std::mem::size_of::<CHOOSECOLORW>() as u32;
        cc.hwndOwner = self.hwnd;
        cc.rgbResult = colorref(self.color);
        cc.lpCustColors = self.custom_colors.as_mut_ptr();
        cc.Flags = CC_FULLOPEN | CC_RGBINIT;
        if ChooseColorW(&mut cc) != 0 {
            self.set_color(0xFF00_0000 | colorref(cc.rgbResult));
        }
    }

    unsafe fn set_color(&mut self, color: u32) {
        self.color = color;
        if let Some(i) = self.selected {
            self.push_undo();
            self.doc.items[i].color = color;
            self.changed();
        }
        self.sync_bar();
    }

    unsafe fn set_width(&mut self, width: f32) {
        self.width = width;
        if let Some(i) = self.selected {
            self.push_undo();
            self.doc.items[i].width = width;
            self.changed();
        }
        self.sync_bar();
    }

    unsafe fn step_font(&mut self, dir: i32) {
        let cur = FONT_SIZES.iter().position(|f| *f >= self.font_size).unwrap_or(FONT_SIZES.len() - 1) as i32;
        let next = (cur + dir).clamp(0, FONT_SIZES.len() as i32 - 1) as usize;
        self.font_size = FONT_SIZES[next];
        if let Some(i) = self.selected.filter(|&i| matches!(self.doc.items[i].tool, Tool::Text | Tool::Step)) {
            self.push_undo();
            self.doc.items[i].size = self.font_size;
            self.changed();
        }
        self.sync_bar();
    }

    /// Reflect the current state in the toolbar and repaint it.
    unsafe fn sync_bar(&mut self) {
        for (i, t) in TOOLS.iter().enumerate() {
            self.bar.set_checked(ID_TOOL + i as i32, t.0 == self.tool);
        }
        for (i, c) in SWATCHES.iter().enumerate() {
            self.bar.set_checked(ID_SWATCH + i as i32, *c == self.color);
        }
        self.bar.set_checked(ID_MORE_COLORS, !SWATCHES.contains(&self.color));
        for (i, (wd, _)) in WIDTHS.iter().enumerate() {
            self.bar.set_checked(ID_WIDTH + i as i32, *wd == self.width);
        }
        self.bar.set_label(ID_FONT_LABEL, &format!("{}", self.font_size));
        self.bar.set_label(ID_ACTUAL, &format!("{:.0}%", self.zoom * 100.0));
        self.bar.set_enabled(ID_UNDO, !self.undo.is_empty());
        self.bar.set_enabled(ID_REDO, !self.redo.is_empty());
        InvalidateRect(self.hwnd, null(), 0);
    }

    unsafe fn command(&mut self, id: i32) {
        match id {
            x if (ID_TOOL..ID_TOOL + TOOLS.len() as i32).contains(&x) => self.set_tool(TOOLS[(x - ID_TOOL) as usize].0),
            x if (ID_SWATCH..ID_SWATCH + SWATCHES.len() as i32).contains(&x) => self.set_color(SWATCHES[(x - ID_SWATCH) as usize]),
            x if (ID_WIDTH..ID_WIDTH + WIDTHS.len() as i32).contains(&x) => self.set_width(WIDTHS[(x - ID_WIDTH) as usize].0),
            ID_MORE_COLORS => self.pick_color(),
            ID_FONT_DOWN => self.step_font(-1),
            ID_FONT_UP => self.step_font(1),
            ID_UNDO => self.undo(),
            ID_REDO => self.redo(),
            ID_FIT => self.zoom_fit(),
            ID_ACTUAL => self.set_zoom(1.0),
            ID_ZOOM_IN => self.set_zoom(self.zoom * 1.25),
            ID_ZOOM_OUT => self.set_zoom(self.zoom / 1.25),
            ID_COPY => self.copy(),
            ID_OCR => self.ocr(),
            ID_SAVE_AS => {
                self.save(true);
            }
            ID_SAVE => {
                self.save(false);
            }
            _ => return,
        }
        if self.text.is_none() {
            SetFocus(self.canvas);
        }
    }

    /// Returns true when the key was used.
    unsafe fn key(&mut self, vk: u16) -> bool {
        let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
        let shift = GetKeyState(VK_SHIFT as i32) < 0;
        let alt = GetKeyState(VK_MENU as i32) < 0;
        if alt {
            return false;
        }
        if ctrl {
            match vk {
                k if k == b'Z' as u16 && shift => self.redo(),
                k if k == b'Z' as u16 => self.undo(),
                k if k == b'Y' as u16 => self.redo(),
                k if k == b'C' as u16 => self.copy(),
                k if k == b'S' as u16 => {
                    self.save(shift);
                }
                k if k == b'0' as u16 => self.zoom_fit(),
                k if k == b'1' as u16 => self.set_zoom(1.0),
                VK_OEM_PLUS | VK_ADD => self.set_zoom(self.zoom * 1.25),
                VK_OEM_MINUS | VK_SUBTRACT => self.set_zoom(self.zoom / 1.25),
                _ => return false,
            }
            return true;
        }
        if shift {
            return false;
        }
        if let Some(t) = TOOLS.iter().find(|t| t.2 as u16 == vk) {
            self.set_tool(t.0);
            return true;
        }
        match vk {
            VK_DELETE | VK_BACK => self.delete_selected(),
            VK_ESCAPE => {
                if !matches!(self.drag, Drag::None) {
                    ReleaseCapture();
                    self.drag = Drag::None;
                }
                self.selected = None;
                self.changed();
            }
            _ => return false,
        }
        true
    }

    unsafe fn scroll_by(&mut self, dx: i32, dy: i32) {
        self.commit_text();
        self.scroll.0 += dx;
        self.scroll.1 += dy;
        self.update_scrollbars();
        self.refresh();
    }

    unsafe fn on_scroll(&mut self, bar: i32, code: u32) {
        let (cw, ch) = self.canvas_size();
        let page = if bar == SB_HORZ { cw } else { ch };
        let mut si = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_TRACKPOS,
            nMin: 0,
            nMax: 0,
            nPage: 0,
            nPos: 0,
            nTrackPos: 0,
        };
        GetScrollInfo(self.canvas, bar, &mut si);
        let cur = if bar == SB_HORZ { self.scroll.0 } else { self.scroll.1 };
        let new = match code as i32 {
            SB_LINEUP => cur - 40,
            SB_LINEDOWN => cur + 40,
            SB_PAGEUP => cur - page,
            SB_PAGEDOWN => cur + page,
            SB_THUMBTRACK | SB_THUMBPOSITION => si.nTrackPos,
            _ => cur,
        };
        if bar == SB_HORZ {
            self.scroll_by(new - cur, 0);
        } else {
            self.scroll_by(0, new - cur);
        }
    }

    unsafe fn dpi_changed(&mut self, dpi: i32) {
        self.dpi = dpi;
        self.fonts = Fonts::new(dpi);
        self.layout();
    }

    unsafe fn close(&mut self) {
        self.commit_text();
        if self.dirty {
            let name = self.path.as_deref().unwrap_or("this capture");
            match util::msgbox(self.hwnd, &format!("Save changes to {name}?"), MB_YESNOCANCEL | MB_ICONQUESTION) {
                IDYES => {
                    if !self.save(false) {
                        return;
                    }
                }
                IDNO => {}
                _ => return,
            }
        }
        DestroyWindow(self.hwnd);
    }
}


unsafe fn save_dialog(owner: HWND, current: Option<&str>) -> Option<String> {
    let mut buf = [0u16; 1024];
    let (dir, name) = match current {
        Some(p) => {
            let path = std::path::Path::new(p);
            (
                path.parent().map(|d| d.to_string_lossy().into_owned()),
                path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            )
        }
        None => (Some(crate::app::settings().save_folder), format!("MonPresso_{}", util::timestamp())),
    };
    for (i, c) in name.encode_utf16().take(1000).enumerate() {
        buf[i] = c;
    }
    let filter: Vec<u16> = "PNG image (*.png)\0*.png\0JPEG image (*.jpg)\0*.jpg\0Bitmap (*.bmp)\0*.bmp\0\0".encode_utf16().collect();
    let def = w("png");
    let dir_w = dir.map(|d| w(&d));
    let mut ofn: OPENFILENAMEW = std::mem::zeroed();
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = owner;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = buf.as_mut_ptr();
    ofn.nMaxFile = buf.len() as u32;
    ofn.lpstrDefExt = def.as_ptr();
    ofn.lpstrInitialDir = dir_w.as_ref().map_or(null(), |d| d.as_ptr());
    ofn.Flags = OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST;
    (GetSaveFileNameW(&mut ofn) != 0).then(|| util::from_w(&buf))
}

unsafe extern "system" fn edit_subclass(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, canvas: usize) -> LRESULT {
    match msg {
        WM_KEYDOWN => {
            let vk = wp as u16;
            if vk == VK_ESCAPE || (vk == VK_RETURN && GetKeyState(VK_CONTROL as i32) < 0) {
                PostMessageW(canvas as HWND, WM_COMMIT_TEXT, 0, 0);
                return 0;
            }
        }
        WM_CHAR if wp == 27 || wp == 10 => return 0,
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let Some(e) = util::state::<Editor>(hwnd) else {
        return DefWindowProcW(hwnd, msg, wp, lp);
    };
    match msg {
        WM_SIZE => {
            e.layout();
            e.refresh();
            return 0;
        }
        WM_GETMINMAXINFO => {
            let mmi = &mut *(lp as *mut MINMAXINFO);
            mmi.ptMinTrackSize = POINT { x: e.sc(520), y: e.sc(360) };
            return 0;
        }
        WM_PAINT => {
            e.paint_chrome();
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_MOUSEMOVE => {
            let (x, y) = util::point_from_lparam(lp);
            let (ox, oy) = e.side.origin();
            let a = e.bar.mouse_move(hwnd, x, y);
            let b = e.side.mouse_move(hwnd, x - ox, y - oy);
            if a || b {
                InvalidateRect(hwnd, null(), 0);
            }
            return 0;
        }
        0x02A3 => {
            // WM_MOUSELEAVE
            let a = e.bar.mouse_leave();
            let b = e.side.mouse_leave();
            if a || b {
                InvalidateRect(hwnd, null(), 0);
            }
            return 0;
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
            let (x, y) = util::point_from_lparam(lp);
            let (ox, oy) = e.side.origin();
            if e.bar.mouse_down(x, y) || e.side.mouse_down(x - ox, y - oy) {
                SetCapture(hwnd);
                InvalidateRect(hwnd, null(), 0);
            }
            return 0;
        }
        WM_LBUTTONUP => {
            let (x, y) = util::point_from_lparam(lp);
            let (ox, oy) = e.side.origin();
            ReleaseCapture();
            let clicked = e.bar.mouse_up(x, y).or(e.side.mouse_up(x - ox, y - oy));
            InvalidateRect(hwnd, null(), 0);
            if let Some(id) = clicked {
                e.command(id);
            }
            return 0;
        }
        WM_KEYDOWN => {
            if e.key(wp as u16) {
                return 0;
            }
        }
        WM_ACTIVATE => {
            if util::loword(wp) != 0 && e.text.is_none() {
                SetFocus(e.canvas);
            }
        }
        WM_DPICHANGED => {
            let r = &*(lp as *const RECT);
            SetWindowPos(hwnd, null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            e.dpi_changed(util::hiword(wp) as i32);
            return 0;
        }
        WM_CLOSE => {
            e.close();
            return 0;
        }
        WM_DESTROY => {
            util::set_state::<Editor>(e.canvas, null_mut());
        }
        WM_NCDESTROY => {
            util::set_state::<Editor>(hwnd, null_mut());
            if !e.back.0.is_null() {
                DeleteDC(e.back.0);
                DeleteObject(e.back.1 as HGDIOBJ);
            }
            drop(Box::from_raw(e as *mut Editor));
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

unsafe extern "system" fn canvas_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let Some(e) = util::state::<Editor>(hwnd) else {
        return DefWindowProcW(hwnd, msg, wp, lp);
    };
    match msg {
        WM_PAINT => {
            e.paint();
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_SIZE => {
            e.update_scrollbars();
            e.refresh();
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
            let (x, y) = util::point_from_lparam(lp);
            e.mouse_down(x, y, msg == WM_LBUTTONDBLCLK);
            return 0;
        }
        WM_MOUSEMOVE => {
            let (x, y) = util::point_from_lparam(lp);
            e.mouse_move(x, y);
            return 0;
        }
        WM_LBUTTONUP => {
            e.mouse_up();
            return 0;
        }
        WM_CAPTURECHANGED => {
            if !matches!(e.drag, Drag::None) && lp as HWND != hwnd {
                e.mouse_up();
            }
        }
        WM_MOUSEWHEEL => {
            let delta = (util::hiword(wp) as i16) as i32;
            let mut pt = POINT { x: (lp & 0xFFFF) as i16 as i32, y: ((lp >> 16) & 0xFFFF) as i16 as i32 };
            ScreenToClient(hwnd, &mut pt);
            if GetKeyState(VK_CONTROL as i32) < 0 {
                let z = e.zoom * 1.15f32.powf(delta as f32 / 120.0);
                e.zoom_at(z, pt.x, pt.y);
            } else if GetKeyState(VK_SHIFT as i32) < 0 {
                e.scroll_by(-delta, 0);
            } else {
                e.scroll_by(0, -delta);
            }
            return 0;
        }
        WM_MOUSEHWHEEL => {
            let delta = (util::hiword(wp) as i16) as i32;
            e.scroll_by(delta, 0);
            return 0;
        }
        WM_HSCROLL => {
            e.on_scroll(SB_HORZ, util::loword(wp));
            return 0;
        }
        WM_VSCROLL => {
            e.on_scroll(SB_VERT, util::loword(wp));
            return 0;
        }
        WM_KEYDOWN => {
            if e.key(wp as u16) {
                return 0;
            }
        }
        WM_SETCURSOR => {
            if util::loword(lp as usize) == HTCLIENT {
                let cur = match e.tool {
                    Tool::Select => IDC_ARROW,
                    Tool::Text => IDC_IBEAM,
                    _ => IDC_CROSS,
                };
                SetCursor(LoadCursorW(null_mut(), cur));
                return 1;
            }
        }
        WM_COMMAND => {
            if util::loword(wp) as i32 == ID_TEXT_EDIT {
                match util::hiword(wp) {
                    EN_CHANGE => e.fit_text_edit(),
                    EN_KILLFOCUS => {
                        PostMessageW(hwnd, WM_COMMIT_TEXT, 0, 0);
                    }
                    _ => {}
                }
            }
            return 0;
        }
        WM_COMMIT_TEXT => {
            e.commit_text();
            return 0;
        }
        WM_CTLCOLOREDIT => {
            if let Some(te) = &e.text {
                SetTextColor(wp as HDC, colorref(te.color));
                SetBkColor(wp as HDC, if luminance(te.color) > 0.8 { 0x505050 } else { 0xFFFFFF });
                return GetStockObject(if luminance(te.color) > 0.8 { DKGRAY_BRUSH } else { WHITE_BRUSH }) as LRESULT;
            }
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ann(tool: Tool, a: P, b: P) -> Ann {
        Ann { tool, a, b, pts: Vec::new(), text: String::new(), n: 1, color: 0xFFE53935, width: 4.0, size: 24.0 }
    }

    fn checker(w: i32, h: i32) -> Image {
        let mut img = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.px[(y * w + x) as usize] = if (x / 2 + y / 2) % 2 == 0 { 0xFFFFFFFF } else { 0xFF000000 };
            }
        }
        img
    }

    #[test]
    fn annotations_change_pixels() {
        gdip::startup();
        let base = checker(200, 120);
        let p = |x, y| P { x, y };
        let mut text = ann(Tool::Text, p(10.0, 60.0), p(10.0, 60.0));
        text.text = "Hi".into();
        let items = vec![
            ann(Tool::Arrow, p(10.0, 10.0), p(190.0, 10.0)),
            ann(Tool::Blur, p(100.0, 40.0), p(160.0, 100.0)),
            ann(Tool::Marker, p(0.0, 100.0), p(60.0, 119.0)),
            ann(Tool::Step, p(180.0, 100.0), p(180.0, 100.0)),
            text,
        ];
        let out = render(&base, &items, None, None);
        // Arrow drawn in red straight into the buffer.
        let px = out.pixel(100, 10);
        assert_eq!(px & 0x00FFFFFF, 0x00E53935, "arrow pixel was {px:08X}");
        // Pixelation averages the 2x2 checker into a uniform gray block.
        let a = out.pixel(110, 50);
        assert_eq!(a, out.pixel(111, 50));
        assert_eq!(a, out.pixel(110, 51));
        assert!(((a >> 8) & 0xFF) > 0x40 && ((a >> 8) & 0xFF) < 0xC0, "blur pixel {a:08X}");
        // Highlight multiplies white with the color.
        let m = out.pixel(0, 100);
        assert_eq!(m & 0x00FFFFFF, 0x00E53935, "marker pixel {m:08X}");
        // Text and step leave marks; untouched area stays as it was.
        assert!((50..80).any(|y| (10..40).any(|x| out.pixel(x, y) != base.pixel(x, y))), "text not drawn");
        assert_eq!(out.pixel(170, 100) & 0x00FFFFFF, 0x00E53935, "step fill");
        assert_eq!(out.pixel(80, 40), base.pixel(80, 40));
    }

    #[test]
    fn crop_and_hit_test() {
        let base = checker(50, 40);
        let c = base.crop(Rect::new(10, 5, 20, 10));
        assert_eq!((c.w, c.h), (20, 10));
        assert_eq!(c.pixel(0, 0), base.pixel(10, 5));
        let line = ann(Tool::Line, P { x: 0.0, y: 0.0 }, P { x: 100.0, y: 0.0 });
        assert!(line.hit(P { x: 50.0, y: 3.0 }, 2.0));
        assert!(!line.hit(P { x: 50.0, y: 20.0 }, 2.0));
    }

    #[test]
    fn annotations_roundtrip_through_sidecar() {
        gdip::startup();
        let mut text = ann(Tool::Text, P { x: 12.5, y: 30.0 }, P { x: 12.5, y: 30.0 });
        text.text = "Line one\twith tab\nline \\ two".into();
        let mut pen = ann(Tool::Pen, P { x: 1.0, y: 2.0 }, P { x: 1.0, y: 2.0 });
        pen.pts = vec![P { x: 1.0, y: 2.0 }, P { x: 3.5, y: 4.25 }];
        let mut step = ann(Tool::Step, P { x: 40.0, y: 40.0 }, P { x: 40.0, y: 40.0 });
        step.n = 7;
        let items = vec![ann(Tool::Arrow, P { x: 0.0, y: 0.0 }, P { x: 20.0, y: 10.0 }), text, pen, step];
        let back = parse_annotations(&serialize_annotations(&items));
        assert_eq!(back.len(), items.len());
        for (a, b) in items.iter().zip(&back) {
            assert!(a.tool == b.tool && a.a == b.a && a.b == b.b && a.pts == b.pts);
            assert_eq!((a.text.as_str(), a.n, a.color, a.width, a.size), (b.text.as_str(), b.n, b.color, b.width, b.size));
        }

        let dir = std::env::temp_dir().join(format!("monpresso-sidecar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let image = dir.join("shot.png");
        let base = checker(30, 20);
        assert!(gdip::save_image(&render(&base, &back, None, None), &image.to_string_lossy()));
        assert!(crate::project::save(&image, &base, &serialize_annotations(&back)));
        assert!(crate::project::has(&image));
        let (loaded, txt) = crate::project::load(&image).expect("sidecar");
        assert_eq!(loaded.px, base.px);
        assert_eq!(parse_annotations(&txt).len(), 4);
        assert_eq!(crate::project::files(&image).len(), 2);
        crate::project::remove(&image);
        assert!(!crate::project::has(&image));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_and_load_roundtrip() {
        gdip::startup();
        let img = checker(33, 17);
        let path = std::env::temp_dir().join("monpresso-test.png");
        assert!(gdip::save_image(&img, &path.to_string_lossy()));
        let back = gdip::load_image(&path.to_string_lossy()).expect("load");
        let _ = std::fs::remove_file(&path);
        assert_eq!((back.w, back.h), (33, 17));
        assert_eq!(back.px, img.px);
    }
}
