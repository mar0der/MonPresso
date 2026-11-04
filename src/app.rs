//! Application shell: single instance, tray icon, global hotkeys and capture flow.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Controls::{InitCommonControlsEx, ICC_BAR_CLASSES, ICC_LISTVIEW_CLASSES, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::image::{self, Image, Rect};
use crate::record::{self, Recording};
use crate::settings::Settings;
use crate::util::{self, w};
use crate::{clipboard, editor, gdip, hotkey, overlay, settings, workspace};

const MAIN_CLASS: &str = "MonPressoMain";
const WM_TRAY: u32 = WM_APP + 1;
const WM_SHOW_WORKSPACE: u32 = WM_APP + 2;
const NIN_BALLOONUSERCLICK: u32 = WM_USER + 5;

const AREA_HINT: &str = "Drag to select an area  ·  Click a window  ·  Space = all screens  ·  C = copy color  ·  Esc = cancel";
const RECORD_HINT: &str = "Select the area to record  ·  Click a window  ·  Space = all screens  ·  Esc = cancel";

#[derive(Clone, Copy)]
pub enum Kind {
    Area,
    FullScreen,
    ActiveWindow,
    LastArea,
}

thread_local! {
    static SETTINGS: RefCell<Settings> = RefCell::new(Settings::default());
    static MAIN: Cell<HWND> = const { Cell::new(null_mut()) };
    static BUSY: Cell<bool> = const { Cell::new(false) };
    static LAST_AREA: Cell<Option<Rect>> = const { Cell::new(None) };
    static RECORDING: RefCell<Option<Recording>> = const { RefCell::new(None) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

pub fn settings() -> Settings {
    SETTINGS.with(|s| s.borrow().clone())
}

fn main_hwnd() -> HWND {
    MAIN.with(|m| m.get())
}

pub fn run() {
    unsafe {
        let name = w("Local\\MonPresso.SingleInstance");
        let _mutex = CreateMutexW(null(), 1, name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if std::env::args().any(|a| a == "--tray") {
                return; // started at sign-in while already running: stay quiet
            }
            let class = w(MAIN_CLASS);
            let other = FindWindowW(class.as_ptr(), null());
            if !other.is_null() {
                PostMessageW(other, WM_SHOW_WORKSPACE, 0, 0);
            }
            return;
        }
        CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32);
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES | ICC_STANDARD_CLASSES | ICC_BAR_CLASSES,
        };
        InitCommonControlsEx(&icc);
        gdip::startup();

        let s = Settings::load();
        let _ = std::fs::create_dir_all(&s.save_folder);
        if s.start_with_windows {
            s.apply_startup(); // keep the Run entry pointing at this exe and approved
        }
        SETTINGS.with(|c| *c.borrow_mut() = s);

        util::register_class(MAIN_CLASS, Some(main_proc), null_mut(), false);
        overlay::register();
        editor::register();
        workspace::register();
        settings::register();
        record::register();

        let tc = w("TaskbarCreated");
        TASKBAR_CREATED.with(|t| t.set(RegisterWindowMessageW(tc.as_ptr())));

        let class = w(MAIN_CLASS);
        let title = w("MonPresso");
        let hwnd = CreateWindowExW(0, class.as_ptr(), title.as_ptr(), WS_OVERLAPPED, 0, 0, 0, 0, null_mut(), null_mut(), util::hinstance(), null());
        MAIN.with(|m| m.set(hwnd));
        tray(NIM_ADD);
        register_hotkeys(true);

        if !std::env::args().any(|a| a == "--tray") {
            workspace::show();
        }

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            let dlg = settings::dialog_hwnd();
            if !dlg.is_null() && IsDialogMessageW(dlg, &msg) != 0 {
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        tray(NIM_DELETE);
        // Everything is saved by now. End the process directly: unloading GDI+/COM
        // DLLs at exit can deadlock, which left the process hanging without windows.
        windows_sys::Win32::System::Threading::TerminateProcess(windows_sys::Win32::System::Threading::GetCurrentProcess(), 0);
    }
}

fn copy_w(dst: &mut [u16], s: &str) {
    for (i, c) in s.encode_utf16().take(dst.len() - 1).enumerate() {
        dst[i] = c;
    }
}

unsafe fn tray_data() -> NOTIFYICONDATAW {
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = main_hwnd();
    nid.uID = 1;
    nid
}

unsafe fn tray(action: u32) {
    let mut nid = tray_data();
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = util::app_icon(GetSystemMetrics(SM_CXSMICON));
    let recording = RECORDING.with(|r| r.borrow().is_some());
    copy_w(&mut nid.szTip, if recording { "MonPresso - recording" } else { "MonPresso" });
    Shell_NotifyIconW(action, &nid);
}

pub fn balloon(title: &str, text: &str) {
    unsafe {
        let mut nid = tray_data();
        nid.uFlags = NIF_INFO;
        copy_w(&mut nid.szInfoTitle, title);
        copy_w(&mut nid.szInfo, text);
        nid.dwInfoFlags = NIIF_INFO;
        Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

fn register_hotkeys(report: bool) {
    let main = main_hwnd();
    let s = settings();
    let keys = [
        (1, &s.hotkey_area, "Capture area"),
        (2, &s.hotkey_fullscreen, "Full screen"),
        (3, &s.hotkey_window, "Active window"),
        (4, &s.hotkey_last_area, "Repeat last area"),
        (5, &s.hotkey_record, "Recording"),
    ];
    let mut failed = Vec::new();
    for (id, text, name) in keys {
        unsafe { UnregisterHotKey(main, id) };
        match hotkey::parse(text) {
            Ok(Some((mods, vk))) => {
                if unsafe { RegisterHotKey(main, id, mods | hotkey::MOD_NOREPEAT, vk) } == 0 {
                    failed.push(format!("{name} ({text})"));
                }
            }
            Ok(None) => {}
            Err(_) => failed.push(format!("{name} ({text})")),
        }
    }
    if report && !failed.is_empty() {
        let mut msg = format!("Already in use: {}. Change them in Settings.", failed.join(", "));
        if failed.iter().any(|f| f.contains("PrintScreen")) {
            msg.push_str(" Tip: turn off \"Use the Print screen key to open screen capture\" in Windows Settings > Accessibility > Keyboard.");
        }
        balloon("Some MonPresso hotkeys are unavailable", &msg);
    }
}

pub fn apply_settings(s: Settings) {
    s.save();
    s.apply_startup();
    let _ = std::fs::create_dir_all(&s.save_folder);
    SETTINGS.with(|c| *c.borrow_mut() = s);
    register_hotkeys(true);
    workspace::refresh();
}

fn new_path(folder: &str, ext: &str) -> String {
    let base = format!("{folder}\\MonPresso_{}", util::timestamp());
    let mut path = format!("{base}.{ext}");
    let mut i = 2;
    while std::path::Path::new(&path).exists() {
        path = format!("{base}_{i}.{ext}");
        i += 1;
    }
    path
}

/// Hide the workspace while capturing from it so it is not in the shot.
fn hide_workspace() -> bool {
    let ws = workspace::hwnd();
    unsafe {
        if !ws.is_null() && IsWindowVisible(ws) != 0 {
            ShowWindow(ws, SW_HIDE);
            return true;
        }
    }
    false
}

fn restore_workspace(hidden: bool) {
    if hidden {
        let ws = workspace::hwnd();
        if !ws.is_null() {
            unsafe { ShowWindow(ws, SW_SHOW) };
        }
    }
}

fn select_area_image() -> Option<Image> {
    let vs = image::virtual_screen();
    let shot = image::capture(vs, false);
    let r = overlay::select_area(&shot, vs, AREA_HINT)?;
    LAST_AREA.with(|l| l.set(Some(r)));
    Some(shot.crop(r.offset(-vs.x, -vs.y)))
}

pub fn capture(kind: Kind, from_ui: bool) {
    if BUSY.with(|b| b.replace(true)) {
        return;
    }
    let s = settings();
    let hidden = from_ui && hide_workspace();
    if from_ui {
        // Let menus and the hidden window fade out first.
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    if s.delay_seconds > 0 {
        std::thread::sleep(std::time::Duration::from_secs(s.delay_seconds as u64));
    }
    let vs = image::virtual_screen();
    let img = match kind {
        Kind::FullScreen => Some(image::capture(vs, s.include_cursor)),
        Kind::ActiveWindow => {
            let fg = unsafe { GetForegroundWindow() };
            image::window_bounds(fg).and_then(|r| r.intersect(&vs)).map(|r| image::capture(r, s.include_cursor))
        }
        Kind::LastArea => match LAST_AREA.with(|l| l.get()).and_then(|r| r.intersect(&vs)) {
            Some(r) => Some(image::capture(r, s.include_cursor)),
            None => select_area_image(),
        },
        Kind::Area => select_area_image(),
    };
    restore_workspace(hidden);
    BUSY.with(|b| b.set(false));
    if let Some(img) = img {
        handle_capture(img, &s);
    }
}

fn handle_capture(img: Image, s: &Settings) {
    let _ = std::fs::create_dir_all(&s.save_folder);
    let path = new_path(&s.save_folder, if s.image_format == "jpg" { "jpg" } else { "png" });
    let saved = gdip::save_image(&img, &path);
    if !saved {
        util::msgbox(null_mut(), &format!("Could not save the capture to {path}"), MB_ICONERROR);
    }
    let copied = s.copy_to_clipboard && clipboard::set_image(main_hwnd(), &img);
    workspace::refresh();
    if s.open_editor {
        editor::open(saved.then_some(path), img);
    } else {
        let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        balloon("Capture saved", &format!("{name}{}", if copied { " (also copied to the clipboard)" } else { "" }));
    }
}

pub fn toggle_recording() {
    if let Some(rec) = RECORDING.with(|r| r.borrow_mut().take()) {
        finish_recording(rec);
        return;
    }
    if BUSY.with(|b| b.get()) {
        return;
    }
    let s = settings();
    let Some(ffmpeg) = record::find_ffmpeg(&s.ffmpeg_path) else {
        util::msgbox(
            null_mut(),
            "Video recording uses FFmpeg, which was not found.\n\nInstall it from a terminal with:\n    winget install Gyan.FFmpeg\n\nthen try again, or set the path to ffmpeg.exe in Settings.",
            MB_ICONINFORMATION,
        );
        return;
    };
    BUSY.with(|b| b.set(true));
    let hidden = hide_workspace();
    if hidden {
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    let vs = image::virtual_screen();
    let shot = image::capture(vs, false);
    let area = overlay::select_area(&shot, vs, RECORD_HINT);
    drop(shot);
    restore_workspace(hidden);
    BUSY.with(|b| b.set(false));
    let Some(area) = area else { return };
    let _ = std::fs::create_dir_all(&s.save_folder);
    let path = PathBuf::from(new_path(&s.save_folder, "mp4"));
    match record::start(&ffmpeg, area, s.video_fps, path) {
        Ok(rec) => {
            RECORDING.with(|r| *r.borrow_mut() = Some(rec));
            unsafe { tray(NIM_MODIFY) };
        }
        Err(e) => {
            util::msgbox(null_mut(), &e, MB_ICONERROR);
        }
    }
}

fn finish_recording(rec: Recording) {
    unsafe { tray(NIM_MODIFY) };
    let s = settings();
    match rec.stop() {
        Ok(mut path) => {
            if s.video_format == "gif" {
                if let Some(ffmpeg) = record::find_ffmpeg(&s.ffmpeg_path) {
                    match record::to_gif(&ffmpeg, &path, s.video_fps) {
                        Ok(gif) => path = gif,
                        Err(e) => {
                            util::msgbox(null_mut(), &format!("GIF conversion failed; the MP4 was kept.\n\n{e}"), MB_ICONWARNING);
                        }
                    }
                }
            }
            workspace::refresh();
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            balloon("Recording saved", &name);
        }
        Err(e) => {
            util::msgbox(null_mut(), &e, MB_ICONERROR);
        }
    }
}

pub fn recording_tick() {
    let exited = RECORDING.with(|r| {
        r.borrow_mut().as_mut().map(|rec| {
            rec.tick();
            rec.has_exited()
        })
    });
    if exited == Some(true) {
        if let Some(rec) = RECORDING.with(|r| r.borrow_mut().take()) {
            finish_recording(rec);
        }
    }
}

pub fn show_ocr_result(owner: HWND, result: Result<String, String>) {
    match result {
        Ok(text) if text.trim().is_empty() => {
            util::msgbox(owner, "No text was found in this image.", MB_ICONINFORMATION);
        }
        Ok(text) => {
            clipboard::set_text(owner, &text);
            let preview: String = text.chars().take(1200).collect();
            let more = if text.chars().count() > 1200 { "\n…" } else { "" };
            util::msgbox(owner, &format!("Text copied to the clipboard:\n\n{preview}{more}"), MB_ICONINFORMATION);
        }
        Err(e) => {
            util::msgbox(owner, &format!("Text recognition failed: {e}"), MB_ICONWARNING);
        }
    }
}

unsafe fn editor_windows() -> Vec<HWND> {
    let class = w("MonPressoEditor");
    let mut out = Vec::new();
    let mut h = FindWindowExW(null_mut(), null_mut(), class.as_ptr(), null());
    while !h.is_null() {
        out.push(h);
        h = FindWindowExW(null_mut(), h, class.as_ptr(), null());
    }
    out
}

unsafe fn exit() {
    for e in editor_windows() {
        SendMessageW(e, WM_CLOSE, 0, 0);
        if IsWindow(e) != 0 {
            return; // the user cancelled closing an unsaved editor
        }
    }
    if let Some(rec) = RECORDING.with(|r| r.borrow_mut().take()) {
        finish_recording(rec);
    }
    let ws = workspace::hwnd();
    if !ws.is_null() {
        DestroyWindow(ws);
    }
    let dlg = settings::dialog_hwnd();
    if !dlg.is_null() {
        DestroyWindow(dlg);
    }
    PostQuitMessage(0);
}

unsafe fn tray_menu(hwnd: HWND) {
    let s = settings();
    let recording = RECORDING.with(|r| r.borrow().is_some());
    let menu = CreatePopupMenu();
    let add = |id: usize, text: String| {
        let t = w(&text);
        AppendMenuW(menu, MF_STRING, id, t.as_ptr());
    };
    let hint = |k: &str| if k.is_empty() { String::new() } else { format!("\t{k}") };
    add(1, format!("Capture area{}", hint(&s.hotkey_area)));
    add(2, format!("Capture full screen{}", hint(&s.hotkey_fullscreen)));
    add(3, format!("Repeat last area{}", hint(&s.hotkey_last_area)));
    add(4, format!("{}{}", if recording { "Stop recording" } else { "Record video" }, hint(&s.hotkey_record)));
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    add(5, "Open workspace".into());
    add(6, "Settings".into());
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    add(7, "Exit".into());
    SetMenuDefaultItem(menu, 5, 0);
    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, null());
    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(menu);
    match cmd {
        1 => capture(Kind::Area, true),
        2 => capture(Kind::FullScreen, true),
        3 => capture(Kind::LastArea, true),
        4 => toggle_recording(),
        5 => workspace::show(),
        6 => settings::show(),
        7 => exit(),
        _ => {}
    }
}

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            match wp {
                1 => capture(Kind::Area, false),
                2 => capture(Kind::FullScreen, false),
                3 => capture(Kind::ActiveWindow, false),
                4 => capture(Kind::LastArea, false),
                5 => toggle_recording(),
                _ => {}
            }
            return 0;
        }
        WM_TRAY => {
            match util::loword(lp as usize) {
                WM_LBUTTONUP | NIN_BALLOONUSERCLICK => workspace::show(),
                WM_RBUTTONUP | WM_CONTEXTMENU => tray_menu(hwnd),
                _ => {}
            }
            return 0;
        }
        WM_SHOW_WORKSPACE => {
            workspace::show();
            return 0;
        }
        WM_ENDSESSION => {
            if wp != 0 {
                tray(NIM_DELETE);
                windows_sys::Win32::System::Threading::TerminateProcess(windows_sys::Win32::System::Threading::GetCurrentProcess(), 0);
            }
        }
        m if m != 0 && m == TASKBAR_CREATED.with(|t| t.get()) => {
            tray(NIM_ADD);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}
