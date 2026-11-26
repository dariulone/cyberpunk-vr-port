//! Lines of text under a hung screen (formerly src/Render/Caption.cpp; include/Render/Caption.hpp keeps CaptionText
//! and the class, which hands the lines over and calls this). A screen the port hangs in the dark can carry a few lines
//! of its own under what it shows -- the boot's screen the port's word under the game's legal notice -- where no ink of
//! the game's is drawn. The lines are set with DirectWrite in the system's fonts (shaped, with fallback fonts where one
//! lacks letters, right to left where the language reads so) into a software Direct2D target over a GDI DIB, then
//! into a picture with all its mips below, made once and uploaded once. Capture thread only.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::c_void;

use cvr_body::cmath;
use cvr_host::log;
use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::SystemInformation::GetTickCount64;

/// The screen's height the text is laid out at (px).
pub const SCREEN_HEIGHT: f32 = 2160.0;
/// The picture's format (Caption::Format).
const FORMAT: DXGI_FORMAT = DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;

/// One line as the C++ class hands it over: its UTF-16 text and its centre, px below the screen's middle.
#[repr(C)]
pub struct CaptionLine {
    pub text: *const u16,
    pub length: u32,
    pub row: f32,
}

#[derive(Clone, PartialEq, Default)]
struct Text {
    lines: Vec<(Vec<u16>, f32)>,
    size: f32,
    light: f32,
    locale: Vec<u16>,
    right_to_left: bool,
}

/// cvr::render::Caption.
#[derive(Default)]
pub struct Caption {
    text: Text,
    dirty: bool,
    failed: bool,
    width: u32,
    height: u32,
    levels: u32,
    top: f32,
    bottom: f32,
    picture: Option<ID3D12Resource>,
    upload: Option<ID3D12Resource>,
    retired: Vec<ID3D12Resource>,
}

fn decode(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        cmath::pow((v + 0.055) / 1.055, 2.4)
    }
}
fn encode(v: f32) -> u8 {
    let v = cmath::clamp(v, 0.0, 1.0);
    let s = if v <= 0.0031308 { v * 12.92 } else { 1.055 * cmath::pow(v, 1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as i64 as u8
}
/// The level below: each texel the mean of the 2 x 2 above it (the edge's own where the level above is odd).
fn halve(above: &[f32], w: u32, h: u32) -> (Vec<f32>, u32, u32) {
    let half_w = 1u32.max(w / 2);
    let half_h = 1u32.max(h / 2);
    let mut out = vec![0.0f32; half_w as usize * half_h as usize];
    for y in 0..half_h {
        let (y0, y1) = ((2 * y).min(h - 1), (2 * y + 1).min(h - 1));
        for x in 0..half_w {
            let (x0, x1) = ((2 * x).min(w - 1), (2 * x + 1).min(w - 1));
            let at = |x: u32, y: u32| above[y as usize * w as usize + x as usize];
            out[y as usize * half_w as usize + x as usize] = 0.25 * (at(x0, y0) + at(x1, y0) + at(x0, y1) + at(x1, y1));
        }
    }
    (out, half_w, half_h)
}

impl Caption {
    /// Set: the lines to show; a new text is drawn anew.
    fn set(&mut self, text: Text) {
        if self.picture.is_some() && text == self.text {
            return;
        }
        self.text = text;
        self.dirty = true;
        self.failed = false;
    }

    /// The lines set into rows of linear light, the picture's top level.
    unsafe fn typeset(&mut self) -> Option<Vec<f32>> {
        let t = &self.text;
        let first_line = t.lines.first()?;
        let (mut first, mut last) = (first_line.1, first_line.1);
        for (_, row) in &t.lines {
            first = cmath::min(first, *row);
            last = cmath::max(last, *row);
        }
        self.top = first - t.size;
        self.bottom = last + t.size;
        self.width = (SCREEN_HEIGHT * 16.0 / 9.0).round() as u32;
        self.height = (self.bottom - self.top).ceil() as u32;
        let (width, height, top) = (self.width, self.height, self.top);

        let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
        let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
        let mut locale = t.locale.clone();
        locale.push(0);
        let format = write
            .CreateTextFormat(
                w!("Bahnschrift"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                t.size,
                PCWSTR(locale.as_ptr()),
            )
            .ok()?;
        let _ = format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
        let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        if t.right_to_left {
            let _ = format.SetReadingDirection(DWRITE_READING_DIRECTION_RIGHT_TO_LEFT);
        }

        // White on black into a top-down 32-bit DIB: each texel's light is how much of the letters cover it.
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: core::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = core::ptr::null_mut();
        let dc = CreateCompatibleDC(None);
        let bitmap = if dc.is_invalid() { None } else { CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).ok() };
        let mut light = None;
        if let (Some(bitmap), false) = (bitmap.filter(|_| !bits.is_null()), bits.is_null()) {
            let was = SelectObject(dc, bitmap.into());
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let whole = RECT { left: 0, top: 0, right: width as i32, bottom: height as i32 };
            let mut ok = false;
            if let Ok(target) = d2d.CreateDCRenderTarget(&props) {
                if target.BindDC(dc, &whole).is_ok() {
                    if let Ok(white) = target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None) {
                        target.BeginDraw();
                        target.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }));
                        target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
                        for (text, row) in &t.lines {
                            let Ok(layout) = write.CreateTextLayout(text, &format, width as f32, 2.0 * t.size) else {
                                continue;
                            };
                            target.DrawTextLayout(windows_numerics::Vector2 { X: 0.0, Y: row - top - t.size }, &layout, &white, D2D1_DRAW_TEXT_OPTIONS_NONE);
                        }
                        ok = target.EndDraw(None, None).is_ok();
                    }
                }
            }
            let _ = GdiFlush();
            if ok {
                let mut table = [0.0f32; 256];
                for (i, v) in table.iter_mut().enumerate() {
                    *v = decode(i as f32 / 255.0) * t.light;
                }
                let count = width as usize * height as usize;
                let texels = core::slice::from_raw_parts(bits as *const u8, count * 4);
                light = Some((0..count).map(|i| table[texels[i * 4 + 2] as usize]).collect::<Vec<f32>>());
            }
            SelectObject(dc, was);
        }
        if let Some(bitmap) = bitmap {
            let _ = DeleteObject(bitmap.into());
        }
        if !dc.is_invalid() {
            let _ = DeleteDC(dc);
        }
        light
    }

    /// Picture: the lines' picture, made and recorded to be uploaded on `list` the first time; None while there is none.
    /// A shader resource once returned.
    unsafe fn picture(&mut self, device: Option<&ID3D12Device>, list: Option<&ID3D12GraphicsCommandList>) -> Option<ID3D12Resource> {
        if !self.dirty {
            return self.picture.clone();
        }
        if self.failed || self.text.lines.is_empty() {
            return None;
        }
        let (device, list) = (device?, list?);
        let started = GetTickCount64();
        let Some(top) = self.typeset() else {
            self.failed = true;
            log!("[caption] the lines were not set: no caption\n");
            return None;
        };
        if let Some(p) = self.picture.take() {
            self.retired.push(p);
        }
        if let Some(u) = self.upload.take() {
            self.retired.push(u);
        }
        self.levels = 1;
        while self.width.max(self.height) >> self.levels != 0 {
            self.levels += 1;
        }
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: self.width as u64,
            Height: self.height,
            DepthOrArraySize: 1,
            MipLevels: self.levels as u16,
            Format: FORMAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            ..Default::default()
        };
        let mut footprints = vec![D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default(); self.levels as usize];
        let mut total = 0u64;
        device.GetCopyableFootprints(&desc, 0, self.levels, 0, Some(footprints.as_mut_ptr()), None, None, Some(&mut total));
        let buffer = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: total,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let gpu_heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
        let upload_heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_UPLOAD, ..Default::default() };
        let mut picture: Option<ID3D12Resource> = None;
        let mut upload: Option<ID3D12Resource> = None;
        let mut mapped: *mut c_void = core::ptr::null_mut();
        let made = device
            .CreateCommittedResource(&gpu_heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut picture)
            .is_ok()
            && device
                .CreateCommittedResource(&upload_heap, D3D12_HEAP_FLAG_NONE, &buffer, D3D12_RESOURCE_STATE_GENERIC_READ, None, &mut upload)
                .is_ok()
            && upload.as_ref().is_some_and(|u| u.Map(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }), Some(&mut mapped)).is_ok());
        let (Some(picture), Some(upload), true) = (picture, upload, made) else {
            self.failed = true;
            log!("[caption] picture {}x{} not made\n", self.width, self.height);
            return None;
        };
        let _ = picture.SetName(w!("CVR_caption"));
        // Each level into its place in the upload buffer, sRGB, then copied into the picture.
        let mut level = top;
        let (mut w, mut h) = (self.width, self.height);
        for mip in 0..self.levels {
            let at = footprints[mip as usize];
            let rows = (mapped as *mut u8).add(at.Offset as usize);
            for y in 0..h {
                let row = rows.add(y as usize * at.Footprint.RowPitch as usize);
                for x in 0..w {
                    let v = encode(level[y as usize * w as usize + x as usize]);
                    let texel = row.add(x as usize * 4);
                    *texel = v;
                    *texel.add(1) = v;
                    *texel.add(2) = v;
                    *texel.add(3) = 255;
                }
            }
            let from = D3D12_TEXTURE_COPY_LOCATION {
                pResource: core::mem::transmute_copy(&upload),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: at },
            };
            let to = D3D12_TEXTURE_COPY_LOCATION {
                pResource: core::mem::transmute_copy(&picture),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: mip },
            };
            list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
            // The locations borrowed their resources without a reference of their own.
            core::mem::forget(from);
            core::mem::forget(to);
            if mip + 1 < self.levels {
                let (next, half_w, half_h) = halve(&level, w, h);
                level = next;
                w = half_w;
                h = half_h;
            }
        }
        upload.Unmap(0, None);
        let barrier = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                    pResource: core::mem::transmute_copy(&picture),
                    Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    StateBefore: D3D12_RESOURCE_STATE_COPY_DEST,
                    StateAfter: D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                }),
            },
        };
        list.ResourceBarrier(&[barrier]);
        self.dirty = false;
        log!(
            "[caption] {} line(s) set: {}x{}, {} levels, {} ms\n",
            self.text.lines.len(),
            self.width,
            self.height,
            self.levels,
            GetTickCount64() - started
        );
        self.picture = Some(picture.clone());
        self.upload = Some(upload);
        Some(picture)
    }

    /// Shutdown: the pictures released; a text still set is drawn again at the next Picture.
    fn shutdown(&mut self) {
        self.picture = None;
        self.upload = None;
        self.retired.clear();
        self.dirty = !self.text.lines.is_empty();
        self.failed = false;
    }
}

// ---- the C ABI (include/Render/Caption.hpp)

#[no_mangle]
pub extern "C" fn cvr_gpu_caption_new() -> *mut Caption {
    Box::into_raw(Box::default())
}
/// # Safety
/// `caption` came from cvr_gpu_caption_new (freed once) or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_caption_free(caption: *mut Caption) {
    if !caption.is_null() {
        drop(Box::from_raw(caption));
    }
}
/// # Safety
/// `caption` is live and used by one thread; `lines` holds `count` lines whose texts hold `length` UTF-16 units each;
/// `locale` is NUL-terminated or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_caption_set(
    caption: *mut Caption,
    lines: *const CaptionLine,
    count: u32,
    size: f32,
    light: f32,
    locale: *const u16,
    right_to_left: bool,
) {
    let Some(c) = caption.as_mut() else { return };
    let lines = if lines.is_null() { &[][..] } else { core::slice::from_raw_parts(lines, count as usize) };
    let text = Text {
        lines: lines
            .iter()
            .map(|l| (if l.text.is_null() { Vec::new() } else { core::slice::from_raw_parts(l.text, l.length as usize).to_vec() }, l.row))
            .collect(),
        size,
        light,
        locale: if locale.is_null() { Vec::new() } else { PCWSTR(locale).as_wide().to_vec() },
        right_to_left,
    };
    c.set(text);
}
/// # Safety
/// `caption` is live and used by one thread; `device` and `list` are the caller's or null. The picture is borrowed: the
/// caption keeps its reference.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_caption_picture(caption: *mut Caption, device: *mut c_void, list: *mut c_void) -> *mut c_void {
    let Some(c) = caption.as_mut() else { return core::ptr::null_mut() };
    match c.picture(ID3D12Device::from_raw_borrowed(&device), ID3D12GraphicsCommandList::from_raw_borrowed(&list)) {
        Some(p) => p.as_raw(),
        None => core::ptr::null_mut(),
    }
}
/// # Safety
/// `caption` is live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_caption_extent(caption: *const Caption, out: *mut [f32; 3]) {
    if let (Some(c), Some(out)) = (caption.as_ref(), out.as_mut()) {
        *out = [c.top, c.bottom, c.width as f32];
    }
}
/// # Safety
/// `caption` is live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_caption_shutdown(caption: *mut Caption) {
    if let Some(c) = caption.as_mut() {
        c.shutdown();
    }
}
