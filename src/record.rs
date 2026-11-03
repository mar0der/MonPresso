//! Screen recording through FFmpeg (gdigrab), with an on-screen frame and stop button
//! that are hidden from the capture itself.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use crate::util::SS_CENTERIMAGE;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::image::Rect;
use crate::util::{self, w};

const FRAME_CLASS: &str = "MonPressoRecFrame";
const PANEL_CLASS: &str = "MonPressoRecPanel";
const WDA_EXCLUDEFROMCAPTURE: u32 = 0x11;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const ID_STOP: i32 = 1;
const KEY: u32 = 0xFF00FF; // transparent color key for the frame window

pub struct Recording {
    child: Child,
    pub path: PathBuf,
    log: PathBuf,
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

pub fn find_ffmpeg(configured: &str) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if !configured.trim().is_empty() {
        candidates.push(PathBuf::from(configured.trim()));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("ffmpeg.exe"));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|d| d.join("ffmpeg.exe")));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        candidates.push(Path::new(&local).join("Microsoft\\WinGet\\Links\\ffmpeg.exe"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

pub fn start(ffmpeg: &Path, area: Rect, fps: u32, path: PathBuf) -> Result<Recording, String> {
    let (wd, ht) = (area.w & !1, area.h & !1);
    if wd < 16 || ht < 16 {
        return Err("The selected area is too small to record.".into());
    }
    let log = std::env::temp_dir().join("monpresso-ffmpeg.log");
    let log_file = std::fs::File::create(&log).map_err(|e| e.to_string())?;
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-f", "gdigrab", "-draw_mouse", "1"])
        .args(["-framerate", &fps.to_string()])
        .args(["-offset_x", &area.x.to_string(), "-offset_y", &area.y.to_string()])
        .args(["-video_size", &format!("{wd}x{ht}"), "-i", "desktop"])
        .args(["-c:v", "libx264", "-preset", "ultrafast", "-crf", "23", "-pix_fmt", "yuv420p", "-movflags", "+faststart"])
        .arg(&path)
        // Make FFmpeg DPI aware so gdigrab coordinates are physical pixels like ours.
        .env("__COMPAT_LAYER", "HighDpiAware")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log_file));
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let child = cmd.spawn().map_err(|e| format!("Could not start FFmpeg: {e}"))?;

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
        let screen = crate::image::virtual_screen();
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
        let label = util::child(panel, "STATIC", "● REC 00:00", SS_CENTERIMAGE as u32, -1, font);
        util::move_win(label, util::sc(10, dpi), 0, util::sc(100, dpi), ph - 2);
        let stop = util::child(panel, "BUTTON", "Stop", 0, ID_STOP, font);
        util::move_win(stop, pw - util::sc(78, dpi), util::sc(5, dpi), util::sc(70, dpi), ph - util::sc(12, dpi));
        SetTimer(panel, 1, 500, None);
        ShowWindow(panel, SW_SHOWNOACTIVATE);

        Ok(Recording { child, path, log, frame, panel, label, font, started: Instant::now() })
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

    pub fn has_exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    pub fn tick(&self) {
        let secs = self.started.elapsed().as_secs();
        util::set_text(self.label, &format!("● REC {:02}:{:02}", secs / 60, secs % 60));
    }

    /// Ask FFmpeg to finish the file and wait for it.
    pub fn stop(mut self) -> Result<PathBuf, String> {
        self.close_ui();
        if let Some(mut stdin) = self.child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(b"q");
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
                _ => {
                    let _ = self.child.kill();
                    break;
                }
            }
        }
        let ok = std::fs::metadata(&self.path).map(|m| m.len() > 0).unwrap_or(false);
        if ok {
            Ok(self.path)
        } else {
            let log = std::fs::read_to_string(&self.log).unwrap_or_default();
            Err(format!("FFmpeg did not produce a video.\n\n{}", log.trim()))
        }
    }
}

/// Convert an MP4 to an optimized GIF next to it and delete the MP4.
pub fn to_gif(ffmpeg: &Path, mp4: &Path, fps: u32) -> Result<PathBuf, String> {
    let gif = mp4.with_extension("gif");
    let filter = format!("fps={},split[a][b];[a]palettegen[p];[b][p]paletteuse", fps.min(15));
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"]).arg(mp4).args(["-vf", &filter]).arg(&gif);
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    let _ = std::fs::remove_file(mp4);
    Ok(gif)
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
