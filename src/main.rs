//! MonPresso: a lightweight, open-source screenshot and annotation tool for Windows.

#![windows_subsystem = "windows"]

mod app;
mod clipboard;
mod editor;
mod gdip;
mod hotkey;
mod image;
mod ocr;
mod overlay;
mod project;
mod record;
mod settings;
mod ui;
mod util;
mod video;
mod workspace;

fn main() {
    app::run();
}
