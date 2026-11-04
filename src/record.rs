//! Screen recording with the built-in encoders (MP4 or GIF), plus the on-screen frame
//! and stop button, which are hidden from the recording itself.

use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::image::{self, Rect};
use crate::util::SS_CENTERIMAGE;
use crate::util::{self, w};
use crate::video;

const FRAME_CLASS: &str = "MonPressoRecFrame";
const PANEL_CLASS: &str = "MonPressoRecPanel";
const WDA_EXCLUDEFROMCAPTURE: u32 = 0x11;
const ID_STOP: i32 = 1;
const KEY: u32 = 0xFF00FF; // transparent color key for the frame window

pub struct Recording {
    worker: Option<JoinHandle<Result<(), String>>>,
    stop: Arc<AtomicBool>,
    pub path: PathBuf,
    frame: HWND,
    panel: HWND,
    label: HWND,
    font: HFONT,
    started: Instant,
}

pub fn register() {
    util::register_class(FRAME_CLASS, Some(frame_proc), null_mut(), false);
    util::register_class(PANEL_CLASS, Some(panel_proc), null_mut(), false);
}

/// Output size for an area: even dimensions, scaled down to what the format handles.
pub fn output_size(w: i32, h: i32, gif: bool) -> (i32, i32) {
    let (max_w, max_h) = if gif { (1600.0, 1600.0) } else { (4096.0, 2304.0) };
    let s = (max_w / w as f64).min(max_h / h as f64).min(1.0);
    let ow = ((w as f64 * s) as i32) & !1;
    let oh = ((h as f64 * s) as i32) & !1;
    (ow.max(2), oh.max(2))
}

/// Capture `area` until `stop` is set and encode it to `path`.
pub fn record_to_file(area: Rect, fps: u32, gif: bool, path: &std::path::Path, stop: &AtomicBool) -> Result<(), String> {
    let fps = if gif { fps.min(15) } else { fps }.clamp(5, 60);
    let (ow, oh) = output_size(area.w, area.h, gif);
    let interval = Duration::from_secs_f64(1.0 / fps as f64);
    let start = Instant::now();
    let mut next = start;
    if gif {
        let mut enc = video::Gif::create(path, ow, oh).map_err(|e| e.to_string())?;
        while !stop.load(Ordering::Relaxed) {
            let t = start.elapsed().as_millis() as u64;
            enc.write(image::capture_scaled(area, ow, oh, true), t).map_err(|e| e.to_string())?;
            pace(&mut next, interval);
        }
        enc.finish(start.elapsed().as_millis() as u64).map_err(|e| e.to_string())
    } else {
        video::mf_startup().map_err(|e| format!("Windows video encoder unavailable: {}", e.message()))?;
        let result = (|| -> windows::core::Result<()> {
            let mut enc = video::Mp4::create(path, ow, oh, fps)?;
            let frame_100ns = interval.as_nanos() as i64 / 100;
            while !stop.load(Ordering::Relaxed) {
                let t = start.elapsed().as_nanos() as i64 / 100;
                enc.write(&image::capture_scaled(area, ow, oh, true), t, frame_100ns)?;
                pace(&mut next, interval);
            }
            enc.finish()
        })();
        video::mf_shutdown();
        result.map_err(|e| format!("Recording failed: {}", e.message()))
    }
}

fn pace(next: &mut Instant, interval: Duration) {
    *next += interval;
    let now = Instant::now();
    if *next > now {
        std::thread::sleep(*next - now);
    } else {
        *next = now; // running behind: drop frames instead of piling up
    }
}

pub fn start(area: Rect, fps: u32, gif: bool, path: PathBuf) -> Result<Recording, String> {
    if area.w < 16 || area.h < 16 {
        return Err("The selected area is too small to record.".into());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let worker = {
        let (stop, path) = (stop.clone(), path.clone());
        std::thread::spawn(move || record_to_file(area, fps, gif, &path, &stop))
    };

    unsafe {
        let dpi = util::cursor_monitor_dpi();
        let pad = util::sc(3, dpi).max(2);
        let fr = Rect::new(area.x - pad, area.y - pad, area.w + pad * 2, area.h + pad * 2);
        let fc = w(FRAME_CLASS);
        let frame = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            fc.as_ptr(),
            null(),
            WS_POPUP,
            fr.x,
            fr.y,
            fr.w,
            fr.h,
            null_mut(),
            null_mut(),
            util::hinstance(),
            null(),
        );
        SetLayeredWindowAttributes(frame, KEY, 0, LWA_COLORKEY);
        SetWindowDisplayAffinity(frame, WDA_EXCLUDEFROMCAPTURE);
        ShowWindow(frame, SW_SHOWNOACTIVATE);

        let pc = w(PANEL_CLASS);
        let (pw, ph) = (util::sc(190, dpi), util::sc(38, dpi));
        let mut px = area.x + area.w - pw;
        let mut py = area.y + area.h + pad + util::sc(6, dpi);
        let screen = image::virtual_screen();
        if py + ph > screen.y + screen.h {
            py = area.y - pad - ph - util::sc(6, dpi);
        }
        if py < screen.y {
            py = area.y + util::sc(8, dpi);
        }
        px = px.max(screen.x);
        let panel = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            pc.as_ptr(),
            null(),
            WS_POPUP | WS_BORDER,
            px,
            py,
            pw,
            ph,
            null_mut(),
            null_mut(),
            util::hinstance(),
            null(),
        );
        SetWindowDisplayAffinity(panel, WDA_EXCLUDEFROMCAPTURE);
        let font = util::ui_font(dpi);
        let label = util::child(panel, "STATIC", "● REC 00:00", SS_CENTERIMAGE, -1, font);
        util::move_win(label, util::sc(10, dpi), 0, util::sc(100, dpi), ph - 2);
        let stop_btn = util::child(panel, "BUTTON", "Stop", 0, ID_STOP, font);
        util::move_win(stop_btn, pw - util::sc(78, dpi), util::sc(5, dpi), util::sc(70, dpi), ph - util::sc(12, dpi));
        SetTimer(panel, 1, 500, None);
        ShowWindow(panel, SW_SHOWNOACTIVATE);

        Ok(Recording { worker: Some(worker), stop, path, frame, panel, label, font, started: Instant::now() })
    }
}

impl Recording {
    fn close_ui(&self) {
        unsafe {
            KillTimer(self.panel, 1);
            DestroyWindow(self.frame);
            DestroyWindow(self.panel);
            DeleteObject(self.font as HGDIOBJ);
        }
    }

    /// True when the encoder stopped on its own (an error).
    pub fn has_exited(&mut self) -> bool {
        self.worker.as_ref().is_some_and(|w| w.is_finished())
    }

    pub fn tick(&self) {
        let secs = self.started.elapsed().as_secs();
        util::set_text(self.label, &format!("● REC {:02}:{:02}", secs / 60, secs % 60));
    }

    /// Stop capturing, finish the file and return its path.
    pub fn stop(mut self) -> Result<PathBuf, String> {
        self.close_ui();
        self.stop.store(true, Ordering::Relaxed);
        let result = match self.worker.take() {
            Some(w) => w.join().unwrap_or_else(|_| Err("The recorder crashed.".into())),
            None => Ok(()),
        };
        result?;
        let ok = std::fs::metadata(&self.path).map(|m| m.len() > 0).unwrap_or(false);
        if ok {
            Ok(self.path.clone())
        } else {
            Err("The recording is empty.".into())
        }
    }
}

unsafe extern "system" fn frame_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetClientRect(hwnd, &mut rc);
        let red = CreateSolidBrush(0x3539E5);
        FillRect(dc, &rc, red);
        DeleteObject(red as HGDIOBJ);
        let key = CreateSolidBrush(KEY);
        let pad = util::sc(3, util::dpi_of(hwnd)).max(2);
        let inner = RECT { left: pad, top: pad, right: rc.right - pad, bottom: rc.bottom - pad };
        FillRect(dc, &inner, key);
        DeleteObject(key as HGDIOBJ);
        EndPaint(hwnd, &ps);
        return 0;
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

unsafe extern "system" fn panel_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND if util::loword(wp) as i32 == ID_STOP => {
            crate::app::toggle_recording();
            return 0;
        }
        WM_TIMER => {
            crate::app::recording_tick();
            return 0;
        }
        WM_CTLCOLORSTATIC => {
            SetTextColor(wp as HDC, 0x3539E5);
            SetBkMode(wp as HDC, TRANSPARENT as _);
            return GetSysColorBrush(COLOR_WINDOW) as LRESULT;
        }
        WM_ERASEBKGND => {
            let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetClientRect(hwnd, &mut rc);
            FillRect(wp as HDC, &rc, GetSysColorBrush(COLOR_WINDOW));
            return 1;
        }
        WM_NCHITTEST => {
            // Let the panel be dragged anywhere outside the button.
            let hit = DefWindowProcW(hwnd, msg, wp, lp);
            return if hit == HTCLIENT as LRESULT { HTCAPTION as LRESULT } else { hit };
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}
