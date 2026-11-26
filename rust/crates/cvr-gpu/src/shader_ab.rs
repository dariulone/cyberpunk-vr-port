//! CVR_SHADER_AB (a development switch): a pass's shaders made from Rust (rust/shaders -> dxil.rs) against the HLSL
//! they replace, in the game's own draws (stage 8 of docs/rust-migration-plan.md: "A/B in both eyes"). A take is one
//! draw of the game -- its constants, its picture, its eye -- drawn again twice, into two scratch targets of the draw's
//! format and size cleared alike: by the reference pipeline (the HLSL, compiled by FXC as before) and by the
//! replacement (the DXIL); both copied to readback buffers. Two seconds later -- the list ran long since -- the bytes
//! are compared and the result logged ("[shader-ab]"). Per key (a pass's pipeline, an eye, a format) the draws
//! numbered in AT are taken -- the first of a pass is often a fade's first, all clear; at most MAX_TAKES in all.
#![allow(unsafe_op_in_unsafe_fn)]
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use cvr_host::log;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// The switch.
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("CVR_SHADER_AB").is_some())
}

/// A pass's pipelines times the eyes and the formats it draws (space_quad: 5 x 2 x 2), times AT.
const MAX_TAKES: usize = 72;
/// The draws of a key taken: its first, and two later in the same picture's life.
const AT: [u32; 3] = [1, 90, 400];
const WAIT: Duration = Duration::from_secs(2);
/// What both targets start as (the blend's destination): not black, not opaque.
const CLEAR: [f32; 4] = [0.25, 0.5, 0.75, 0.5];

struct Take {
    key: String,
    rows: u32,
    row_bytes: usize,
    /// The readback buffer's size: the last row is only row_bytes long.
    total: usize,
    pitch: usize,
    pixel_bytes: usize,
    buffers: [ID3D12Resource; 2],
    // the scratch targets the list draws into, kept until it has run
    _targets: [ID3D12Resource; 2],
}

#[derive(Default)]
pub struct ShaderAb {
    heap: Option<ID3D12DescriptorHeap>,
    stride: usize,
    /// Each key's draws so far.
    draws: BTreeMap<String, u32>,
    taken: usize,
}

fn transition(resource: &ID3D12Resource, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                pResource: unsafe { core::mem::transmute_copy(resource) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

/// Bytes a pixel of the formats the passes draw into (the comparison is bytewise; this only counts pixels).
fn pixel_bytes(format: DXGI_FORMAT) -> usize {
    match format {
        DXGI_FORMAT_R16G16B16A16_FLOAT | DXGI_FORMAT_R16G16B16A16_UNORM => 8,
        DXGI_FORMAT_R32G32B32A32_FLOAT => 16,
        _ => 4,
    }
}

impl ShaderAb {
    /// Whether this draw of `key` is one to take (each call counts a draw of the key).
    pub fn wants(&mut self, key: &str) -> bool {
        if !enabled() || self.taken >= MAX_TAKES {
            return false;
        }
        let n = self.draws.entry(key.to_string()).or_insert(0);
        *n += 1;
        AT.contains(n)
    }

    /// The take for `key`: `draw(replacement)` called with each scratch target bound (viewport and scissor its whole
    /// size), the reference first; the root signature, its tables and topology are the caller's, already set. The
    /// caller's render target, viewport and scissor are not restored here.
    ///
    /// # Safety
    /// `list` records on `device`; `draw` records only state the take's targets can take.
    pub unsafe fn take(
        &mut self,
        device: &ID3D12Device,
        list: &ID3D12GraphicsCommandList,
        key: &str,
        format: DXGI_FORMAT,
        width: u32,
        height: u32,
        mut draw: impl FnMut(bool),
    ) -> bool {
        if width == 0 || height == 0 || self.taken >= MAX_TAKES {
            return false;
        }
        let draw_number = self.draws.get(key).copied().unwrap_or(0);
        let key = &format!("{key} draw {draw_number}");
        self.taken += 1;
        if self.heap.is_none() {
            let desc = D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV, NumDescriptors: 2 * MAX_TAKES as u32, ..Default::default() };
            self.heap = device.CreateDescriptorHeap(&desc).ok();
            self.stride = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) as usize;
        }
        let Some(heap) = &self.heap else { return false };
        let texture = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
            ..Default::default()
        };
        let mut footprint = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
        let (mut rows, mut row_bytes, mut total) = (0u32, 0u64, 0u64);
        device.GetCopyableFootprints(&texture, 0, 1, 0, Some(&mut footprint), Some(&mut rows), Some(&mut row_bytes), Some(&mut total));
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
        let gpu = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
        let readback = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_READBACK, ..Default::default() };
        let clear = D3D12_CLEAR_VALUE { Format: format, Anonymous: D3D12_CLEAR_VALUE_0 { Color: CLEAR } };
        let mut made: Vec<ID3D12Resource> = Vec::new();
        for _ in 0..2 {
            let mut t: Option<ID3D12Resource> = None;
            if device.CreateCommittedResource(&gpu, D3D12_HEAP_FLAG_NONE, &texture, D3D12_RESOURCE_STATE_RENDER_TARGET, Some(&clear), &mut t).is_err() {
                log!("[shader-ab] {key}: scratch target {width}x{height} format {} not made\n", format.0);
                return false;
            }
            made.extend(t);
        }
        let mut buffers: Vec<ID3D12Resource> = Vec::new();
        for _ in 0..2 {
            let mut b: Option<ID3D12Resource> = None;
            if device.CreateCommittedResource(&readback, D3D12_HEAP_FLAG_NONE, &buffer, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut b).is_err() {
                log!("[shader-ab] {key}: readback {total} bytes not made\n");
                return false;
            }
            buffers.extend(b);
        }
        let slot = ((self.taken - 1) % MAX_TAKES) * 2;
        let full = RECT { left: 0, top: 0, right: width as i32, bottom: height as i32 };
        let viewport = D3D12_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: width as f32, Height: height as f32, MinDepth: 0.0, MaxDepth: 1.0 };
        for (i, target) in made.iter().enumerate() {
            let mut rtv = heap.GetCPUDescriptorHandleForHeapStart();
            rtv.ptr += (slot + i) * self.stride;
            device.CreateRenderTargetView(target, None, rtv);
            list.ClearRenderTargetView(rtv, &CLEAR, None);
            list.OMSetRenderTargets(1, Some(&rtv), false, None);
            list.RSSetViewports(&[viewport]);
            list.RSSetScissorRects(&[full]);
            draw(i == 1);
        }
        let barriers = [
            transition(&made[0], D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_SOURCE),
            transition(&made[1], D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_SOURCE),
        ];
        list.ResourceBarrier(&barriers);
        for (target, buf) in made.iter().zip(&buffers) {
            let to = D3D12_TEXTURE_COPY_LOCATION {
                pResource: core::mem::transmute_copy(buf),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: footprint },
            };
            let from = D3D12_TEXTURE_COPY_LOCATION {
                pResource: core::mem::transmute_copy(target),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
            };
            list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
            // the locations borrowed their resources without a reference of their own
            core::mem::forget(to);
            core::mem::forget(from);
        }
        let [a, b] = <[ID3D12Resource; 2]>::try_from(made).ok().unwrap_or_else(|| unreachable!());
        let [ra, rb] = <[ID3D12Resource; 2]>::try_from(buffers).ok().unwrap_or_else(|| unreachable!());
        compare_later(Take {
            key: key.to_string(),
            rows,
            row_bytes: row_bytes as usize,
            total: total as usize,
            pitch: footprint.Footprint.RowPitch as usize,
            pixel_bytes: pixel_bytes(format),
            buffers: [ra, rb],
            _targets: [a, b],
        });
        true
    }

    /// Nothing to do: each take compares itself on a thread of its own (a pass may draw no more after it).
    pub fn poll(&mut self) {}
}

/// The takes made and compared, and the identical ones.
static MADE: AtomicUsize = AtomicUsize::new(0);
static COMPARED: AtomicUsize = AtomicUsize::new(0);
static IDENTICAL: AtomicUsize = AtomicUsize::new(0);

/// A take handed to its comparing thread (Direct3D 12's resources are free-threaded).
struct Sent(Take);
// SAFETY: the readback buffers are mapped and read on that thread only; the targets are only released there.
unsafe impl Send for Sent {}

/// `take` compared WAIT later, on a thread of its own, and logged.
fn compare_later(take: Take) {
    MADE.fetch_add(1, Ordering::Relaxed);
    let sent = Sent(take);
    let _ = std::thread::Builder::new().name("cvr-shader-ab".into()).spawn(move || {
        let sent = sent;
        let take = &sent.0;
        std::thread::sleep(WAIT);
        // SAFETY: readback buffers the GPU wrote long since; read only within their size.
        let result = unsafe { compare(take) };
        let n = COMPARED.fetch_add(1, Ordering::Relaxed) + 1;
        match result {
            Some(r) if r.blank => log!(
                "[shader-ab] {}: NOT DRAWN -- both readbacks are zero (the list did not run in {} s)\n",
                take.key,
                WAIT.as_secs()
            ),
            Some(r) => {
                let same = r.differing == 0;
                if same {
                    IDENTICAL.fetch_add(1, Ordering::Relaxed);
                }
                log!(
                    "[shader-ab] {}: {} pixels, {} distinct in the reference, {} (reference HLSL/FXC vs replacement Rust/DXIL)\n",
                    take.key,
                    r.pixels,
                    if r.distinct >= DISTINCT_CAP { format!("{DISTINCT_CAP}+") } else { r.distinct.to_string() },
                    if same { "identical".to_string() } else { format!("{} differ, by at most {} in a byte", r.differing, r.max) }
                );
            }
            None => log!("[shader-ab] {}: the readback could not be mapped\n", take.key),
        }
        let made = MADE.load(Ordering::Relaxed);
        if n == made {
            log!("[shader-ab] {n} of {made} take(s) compared, {} identical\n", IDENTICAL.load(Ordering::Relaxed));
        }
    });
}

const DISTINCT_CAP: usize = 4096;
struct Compared {
    pixels: usize,
    differing: usize,
    max: u8,
    distinct: usize,
    blank: bool,
}

unsafe fn compare(take: &Take) -> Option<Compared> {
    let range = D3D12_RANGE { Begin: 0, End: take.total };
    let mut pa = core::ptr::null_mut();
    let mut pb = core::ptr::null_mut();
    take.buffers[0].Map(0, Some(&range), Some(&mut pa)).ok()?;
    if take.buffers[1].Map(0, Some(&range), Some(&mut pb)).is_err() {
        take.buffers[0].Unmap(0, None);
        return None;
    }
    let (mut pixels, mut differing, mut max) = (0usize, 0usize, 0u8);
    let mut nonzero = false;
    let mut distinct: BTreeSet<Vec<u8>> = BTreeSet::new();
    for row in 0..take.rows as usize {
        let a = core::slice::from_raw_parts((pa as *const u8).add(row * take.pitch), take.row_bytes);
        let b = core::slice::from_raw_parts((pb as *const u8).add(row * take.pitch), take.row_bytes);
        nonzero |= a.iter().chain(b).any(|&x| x != 0);
        for (pa, pb) in a.chunks(take.pixel_bytes).zip(b.chunks(take.pixel_bytes)) {
            pixels += 1;
            if distinct.len() < DISTINCT_CAP {
                distinct.insert(pa.to_vec());
            }
            if pa != pb {
                differing += 1;
                max = max.max(pa.iter().zip(pb).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0));
            }
        }
    }
    let none = D3D12_RANGE { Begin: 0, End: 0 };
    take.buffers[0].Unmap(0, Some(&none));
    take.buffers[1].Unmap(0, Some(&none));
    Some(Compared { pixels, differing, max, distinct: distinct.len(), blank: !nonzero })
}
