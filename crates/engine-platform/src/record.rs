//! Video clips of the game's frames (the Windows test room, docs/WINDOWS-ROOM.md): the frames
//! sized for the clip ([`fit`], [`scale_bgra`]) and turned into NV12 ([`bgra_to_nv12`]), written
//! to an MP4 file as H.264 by Windows' own Media Foundation ([`Mp4`]: its sink writer, a hardware
//! encoder where the card has one). Plus what the room's Record button needs of the window
//! ([`cursor_on_window`], [`left_button_down`]) and the clip's name ([`local_stamp`]).

/// The clip's size for frames of `width` by `height`: the same, or scaled down (its aspect kept)
/// to fit `most` (width, height); even in both (H.264's 4:2:0), at least 2 by 2.
pub fn fit(width: u32, height: u32, most: (u32, u32)) -> (u32, u32) {
    let (w, h) = (width.max(2) as f64, height.max(2) as f64);
    let k = (most.0 as f64 / w).min(most.1 as f64 / h).min(1.0);
    let even = |v: f64| ((v.floor() as u32) & !1).max(2);
    (even(w * k), even(h * k))
}

/// `src` (B, G, R, A rows of `sw` by `sh`) scaled to `dw` by `dh`: each output pixel the average
/// of the source pixels it covers (a box filter; nearest when it grows).
pub fn scale_bgra(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let (sw, sh, dw, dh) = (sw as usize, sh as usize, dw as usize, dh as usize);
    if sw == dw && sh == dh {
        return src.to_vec();
    }
    let mut out = vec![0u8; dw * dh * 4];
    if sw == 0 || sh == 0 || src.len() < sw * sh * 4 {
        return out;
    }
    for y in 0..dh {
        let y0 = y * sh / dh;
        let y1 = ((y + 1) * sh / dh).max(y0 + 1).min(sh);
        for x in 0..dw {
            let x0 = x * sw / dw;
            let x1 = ((x + 1) * sw / dw).max(x0 + 1).min(sw);
            let mut sum = [0u32; 4];
            for yy in y0..y1 {
                let row = yy * sw * 4;
                for xx in x0..x1 {
                    let p = row + xx * 4;
                    for c in 0..4 {
                        sum[c] += u32::from(src[p + c]);
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            let o = (y * dw + x) * 4;
            for c in 0..4 {
                out[o + c] = ((sum[c] + n / 2) / n) as u8;
            }
        }
    }
    out
}

/// `src` (B, G, R, A rows of `w` by `h`, both even) as NV12: the luma plane, then the chroma
/// pairs (U, V) at half size, BT.709 limited range (what an HD H.264 player expects).
pub fn bgra_to_nv12(src: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let mut out = vec![0u8; w * h + w * h / 2];
    if src.len() < w * h * 4 {
        return out;
    }
    let (luma, chroma) = out.split_at_mut(w * h);
    let px = |x: usize, y: usize| {
        let p = (y * w + x) * 4;
        (
            f32::from(src[p + 2]),
            f32::from(src[p + 1]),
            f32::from(src[p]),
        )
    };
    for y in 0..h {
        for x in 0..w {
            let (r, g, b) = px(x, y);
            let yv = 16.0 + 0.1826 * r + 0.6142 * g + 0.0620 * b;
            luma[y * w + x] = yv.round().clamp(0.0, 255.0) as u8;
        }
    }
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let mut acc = (0.0f32, 0.0f32, 0.0f32);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (r, g, b) = px((x + dx).min(w - 1), (y + dy).min(h - 1));
                acc = (acc.0 + r, acc.1 + g, acc.2 + b);
            }
            let (r, g, b) = (acc.0 / 4.0, acc.1 / 4.0, acc.2 / 4.0);
            let u = 128.0 - 0.1006 * r - 0.3386 * g + 0.4392 * b;
            let v = 128.0 + 0.4392 * r - 0.3989 * g - 0.0403 * b;
            let at = (y / 2) * w + x;
            chroma[at] = u.round().clamp(0.0, 255.0) as u8;
            chroma[at + 1] = v.round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// The local date and time for a clip's name (`YYYY-MM-DD_HH-MM-SS`; UTC off Windows).
pub fn local_stamp() -> String {
    #[cfg(windows)]
    {
        // SAFETY: GetLocalTime only fills the structure it is given.
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        format!(
            "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
        )
    }
    #[cfg(not(windows))]
    {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let (days, rest) = (secs / 86_400, secs % 86_400);
        let (y, m, d) = civil(days as i64);
        format!(
            "{y:04}-{m:02}-{d:02}_{:02}-{:02}-{:02}",
            rest / 3600,
            rest / 60 % 60,
            rest % 60
        )
    }
}

/// The calendar date `days` after 1970-01-01 (Howard Hinnant's days-to-civil).
#[cfg(any(not(windows), test))]
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// Where the mouse cursor is over the game's window `hwnd`, as fractions of its client area
/// (0 to 1, left to right, top to bottom), and the area's width over its height, when the cursor
/// is shown and over it (in the game's menus; in play the game hides it and holds the mouse for
/// the look).
#[cfg(windows)]
pub fn cursor_on_window(hwnd: isize) -> Option<([f32; 2], f32)> {
    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::Graphics::Gdi::ScreenToClient;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetCursorInfo, CURSORINFO, CURSOR_SHOWING,
    };
    if hwnd == 0 {
        return None;
    }
    let window = HWND(hwnd as *mut core::ffi::c_void);
    let mut info = CURSORINFO {
        cbSize: std::mem::size_of::<CURSORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: plain Win32 queries filling the structures they are given.
    unsafe {
        GetCursorInfo(&mut info).ok()?;
        if info.flags.0 & CURSOR_SHOWING.0 == 0 {
            return None;
        }
        let mut p: POINT = info.ptScreenPos;
        if !ScreenToClient(window, &mut p).as_bool() {
            return None;
        }
        let mut r = RECT::default();
        GetClientRect(window, &mut r).ok()?;
        let (w, h) = ((r.right - r.left) as f32, (r.bottom - r.top) as f32);
        if w <= 0.0 || h <= 0.0 {
            return None;
        }
        let f = [p.x as f32 / w, p.y as f32 / h];
        ((0.0..=1.0).contains(&f[0]) && (0.0..=1.0).contains(&f[1])).then_some((f, w / h))
    }
}

/// Whether the left mouse button is held now (the whole desktop's mouse).
#[cfg(windows)]
pub fn left_button_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    // SAFETY: a plain query.
    unsafe { GetAsyncKeyState(i32::from(VK_LBUTTON.0)) as u16 & 0x8000 != 0 }
}

/// A Media Foundation call's error, said with `what` failed.
#[cfg(windows)]
fn mf(what: &'static str) -> impl Fn(windows::core::Error) -> String {
    move |err| format!("{what}: {err}")
}

/// An MP4 file being written: H.264 video of NV12 frames ([`bgra_to_nv12`]) through Media
/// Foundation's sink writer (Windows 7 and later; a hardware encoder where the card has one).
#[cfg(windows)]
pub struct Mp4 {
    writer: windows::Win32::Media::MediaFoundation::IMFSinkWriter,
    stream: u32,
    width: u32,
    height: u32,
}

#[cfg(windows)]
impl Mp4 {
    /// A new clip at `path`, `width` by `height` (even), `fps` frames a second nominal (each
    /// frame keeps the time it is given), at about `bitrate` bits a second.
    pub fn create(
        path: &std::path::Path,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    ) -> Result<Mp4, String> {
        use windows::core::{HSTRING, PCWSTR};
        use windows::Win32::Media::MediaFoundation::*;
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        let e = mf;
        // SAFETY: Media Foundation's documented calls, each on interfaces it made; the path lives
        // through the call that reads it.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(e("MFStartup"))?;
            let mut attributes: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attributes, 1).map_err(e("MFCreateAttributes"))?;
            if let Some(a) = &attributes {
                let _ = a.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1);
            }
            let wide = HSTRING::from(path.as_os_str());
            let writer =
                MFCreateSinkWriterFromURL(PCWSTR(wide.as_ptr()), None, attributes.as_ref())
                    .map_err(e("MFCreateSinkWriterFromURL"))?;
            let frame_size = (u64::from(width) << 32) | u64::from(height);
            let frame_rate = (u64::from(fps.max(1)) << 32) | 1;
            let out = MFCreateMediaType().map_err(e("MFCreateMediaType"))?;
            out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(e("out major type"))?;
            out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
                .map_err(e("out subtype"))?;
            out.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)
                .map_err(e("out bitrate"))?;
            out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                .map_err(e("out interlace"))?;
            out.SetUINT64(&MF_MT_FRAME_SIZE, frame_size)
                .map_err(e("out size"))?;
            out.SetUINT64(&MF_MT_FRAME_RATE, frame_rate)
                .map_err(e("out rate"))?;
            out.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1 << 32) | 1)
                .map_err(e("out aspect"))?;
            let stream = writer.AddStream(&out).map_err(e("AddStream"))?;
            let input = MFCreateMediaType().map_err(e("MFCreateMediaType"))?;
            input
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(e("in major type"))?;
            input
                .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)
                .map_err(e("in subtype"))?;
            input
                .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                .map_err(e("in interlace"))?;
            input
                .SetUINT64(&MF_MT_FRAME_SIZE, frame_size)
                .map_err(e("in size"))?;
            input
                .SetUINT64(&MF_MT_FRAME_RATE, frame_rate)
                .map_err(e("in rate"))?;
            input
                .SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1 << 32) | 1)
                .map_err(e("in aspect"))?;
            let _ = input.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32);
            writer
                .SetInputMediaType(stream, &input, None)
                .map_err(e("SetInputMediaType"))?;
            writer.BeginWriting().map_err(e("BeginWriting"))?;
            Ok(Mp4 {
                writer,
                stream,
                width,
                height,
            })
        }
    }

    /// The clip's size.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// One NV12 frame (`width * height * 3 / 2` bytes) at `time` and for `duration` (100 ns
    /// units from the clip's start).
    pub fn write(&mut self, nv12: &[u8], time: i64, duration: i64) -> Result<(), String> {
        use windows::Win32::Media::MediaFoundation::*;
        let e = mf;
        let len = (self.width * self.height * 3 / 2) as usize;
        if nv12.len() < len {
            return Err(format!("a frame of {} bytes, not {len}", nv12.len()));
        }
        // SAFETY: the buffer is locked for the copy of exactly its length, then unlocked.
        unsafe {
            let buffer = MFCreateMemoryBuffer(len as u32).map_err(e("MFCreateMemoryBuffer"))?;
            let mut data: *mut u8 = std::ptr::null_mut();
            buffer
                .Lock(&mut data, None, None)
                .map_err(e("buffer Lock"))?;
            std::ptr::copy_nonoverlapping(nv12.as_ptr(), data, len);
            buffer.Unlock().map_err(e("buffer Unlock"))?;
            buffer
                .SetCurrentLength(len as u32)
                .map_err(e("buffer length"))?;
            let sample = MFCreateSample().map_err(e("MFCreateSample"))?;
            sample.AddBuffer(&buffer).map_err(e("AddBuffer"))?;
            sample.SetSampleTime(time).map_err(e("SetSampleTime"))?;
            sample
                .SetSampleDuration(duration.max(1))
                .map_err(e("SetSampleDuration"))?;
            self.writer
                .WriteSample(self.stream, &sample)
                .map_err(e("WriteSample"))
        }
    }

    /// The clip finished (the MP4's index written); the file is complete only after this.
    pub fn finish(self) -> Result<(), String> {
        // SAFETY: the writer's own call; MFShutdown balances the MFStartup of `create`.
        unsafe {
            let r = self.writer.Finalize().map_err(|e| format!("Finalize: {e}"));
            drop(self.writer);
            let _ = windows::Win32::Media::MediaFoundation::MFShutdown();
            r
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clip_fits_its_frames_in_even_sizes() {
        assert_eq!(fit(1920, 1080, (1920, 1080)), (1920, 1080));
        assert_eq!(fit(3840, 2160, (1920, 1080)), (1920, 1080));
        assert_eq!(fit(2560, 1440, (1920, 1080)), (1920, 1080));
        assert_eq!(fit(1366, 767, (1920, 1080)), (1366, 766));
        assert_eq!(fit(3440, 1440, (1920, 1080)), (1920, 802));
        assert_eq!(fit(1, 1, (1920, 1080)), (2, 2));
    }

    #[test]
    fn frames_scale_by_averaging_and_turn_nv12() {
        // 4x2 to 2x1: each output pixel the average of a 2x2 block.
        let mut src = vec![0u8; 4 * 2 * 4];
        for (i, p) in src.chunks_exact_mut(4).enumerate() {
            let v = if i % 4 < 2 { 100 } else { 200 };
            p.copy_from_slice(&[v, v, v, 255]);
        }
        let out = scale_bgra(&src, 4, 2, 2, 1);
        assert_eq!(out, vec![100, 100, 100, 255, 200, 200, 200, 255]);
        // White and black: 235 and 16 in luma, 128 in chroma (limited range).
        let white = vec![255u8; 2 * 2 * 4];
        let nv12 = bgra_to_nv12(&white, 2, 2);
        assert_eq!(nv12.len(), 6);
        assert_eq!(&nv12[..4], &[235, 235, 235, 235]);
        assert_eq!(&nv12[4..], &[128, 128]);
        let black = [0, 0, 0, 255].repeat(4);
        assert_eq!(&bgra_to_nv12(&black, 2, 2)[..4], &[16, 16, 16, 16]);
        // Pure red (B, G, R = 0, 0, 255): V high, U low.
        let red = [0, 0, 255, 255].repeat(4);
        let n = bgra_to_nv12(&red, 2, 2);
        assert!(n[5] > 200 && n[4] < 128, "{n:?}");
    }

    #[test]
    fn days_turn_into_dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_773), (2026, 11, 16));
        assert_eq!(civil(11_016), (2000, 2, 29));
    }
}
