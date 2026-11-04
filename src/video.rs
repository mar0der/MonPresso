//! Built-in video encoders, so recording needs nothing besides MonPresso.exe:
//! MP4 (H.264) through Windows Media Foundation, and animated GIF with our own encoder.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use windows::core::HSTRING;
use windows::Win32::Media::MediaFoundation::*;

use crate::image::Image;

// ---------------------------------------------------------------------------
// MP4

pub struct Mp4 {
    writer: IMFSinkWriter,
    stream: u32,
    w: i32,
    h: i32,
}

fn pack(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

impl Mp4 {
    /// `w` and `h` must be even. Media Foundation must be started on this thread.
    pub fn create(path: &Path, w: i32, h: i32, fps: u32) -> windows::core::Result<Mp4> {
        unsafe {
            let mut attrs: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attrs, 2)?;
            let attrs = attrs.unwrap();
            attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
            attrs.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1)?;
            let writer = MFCreateSinkWriterFromURL(&HSTRING::from(path.as_os_str()), None, &attrs)?;

            let bitrate = ((w as u64 * h as u64 * fps as u64) / 8).clamp(2_000_000, 40_000_000) as u32;
            let out = MFCreateMediaType()?;
            out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            out.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
            out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            out.SetUINT64(&MF_MT_FRAME_SIZE, pack(w as u32, h as u32))?;
            out.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1))?;
            out.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
            let stream = writer.AddStream(&out)?;

            let input = MFCreateMediaType()?;
            input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
            input.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            input.SetUINT64(&MF_MT_FRAME_SIZE, pack(w as u32, h as u32))?;
            input.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1))?;
            input.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
            // Rows are delivered top-down.
            input.SetUINT32(&MF_MT_DEFAULT_STRIDE, (w * 4) as u32)?;
            writer.SetInputMediaType(stream, &input, None)?;
            writer.BeginWriting()?;
            Ok(Mp4 { writer, stream, w, h })
        }
    }

    /// `time` and `duration` in 100 ns units.
    pub fn write(&mut self, img: &Image, time: i64, duration: i64) -> windows::core::Result<()> {
        debug_assert!(img.w == self.w && img.h == self.h);
        let len = (self.w * self.h * 4) as u32;
        unsafe {
            let buf = MFCreateMemoryBuffer(len)?;
            let mut p: *mut u8 = std::ptr::null_mut();
            buf.Lock(&mut p, None, None)?;
            std::ptr::copy_nonoverlapping(img.px.as_ptr() as *const u8, p, len as usize);
            buf.Unlock()?;
            buf.SetCurrentLength(len)?;
            let sample = MFCreateSample()?;
            sample.AddBuffer(&buf)?;
            sample.SetSampleTime(time)?;
            sample.SetSampleDuration(duration)?;
            self.writer.WriteSample(self.stream, &sample)
        }
    }

    pub fn finish(self) -> windows::core::Result<()> {
        unsafe { self.writer.Finalize() }
    }
}

/// Start Media Foundation for the calling thread (pair with `mf_shutdown`).
pub fn mf_startup() -> windows::core::Result<()> {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        MFStartup(MF_VERSION, MFSTARTUP_FULL)
    }
}

pub fn mf_shutdown() {
    unsafe {
        let _ = MFShutdown();
    }
}

// ---------------------------------------------------------------------------
// GIF

struct Pending {
    img: Image,
    time_ms: u64,
}

/// Animated GIF writer. Each frame stores only the rectangle that changed since the
/// previous one, with its own 256-color palette.
pub struct Gif {
    out: BufWriter<File>,
    w: i32,
    h: i32,
    shown: Option<Image>,
    pending: Option<Pending>,
    lzw_table: Vec<u16>,
}

impl Gif {
    pub fn create(path: &Path, w: i32, h: i32) -> std::io::Result<Gif> {
        let mut out = BufWriter::new(File::create(path)?);
        out.write_all(b"GIF89a")?;
        out.write_all(&(w as u16).to_le_bytes())?;
        out.write_all(&(h as u16).to_le_bytes())?;
        out.write_all(&[0, 0, 0])?; // no global palette
        // Loop forever.
        out.write_all(&[0x21, 0xFF, 0x0B])?;
        out.write_all(b"NETSCAPE2.0")?;
        out.write_all(&[0x03, 0x01, 0x00, 0x00, 0x00])?;
        Ok(Gif { out, w, h, shown: None, pending: None, lzw_table: vec![0; 4096 * 256] })
    }

    /// Add a frame captured at `time_ms` (milliseconds since the start).
    pub fn write(&mut self, img: Image, time_ms: u64) -> std::io::Result<()> {
        if let Some(p) = &self.pending {
            if p.img.px == img.px {
                return Ok(()); // unchanged: the previous frame simply stays longer
            }
            let p = self.pending.take().unwrap();
            self.emit(p.img, time_ms.saturating_sub(p.time_ms))?;
        }
        self.pending = Some(Pending { img, time_ms });
        Ok(())
    }

    pub fn finish(mut self, end_ms: u64) -> std::io::Result<()> {
        if let Some(p) = self.pending.take() {
            self.emit(p.img, end_ms.saturating_sub(p.time_ms).max(100))?;
        }
        self.out.write_all(&[0x3B])?;
        self.out.flush()
    }

    fn emit(&mut self, img: Image, duration_ms: u64) -> std::io::Result<()> {
        // Only the part that changed since the frame on screen.
        let (x0, y0, x1, y1) = match &self.shown {
            None => (0, 0, self.w, self.h),
            Some(prev) => changed_box(prev, &img).unwrap_or((0, 0, 1, 1)),
        };
        let (bw, bh) = (x1 - x0, y1 - y0);
        let mut region = Vec::with_capacity((bw * bh) as usize);
        for y in y0..y1 {
            region.extend_from_slice(&img.px[(y * self.w + x0) as usize..(y * self.w + x1) as usize]);
        }
        let (palette, indices) = quantize(&region);

        let delay = (duration_ms / 10).clamp(2, 65535) as u16;
        let o = &mut self.out;
        o.write_all(&[0x21, 0xF9, 0x04, 0x04])?; // graphic control: keep previous frame
        o.write_all(&delay.to_le_bytes())?;
        o.write_all(&[0x00, 0x00])?;
        o.write_all(&[0x2C])?;
        for v in [x0, y0, bw, bh] {
            o.write_all(&(v as u16).to_le_bytes())?;
        }
        o.write_all(&[0x87])?; // local palette, 256 entries
        for c in &palette {
            o.write_all(&[(c >> 16) as u8, (c >> 8) as u8, *c as u8])?;
        }
        o.write_all(&[8])?;
        let data = lzw(&indices, &mut self.lzw_table);
        for chunk in data.chunks(255) {
            o.write_all(&[chunk.len() as u8])?;
            o.write_all(chunk)?;
        }
        o.write_all(&[0])?;
        self.shown = Some(img);
        Ok(())
    }
}

fn changed_box(a: &Image, b: &Image) -> Option<(i32, i32, i32, i32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, -1, -1);
    for y in 0..a.h {
        let ra = &a.px[(y * a.w) as usize..((y + 1) * a.w) as usize];
        let rb = &b.px[(y * b.w) as usize..((y + 1) * b.w) as usize];
        if ra == rb {
            continue;
        }
        let first = ra.iter().zip(rb).position(|(p, q)| p != q).unwrap() as i32;
        let last = (a.w - 1) - ra.iter().rev().zip(rb.iter().rev()).position(|(p, q)| p != q).unwrap() as i32;
        x0 = x0.min(first);
        x1 = x1.max(last);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    (y1 >= 0).then(|| (x0, y0, x1 + 1, y1 + 1))
}

/// Reduce to 256 colors: the most frequent 15-bit colors become the palette,
/// every other color maps to its nearest palette entry.
fn quantize(px: &[u32]) -> (Vec<u32>, Vec<u8>) {
    let key = |c: u32| (((c >> 19) & 0x1F) << 10 | ((c >> 11) & 0x1F) << 5 | ((c >> 3) & 0x1F)) as usize;
    let mut hist = vec![0u32; 32768];
    for &c in px {
        hist[key(c)] += 1;
    }
    let mut used: Vec<usize> = (0..32768).filter(|&k| hist[k] > 0).collect();
    used.sort_by(|a, b| hist[*b].cmp(&hist[*a]));
    let center = |k: usize| -> (i32, i32, i32) { (((k >> 10) as i32) << 3 | 4, (((k >> 5) & 0x1F) as i32) << 3 | 4, ((k & 0x1F) as i32) << 3 | 4) };
    let pal_keys: Vec<usize> = used.iter().take(256).copied().collect();
    let palette_rgb: Vec<(i32, i32, i32)> = pal_keys.iter().map(|&k| center(k)).collect();
    let mut lut = vec![0u8; 32768];
    for (i, &k) in pal_keys.iter().enumerate() {
        lut[k] = i as u8;
    }
    for &k in used.iter().skip(256) {
        let (r, g, b) = center(k);
        let best = palette_rgb
            .iter()
            .enumerate()
            .min_by_key(|(_, (pr, pg, pb))| (r - pr).pow(2) * 3 + (g - pg).pow(2) * 4 + (b - pb).pow(2) * 2)
            .map(|(i, _)| i)
            .unwrap_or(0);
        lut[k] = best as u8;
    }
    let mut palette: Vec<u32> = palette_rgb.iter().map(|(r, g, b)| ((*r as u32) << 16) | ((*g as u32) << 8) | *b as u32).collect();
    palette.resize(256, 0);
    (palette, px.iter().map(|&c| lut[key(c)]).collect())
}

/// GIF LZW compression of 8-bit indices (minimum code size 8).
fn lzw(indices: &[u8], table: &mut [u16]) -> Vec<u8> {
    const MIN: u32 = 8;
    let clear = 1u32 << MIN;
    let eoi = clear + 1;
    let mut out = Vec::with_capacity(indices.len() / 2 + 16);
    let (mut acc, mut nbits) = (0u32, 0u32);
    let mut size = MIN + 1;
    let mut next = eoi + 1;
    table.fill(0);

    let mut put = |code: u32, size: u32, out: &mut Vec<u8>| {
        acc |= code << nbits;
        nbits += size;
        while nbits >= 8 {
            out.push(acc as u8);
            acc >>= 8;
            nbits -= 8;
        }
    };

    put(clear, size, &mut out);
    let Some((&first, rest)) = indices.split_first() else {
        put(eoi, size, &mut out);
        if nbits > 0 {
            out.push(acc as u8);
        }
        return out;
    };
    let mut prefix = first as u32;
    for &k in rest {
        let slot = ((prefix as usize) << 8) | k as usize;
        let code = table[slot];
        if code != 0 {
            prefix = code as u32;
            continue;
        }
        put(prefix, size, &mut out);
        if next > (1 << size) - 1 && size < 12 {
            size += 1;
        }
        if next < 4096 {
            table[slot] = next as u16;
            next += 1;
        } else {
            put(clear, size, &mut out);
            table.fill(0);
            size = MIN + 1;
            next = eoi + 1;
        }
        prefix = k as u32;
    }
    put(prefix, size, &mut out);
    if next > (1 << size) - 1 && size < 12 {
        size += 1;
    }
    put(eoi, size, &mut out);
    if nbits > 0 {
        out.push(acc as u8);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("monpresso-{}-{name}", std::process::id()))
    }

    /// A busy frame with ~200 colors that sit exactly on GIF palette centers.
    fn pattern(w: i32, h: i32, seed: u32) -> Image {
        let mut img = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let v = ((x as u32 * 7 + y as u32 * 13 + seed) * 2654435761) >> 24;
                let i = v % 200;
                let (r, g, b) = ((i % 8) * 4, ((i / 8) % 5) * 6, (i / 40) * 6);
                let c = |q: u32| (q << 3) | 4;
                img.px[(y * w + x) as usize] = 0xFF00_0000 | (c(r) << 16) | (c(g) << 8) | c(b);
            }
        }
        img
    }

    #[test]
    fn gif_frames_decode_exactly() {
        crate::gdip::startup();
        let path = temp("t.gif");
        let f1 = pattern(200, 150, 1);
        let mut f2 = f1.clone();
        for y in 40..60 {
            for x in 50..90 {
                f2.px[(y * 200 + x) as usize] = 0xFFFC_0404;
            }
        }
        let mut g = Gif::create(&path, 200, 150).unwrap();
        g.write(f1.clone(), 0).unwrap();
        g.write(f1.clone(), 50).unwrap(); // unchanged: merged into the first frame
        g.write(f2.clone(), 100).unwrap();
        g.finish(200).unwrap();

        let (d1, count) = crate::gdip::load_frame(&path.to_string_lossy(), 0).expect("decode frame 0");
        assert_eq!(count, 2, "identical frames are merged");
        assert!(d1.px == f1.px, "frame 0 differs");
        let (d2, _) = crate::gdip::load_frame(&path.to_string_lossy(), 1).expect("decode frame 1");
        assert!(d2.px == f2.px, "frame 1 (delta) differs");
        let _ = std::fs::remove_file(&path);
    }

    /// Decode the first frame of an MP4 with Windows' own decoder.
    fn first_frame(path: &Path) -> (i32, i32, Vec<u8>, i32) {
        unsafe {
            let mut attrs: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attrs, 1).unwrap();
            let attrs = attrs.unwrap();
            attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1).unwrap();
            let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), &attrs).unwrap();
            let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
            let t = MFCreateMediaType().unwrap();
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).unwrap();
            t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).unwrap();
            reader.SetCurrentMediaType(stream, None, &t).unwrap();
            let cur = reader.GetCurrentMediaType(stream).unwrap();
            let size = cur.GetUINT64(&MF_MT_FRAME_SIZE).unwrap();
            let (w, h) = ((size >> 32) as i32, (size & 0xFFFF_FFFF) as i32);
            let stride = cur.GetUINT32(&MF_MT_DEFAULT_STRIDE).map(|v| v as i32).unwrap_or(w * 4);
            for _ in 0..20 {
                let mut flags = 0u32;
                let mut sample: Option<IMFSample> = None;
                reader.ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample)).unwrap();
                if let Some(sample) = sample {
                    let buf = sample.ConvertToContiguousBuffer().unwrap();
                    let mut p: *mut u8 = std::ptr::null_mut();
                    let mut len = 0u32;
                    buf.Lock(&mut p, None, Some(&mut len)).unwrap();
                    let bytes = std::slice::from_raw_parts(p, len as usize).to_vec();
                    buf.Unlock().unwrap();
                    return (w, h, bytes, stride);
                }
            }
            panic!("no frame decoded");
        }
    }

    fn rgb_at(bytes: &[u8], w: i32, h: i32, stride: i32, x: i32, y: i32) -> (u8, u8, u8) {
        let row = if stride > 0 { y } else { h - 1 - y };
        let i = (row * stride.abs() + x * 4) as usize;
        let _ = w;
        (bytes[i + 2], bytes[i + 1], bytes[i])
    }

    #[test]
    fn mp4_encodes_upright_frames() {
        mf_startup().unwrap();
        let path = temp("t.mp4");
        let (w, h) = (320, 240);
        let mut img = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.px[(y * w + x) as usize] = if y < h / 2 { 0xFFE0_2020 } else { 0xFF20_40E0 };
            }
        }
        let mut enc = Mp4::create(&path, w, h, 30).unwrap();
        for i in 0..30 {
            enc.write(&img, i * 333_333, 333_333).unwrap();
        }
        enc.finish().unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() > 1000);
        let (dw, dh, bytes, stride) = first_frame(&path);
        assert_eq!((dw, dh), (w, h));
        let top = rgb_at(&bytes, dw, dh, stride, w / 2, 20);
        let bottom = rgb_at(&bytes, dw, dh, stride, w / 2, h - 20);
        assert!(top.0 > 180 && top.2 < 90, "top should be red, got {top:?}");
        assert!(bottom.2 > 180 && bottom.0 < 90, "bottom should be blue, got {bottom:?}");
        mf_shutdown();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn records_the_real_screen() {
        let path = temp("screen.mp4");
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker = {
            let (stop, path) = (stop.clone(), path.clone());
            std::thread::spawn(move || {
                crate::record::record_to_file(crate::image::Rect::new(0, 0, 640, 360), 30, false, &path, &stop)
            })
        };
        std::thread::sleep(std::time::Duration::from_millis(1500));
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        worker.join().unwrap().unwrap();
        mf_startup().unwrap();
        let (dw, dh, _, _) = first_frame(&path);
        mf_shutdown();
        assert_eq!((dw, dh), (640, 360));
        let _ = std::fs::remove_file(&path);
    }
}
