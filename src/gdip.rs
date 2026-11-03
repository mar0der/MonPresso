//! Minimal GDI+ flat-API bindings: image load/save, thumbnails and antialiased drawing.

#![allow(non_snake_case)]

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use crate::image::Image;
use crate::util::w;

type Gp = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PointF {
    pub x: f32,
    pub y: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[repr(C)]
struct GpRect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

#[repr(C)]
struct BitmapData {
    width: u32,
    height: u32,
    stride: i32,
    format: i32,
    scan0: *mut c_void,
    reserved: usize,
}

#[repr(C)]
struct StartupInput {
    version: u32,
    callback: *mut c_void,
    suppress_background: i32,
    suppress_codecs: i32,
}

#[repr(C)]
struct Guid {
    d1: u32,
    d2: u16,
    d3: u16,
    d4: [u8; 8],
}

#[repr(C)]
struct EncoderParameter {
    guid: Guid,
    count: u32,
    kind: u32,
    value: *mut c_void,
}

#[repr(C)]
struct EncoderParameters {
    count: u32,
    params: [EncoderParameter; 1],
}

const PARGB: i32 = 0xE200B;
const LOCK_READ: u32 = 1;
const LOCK_USER_BUFFER: u32 = 4;
const UNIT_PIXEL: i32 = 2;
const SMOOTH_ANTIALIAS: i32 = 4;
const TEXT_ANTIALIAS: i32 = 4;
const PIXEL_OFFSET_HQ: i32 = 2;
const INTERP_HQ_BICUBIC: i32 = 7;
const CAP_ROUND: i32 = 2;
const JOIN_ROUND: i32 = 2;
const COMBINE_EXCLUDE: i32 = 4;
const FONT_BOLD: i32 = 1;

#[link(name = "gdiplus")]
extern "system" {
    fn GdiplusStartup(token: *mut usize, input: *const StartupInput, output: *mut c_void) -> i32;
    fn GdipCreateBitmapFromScan0(w: i32, h: i32, stride: i32, format: i32, scan0: *mut u8, bitmap: *mut Gp) -> i32;
    fn GdipCreateBitmapFromFile(file: *const u16, bitmap: *mut Gp) -> i32;
    fn GdipGetImageWidth(image: Gp, w: *mut u32) -> i32;
    fn GdipGetImageHeight(image: Gp, h: *mut u32) -> i32;
    fn GdipBitmapLockBits(bitmap: Gp, rect: *const GpRect, flags: u32, format: i32, data: *mut BitmapData) -> i32;
    fn GdipBitmapUnlockBits(bitmap: Gp, data: *mut BitmapData) -> i32;
    fn GdipDisposeImage(image: Gp) -> i32;
    fn GdipSaveImageToFile(image: Gp, file: *const u16, clsid: *const Guid, params: *const EncoderParameters) -> i32;
    fn GdipGetImageGraphicsContext(image: Gp, graphics: *mut Gp) -> i32;
    fn GdipDeleteGraphics(graphics: Gp) -> i32;
    fn GdipFlush(graphics: Gp, intention: i32) -> i32;
    fn GdipSetSmoothingMode(graphics: Gp, mode: i32) -> i32;
    fn GdipSetTextRenderingHint(graphics: Gp, hint: i32) -> i32;
    fn GdipSetPixelOffsetMode(graphics: Gp, mode: i32) -> i32;
    fn GdipSetInterpolationMode(graphics: Gp, mode: i32) -> i32;
    fn GdipDrawImageRectI(graphics: Gp, image: Gp, x: i32, y: i32, w: i32, h: i32) -> i32;
    fn GdipGraphicsClear(graphics: Gp, color: u32) -> i32;
    fn GdipCreatePen1(color: u32, width: f32, unit: i32, pen: *mut Gp) -> i32;
    fn GdipDeletePen(pen: Gp) -> i32;
    fn GdipSetPenLineCap197819(pen: Gp, start: i32, end: i32, dash: i32) -> i32;
    fn GdipSetPenLineJoin(pen: Gp, join: i32) -> i32;
    fn GdipCreateSolidFill(color: u32, brush: *mut Gp) -> i32;
    fn GdipDeleteBrush(brush: Gp) -> i32;
    fn GdipDrawLine(graphics: Gp, pen: Gp, x1: f32, y1: f32, x2: f32, y2: f32) -> i32;
    fn GdipDrawLines(graphics: Gp, pen: Gp, points: *const PointF, count: i32) -> i32;
    fn GdipDrawRectangle(graphics: Gp, pen: Gp, x: f32, y: f32, w: f32, h: f32) -> i32;
    fn GdipFillRectangle(graphics: Gp, brush: Gp, x: f32, y: f32, w: f32, h: f32) -> i32;
    fn GdipDrawEllipse(graphics: Gp, pen: Gp, x: f32, y: f32, w: f32, h: f32) -> i32;
    fn GdipFillEllipse(graphics: Gp, brush: Gp, x: f32, y: f32, w: f32, h: f32) -> i32;
    fn GdipFillPolygon(graphics: Gp, brush: Gp, points: *const PointF, count: i32, fill_mode: i32) -> i32;
    fn GdipSetClipRect(graphics: Gp, x: f32, y: f32, w: f32, h: f32, mode: i32) -> i32;
    fn GdipResetClip(graphics: Gp) -> i32;
    fn GdipCreateFontFamilyFromName(name: *const u16, collection: Gp, family: *mut Gp) -> i32;
    fn GdipCreatePath(fill_mode: i32, path: *mut Gp) -> i32;
    fn GdipDeletePath(path: Gp) -> i32;
    fn GdipAddPathString(path: Gp, text: *const u16, len: i32, family: Gp, style: i32, em: f32, layout: *const RectF, format: Gp) -> i32;
    fn GdipDrawPath(graphics: Gp, pen: Gp, path: Gp) -> i32;
    fn GdipAddPathArc(path: Gp, x: f32, y: f32, w: f32, h: f32, start: f32, sweep: f32) -> i32;
    fn GdipAddPathBezier(path: Gp, x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32, x4: f32, y4: f32) -> i32;
    fn GdipAddPathLine2(path: Gp, points: *const PointF, count: i32) -> i32;
    fn GdipClosePathFigure(path: Gp) -> i32;
    fn GdipStartPathFigure(path: Gp) -> i32;
    fn GdipDrawArc(graphics: Gp, pen: Gp, x: f32, y: f32, w: f32, h: f32, start: f32, sweep: f32) -> i32;
    fn GdipFillPath(graphics: Gp, brush: Gp, path: Gp) -> i32;
    fn GdipGetPathWorldBounds(path: Gp, bounds: *mut RectF, matrix: Gp, pen: Gp) -> i32;
    fn GdipCreateStringFormat(attributes: i32, language: u16, format: *mut Gp) -> i32;
    fn GdipSetStringFormatAlign(format: Gp, align: i32) -> i32;
    fn GdipSetStringFormatLineAlign(format: Gp, align: i32) -> i32;
    fn GdipDeleteStringFormat(format: Gp) -> i32;
}

pub fn startup() {
    let mut token = 0usize;
    let input = StartupInput { version: 1, callback: null_mut(), suppress_background: 0, suppress_codecs: 0 };
    unsafe { GdiplusStartup(&mut token, &input, null_mut()) };
}

fn encoder_for(path: &str) -> (Guid, bool) {
    let lower = path.to_ascii_lowercase();
    let d1 = if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        0x557CF401
    } else if lower.ends_with(".bmp") {
        0x557CF400
    } else {
        0x557CF406 // png
    };
    let guid = Guid { d1, d2: 0x1A04, d3: 0x11D3, d4: [0x9A, 0x73, 0x00, 0x00, 0xF8, 0x1E, 0xF3, 0x2E] };
    (guid, d1 == 0x557CF401)
}

/// Load any GDI+-supported image (PNG, JPEG, BMP, GIF, TIFF) as premultiplied BGRA.
pub fn load_image(path: &str) -> Option<Image> {
    let wpath = w(path);
    unsafe {
        let mut bmp: Gp = null_mut();
        if GdipCreateBitmapFromFile(wpath.as_ptr(), &mut bmp) != 0 || bmp.is_null() {
            return None;
        }
        let (mut iw, mut ih) = (0u32, 0u32);
        GdipGetImageWidth(bmp, &mut iw);
        GdipGetImageHeight(bmp, &mut ih);
        if iw == 0 || ih == 0 {
            GdipDisposeImage(bmp);
            return None;
        }
        let mut img = Image::new(iw as i32, ih as i32);
        let rect = GpRect { x: 0, y: 0, w: iw as i32, h: ih as i32 };
        let mut data = BitmapData {
            width: iw,
            height: ih,
            stride: iw as i32 * 4,
            format: PARGB,
            scan0: img.px.as_mut_ptr() as *mut c_void,
            reserved: 0,
        };
        let ok = GdipBitmapLockBits(bmp, &rect, LOCK_READ | LOCK_USER_BUFFER, PARGB, &mut data) == 0;
        if ok {
            GdipBitmapUnlockBits(bmp, &mut data);
        }
        GdipDisposeImage(bmp);
        ok.then_some(img)
    }
}

pub fn save_image(img: &Image, path: &str) -> bool {
    let (clsid, jpeg) = encoder_for(path);
    let wpath = w(path);
    unsafe {
        let mut bmp: Gp = null_mut();
        if GdipCreateBitmapFromScan0(img.w, img.h, img.w * 4, PARGB, img.px.as_ptr() as *mut u8, &mut bmp) != 0 {
            return false;
        }
        let mut quality: u32 = 92;
        let params = EncoderParameters {
            count: 1,
            params: [EncoderParameter {
                guid: Guid {
                    d1: 0x1D5BE4B5,
                    d2: 0xFA4A,
                    d3: 0x452D,
                    d4: [0x9C, 0xDD, 0x5D, 0xB3, 0x51, 0x05, 0xE7, 0xEB],
                },
                count: 1,
                kind: 4, // EncoderParameterValueTypeLong
                value: &mut quality as *mut u32 as *mut c_void,
            }],
        };
        let ok = GdipSaveImageToFile(bmp, wpath.as_ptr(), &clsid, if jpeg { &params } else { null() }) == 0;
        GdipDisposeImage(bmp);
        ok
    }
}

/// Decode a file and scale it to fit a `tw` x `th` tile on a light background.
/// Returns the tile plus the source image size.
pub fn thumbnail(path: &str, tw: i32, th: i32, background: u32) -> Option<(Image, u32, u32)> {
    let wpath = w(path);
    unsafe {
        let mut src: Gp = null_mut();
        if GdipCreateBitmapFromFile(wpath.as_ptr(), &mut src) != 0 || src.is_null() {
            return None;
        }
        let (mut iw, mut ih) = (0u32, 0u32);
        GdipGetImageWidth(src, &mut iw);
        GdipGetImageHeight(src, &mut ih);
        let mut out = Image::new(tw, th);
        if iw > 0 && ih > 0 {
            let c = Canvas::over(&mut out);
            GdipGraphicsClear(c.g, background);
            GdipSetInterpolationMode(c.g, INTERP_HQ_BICUBIC);
            let s = (tw as f32 / iw as f32).min(th as f32 / ih as f32).min(1.0);
            let dw = ((iw as f32 * s).round() as i32).max(1);
            let dh = ((ih as f32 * s).round() as i32).max(1);
            GdipDrawImageRectI(c.g, src, (tw - dw) / 2, (th - dh) / 2, dw, dh);
        }
        GdipDisposeImage(src);
        Some((out, iw, ih))
    }
}

thread_local! {
    static FAMILY: Cell<Gp> = const { Cell::new(null_mut()) };
}

fn font_family() -> Gp {
    FAMILY.with(|f| {
        if f.get().is_null() {
            for name in ["Segoe UI", "Arial"] {
                let n = w(name);
                let mut fam: Gp = null_mut();
                if unsafe { GdipCreateFontFamilyFromName(n.as_ptr(), null_mut(), &mut fam) } == 0 && !fam.is_null() {
                    f.set(fam);
                    break;
                }
            }
        }
        f.get()
    })
}

fn text_path(text: &str, size: f32, layout: RectF, format: Gp) -> Gp {
    let t: Vec<u16> = text.encode_utf16().collect();
    unsafe {
        let mut path: Gp = null_mut();
        GdipCreatePath(0, &mut path);
        GdipAddPathString(path, t.as_ptr(), t.len() as i32, font_family(), FONT_BOLD, size, &layout, format);
        path
    }
}

/// Tight bounds of a text annotation whose layout origin is (x, y).
pub fn text_bounds(text: &str, size: f32, x: f32, y: f32) -> RectF {
    let path = text_path(text, size, RectF { x, y, w: 0.0, h: 0.0 }, null_mut());
    let mut r = RectF::default();
    unsafe {
        GdipGetPathWorldBounds(path, &mut r, null_mut(), null_mut());
        GdipDeletePath(path);
    }
    r
}

/// A GDI+ drawing surface over an `Image` buffer (drawing writes straight into the pixels).
pub struct Canvas {
    bmp: Gp,
    g: Gp,
}

impl Canvas {
    pub fn over(img: &mut Image) -> Canvas {
        Canvas::over_raw(img.px.as_mut_ptr(), img.w, img.h)
    }

    /// Draw onto any top-down 32-bit pixel buffer (for example a DIB section).
    pub fn over_raw(px: *mut u32, w: i32, h: i32) -> Canvas {
        unsafe {
            let mut bmp: Gp = null_mut();
            GdipCreateBitmapFromScan0(w, h, w * 4, PARGB, px as *mut u8, &mut bmp);
            let mut g: Gp = null_mut();
            GdipGetImageGraphicsContext(bmp, &mut g);
            GdipSetSmoothingMode(g, SMOOTH_ANTIALIAS);
            GdipSetTextRenderingHint(g, TEXT_ANTIALIAS);
            GdipSetPixelOffsetMode(g, PIXEL_OFFSET_HQ);
            Canvas { bmp, g }
        }
    }

    pub fn flush(&self) {
        unsafe { GdipFlush(self.g, 1) };
    }

    fn pen(color: u32, width: f32) -> Gp {
        unsafe {
            let mut pen: Gp = null_mut();
            GdipCreatePen1(color, width, UNIT_PIXEL, &mut pen);
            GdipSetPenLineCap197819(pen, CAP_ROUND, CAP_ROUND, 0);
            GdipSetPenLineJoin(pen, JOIN_ROUND);
            pen
        }
    }

    fn brush(color: u32) -> Gp {
        let mut b: Gp = null_mut();
        unsafe { GdipCreateSolidFill(color, &mut b) };
        b
    }

    pub fn line(&self, a: PointF, b: PointF, color: u32, width: f32) {
        unsafe {
            let p = Self::pen(color, width);
            GdipDrawLine(self.g, p, a.x, a.y, b.x, b.y);
            GdipDeletePen(p);
        }
    }

    pub fn polyline(&self, pts: &[PointF], color: u32, width: f32) {
        if pts.len() < 2 {
            return;
        }
        unsafe {
            let p = Self::pen(color, width);
            GdipDrawLines(self.g, p, pts.as_ptr(), pts.len() as i32);
            GdipDeletePen(p);
        }
    }

    pub fn rect(&self, r: RectF, color: u32, width: f32) {
        unsafe {
            let p = Self::pen(color, width);
            GdipDrawRectangle(self.g, p, r.x, r.y, r.w, r.h);
            GdipDeletePen(p);
        }
    }

    pub fn ellipse(&self, r: RectF, color: u32, width: f32) {
        unsafe {
            let p = Self::pen(color, width);
            GdipDrawEllipse(self.g, p, r.x, r.y, r.w, r.h);
            GdipDeletePen(p);
        }
    }

    pub fn fill_ellipse(&self, r: RectF, color: u32) {
        unsafe {
            let b = Self::brush(color);
            GdipFillEllipse(self.g, b, r.x, r.y, r.w, r.h);
            GdipDeleteBrush(b);
        }
    }

    pub fn fill_rect(&self, r: RectF, color: u32) {
        unsafe {
            let b = Self::brush(color);
            GdipFillRectangle(self.g, b, r.x, r.y, r.w, r.h);
            GdipDeleteBrush(b);
        }
    }

    pub fn fill_polygon(&self, pts: &[PointF], color: u32) {
        unsafe {
            let b = Self::brush(color);
            GdipFillPolygon(self.g, b, pts.as_ptr(), pts.len() as i32, 0);
            GdipDeleteBrush(b);
        }
    }

    fn round_path(r: RectF, radius: f32) -> Gp {
        let d = (radius * 2.0).min(r.w).min(r.h).max(0.1);
        unsafe {
            let mut path: Gp = null_mut();
            GdipCreatePath(0, &mut path);
            GdipAddPathArc(path, r.x, r.y, d, d, 180.0, 90.0);
            GdipAddPathArc(path, r.x + r.w - d, r.y, d, d, 270.0, 90.0);
            GdipAddPathArc(path, r.x + r.w - d, r.y + r.h - d, d, d, 0.0, 90.0);
            GdipAddPathArc(path, r.x, r.y + r.h - d, d, d, 90.0, 90.0);
            GdipClosePathFigure(path);
            path
        }
    }

    pub fn fill_round(&self, r: RectF, radius: f32, color: u32) {
        unsafe {
            let path = Self::round_path(r, radius);
            let b = Self::brush(color);
            GdipFillPath(self.g, b, path);
            GdipDeleteBrush(b);
            GdipDeletePath(path);
        }
    }

    pub fn stroke_round(&self, r: RectF, radius: f32, color: u32, width: f32) {
        unsafe {
            let path = Self::round_path(r, radius);
            let p = Self::pen(color, width);
            GdipDrawPath(self.g, p, path);
            GdipDeletePen(p);
            GdipDeletePath(path);
        }
    }

    pub fn arc(&self, r: RectF, start: f32, sweep: f32, color: u32, width: f32) {
        unsafe {
            let p = Self::pen(color, width);
            GdipDrawArc(self.g, p, r.x, r.y, r.w, r.h, start, sweep);
            GdipDeletePen(p);
        }
    }

    /// Stroke a smooth curve made of cubic Bezier segments (start, then 3 points per segment).
    pub fn curve(&self, pts: &[PointF], color: u32, width: f32) {
        unsafe {
            let mut path: Gp = null_mut();
            GdipCreatePath(0, &mut path);
            for seg in pts.windows(4).step_by(3) {
                GdipAddPathBezier(path, seg[0].x, seg[0].y, seg[1].x, seg[1].y, seg[2].x, seg[2].y, seg[3].x, seg[3].y);
            }
            let p = Self::pen(color, width);
            GdipDrawPath(self.g, p, path);
            GdipDeletePen(p);
            GdipDeletePath(path);
        }
    }

    pub fn polygon(&self, pts: &[PointF], color: u32, width: f32) {
        unsafe {
            let mut path: Gp = null_mut();
            GdipCreatePath(0, &mut path);
            GdipStartPathFigure(path);
            GdipAddPathLine2(path, pts.as_ptr(), pts.len() as i32);
            GdipClosePathFigure(path);
            let p = Self::pen(color, width);
            GdipDrawPath(self.g, p, path);
            GdipDeletePen(p);
            GdipDeletePath(path);
        }
    }

    /// Fill everything except `hole` (spotlight effect).
    pub fn fill_except(&self, all: RectF, hole: RectF, color: u32) {
        unsafe {
            GdipSetClipRect(self.g, hole.x, hole.y, hole.w, hole.h, COMBINE_EXCLUDE);
            self.fill_rect(all, color);
            GdipResetClip(self.g);
        }
    }

    /// Bold text with a contrasting halo so it stays readable on any background.
    pub fn text(&self, x: f32, y: f32, text: &str, size: f32, color: u32, halo: u32) {
        let path = text_path(text, size, RectF { x, y, w: 0.0, h: 0.0 }, null_mut());
        unsafe {
            let p = Self::pen(halo, (size * 0.16).max(2.0));
            GdipDrawPath(self.g, p, path);
            GdipDeletePen(p);
            let b = Self::brush(color);
            GdipFillPath(self.g, b, path);
            GdipDeleteBrush(b);
            GdipDeletePath(path);
        }
    }

    pub fn text_centered(&self, r: RectF, text: &str, size: f32, color: u32) {
        unsafe {
            let mut fmt: Gp = null_mut();
            GdipCreateStringFormat(0, 0, &mut fmt);
            GdipSetStringFormatAlign(fmt, 1);
            GdipSetStringFormatLineAlign(fmt, 1);
            let path = text_path(text, size, r, fmt);
            let b = Self::brush(color);
            GdipFillPath(self.g, b, path);
            GdipDeleteBrush(b);
            GdipDeletePath(path);
            GdipDeleteStringFormat(fmt);
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            GdipFlush(self.g, 1);
            GdipDeleteGraphics(self.g);
            GdipDisposeImage(self.bmp);
        }
    }
}
