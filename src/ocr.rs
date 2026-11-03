//! Text recognition through the built-in Windows OCR engine.

use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;

use crate::image::Image;

fn run(img: Image) -> windows::core::Result<String> {
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
    let max = OcrEngine::MaxImageDimension().unwrap_or(10000) as i32;
    let img = if img.w > max || img.h > max {
        // Downscale by keeping every n-th pixel; OCR only needs legible text.
        let n = (img.w.max(img.h) + max - 1) / max;
        let mut out = Image::new(img.w / n, img.h / n);
        for y in 0..out.h {
            for x in 0..out.w {
                out.px[(y * out.w + x) as usize] = img.pixel(x * n, y * n);
            }
        }
        out
    } else {
        img
    };
    let bytes: Vec<u8> = img.px.iter().flat_map(|p| p.to_le_bytes()).collect();
    let writer = DataWriter::new()?;
    writer.WriteBytes(&bytes)?;
    let buffer = writer.DetachBuffer()?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, img.w, img.h)?;
    let result = engine.RecognizeAsync(&bitmap)?.get()?;
    let lines = result.Lines()?;
    let mut text = String::new();
    for i in 0..lines.Size()? {
        if !text.is_empty() {
            text.push_str("\r\n");
        }
        text.push_str(&lines.GetAt(i)?.Text()?.to_string_lossy());
    }
    Ok(text)
}

/// Runs on a worker thread (WinRT async waits must not block the UI apartment).
pub fn recognize(img: &Image) -> Result<String, String> {
    let img = img.clone();
    std::thread::spawn(move || run(img).map_err(|e| e.message().to_string()))
        .join()
        .unwrap_or_else(|_| Err("OCR crashed".into()))
}
