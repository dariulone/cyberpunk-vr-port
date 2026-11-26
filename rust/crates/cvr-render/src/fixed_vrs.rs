//! Fixed foveated variable-rate shading on the GPU (formerly src/Render/FixedVrs.cpp; the policy is `vrs`): per
//! command list, the state the engine set (rate, combiners, image, pipeline, viewport, targets) is tracked, and a
//! scene draw (RenderElements, one eye per target) of an eligible pipeline gets the port's rate map bound in place of
//! the engine's image; any other draw gets the engine's own state back. The maps are built on the present queue, one
//! per view size and setting, and bound only once their upload has completed.
//!
//! The C++ half keeps the exports (`CyberpunkVR_FixedVrsReport*`), the pipeline-stream reader and the engine-stereo
//! query, attached here at its static initialisation.
use cvr_host::fast::FastMap;
use core::cell::Cell;
use core::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

use windows::core::{Interface, GUID};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R8_UINT, DXGI_SAMPLE_DESC};
use windows::Win32::System::SystemInformation::GetTickCount64;

use crate::vrs::{build_rates, eligible_pipeline, normalize, rate_count, Settings};

const PIPELINE_KEY: GUID = GUID::from_values(0x5a4ac73c, 0x80da, 0x4983, [0xb5, 0x12, 0x1f, 0x54, 0xa9, 0xa0, 0xbe, 0x47]);
/// RenderElements (game 2.5): the only node whose draws are scene draws.
const SCENE_NODE: u32 = 0x33DDA4;

/// cvr::vrs::Status.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Status {
    pub checked: bool,
    pub supported: bool,
    pub additional_rates: bool,
    pub failed: bool,
    pub tile_size: u32,
    pub ready_maps: u32,
    pub draws: [u64; 2],
    pub scene_draws: [u64; 2],
    pub eligible_draws: [u64; 2],
}
const _: () = assert!(core::mem::size_of::<Status>() == 64);

pub type RateFn = unsafe extern "system" fn(*mut c_void, D3D12_SHADING_RATE, *const D3D12_SHADING_RATE_COMBINER);
pub type ImageFn = unsafe extern "system" fn(*mut c_void, *mut c_void);

const PASSTHROUGH: [D3D12_SHADING_RATE_COMBINER; 2] = [D3D12_SHADING_RATE_COMBINER_PASSTHROUGH, D3D12_SHADING_RATE_COMBINER_PASSTHROUGH];

/// One command list's recording state. Recording one list is externally serialised by D3D12's contract, so a state is
/// only touched by the thread recording its list.
struct State {
    known: bool,
    eligible: bool,
    depth: bool,
    targets: u32,
    expected_targets: u32,
    width: u32,
    height: u32,
    rate: D3D12_SHADING_RATE,
    combiners: [D3D12_SHADING_RATE_COMBINER; 2],
    image: *mut c_void,
    retained: *mut c_void,
    pipeline: *mut c_void,
    revision: u64,
    recording_epoch: u64,
    map_width: u32,
    map_height: u32,
    selected: Option<ID3D12Resource>,
    override_image: *mut c_void,
    rate_fn: Option<RateFn>,
    image_fn: Option<ImageFn>,
}
impl Default for State {
    fn default() -> State {
        State {
            known: false,
            eligible: false,
            depth: false,
            targets: 0,
            expected_targets: 0,
            width: 0,
            height: 0,
            rate: D3D12_SHADING_RATE_1X1,
            combiners: PASSTHROUGH,
            image: core::ptr::null_mut(),
            retained: core::ptr::null_mut(),
            pipeline: core::ptr::null_mut(),
            revision: 0,
            recording_epoch: 0,
            map_width: 0,
            map_height: 0,
            selected: None,
            override_image: core::ptr::null_mut(),
            rate_fn: None,
            image_fn: None,
        }
    }
}

struct Map {
    settings: Settings,
    width: u32,
    height: u32,
    image: Option<ID3D12Resource>,
    upload: Option<ID3D12Resource>,
    allocator: Option<ID3D12CommandAllocator>,
    list: Option<ID3D12GraphicsCommandList>,
    completion: u64,
    last_used: u64,
    ready: bool,
}

struct Global {
    requested: Settings,
    applied: Settings,
    apply_after: u64,
    fence_value: u64,
    use_serial: u64,
    last_report: u64,
    status: Status,
    device: Option<ID3D12Device>,
    fence: Option<ID3D12Fence>,
    maps: Vec<Map>,
    requests: Vec<(u32, u32)>,
}
// SAFETY: free-threaded D3D12 objects, used under the global mutex.
unsafe impl Send for Global {}

static GLOBAL: Mutex<Option<Global>> = Mutex::new(None);
fn with_global<R>(f: impl FnOnce(&mut Global) -> R) -> R {
    let mut guard = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    f(guard.get_or_insert_with(|| Global {
        requested: Settings::default(),
        applied: Settings::default(),
        apply_after: 0,
        fence_value: 0,
        use_serial: 0,
        last_report: 0,
        status: Status::default(),
        device: None,
        fence: None,
        maps: Vec::new(),
        requests: Vec::new(),
    }))
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static EVER_ENABLED: AtomicBool = AtomicBool::new(false);
static REVISION: AtomicU64 = AtomicU64::new(1);
static RECORDING_EPOCH: AtomicU64 = AtomicU64::new(1);
static DRAWS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static SCENE_DRAWS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static ELIGIBLE_DRAWS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

/// The registry of list states: list address -> its state, leaked so it never moves (entries are only reset).
static STATES: Mutex<Option<FastMap<usize, usize>>> = Mutex::new(None);
thread_local! {
    static CACHED: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}
fn find_state(list: *mut c_void, create: bool) -> Option<&'static mut State> {
    let key = list as usize;
    let (previous, cached) = CACHED.get();
    if previous == key && cached != 0 {
        // SAFETY: a leaked state of this list, touched only by the thread recording it.
        return Some(unsafe { &mut *(cached as *mut State) });
    }
    let found = {
        let mut guard = STATES.lock().unwrap_or_else(|p| p.into_inner());
        let states = guard.get_or_insert_with(cvr_host::fast::fast_map);
        match states.get(&key) {
            Some(&state) => state,
            None if create && states.len() < 512 => {
                let state = Box::into_raw(Box::<State>::default()) as usize;
                states.insert(key, state);
                state
            }
            None => 0,
        }
    };
    CACHED.set((key, found));
    // SAFETY: as above.
    (found != 0).then(|| unsafe { &mut *(found as *mut State) })
}

/// What the C++ half attaches.
#[repr(C)]
pub struct Attach {
    pub engine_stereo_enabled: extern "C" fn() -> bool,
    pub report_seq: *const AtomicU32,
    pub report: *mut u8,
    pub report_bytes: usize,
}
static ATTACHED: AtomicPtr<Attach> = AtomicPtr::new(core::ptr::null_mut());
fn attach() -> Option<&'static Attach> {
    // SAFETY: a table the C++ half attached for the process's lifetime.
    unsafe { ATTACHED.load(Ordering::Acquire).as_ref() }
}
/// # Safety
/// `table` and everything it points to live for the rest of the process.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_vrs_attach(table: *const Attach) {
    ATTACHED.store(table.cast_mut(), Ordering::Release);
}

fn internal() -> bool {
    cvr_host::probe::internal()
}

fn restore_state(list: *mut c_void, s: &mut State) {
    if s.override_image.is_null() {
        return;
    }
    if let (Some(image_fn), Some(rate_fn)) = (s.image_fn, s.rate_fn) {
        // SAFETY: the engine's own setters, with the state it set.
        unsafe {
            image_fn(list, s.image);
            rate_fn(list, s.rate, s.combiners.as_ptr());
        }
    }
    s.override_image = core::ptr::null_mut();
}
fn edit(list: *mut c_void, f: impl FnOnce(&mut State)) {
    if internal() || !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if let Some(s) = find_state(list, false) {
        if s.known && s.recording_epoch == RECORDING_EPOCH.load(Ordering::Acquire) {
            f(s);
        }
    }
}
fn set_pipeline(s: &mut State, pso: *mut c_void) {
    if s.pipeline == pso {
        return;
    }
    s.pipeline = pso;
    let (mut count, mut size) = (0u32, 4u32);
    // SAFETY: the engine's live pipeline (or null); at most `size` bytes written.
    s.eligible = unsafe { ID3D12PipelineState::from_raw_borrowed(&pso) }.is_some_and(|p| {
        unsafe { p.GetPrivateData(&PIPELINE_KEY, &mut size, Some(&mut count as *mut u32 as *mut c_void)) }.is_ok() && size == 4 && (3..=4).contains(&count)
    });
    s.expected_targets = if s.eligible { count } else { 0 };
}

fn create_map(g: &mut Global, map: &mut Map, queue: &ID3D12CommandQueue) -> bool {
    let (tile, additional) = (g.status.tile_size, g.status.additional_rates);
    let mut bytes = vec![0u8; rate_count(map.width, map.height, tile)];
    if bytes.is_empty() {
        return false;
    }
    build_rates(map.settings, map.width, map.height, tile, additional, &mut bytes);
    let (Some(device), Some(fence)) = (g.device.clone(), g.fence.clone()) else {
        return false;
    };
    let d = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: map.width.div_ceil(tile) as u64,
        Height: map.height.div_ceil(tile),
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_R8_UINT,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        ..Default::default()
    };
    // SAFETY (whole body): the live device and queue; descriptions on the stack outliving the calls that read them.
    unsafe {
        let mut heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };
        let mut image: Option<ID3D12Resource> = None;
        if device
            .CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &d, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut image)
            .is_err()
        {
            return false;
        }
        let Some(image) = image else { return false };
        let _ = image.SetName(windows::core::w!("CVR_fixed_foveated_shading_rate"));
        map.image = Some(image.clone());
        let mut fp = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
        let mut size = 0u64;
        device.GetCopyableFootprints(&d, 0, 1, 0, Some(&mut fp), None, None, Some(&mut size));
        let buffer = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: size,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        heap.Type = D3D12_HEAP_TYPE_UPLOAD;
        let mut upload: Option<ID3D12Resource> = None;
        if device
            .CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &buffer, D3D12_RESOURCE_STATE_GENERIC_READ, None, &mut upload)
            .is_err()
        {
            return false;
        }
        let Some(upload) = upload else { return false };
        map.upload = Some(upload.clone());
        let mut data = core::ptr::null_mut();
        let no_read = D3D12_RANGE { Begin: 0, End: 0 };
        if upload.Map(0, Some(&no_read), Some(&mut data)).is_err() || data.is_null() {
            return false;
        }
        core::ptr::write_bytes(data as *mut u8, 0, size as usize);
        let row = d.Width as usize;
        for y in 0..d.Height as usize {
            let at = (data as *mut u8).add(fp.Offset as usize + y * fp.Footprint.RowPitch as usize);
            core::ptr::copy_nonoverlapping(bytes.as_ptr().add(y * row), at, row);
        }
        upload.Unmap(0, None);
        let Ok(allocator) = device.CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT) else {
            return false;
        };
        let Ok(list) = device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None::<&ID3D12PipelineState>)
        else {
            return false;
        };
        map.allocator = Some(allocator.clone());
        map.list = Some(list.clone());
        let from = D3D12_TEXTURE_COPY_LOCATION {
            pResource: core::mem::transmute_copy(&upload),
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: fp },
        };
        let to = D3D12_TEXTURE_COPY_LOCATION {
            pResource: core::mem::transmute_copy(&image),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
        };
        list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
        let barrier = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                    pResource: core::mem::transmute_copy(&image),
                    Subresource: 0,
                    StateBefore: D3D12_RESOURCE_STATE_COPY_DEST,
                    StateAfter: D3D12_RESOURCE_STATE_SHADING_RATE_SOURCE,
                }),
            },
        };
        list.ResourceBarrier(&[barrier]);
        if list.Close().is_err() || !cvr_gpu::command_resources::keep(&list, &[image.as_raw(), upload.as_raw(), allocator.as_raw()]) {
            return false;
        }
        let lists = [list.as_raw()];
        let resources = cvr_gpu::command_resources::prepare(&lists);
        queue.ExecuteCommandLists(&[Some((*list).clone())]);
        cvr_gpu::command_resources::submit(Some(queue), resources);
        g.fence_value += 1;
        map.completion = g.fence_value;
        queue.Signal(&fence, map.completion).is_ok()
    }
}

/// GetSettings.
pub fn get_settings() -> Settings {
    with_global(|g| g.requested)
}
/// SetSettings: the request applies a second after its last change; off applies at once. The rate map is laid out for
/// one eye per target; engine stereo has both in one, so the setting is kept but stays off while it is on.
pub fn set_settings(s: Settings) {
    let s = normalize(s);
    let stereo = attach().is_some_and(|t| (t.engine_stereo_enabled)());
    with_global(|g| {
        let on = s.mode != 0 && !stereo;
        if ENABLED.load(Ordering::Relaxed) != on {
            RECORDING_EPOCH.fetch_add(1, Ordering::Release);
        }
        ENABLED.store(on, Ordering::Relaxed);
        if on {
            EVER_ENABLED.store(true, Ordering::Relaxed);
        }
        if s == g.requested {
            return;
        }
        g.requested = s;
        // SAFETY: a plain clock read.
        g.apply_after = unsafe { GetTickCount64() } + 1000;
        if s.mode == 0 {
            g.applied = s;
            REVISION.fetch_add(1, Ordering::Release);
        }
    });
}
/// GetStatus.
pub fn get_status() -> Status {
    let mut value = with_global(|g| g.status);
    for i in 0..2 {
        value.draws[i] = DRAWS[i].load(Ordering::SeqCst);
        value.scene_draws[i] = SCENE_DRAWS[i].load(Ordering::SeqCst);
        value.eligible_draws[i] = ELIGIBLE_DRAWS[i].load(Ordering::SeqCst);
    }
    value
}
/// GraphicsCreated: the pipeline's eligible target count, kept in its private data (0: not eligible).
pub fn graphics_created(pso: &ID3D12PipelineState, d: &D3D12_GRAPHICS_PIPELINE_STATE_DESC) {
    if internal() {
        return;
    }
    let count: u32 = if eligible_pipeline(d) { d.NumRenderTargets } else { 0 };
    // SAFETY: four bytes copied by the runtime.
    let _ = unsafe { pso.SetPrivateData(&PIPELINE_KEY, 4, Some(&count as *const u32 as *const c_void)) };
}
/// Reset: a list starts recording (`known`: its engine-side rate state is known).
pub fn reset(list: *mut c_void, known: bool, pso: *mut c_void) {
    if internal() || !EVER_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    // SAFETY: the engine's live list (or null).
    let direct = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }.is_some_and(|l| unsafe { l.GetType() } == D3D12_COMMAND_LIST_TYPE_DIRECT);
    let active = ENABLED.load(Ordering::Relaxed) && known && direct;
    if let Some(s) = find_state(list, active) {
        *s = State {
            known: active,
            recording_epoch: RECORDING_EPOCH.load(Ordering::Acquire),
            ..State::default()
        };
        if active {
            set_pipeline(s, pso);
        }
    }
}
/// Invalidate: the list's state is unknown from here (a bundle ran).
pub fn invalidate(list: *mut c_void) {
    if internal() || !EVER_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if let Some(s) = find_state(list, false) {
        *s = State::default();
    }
}
pub fn pipeline(list: *mut c_void, pso: *mut c_void) {
    edit(list, |s| set_pipeline(s, pso));
}
/// Viewports: `one` is the list's viewport when exactly one was set (else the scene size is unknown).
pub fn viewports(list: *mut c_void, one: Option<&D3D12_VIEWPORT>) {
    edit(list, |s| {
        s.width = 0;
        s.height = 0;
        let Some(v) = one else { return };
        let size = |x: f32| (64.0..=16384.0).contains(&x) && (x as u32) as f32 == x;
        if v.TopLeftX != 0.0 || v.TopLeftY != 0.0 || !size(v.Width) || !size(v.Height) {
            return;
        }
        s.width = v.Width as u32;
        s.height = v.Height as u32;
    });
}
pub fn targets(list: *mut c_void, count: u32, depth: bool) {
    edit(list, |s| {
        s.targets = count;
        s.depth = depth;
    });
}
pub fn shading_rate(list: *mut c_void, rate: D3D12_SHADING_RATE, combiners: Option<&[D3D12_SHADING_RATE_COMBINER; 2]>) {
    edit(list, |s| {
        s.rate = rate;
        s.combiners = combiners.copied().unwrap_or(PASSTHROUGH);
    });
}
pub fn shading_image(list: *mut c_void, image: *mut c_void) {
    edit(list, |s| s.image = image);
}

/// PrepareDraw: before a draw at `node` for `eye`, with the engine's own setters.
///
/// # Safety
/// `list` is the engine's live list being recorded on this thread; the setters are its own.
pub unsafe fn prepare_draw(list: *mut c_void, node: u32, eye: i32, set_rate: Option<RateFn>, set_image: Option<ImageFn>) {
    if internal() || !EVER_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let Some(s) = find_state(list, false) else { return };
    if !s.known {
        return;
    }
    let (Some(set_rate), Some(set_image)) = (set_rate, set_image) else {
        restore_state(list, s);
        return;
    };
    if !ENABLED.load(Ordering::Relaxed) || s.recording_epoch != RECORDING_EPOCH.load(Ordering::Acquire) || node != SCENE_NODE || !(0..=1).contains(&eye) {
        restore_state(list, s);
        return;
    }
    let eye = eye as usize;
    SCENE_DRAWS[eye].fetch_add(1, Ordering::Relaxed);
    if !s.eligible || !s.depth || s.targets != s.expected_targets || s.width == 0 || s.height == 0 {
        restore_state(list, s);
        return;
    }
    ELIGIBLE_DRAWS[eye].fetch_add(1, Ordering::Relaxed);
    let version = REVISION.load(Ordering::Acquire);
    if s.revision != version || s.map_width != s.width || s.map_height != s.height {
        with_global(|g| {
            s.selected = None;
            s.revision = version;
            s.map_width = s.width;
            s.map_height = s.height;
            if g.applied.mode != 0 && g.status.supported && !g.status.failed {
                let applied = g.applied;
                match g.maps.iter().position(|m| m.settings == applied && m.width == s.width && m.height == s.height) {
                    Some(i) => {
                        if g.maps[i].ready {
                            s.selected = g.maps[i].image.clone();
                            g.use_serial += 1;
                            g.maps[i].last_used = g.use_serial;
                        }
                    }
                    None => {
                        let size = (s.width, s.height);
                        if g.requests.len() < 8 && !g.requests.contains(&size) {
                            g.requests.push(size);
                        }
                    }
                }
            }
        });
    }
    let Some(image) = s.selected.clone() else {
        restore_state(list, s);
        return;
    };
    let raw = image.as_raw();
    if s.retained != raw {
        // SAFETY: the engine's live list.
        let kept = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }.is_some_and(|l| cvr_gpu::command_resources::keep(l, &[raw]));
        if !kept {
            restore_state(list, s);
            return;
        }
        s.retained = raw;
    }
    DRAWS[eye].fetch_add(1, Ordering::Relaxed);
    if s.override_image == raw {
        return;
    }
    restore_state(list, s);
    s.rate_fn = Some(set_rate);
    s.image_fn = Some(set_image);
    s.override_image = raw;
    let combine = [D3D12_SHADING_RATE_COMBINER_PASSTHROUGH, D3D12_SHADING_RATE_COMBINER_OVERRIDE];
    // SAFETY: the engine's own setters on its list, with the port's live (retained) image.
    unsafe {
        set_image(list, raw);
        set_rate(list, D3D12_SHADING_RATE_1X1, combine.as_ptr());
    }
}
/// Restore: the engine's own rate state back on the list.
///
/// # Safety
/// `list` is the engine's live list being recorded on this thread.
pub unsafe fn restore(list: *mut c_void) {
    if internal() || !EVER_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if let Some(s) = find_state(list, false) {
        restore_state(list, s);
    }
}

fn publish_report(t: &Attach, g: &Global) {
    let text = format!(
        "{{\"requested\":{},\"applied\":{},\"checked\":{},\"supported\":{},\"additional\":{},\"failed\":{},\"tile\":{},\"maps\":{},\"draws\":[{},{}],\"scene\":[{},{}],\"eligible\":[{},{}]}}",
        g.requested.mode,
        g.applied.mode,
        g.status.checked as i32,
        g.status.supported as i32,
        g.status.additional_rates as i32,
        g.status.failed as i32,
        g.status.tile_size,
        g.status.ready_maps,
        DRAWS[0].load(Ordering::SeqCst),
        DRAWS[1].load(Ordering::SeqCst),
        SCENE_DRAWS[0].load(Ordering::SeqCst),
        SCENE_DRAWS[1].load(Ordering::SeqCst),
        ELIGIBLE_DRAWS[0].load(Ordering::SeqCst),
        ELIGIBLE_DRAWS[1].load(Ordering::SeqCst)
    );
    if t.report.is_null() || t.report_bytes == 0 {
        return;
    }
    // SAFETY: the attached export (seqlock: odd while it is written).
    unsafe {
        let seq = &*t.report_seq;
        seq.fetch_add(1, Ordering::SeqCst);
        let n = text.len().min(t.report_bytes - 1);
        core::ptr::copy_nonoverlapping(text.as_ptr(), t.report, n);
        *t.report.add(n) = 0;
        seq.fetch_add(1, Ordering::SeqCst);
    }
}

/// Tick: once per presented frame on the present queue: the report, the device's support, settings applied, maps
/// finished, retired and built.
pub fn tick(current: &ID3D12Device, queue: &ID3D12CommandQueue) {
    let _internal = cvr_host::probe::Internal::new();
    with_global(|g| {
        // SAFETY: a plain clock read.
        let now = unsafe { GetTickCount64() };
        if now.wrapping_sub(g.last_report) >= 250 {
            g.last_report = now;
            if let Some(t) = attach() {
                publish_report(t, g);
            }
        }
        if !ENABLED.load(Ordering::Relaxed) {
            return;
        }
        if g.device.as_ref().is_some_and(|d| d.as_raw() != current.as_raw()) {
            g.status.failed = true;
            return;
        }
        if !g.status.checked {
            g.device = Some(current.clone());
            g.status.checked = true;
            let mut options = D3D12_FEATURE_DATA_D3D12_OPTIONS6::default();
            // SAFETY: the options struct and its size.
            let ok = unsafe {
                current.CheckFeatureSupport(
                    D3D12_FEATURE_D3D12_OPTIONS6,
                    &mut options as *mut _ as *mut c_void,
                    core::mem::size_of_val(&options) as u32,
                )
            }
            .is_ok();
            g.status.supported = ok && options.VariableShadingRateTier.0 >= D3D12_VARIABLE_SHADING_RATE_TIER_2.0 && options.ShadingRateImageTileSize > 0;
            g.status.tile_size = options.ShadingRateImageTileSize;
            g.status.additional_rates = options.AdditionalShadingRatesSupported.as_bool();
            if g.status.supported {
                // SAFETY: the live device.
                match unsafe { current.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) } {
                    Ok(f) => g.fence = Some(f),
                    Err(_) => g.status.failed = true,
                }
            }
            REVISION.fetch_add(1, Ordering::Release);
        }
        if !g.status.supported || g.status.failed {
            return;
        }
        // SAFETY: a plain clock read.
        if unsafe { GetTickCount64() } >= g.apply_after && g.applied != g.requested {
            g.applied = g.requested;
            g.requests.clear();
            REVISION.fetch_add(1, Ordering::Release);
        }
        // SAFETY: the port's live fence.
        let done = g.fence.as_ref().map_or(0, |f| unsafe { f.GetCompletedValue() });
        if done == u64::MAX {
            g.status.failed = true;
            return;
        }
        g.status.ready_maps = 0;
        let applied = g.applied;
        for map in &mut g.maps {
            if map.completion != 0 && map.completion <= done {
                if !map.ready {
                    REVISION.fetch_add(1, Ordering::Release);
                }
                map.ready = true;
                map.upload = None;
                map.allocator = None;
                map.list = None;
                if map.settings == applied {
                    g.status.ready_maps += 1;
                }
            }
        }
        g.maps.retain(|m| !(m.ready && m.settings != applied));
        if applied.mode == 0 {
            return;
        }
        let pending = core::mem::take(&mut g.requests);
        for (w, h) in pending {
            if g.maps.iter().any(|m| m.width == w && m.height == h && m.settings == applied) {
                continue;
            }
            if g.maps.len() >= 16 {
                let mut victim: Option<usize> = None;
                for (i, m) in g.maps.iter().enumerate() {
                    if m.ready && victim.is_none_or(|v| m.last_used < g.maps[v].last_used) {
                        victim = Some(i);
                    }
                }
                let Some(victim) = victim else { break };
                // Recorded lists and submissions retain any in-flight image.
                g.maps.remove(victim);
            }
            let mut map = Map {
                settings: applied,
                width: w,
                height: h,
                image: None,
                upload: None,
                allocator: None,
                list: None,
                completion: 0,
                last_used: 0,
                ready: false,
            };
            let created = create_map(g, &mut map, queue);
            g.maps.push(map);
            if !created {
                g.status.failed = true;
                break;
            }
        }
    });
}
/// Shutdown: off; uploads already submitted stay retained until their fence completes.
pub fn shutdown() {
    ENABLED.store(false, Ordering::Relaxed);
    RECORDING_EPOCH.fetch_add(1, Ordering::Release);
    with_global(|g| {
        g.requests.clear();
        // SAFETY: the port's live fence.
        let done = g.fence.as_ref().map_or(0, |f| unsafe { f.GetCompletedValue() });
        g.maps.retain(|m| !(m.completion != 0 && m.completion <= done));
    });
}

// The header's forwarders (FixedVrs.hpp in the Rust build).

#[unsafe(no_mangle)]
pub extern "C" fn cvr_vrs_get_settings() -> Settings {
    get_settings()
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_vrs_set_settings(s: Settings) {
    set_settings(s);
}
/// # Safety
/// `out` points to a Status.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_vrs_get_status(out: *mut Status) {
    // SAFETY: the caller's output.
    unsafe { *out = get_status() };
}
/// # Safety
/// `pso` is null or live; `desc` points to its description.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_vrs_graphics_created(pso: *mut c_void, desc: *const D3D12_GRAPHICS_PIPELINE_STATE_DESC) {
    // SAFETY: the caller's pipeline and description.
    if let (Some(p), Some(d)) = (unsafe { ID3D12PipelineState::from_raw_borrowed(&pso) }, unsafe { desc.as_ref() }) {
        graphics_created(p, d);
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_vrs_reset(list: *mut c_void, known: bool, pso: *mut c_void) {
    reset(list, known, pso);
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_vrs_invalidate(list: *mut c_void) {
    invalidate(list);
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_vrs_pipeline(list: *mut c_void, pso: *mut c_void) {
    pipeline(list, pso);
}
/// # Safety
/// `v` points to `count` viewports (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_vrs_viewports(list: *mut c_void, count: u32, v: *const D3D12_VIEWPORT) {
    // SAFETY: the caller's viewport, when there is exactly one.
    viewports(list, if count == 1 { unsafe { v.as_ref() } } else { None });
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_vrs_targets(list: *mut c_void, count: u32, depth: bool) {
    targets(list, count, depth);
}
/// # Safety
/// `combiners` is null or points to two combiners.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_vrs_shading_rate(list: *mut c_void, rate: D3D12_SHADING_RATE, combiners: *const [D3D12_SHADING_RATE_COMBINER; 2]) {
    // SAFETY: the caller's combiners.
    shading_rate(list, rate, unsafe { combiners.as_ref() });
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_vrs_shading_image(list: *mut c_void, image: *mut c_void) {
    shading_image(list, image);
}
/// # Safety
/// `device` and `queue` are null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_vrs_tick(device: *mut c_void, queue: *mut c_void) {
    // SAFETY: borrowed for this call.
    if let (Some(d), Some(q)) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }, unsafe {
        ID3D12CommandQueue::from_raw_borrowed(&queue)
    }) {
        tick(d, q);
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_vrs_shutdown() {
    shutdown();
}
/// # Safety
/// As prepare_draw.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_vrs_prepare_draw(list: *mut c_void, node: u32, eye: i32, set_rate: Option<RateFn>, set_image: Option<ImageFn>) {
    // SAFETY: the engine's list and setters.
    unsafe { prepare_draw(list, node, eye, set_rate, set_image) };
}
/// # Safety
/// As restore.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_vrs_restore(list: *mut c_void) {
    // SAFETY: the engine's list.
    unsafe { restore(list) };
}
