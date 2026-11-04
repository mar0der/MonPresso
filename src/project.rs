//! Keeps annotations editable. Next to every annotated image MonPresso stores the
//! original (un-annotated) capture and the list of annotations in a hidden
//! ".monpresso" folder. The image itself stays a normal PNG/JPG for sharing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use windows_sys::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN};

use crate::image::Image;
use crate::util::w;

const DIR: &str = ".monpresso";

fn paths(image: &Path) -> Option<(PathBuf, PathBuf)> {
    let dir = image.parent()?.join(DIR);
    let name = image.file_name()?.to_string_lossy().into_owned();
    Some((dir.join(format!("{name}.base.png")), dir.join(format!("{name}.annotations"))))
}

/// True when the image has editable annotations stored.
pub fn has(image: &Path) -> bool {
    paths(image).is_some_and(|(b, a)| b.is_file() && a.is_file())
}

/// The stored files that belong to an image (to delete them along with it).
pub fn files(image: &Path) -> Vec<PathBuf> {
    paths(image).map(|(b, a)| [b, a].into_iter().filter(|p| p.is_file()).collect()).unwrap_or_default()
}

pub fn save(image: &Path, base: &Image, annotations: &str) -> bool {
    let Some((b, a)) = paths(image) else { return false };
    let Some(dir) = b.parent() else { return false };
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let wide = w(&dir.to_string_lossy());
    unsafe { SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN) };
    crate::gdip::save_image(base, &b.to_string_lossy()) && std::fs::write(&a, annotations).is_ok()
}

pub fn remove(image: &Path) {
    for p in files(image) {
        let _ = std::fs::remove_file(p);
    }
}

/// The original capture and the annotation text, unless the image was changed by
/// another program after MonPresso saved it.
pub fn load(image: &Path) -> Option<(Image, String)> {
    let (b, a) = paths(image)?;
    let text = std::fs::read_to_string(&a).ok()?;
    let image_time = std::fs::metadata(image).ok()?.modified().ok()?;
    let saved_time = std::fs::metadata(&a).ok()?.modified().ok()?;
    if image_time > saved_time + Duration::from_secs(5) {
        return None;
    }
    Some((crate::gdip::load_image(&b.to_string_lossy())?, text))
}
