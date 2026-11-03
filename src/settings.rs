//! Settings: stored as a small key=value file in %APPDATA%\MonPresso\settings.ini,
//! plus the settings window.

use std::path::PathBuf;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::UI::Controls::Dialogs::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::util::{self, w};

#[derive(Clone)]
pub struct Settings {
    pub save_folder: String,
    pub image_format: String,
    pub copy_to_clipboard: bool,
    pub open_editor: bool,
    pub include_cursor: bool,
    pub delay_seconds: u32,
    pub hotkey_area: String,
    pub hotkey_fullscreen: String,
    pub hotkey_window: String,
    pub hotkey_last_area: String,
    pub hotkey_record: String,
    pub ffmpeg_path: String,
    pub video_format: String,
    pub video_fps: u32,
    pub start_with_windows: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            save_folder: default_folder(),
            image_format: "png".into(),
            copy_to_clipboard: true,
            open_editor: true,
            include_cursor: false,
            delay_seconds: 0,
            hotkey_area: "PrintScreen".into(),
            hotkey_fullscreen: "Ctrl+PrintScreen".into(),
            hotkey_window: "Alt+PrintScreen".into(),
            hotkey_last_area: "Shift+PrintScreen".into(),
            hotkey_record: "Ctrl+Shift+PrintScreen".into(),
            ffmpeg_path: String::new(),
            video_format: "mp4".into(),
            video_fps: 30,
            start_with_windows: false,
        }
    }
}

fn default_folder() -> String {
    unsafe {
        let mut p: windows_sys::core::PWSTR = null_mut();
        if SHGetKnownFolderPath(&FOLDERID_Pictures, 0, null_mut(), &mut p) == 0 && !p.is_null() {
            let len = (0..).take_while(|&i| *p.add(i) != 0).count();
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
            CoTaskMemFree(p as *const _);
            return format!("{s}\\MonPresso");
        }
    }
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
    format!("{home}\\Pictures\\MonPresso")
}

fn config_path() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("MonPresso").join("settings.ini")
}

fn yes(v: &str) -> bool {
    matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

impl Settings {
    pub fn load() -> Settings {
        let mut s = Settings::default();
        let Ok(text) = std::fs::read_to_string(config_path()) else {
            return s;
        };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim().to_string();
            match k.trim() {
                "save_folder" if !v.is_empty() => s.save_folder = v,
                "image_format" => s.image_format = if v == "jpg" { v } else { "png".into() },
                "copy_to_clipboard" => s.copy_to_clipboard = yes(&v),
                "open_editor" => s.open_editor = yes(&v),
                "include_cursor" => s.include_cursor = yes(&v),
                "delay_seconds" => s.delay_seconds = v.parse().unwrap_or(0).min(30),
                "hotkey_area" => s.hotkey_area = v,
                "hotkey_fullscreen" => s.hotkey_fullscreen = v,
                "hotkey_window" => s.hotkey_window = v,
                "hotkey_last_area" => s.hotkey_last_area = v,
                "hotkey_record" => s.hotkey_record = v,
                "ffmpeg_path" => s.ffmpeg_path = v,
                "video_format" => s.video_format = if v == "gif" { v } else { "mp4".into() },
                "video_fps" => s.video_fps = v.parse().unwrap_or(30).clamp(5, 60),
                "start_with_windows" => s.start_with_windows = yes(&v),
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) {
        let path = config_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text = format!(
            "save_folder={}\nimage_format={}\ncopy_to_clipboard={}\nopen_editor={}\ninclude_cursor={}\n\
             delay_seconds={}\nhotkey_area={}\nhotkey_fullscreen={}\nhotkey_window={}\nhotkey_last_area={}\n\
             hotkey_record={}\nffmpeg_path={}\nvideo_format={}\nvideo_fps={}\nstart_with_windows={}\n",
            self.save_folder,
            self.image_format,
            self.copy_to_clipboard,
            self.open_editor,
            self.include_cursor,
            self.delay_seconds,
            self.hotkey_area,
            self.hotkey_fullscreen,
            self.hotkey_window,
            self.hotkey_last_area,
            self.hotkey_record,
            self.ffmpeg_path,
            self.video_format,
            self.video_fps,
            self.start_with_windows
        );
        let _ = std::fs::write(path, text);
    }

    /// Add or remove the HKCU Run entry.
    pub fn apply_startup(&self) {
        let key = w("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        let name = w("MonPresso");
        unsafe {
            let mut hkey: HKEY = null_mut();
            if RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_SET_VALUE, &mut hkey) != 0 {
                return;
            }
            if self.start_with_windows {
                let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                let value = w(&format!("\"{exe}\" --tray"));
                RegSetValueExW(hkey, name.as_ptr(), 0, REG_SZ, value.as_ptr() as *const u8, (value.len() * 2) as u32);
            } else {
                RegDeleteValueW(hkey, name.as_ptr());
            }
            RegCloseKey(hkey);
        }
    }
}

// ---------------------------------------------------------------------------
// Settings window

const CLASS: &str = "MonPressoSettings";
const ID_FOLDER: i32 = 10;
const ID_BROWSE_FOLDER: i32 = 11;
const ID_FORMAT: i32 = 12;
const ID_COPY: i32 = 13;
const ID_EDITOR: i32 = 14;
const ID_CURSOR: i32 = 15;
const ID_DELAY: i32 = 16;
const ID_HK_BASE: i32 = 20;
const ID_FFMPEG: i32 = 30;
const ID_BROWSE_FFMPEG: i32 = 31;
const ID_VFORMAT: i32 = 32;
const ID_FPS: i32 = 33;
const ID_STARTUP: i32 = 34;

const DELAYS: [u32; 6] = [0, 1, 2, 3, 5, 10];
const FPS: [u32; 4] = [15, 24, 30, 60];
const HOTKEY_LABELS: [&str; 5] = ["Capture area", "Full screen", "Active window", "Repeat last area", "Start / stop recording"];

struct Dialog {
    fonts: crate::ui::Fonts,
    footer_y: i32,
}

const ID_HEADING: i32 = 90;
const ID_HINT: i32 = 91;

thread_local! {
    static OPEN: std::cell::Cell<HWND> = const { std::cell::Cell::new(null_mut()) };
}

pub fn register() {
    util::register_class(CLASS, Some(wndproc), unsafe { GetStockObject(WHITE_BRUSH) } as HBRUSH, false);
}

pub fn dialog_hwnd() -> HWND {
    OPEN.with(|o| o.get())
}

pub fn show() {
    let existing = dialog_hwnd();
    if !existing.is_null() {
        util::bring_to_front(existing);
        return;
    }
    let dpi = util::cursor_monitor_dpi();
    let (cw, ch) = (util::sc(580, dpi), util::sc(700, dpi));
    let (x, y) = util::centered_rect(cw, ch);
    let class = w(CLASS);
    let title = w("MonPresso settings");
    unsafe {
        let hwnd = CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            x,
            y,
            cw,
            ch,
            null_mut(),
            null_mut(),
            util::hinstance(),
            null(),
        );
        crate::ui::style_titlebar(hwnd);
        let dpi = util::dpi_of(hwnd);
        let st = Box::into_raw(Box::new(Dialog { fonts: crate::ui::Fonts::new(dpi), footer_y: 0 }));
        util::set_state(hwnd, st);
        build(hwnd, &mut *st, &crate::app::settings(), dpi);
        OPEN.with(|o| o.set(hwnd));
        ShowWindow(hwnd, SW_SHOW);
    }
}

unsafe fn build(hwnd: HWND, d: &mut Dialog, s: &Settings, dpi: i32) {
    let f = d.fonts.normal;
    let sc = |v: i32| util::sc(v, dpi);
    let width = sc(560);
    let (lx, cx, row) = (sc(28), sc(190), sc(34));
    let full = width - sc(28) - cx;
    let mut y = sc(18);
    let place = |h: HWND, x: i32, y: i32, wdt: i32, hgt: i32| {
        util::move_win(h, x, y, wdt, hgt);
        h
    };
    let label = |text: &str| util::child(hwnd, "STATIC", text, 0, -1, f);
    let hint = |text: &str| util::child(hwnd, "STATIC", text, 0, ID_HINT, d.fonts.small);
    let heading = |text: &str| util::child(hwnd, "STATIC", text, 0, ID_HEADING, d.fonts.title);
    let edit = |text: &str, id: i32| util::child(hwnd, "EDIT", text, WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32, id, f);
    let check = |text: &str, id: i32, on: bool| {
        let h = util::child(hwnd, "BUTTON", text, WS_TABSTOP | BS_AUTOCHECKBOX as u32, id, f);
        SendMessageW(h, BM_SETCHECK, on as usize, 0);
        h
    };
    let combo = |id: i32, items: &[String], sel: usize| {
        let h = util::child(hwnd, "COMBOBOX", "", WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32, id, f);
        for it in items {
            let t = w(it);
            SendMessageW(h, CB_ADDSTRING, 0, t.as_ptr() as LPARAM);
        }
        SendMessageW(h, CB_SETCURSEL, sel, 0);
        h
    };
    let button = |text: &str, id: i32| util::child(hwnd, "BUTTON", text, WS_TABSTOP, id, f);
    let field_label = |text: &str, y: i32| place(label(text), lx, y + sc(4), cx - lx - sc(8), sc(20));

    place(heading("Captures"), lx, y, full, sc(26));
    y += sc(36);
    field_label("Save captures in", y);
    place(edit(&s.save_folder, ID_FOLDER), cx, y, full - sc(96), sc(26));
    place(button("Browse…", ID_BROWSE_FOLDER), cx + full - sc(88), y - 1, sc(88), sc(28));
    y += row;
    field_label("Image format", y);
    let fmt_sel = if s.image_format == "jpg" { 1 } else { 0 };
    place(combo(ID_FORMAT, &["PNG (lossless)".into(), "JPG (smaller)".into()], fmt_sel), cx, y, sc(160), sc(200));
    y += row;
    field_label("Capture delay", y);
    let delays: Vec<String> = DELAYS.iter().map(|d| if *d == 0 { "None".into() } else { format!("{d} seconds") }).collect();
    let dsel = DELAYS.iter().position(|d| *d == s.delay_seconds).unwrap_or(0);
    place(combo(ID_DELAY, &delays, dsel), cx, y, sc(160), sc(200));
    y += row + sc(2);
    place(check("Copy every capture to the clipboard", ID_COPY, s.copy_to_clipboard), cx, y, full, sc(22));
    y += sc(28);
    place(check("Open the editor after a capture", ID_EDITOR, s.open_editor), cx, y, full, sc(22));
    y += sc(28);
    place(check("Include the mouse pointer", ID_CURSOR, s.include_cursor), cx, y, full, sc(22));
    y += sc(44);

    place(heading("Hotkeys"), lx, y, full, sc(26));
    y += sc(30);
    place(hint("Type a combination such as Ctrl+Shift+S or PrintScreen. Leave empty to disable."), lx, y, width - lx * 2, sc(18));
    y += sc(28);
    let values = [&s.hotkey_area, &s.hotkey_fullscreen, &s.hotkey_window, &s.hotkey_last_area, &s.hotkey_record];
    for (i, (name, value)) in HOTKEY_LABELS.iter().zip(values).enumerate() {
        field_label(name, y);
        place(edit(value, ID_HK_BASE + i as i32), cx, y, sc(220), sc(26));
        y += row;
    }
    y += sc(12);

    place(heading("Video"), lx, y, full, sc(26));
    y += sc(36);
    field_label("FFmpeg", y);
    place(edit(&s.ffmpeg_path, ID_FFMPEG), cx, y, full - sc(96), sc(26));
    place(button("Browse…", ID_BROWSE_FFMPEG), cx + full - sc(88), y - 1, sc(88), sc(28));
    y += sc(30);
    place(hint("Empty = ffmpeg.exe from PATH or next to MonPresso.exe  (winget install Gyan.FFmpeg)"), cx, y, full, sc(18));
    y += sc(28);
    field_label("Format", y);
    let vsel = if s.video_format == "gif" { 1 } else { 0 };
    place(combo(ID_VFORMAT, &["MP4 video".into(), "Animated GIF".into()], vsel), cx, y, sc(160), sc(200));
    let fps: Vec<String> = FPS.iter().map(|f| format!("{f} fps")).collect();
    let fsel = FPS.iter().position(|f| *f == s.video_fps).unwrap_or(2);
    place(combo(ID_FPS, &fps, fsel), cx + sc(170), y, sc(100), sc(200));
    y += sc(48);

    place(heading("General"), lx, y, full, sc(26));
    y += sc(36);
    place(check("Start MonPresso with Windows (in the tray)", ID_STARTUP, s.start_with_windows), cx, y, full, sc(22));
    y += sc(44);

    d.footer_y = y;
    y += sc(14);
    let ok = util::child(hwnd, "BUTTON", "Save", WS_TABSTOP | BS_DEFPUSHBUTTON as u32, IDOK, f);
    place(ok, width - sc(28) - sc(196), y, sc(96), sc(30));
    place(button("Cancel", IDCANCEL), width - sc(28) - sc(92), y, sc(92), sc(30));
    y += sc(30) + sc(14);

    // Fit the window to its content.
    let mut rc = RECT { left: 0, top: 0, right: width, bottom: y };
    windows_sys::Win32::UI::HiDpi::AdjustWindowRectExForDpi(
        &mut rc,
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
        0,
        WS_EX_DLGMODALFRAME,
        dpi as u32,
    );
    SetWindowPos(hwnd, null_mut(), 0, 0, rc.right - rc.left, rc.bottom - rc.top, SWP_NOMOVE | SWP_NOZORDER);
}

unsafe fn control(hwnd: HWND, id: i32) -> HWND {
    GetDlgItem(hwnd, id)
}

unsafe fn checked(hwnd: HWND, id: i32) -> bool {
    SendMessageW(control(hwnd, id), BM_GETCHECK, 0, 0) == 1
}

unsafe fn selected(hwnd: HWND, id: i32) -> usize {
    SendMessageW(control(hwnd, id), CB_GETCURSEL, 0, 0).max(0) as usize
}

unsafe fn collect(hwnd: HWND) -> Result<Settings, String> {
    let mut s = crate::app::settings();
    s.save_folder = util::get_text(control(hwnd, ID_FOLDER)).trim().to_string();
    if s.save_folder.is_empty() {
        return Err("Choose a folder for the captures.".into());
    }
    s.image_format = if selected(hwnd, ID_FORMAT) == 1 { "jpg" } else { "png" }.into();
    s.delay_seconds = DELAYS[selected(hwnd, ID_DELAY).min(DELAYS.len() - 1)];
    s.copy_to_clipboard = checked(hwnd, ID_COPY);
    s.open_editor = checked(hwnd, ID_EDITOR);
    s.include_cursor = checked(hwnd, ID_CURSOR);
    s.start_with_windows = checked(hwnd, ID_STARTUP);
    let mut hk = Vec::new();
    for i in 0..5 {
        let text = util::get_text(control(hwnd, ID_HK_BASE + i)).trim().to_string();
        crate::hotkey::parse(&text)?;
        hk.push(text);
    }
    s.hotkey_area = hk[0].clone();
    s.hotkey_fullscreen = hk[1].clone();
    s.hotkey_window = hk[2].clone();
    s.hotkey_last_area = hk[3].clone();
    s.hotkey_record = hk[4].clone();
    s.ffmpeg_path = util::get_text(control(hwnd, ID_FFMPEG)).trim().to_string();
    s.video_format = if selected(hwnd, ID_VFORMAT) == 1 { "gif" } else { "mp4" }.into();
    s.video_fps = FPS[selected(hwnd, ID_FPS).min(FPS.len() - 1)];
    Ok(s)
}

unsafe fn browse_folder(hwnd: HWND) {
    let title = w("Folder for MonPresso captures");
    let mut bi: BROWSEINFOW = std::mem::zeroed();
    bi.hwndOwner = hwnd;
    bi.lpszTitle = title.as_ptr();
    bi.ulFlags = BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE;
    let pidl = SHBrowseForFolderW(&bi);
    if pidl.is_null() {
        return;
    }
    let mut buf = [0u16; 1024];
    if SHGetPathFromIDListW(pidl, buf.as_mut_ptr()) != 0 {
        util::set_text(control(hwnd, ID_FOLDER), &util::from_w(&buf));
    }
    CoTaskMemFree(pidl as *const _);
}

unsafe fn browse_ffmpeg(hwnd: HWND) {
    let mut buf = [0u16; 1024];
    let filter: Vec<u16> = "ffmpeg.exe\0ffmpeg.exe\0Programs\0*.exe\0\0".encode_utf16().collect();
    let mut ofn: OPENFILENAMEW = std::mem::zeroed();
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = hwnd;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = buf.as_mut_ptr();
    ofn.nMaxFile = buf.len() as u32;
    ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST;
    if GetOpenFileNameW(&mut ofn) != 0 {
        util::set_text(control(hwnd, ID_FFMPEG), &util::from_w(&buf));
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            match util::loword(wp) as i32 {
                IDOK => match collect(hwnd) {
                    Ok(s) => {
                        crate::app::apply_settings(s);
                        DestroyWindow(hwnd);
                    }
                    Err(e) => {
                        util::msgbox(hwnd, &e, MB_ICONWARNING);
                    }
                },
                IDCANCEL => {
                    DestroyWindow(hwnd);
                }
                ID_BROWSE_FOLDER => browse_folder(hwnd),
                ID_BROWSE_FFMPEG => browse_ffmpeg(hwnd),
                _ => {}
            }
            return 0;
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            return 0;
        }
        WM_CTLCOLORSTATIC => {
            let dc = wp as HDC;
            SetBkMode(dc, TRANSPARENT as _);
            let color = match GetDlgCtrlID(lp as HWND) {
                ID_HINT => crate::ui::theme::MUTED,
                ID_HEADING => crate::ui::theme::TEXT,
                _ => 0xFF3A3F47,
            };
            SetTextColor(dc, crate::ui::cref(color));
            return GetStockObject(WHITE_BRUSH) as LRESULT;
        }
        WM_ERASEBKGND => {
            let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetClientRect(hwnd, &mut rc);
            FillRect(wp as HDC, &rc, GetStockObject(WHITE_BRUSH) as HBRUSH);
            if let Some(d) = util::state::<Dialog>(hwnd) {
                let band = RECT { top: d.footer_y, ..rc };
                let b = CreateSolidBrush(crate::ui::cref(crate::ui::theme::BG));
                FillRect(wp as HDC, &band, b);
                DeleteObject(b as HGDIOBJ);
                let line = RECT { top: d.footer_y, bottom: d.footer_y + 1, ..rc };
                let b = CreateSolidBrush(crate::ui::cref(crate::ui::theme::BORDER));
                FillRect(wp as HDC, &line, b);
                DeleteObject(b as HGDIOBJ);
            }
            return 1;
        }
        WM_NCDESTROY => {
            if let Some(d) = util::state::<Dialog>(hwnd) {
                drop(Box::from_raw(d as *mut Dialog));
            }
            util::set_state::<Dialog>(hwnd, null_mut());
            OPEN.with(|o| o.set(null_mut()));
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}
