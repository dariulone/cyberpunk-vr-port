//! THE CONSTANT BANK PASS (formerly src/Render/NativeConstantCapture.cpp's Poll -- the depth comparison's read and the
//! bank request -- and CopyConstantPair, with the plugin's halves of StereoGeometryPass.cpp's Create / Record and
//! StereoDepthComparison.cpp's Begin / Read): a request's captured constant pairs (each its registry family's single
//! CBVs, both eyes' payloads) become one paired binding bank; with geometry asked, the pairs' draws (the frozen copies when
//! the freeze ran, else the native draws' leases) become paired geometry commands; over their shared targets (two-layer
//! arrays) the pass is created and validated; with the depth comparison asked (a frozen, colourless pass) the reference
//! and the paired draws are recorded on a private list and submitted, the comparison read at a later Poll.
//!
//! The binding bank (binding_bank.rs), the geometry commands (geometry_commands.rs), the pass's checks, coverage, views
//! and recording state (geometry_pass_*.rs) and the comparison's targets, planes, recording, copies, submission and read
//! (depth_comparison.rs, private_submits.rs) were Rust's already; this module is what lent them C++ inputs. The Rust test
//! variants keep the .cpp's classes.
use core::ffi::c_void;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering::*};
use std::sync::{Arc, Mutex};

use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_NOINTERFACE, E_OUTOFMEMORY, E_POINTER, S_FALSE, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_D32_FLOAT_S8X24_UINT, DXGI_FORMAT_UNKNOWN};
use windows::Win32::Graphics::Dxgi::DXGI_ERROR_DEVICE_REMOVED;

use crate::declared_moved_exports::CyberpunkVR_GeometryFreezeCount;
use crate::early_depth_compare::{Comparison, Plane};
use crate::families_native::Family;
use crate::geometry_leases::{DrawResources, DrawState};
use crate::native_state_exports::*;
use crate::paired_root::Binding;

const E_PENDING: HRESULT = HRESULT(0x8000000Au32 as i32);
/// constants::BankPairCapacity.
const BANK_PAIR_CAPACITY: u32 = 2048;
const NO_COMPARISON: Comparison = Comparison {
    width: 0,
    height: 0,
    draws: 0,
    complete: 0,
    depth_different: [0; 2],
    stencil_different: [0; 2],
    written: [0; 2],
    invalid_depth: [0; 2],
    max_depth_error: [0.0; 2],
};
/// The depth comparison's result (formerly NativeConstantCapture.cpp's definition).
#[no_mangle]
pub static mut CyberpunkVR_ConstantCompareData: Comparison = NO_COMPARISON;

// ---- a COM holder (the C++ storages a list kept were IUnknowns)

#[repr(C)]
struct Holder {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    _value: Box<dyn core::any::Any>,
}
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
unsafe extern "system" fn holder_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live holder.
    unsafe {
        *out = null_mut();
        if iid.is_null() || *iid != IID_IUNKNOWN {
            return E_NOINTERFACE;
        }
        *out = this;
        holder_add_ref(this);
    }
    S_OK
}
unsafe extern "system" fn holder_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live holder.
    unsafe { (*(this as *const Holder)).refs.fetch_add(1, Relaxed) + 1 }
}
unsafe extern "system" fn holder_release(this: *mut c_void) -> u32 {
    // SAFETY: a live holder; the last release frees it.
    unsafe {
        let n = (*(this as *const Holder)).refs.fetch_sub(1, AcqRel) - 1;
        if n == 0 {
            drop(Box::from_raw(this as *mut Holder));
        }
        n
    }
}
static HOLDER_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: holder_query, AddRef: holder_add_ref, Release: holder_release };
/// `value` held by a COM object (its last release drops it).
fn holder(value: impl core::any::Any) -> IUnknown {
    let boxed = Box::new(Holder { vtbl: &HOLDER_VTBL, refs: AtomicU32::new(1), _value: Box::new(value) });
    // SAFETY: the box's one reference becomes the IUnknown's.
    unsafe { IUnknown::from_raw(Box::into_raw(boxed).cast()) }
}

// ---- the command resources, the signature receipts, the devices

unsafe extern "C-unwind" fn keep(list: *mut c_void, object: *mut c_void) -> bool {
    // SAFETY: the recording list.
    unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }.is_some_and(|l| cvr_gpu::command_resources::keep(l, &[object]))
}
unsafe extern "C-unwind" fn has(list: *mut c_void, object: *mut c_void) -> bool {
    // SAFETY: the recording list and a live object.
    unsafe { cvr_gpu::command_resources::cvr_gpu_has_command_resource(list, object) }
}
unsafe extern "C-unwind" fn describe(signature: *mut c_void, receipt: *mut crate::indirect_policy::Receipt) {
    // SAFETY: a live signature (or null); the caller's receipt.
    unsafe {
        *receipt = match ID3D12CommandSignature::from_raw_borrowed(&signature) {
            Some(s) => crate::indirect_policy::describe(s),
            None => Default::default(),
        };
    }
}
fn same_device(device: &ID3D12Device, child: &ID3D12DeviceChild) -> bool {
    let mut actual: Option<ID3D12Device> = None;
    // SAFETY: a live child; a local output.
    unsafe { child.GetDevice(&mut actual) }.is_ok() && actual.is_some_and(|a| a.as_raw() == device.as_raw())
}
fn raw<T: Interface>(value: &Option<T>) -> *mut c_void {
    value.as_ref().map_or(null_mut(), |v| v.as_raw())
}

// ---- the bank and the geometry commands (StereoBindingBank's CreatePass, StereoGeometryCommands' Create)

/// A captured constant pair's binding and both eyes' bytes (a BankPair with only its constants).
#[derive(Clone)]
struct OwnedPair {
    binding: Binding,
    bytes: [Vec<u8>; 2],
}
fn lend_value(bytes: &[u8]) -> crate::binding_bank::Value {
    crate::binding_bank::Value {
        constants: bytes.as_ptr(),
        constants_len: bytes.len(),
        source: null_mut(),
        // SAFETY: plain descriptions; all zeroes BankValue's defaults.
        srv: unsafe { core::mem::zeroed() },
        state: D3D12_RESOURCE_STATE_COMMON,
        // SAFETY: as above.
        gpu_cbv: unsafe { core::mem::zeroed() },
        owner: null_mut(),
        borrowed_srv: false,
        implicit_srv: false,
    }
}
fn plan_view(plan: &crate::paired_root::Plan) -> crate::binding_bank::PlanView {
    crate::binding_bank::PlanView {
        serialized: plan.serialized.as_ptr(),
        serialized_len: plan.serialized.len(),
        bindings: plan.bindings.as_ptr(),
        binding_count: plan.bindings.len(),
        parameters: plan.parameters,
        descriptors: plan.descriptors,
        original_parameters: plan.original_parameters,
        root_dwords: plan.root_dwords,
    }
}
/// A bank draw: the family's paired root and plan, its pairs, its source root when asked.
struct BankDraw<'a> {
    family: &'a Family,
    pairs: &'a [OwnedPair],
    source_root: bool,
}
/// The bank's storage (a COM object of binding_bank.rs').
fn create_bank(device: &ID3D12Device, draws: &[BankDraw]) -> Result<IUnknown, HRESULT> {
    if draws.is_empty() || draws.len() > 32768 {
        return Err(E_INVALIDARG);
    }
    let plans: Vec<_> = draws.iter().map(|d| plan_view(&d.family.paired_root.plan)).collect();
    let pairs: Vec<Vec<crate::binding_bank::Pair>> = draws
        .iter()
        .map(|d| d.pairs.iter().map(|p| crate::binding_bank::Pair { binding: p.binding, eyes: [lend_value(&p.bytes[0]), lend_value(&p.bytes[1])] }).collect())
        .collect();
    let lent: Vec<crate::binding_bank::DrawInput> = draws
        .iter()
        .zip(plans.iter().zip(&pairs))
        .map(|(d, (plan, pairs))| crate::binding_bank::DrawInput {
            root: d.family.paired_root.root.as_raw(),
            plan,
            pairs: pairs.as_ptr(),
            pair_count: pairs.len(),
            shared: null(),
            shared_count: 0,
            source_root: if d.source_root { d.family.source_root.as_raw() } else { null_mut() },
        })
        .collect();
    let mut created = null_mut();
    // SAFETY: the lent draws live for the call.
    let hr = unsafe { crate::binding_bank::cvr_bank_create_pass(device.as_raw(), lent.as_ptr(), lent.len(), &mut created) };
    if hr.is_err() {
        return Err(hr);
    }
    // SAFETY: the new storage's one reference.
    Ok(unsafe { IUnknown::from_raw(created) })
}
fn bank_counts(bank: &IUnknown) -> crate::binding_bank::Counts {
    let mut counts = crate::binding_bank::Counts { draws: 0, descriptors: 0, snapshots: 0, constant_bytes: 0 };
    // SAFETY: a live storage; a local output.
    unsafe { crate::binding_bank::cvr_bank_counts(bank.as_raw(), &mut counts) };
    counts
}
/// GeometryCommandPair.
#[derive(Clone)]
struct GeometryPair {
    eyes: [DrawResources; 2],
    instance_rebase_safe: bool,
}
/// The commands' storage (a COM object of geometry_commands.rs').
fn create_geometry(device: &ID3D12Device, pairs: &[GeometryPair]) -> Result<IUnknown, HRESULT> {
    if pairs.is_empty() || pairs.len() > 2048 {
        return Err(E_INVALIDARG);
    }
    let lent: Vec<crate::geometry_commands::PairInput> = pairs
        .iter()
        .map(|p| crate::geometry_commands::PairInput { eyes: [p.eyes[0].lend(), p.eyes[1].lend()], instance_rebase_safe: p.instance_rebase_safe })
        .collect();
    let mut created = null_mut();
    // SAFETY: the lent pairs live for the call.
    let hr = unsafe { crate::geometry_commands::cvr_sgc_create(device.as_raw(), lent.as_ptr(), lent.len(), &mut created) };
    if hr.is_err() {
        return Err(hr);
    }
    // SAFETY: the new storage's one reference.
    Ok(unsafe { IUnknown::from_raw(created) })
}
fn geometry_counts(geometry: &IUnknown) -> (u32, u64) {
    let (mut draws, mut sources, mut bytes) = (0, 0, 0);
    // SAFETY: a live storage; local outputs.
    unsafe { crate::geometry_commands::cvr_sgc_counts(geometry.as_raw(), &mut draws, &mut sources, &mut bytes) };
    (draws, bytes)
}
fn record_geometry(geometry: &IUnknown, list: &ID3D12GraphicsCommandList, index: u32) -> bool {
    // SAFETY: a live storage and recording list.
    unsafe { crate::geometry_commands::cvr_sgc_record(geometry.as_raw(), list.as_raw(), index, false, keep, describe) }
}

// ---- the paired geometry pass (StereoGeometryPass's Create and Record)

/// GeometryPassDraw as the bank lends it: no shared tables, no shared-read contract.
struct PassDraw<'a> {
    geometry: GeometryPair,
    family: Arc<Family>,
    constants: &'a [OwnedPair],
    view_mask: u32,
}
struct PassStep {
    pipeline: ID3D12PipelineState,
    state: DrawState,
    viewport: D3D12_VIEWPORT,
    scissor: windows::Win32::Foundation::RECT,
    mask: u32,
}
struct PassStorage {
    device: ID3D12Device,
    bank: IUnknown,
    geometry: IUnknown,
    draws: Vec<PassStep>,
    _colors: Vec<ID3D12Resource>,
    _depth: Option<ID3D12Resource>,
    color_count: u32,
    rtvs: Option<ID3D12DescriptorHeap>,
    dsv: Option<ID3D12DescriptorHeap>,
}
#[derive(Default)]
struct GeometryPass {
    storage: Option<Arc<PassStorage>>,
    recorded: bool,
    validation_stage: u32,
    validation_draw: u32,
    validation_bits: u32,
}
fn eye_state(s: &DrawState) -> crate::geometry_pass_state::EyeState {
    crate::geometry_pass_state::EyeState {
        original: raw(&s.original) as u64,
        rate_image: raw(&s.rate_image) as u64,
        rate: s.rate,
        topology: s.topology,
        stencil: s.stencil,
        blend: s.blend,
    }
}
fn target(device: &ID3D12Device, resource: &ID3D12Resource, format: DXGI_FORMAT, depth: bool, width: &mut u64, height: &mut u32) -> bool {
    if !same_device(device, resource) {
        return false;
    }
    // SAFETY: a live resource.
    let d = unsafe { resource.GetDesc() };
    crate::geometry_pass_checks::target(&d, format, depth, width, height)
}
fn str_of(text: &str) -> crate::geometry_pass_coverage::Str {
    crate::geometry_pass_coverage::Str { data: text.as_ptr(), len: text.len() }
}
/// The coverage of the draw's family, plan and pairs (no shared tables, no selectors); `needs` a shared-read contract.
fn coverage(input: &PassDraw, needs: &mut bool) -> bool {
    use crate::geometry_pass_coverage as c;
    let family = &*input.family;
    let plan = &family.paired_root.plan;
    let pairs: Vec<c::Pair> = input.constants.iter().map(|p| c::Pair { binding: p.binding, source: [0, 0], bytes: [p.bytes[0].len() as u64, p.bytes[1].len() as u64] }).collect();
    let eye = |e: usize| {
        let draw = &input.geometry.eyes[e].draw;
        // SAFETY: a plain scope; all zeroes the contract's default.
        c::Eye { scope: unsafe { core::mem::zeroed() }, root: draw.root, side: draw.side, owner: 0, selectors: null() }
    };
    let lent = c::Input {
        source_root: family.source_root.as_raw() as u64,
        uses: family.uses.as_ptr().cast(),
        use_count: family.uses.len(),
        bindings: plan.bindings.as_ptr(),
        binding_count: plan.bindings.len(),
        serialized: plan.serialized.as_ptr(),
        serialized_len: plan.serialized.len(),
        pairs: pairs.as_ptr(),
        pair_count: pairs.len(),
        shared: null(),
        shared_count: 0,
        original_parameters: plan.original_parameters,
        family: [str_of(&family.root_sha), str_of(&family.vertex_sha), str_of(&family.pixel_sha)],
        eyes: [eye(0), eye(1)],
    };
    // SAFETY: the lent views live for the call.
    unsafe { c::cvr_sgp_coverage(&lent, needs) }
}
impl GeometryPass {
    /// Create: every draw admitted (its family's devices, admission bits, coverage, targets, view, blend), the bank,
    /// the geometry commands and the targets' views.
    fn create(&mut self, device: &ID3D12Device, inputs: &[PassDraw], colors: &[ID3D12Resource], depth: Option<&ID3D12Resource>) -> HRESULT {
        self.validation_stage = 1;
        self.validation_draw = 0;
        self.validation_bits = 0;
        if self.storage.is_some() || inputs.is_empty() || inputs.len() > 2048 || colors.len() > 8 || (depth.is_none() && colors.is_empty()) {
            return E_INVALIDARG;
        }
        let mut banks: Vec<BankDraw> = Vec::with_capacity(inputs.len());
        let mut geometry: Vec<GeometryPair> = Vec::with_capacity(inputs.len());
        let mut draws: Vec<PassStep> = Vec::with_capacity(inputs.len());
        let mut first: Option<crate::stereo_family_contracts::PassContract> = None;
        let (mut width, mut height) = (0u64, 0u32);
        for input in inputs {
            self.validation_draw = banks.len() as u32;
            self.validation_stage = 2;
            let family = &*input.family;
            if !same_device(device, &family.pipeline)
                || !same_device(device, &family.source_root)
                || !same_device(device, &family.paired_root.root)
                || input.view_mask == 0
                || input.view_mask & !3 != 0
            {
                return E_INVALIDARG;
            }
            let contract = &family.pass;
            let (a, b) = (&input.geometry.eyes[0], &input.geometry.eyes[1]);
            self.validation_stage = 3;
            let original = raw(&a.state.original) as u64;
            let admission = crate::geometry_pass_admission::Inputs {
                rebase_safe: family.instance_rebase_safe,
                fixed_raster: contract.fixed_raster_state,
                sample_count: contract.samples.Count,
                sample_quality: contract.samples.Quality,
                topology_type: contract.topology.0,
                render_targets: contract.targets.NumRenderTargets,
                colors: colors.len() as u32,
                has_depth: depth.is_some(),
                depth_format: contract.depth != DXGI_FORMAT_UNKNOWN,
                original_matches: original == a.draw.pipeline && a.state.original.as_ref().is_some_and(|o| same_device(device, o)),
                topology: a.state.topology,
                differences: crate::geometry_pass_state::state_differences(&eye_state(&a.state), &eye_state(&b.state)),
                pixel_shader: contract.pixel_shader,
            };
            self.validation_bits = crate::geometry_pass_admission::validation_bits(&admission);
            if self.validation_bits != 0 {
                return E_INVALIDARG;
            }
            self.validation_stage = 4;
            let mut needs_contract = false;
            if !coverage(input, &mut needs_contract) {
                return E_INVALIDARG;
            }
            // The bank's draws carry no shared-read contract (ReadContract refuses its empty list).
            if needs_contract {
                return E_INVALIDARG;
            }
            self.validation_stage = 5;
            match &first {
                None => {
                    first = Some(*contract);
                    for (i, color) in colors.iter().enumerate() {
                        if !target(device, color, contract.targets.RTFormats[i], false, &mut width, &mut height) {
                            return E_INVALIDARG;
                        }
                    }
                    if let Some(depth) = depth {
                        if !target(device, depth, contract.depth, true, &mut width, &mut height) {
                            return E_INVALIDARG;
                        }
                    }
                }
                Some(f) => {
                    if contract.depth != f.depth || contract.targets.RTFormats[..colors.len()] != f.targets.RTFormats[..colors.len()] {
                        return E_INVALIDARG;
                    }
                }
            }
            self.validation_stage = 6;
            if !crate::geometry_pass_checks::view(&a.draw, width, height) || a.draw.rt_count as usize != colors.len() || b.draw.rt_count as usize != colors.len() {
                return E_INVALIDARG;
            }
            self.validation_stage = 7;
            if !a.state.blend.iter().all(|f| f.is_finite()) {
                return E_INVALIDARG;
            }
            banks.push(BankDraw { family, pairs: input.constants, source_root: false });
            let mut pair = input.geometry.clone();
            pair.instance_rebase_safe = family.instance_rebase_safe;
            geometry.push(pair);
            let mut state = a.state.clone();
            // VRS changes pixel-shader invocation rate, not depth/stencil coverage; a PS-less pipeline has no reader for an
            // inherited shading-rate image.
            if !contract.pixel_shader {
                state.rate = Default::default();
                state.rate.known = true;
                state.rate_image = None;
            }
            draws.push(PassStep { pipeline: family.pipeline.clone(), state, viewport: a.draw.viewport, scissor: a.draw.scissor, mask: input.view_mask });
        }
        let Some(first) = first else { return E_INVALIDARG };
        self.validation_stage = 8;
        let bank = match create_bank(device, &banks) {
            Ok(bank) => bank,
            Err(hr) => return hr,
        };
        self.validation_stage = 9;
        let commands = match create_geometry(device, &geometry) {
            Ok(commands) => commands,
            Err(hr) => return hr,
        };
        self.validation_stage = 10;
        let _internal = cvr_host::probe::Internal::new();
        let raws: Vec<*mut c_void> = colors.iter().map(|c| c.as_raw()).collect();
        let views = match crate::geometry_pass_commands::views(device, &raws, &first.targets.RTFormats[..colors.len()], depth.map_or(null_mut(), |d| d.as_raw()), first.depth) {
            Ok(views) => views,
            Err(hr) => return HRESULT(hr),
        };
        self.storage = Some(Arc::new(PassStorage {
            device: device.clone(),
            bank,
            geometry: commands,
            draws,
            _colors: colors.to_vec(),
            _depth: depth.cloned(),
            color_count: colors.len() as u32,
            rtvs: views.rtvs,
            dsv: views.dsv,
        }));
        self.validation_stage = 0;
        self.validation_draw = 0;
        self.validation_bits = 0;
        S_OK
    }
    fn draw_count(&self) -> u32 {
        self.storage.as_ref().map_or(0, |s| s.draws.len() as u32)
    }
    /// Record: once, on a direct list of the pass's device -- the storage kept with the list, both eyes' bank captured,
    /// the targets bound and each draw's bank, state and geometry.
    fn record(&mut self, list: &ID3D12GraphicsCommandList) -> bool {
        let Some(s) = self.storage.clone() else { return false };
        // SAFETY: a live list.
        if self.recorded || unsafe { list.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT || !same_device(&s.device, list) {
            return false;
        }
        let Ok(views) = list.cast::<ID3D12GraphicsCommandList1>() else { return false };
        let Ok(rates) = list.cast::<ID3D12GraphicsCommandList5>() else { return false };
        let kept = holder(s.clone());
        if !cvr_gpu::command_resources::keep(list, &[kept.as_raw()]) {
            return false;
        }
        self.recorded = true;
        let _internal = cvr_host::probe::Internal::new();
        // SAFETY: a live bank storage and the recording list.
        let captured = unsafe { (0..2).all(|eye| crate::binding_bank::cvr_bank_capture_eye(s.bank.as_raw(), list.as_raw(), eye, keep, has)) };
        if !captured {
            return false;
        }
        crate::geometry_pass_commands::begin(list, &rates, s.rtvs.as_ref(), s.color_count, s.dsv.as_ref());
        for (i, draw) in s.draws.iter().enumerate() {
            // SAFETY: the recording list and the family's pipeline.
            unsafe { list.SetPipelineState(&draw.pipeline) };
            // SAFETY: as above.
            if !unsafe { crate::binding_bank::cvr_bank_bind(s.bank.as_raw(), list.as_raw(), u32::MAX, i as u32, has) } {
                return false;
            }
            let state = crate::geometry_pass_commands::State {
                rate: draw.state.rate,
                image: null_mut(),
                set_image: false,
                set_mask: true,
                topology: draw.state.topology,
                stencil: draw.state.stencil,
                blend: draw.state.blend,
                viewport: draw.viewport,
                scissor: draw.scissor,
                mask: draw.mask,
            };
            crate::geometry_pass_commands::state(list, &rates, Some(&views), &state);
            if !record_geometry(&s.geometry, list, i as u32) {
                return false;
            }
        }
        true
    }
}

// ---- the depth comparison (StereoDepthComparison's Begin and Read)

struct ComparisonStorage {
    _device: ID3D12Device,
    _queue: Option<ID3D12CommandQueue>,
    _allocator: Option<ID3D12CommandAllocator>,
    _commands: Option<ID3D12GraphicsCommandList>,
    fence: Option<ID3D12Fence>,
    _bank: IUnknown,
    _geometry: IUnknown,
    _paired: Option<Arc<PassStorage>>,
    _originals: Vec<Option<ID3D12PipelineState>>,
    _depth: [Option<ID3D12Resource>; 3],
    _dsvs: Option<ID3D12DescriptorHeap>,
    readback: Option<ID3D12Resource>,
    planes: [Plane; 8],
    readback_bytes: u64,
    clear: f32,
}
struct DepthComparison {
    storage: Option<Arc<ComparisonStorage>>,
    result: Comparison,
    stage: u32,
}
/// The private list's recording ownership detached on every exit (it is never replayed; a submitted batch keeps its
/// objects through its fence).
struct RecordingGuard<'a>(&'a ID3D12GraphicsCommandList);
impl Drop for RecordingGuard<'_> {
    fn drop(&mut self) {
        cvr_gpu::command_resources::reset(self.0);
    }
}
unsafe extern "C-unwind" fn bind_eye(user: *mut c_void, list: *mut c_void, eye: u32, index: u32) -> bool {
    // SAFETY: the comparison's bank (the host's user) and the recording list.
    let bank = unsafe { &*user.cast::<IUnknown>() };
    eye < 2 && !list.is_null() && unsafe { crate::binding_bank::cvr_bank_bind(bank.as_raw(), list, eye, index, has) }
}
unsafe extern "C-unwind" fn reference_geometry(user: *mut c_void, list: *mut c_void, index: u32) -> bool {
    // SAFETY: the comparison's geometry (after the bank in the user pair) and the recording list.
    let geometry = unsafe { &*user.cast::<IUnknown>().add(1) };
    !list.is_null() && unsafe { crate::geometry_commands::cvr_sgc_record(geometry.as_raw(), list, index, false, keep, describe) }
}
impl DepthComparison {
    fn new() -> Self {
        DepthComparison { storage: None, result: NO_COMPARISON, stage: 0 }
    }
    /// Begin: each eye's reference draws and the paired pass recorded on a private list into three depth targets, their
    /// planes copied into a readback, the list submitted on its own queue.
    fn begin(&mut self, device: &ID3D12Device, inputs: &[PassDraw], target: &D3D12_RESOURCE_DESC) -> HRESULT {
        self.stage = 1;
        // SAFETY: a plain description.
        if self.storage.is_some() || inputs.is_empty() || inputs.len() > 2048 || !unsafe { crate::depth_comparison::cvr_sdc_target_shape(target) } {
            return E_INVALIDARG;
        }
        self.result.width = target.Width as u32;
        self.result.height = target.Height;
        self.result.draws = inputs.len() as u32;
        let mut banks: Vec<BankDraw> = Vec::with_capacity(inputs.len());
        let mut geometry: Vec<GeometryPair> = Vec::with_capacity(inputs.len());
        let mut originals = Vec::with_capacity(inputs.len());
        let mut clear = 0.0f32;
        self.stage = 2;
        for input in inputs {
            let pass = &input.family.pass;
            if pass.pixel_shader || pass.targets.NumRenderTargets != 0 || pass.depth != DXGI_FORMAT_D32_FLOAT_S8X24_UINT {
                return E_INVALIDARG;
            }
            let s = &pass.depth_state;
            let Some(value) = crate::depth_clear_value::clear_depth(pass.depth_state_known, s.DepthEnable.as_bool(), s.DepthWriteMask.0, s.DepthFunc.0) else {
                return E_INVALIDARG;
            };
            if !banks.is_empty() && value != clear {
                return E_INVALIDARG;
            }
            clear = value;
            banks.push(BankDraw { family: &*input.family, pairs: input.constants, source_root: true });
            geometry.push(input.geometry.clone());
            originals.push(input.geometry.eyes[0].state.original.clone());
        }
        self.stage = 3;
        let _internal = cvr_host::probe::Internal::new();
        let bank = match create_bank(device, &banks) {
            Ok(bank) => bank,
            Err(hr) => return hr,
        };
        self.stage = 4;
        let commands = match create_geometry(device, &geometry) {
            Ok(commands) => commands,
            Err(hr) => return hr,
        };
        self.stage = 5;
        let mut created = crate::depth_comparison::Targets { depth: [null_mut(); 3], dsvs: null_mut() };
        let hr = crate::depth_comparison::create_targets(device, target, clear, &mut created);
        // SAFETY: each created object handed over with its reference.
        let depth: [Option<ID3D12Resource>; 3] = created.depth.map(|d| (!d.is_null()).then(|| unsafe { ID3D12Resource::from_raw(d) }));
        let dsvs = (!created.dsvs.is_null()).then(|| unsafe { ID3D12DescriptorHeap::from_raw(created.dsvs) });
        if hr.is_err() {
            return hr;
        }
        let Some(heap) = dsvs.as_ref() else { return E_FAIL };
        // SAFETY: the live device and heap.
        let (stride, start) = unsafe { (device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV) as usize, heap.GetCPUDescriptorHandleForHeapStart().ptr) };
        self.stage = 6;
        let mut paired = GeometryPass::default();
        let hr = paired.create(device, inputs, &[], depth[2].as_ref());
        if hr.is_err() {
            return hr;
        }
        let depth_raw = [raw(&depth[0]), raw(&depth[1]), raw(&depth[2])];
        let mut planes = [Plane::default(); 8];
        self.stage = 7;
        let readback_bytes = match crate::depth_comparison::plan(device, &depth_raw, target.Width as u32, target.Height, &mut planes) {
            Ok(bytes) => bytes,
            Err(hr) => return hr,
        };
        self.stage = 8;
        let mut readback_raw = null_mut();
        let hr = crate::depth_comparison::create_readback(device, readback_bytes, &mut readback_raw);
        // SAFETY: the readback handed over with its reference.
        let readback = (!readback_raw.is_null()).then(|| unsafe { ID3D12Resource::from_raw(readback_raw) });
        if hr.is_err() {
            return hr;
        }
        self.stage = 9;
        let (mut allocator, mut list) = (None, None);
        let hr = crate::private_submits::commands(device, &mut allocator, &mut list);
        if hr.is_err() {
            return hr;
        }
        let Some(list) = list else { return E_FAIL };
        let recording = RecordingGuard(&list);
        let Ok(rates) = list.cast::<ID3D12GraphicsCommandList5>() else { return E_NOINTERFACE };
        // The pending objects kept with the list while it records (as the .cpp kept its storage).
        let kept = holder((bank.clone(), commands.clone(), paired.storage.clone(), depth.clone(), dsvs.clone(), readback.clone(), originals.clone()));
        if !cvr_gpu::command_resources::keep(&list, &[kept.as_raw()]) {
            return E_OUTOFMEMORY;
        }
        self.stage = 10;
        // SAFETY: the live bank and recording list.
        if !unsafe { (0..2).all(|eye| crate::binding_bank::cvr_bank_capture_eye(bank.as_raw(), list.as_raw(), eye, keep, has)) } {
            return E_FAIL;
        }
        let draws: Vec<crate::depth_comparison::DrawInput> = inputs
            .iter()
            .map(|input| crate::depth_comparison::DrawInput {
                view_mask: input.view_mask,
                _pad: 0,
                eyes: core::array::from_fn(|eye| {
                    let source = &input.geometry.eyes[eye];
                    crate::depth_comparison::EyeDraw {
                        pipeline: raw(&source.state.original),
                        topology: source.state.topology,
                        stencil: source.state.stencil,
                        blend: source.state.blend,
                        viewport: source.draw.viewport,
                        scissor: source.draw.scissor,
                    }
                }),
            })
            .collect();
        // The host's user: the bank, then the geometry.
        let objects = [bank.clone(), commands.clone()];
        let host = crate::depth_comparison::Host { user: objects.as_ptr() as *mut c_void, bind_eye, geometry: reference_geometry };
        // SAFETY: the recording list, its interfaces, the DSV heap's start and stride, the live inputs and host.
        if !unsafe { crate::depth_comparison::record(&list, Some(&rates), start, stride, clear, &draws, &host) } {
            return E_FAIL;
        }
        self.stage = 11;
        if !paired.record(&list) {
            return E_FAIL;
        }
        self.stage = 12;
        // SAFETY: the recording list; the targets in DEPTH_WRITE, the readback in COPY_DEST.
        unsafe { crate::depth_comparison::copies(&list, &depth_raw, raw(&readback), &planes) };
        // SAFETY: the recording list.
        if let Err(e) = unsafe { list.Close() } {
            return e.code();
        }
        let (mut queue, mut fence, mut stage) = (None, None, 0u32);
        let hr = crate::private_submits::submit(device, &list, &mut queue, &mut fence, &mut stage);
        self.stage = stage;
        if hr.is_err() {
            return hr;
        }
        drop(recording);
        self.storage = Some(Arc::new(ComparisonStorage {
            _device: device.clone(),
            _queue: queue,
            _allocator: allocator,
            _commands: Some(list.clone()),
            fence,
            _bank: bank,
            _geometry: commands,
            _paired: paired.storage.clone(),
            _originals: originals,
            _depth: depth,
            _dsvs: dsvs,
            readback,
            planes,
            readback_bytes,
            clear,
        }));
        self.stage = 0;
        S_OK
    }
    /// Read: S_FALSE until the private fence completes, then the readback compared (once).
    fn read(&mut self, result: &mut Comparison) -> HRESULT {
        let Some(s) = self.storage.as_ref() else { return windows::Win32::Foundation::E_UNEXPECTED };
        if self.result.complete != 0 {
            *result = self.result;
            return S_OK;
        }
        let Some(fence) = s.fence.as_ref() else { return E_FAIL };
        // SAFETY: the live fence.
        let completed = unsafe { fence.GetCompletedValue() };
        if completed == u64::MAX {
            return DXGI_ERROR_DEVICE_REMOVED;
        }
        if completed < 1 {
            return S_FALSE;
        }
        // SAFETY: the live readback of `readback_bytes` and its planes.
        let hr = unsafe { crate::depth_comparison::cvr_sdc_read(raw(&s.readback), s.readback_bytes, &s.planes, s.clear, &mut self.result) };
        if hr.is_err() {
            return hr;
        }
        *result = self.result;
        S_OK
    }
}

// ---- Poll (formerly NativeConstantCapture.cpp's, after PollRegisteredIntent)

struct Comparing(Option<DepthComparison>);
// SAFETY: free-threaded D3D12 objects, touched only under the mutex.
unsafe impl Send for Comparing {}
static COMPARISON: Mutex<Comparing> = Mutex::new(Comparing(None));

/// CopyConstantPair: the pair's family and its plan's bindings' two eyes' bytes.
struct PairInput {
    family: Arc<Family>,
    pairs: Vec<OwnedPair>,
}
fn copy_constant_pair(left: u32, right: u32) -> Option<PairInput> {
    let (family, bytes) = crate::constant_capture_state::copy_pair(left, right)?;
    let pairs = family.paired_root.plan.bindings.iter().zip(bytes).map(|(b, bytes)| OwnedPair { binding: b.binding, bytes }).collect();
    Some(PairInput { family, pairs })
}
fn sequences() -> &'static [u32; 4096] {
    // SAFETY: the request's exported sequences, written by the tools before the request.
    unsafe { &*(&raw const CyberpunkVR_ConstantBankSequences) }
}
/// The comparison's read (state 1), the capture's poll, then a bank request.
#[no_mangle]
pub extern "C-unwind" fn cvr_ncb_poll() {
    let mut comparing = COMPARISON.lock().unwrap_or_else(|e| e.into_inner());
    if CyberpunkVR_ConstantCompareState.load(SeqCst) == 1 {
        if let Some(comparison) = comparing.0.as_mut() {
            let mut result = NO_COMPARISON;
            let hr = comparison.read(&mut result);
            if hr != S_FALSE {
                CyberpunkVR_ConstantCompareSeq.fetch_add(1, SeqCst);
                // SAFETY: the exports, written by this poll only.
                unsafe {
                    CyberpunkVR_ConstantCompareData = result;
                    CyberpunkVR_ConstantCompareResult = hr.0;
                }
                CyberpunkVR_ConstantCompareState.store(if hr.is_ok() { 2 } else { 3 }, SeqCst);
                comparing.0 = None;
                CyberpunkVR_ConstantCompareSeq.fetch_add(1, SeqCst);
            }
        }
    }
    // The registered trace's collection and the deferred bindings.
    crate::constant_capture_state::cvr_ncc2_poll();
    let request = CyberpunkVR_ConstantBankRequest.swap(0, SeqCst);
    if request == 0 {
        return;
    }
    // SAFETY: the exports, written by this poll only.
    unsafe {
        if CyberpunkVR_ConstantCompareState.load(SeqCst) == 1 {
            CyberpunkVR_ConstantBankResult = E_PENDING.0;
            CyberpunkVR_ConstantBankDone.store(request, SeqCst);
            return;
        }
        let compare_depth = CyberpunkVR_ConstantBankCompareDepth.load(SeqCst) != 0;
        if compare_depth {
            CyberpunkVR_ConstantCompareSeq.fetch_add(1, SeqCst);
            CyberpunkVR_ConstantCompareToken.store(request, SeqCst);
            CyberpunkVR_ConstantCompareData = NO_COMPARISON;
            CyberpunkVR_ConstantCompareResult = E_PENDING.0;
            CyberpunkVR_ConstantCompareStage = 0;
            CyberpunkVR_ConstantCompareState.store(0, SeqCst);
            CyberpunkVR_ConstantCompareSeq.fetch_add(1, SeqCst);
        }
        CyberpunkVR_ConstantBankResult = E_FAIL.0;
        CyberpunkVR_ConstantBankDescriptors = 0;
        CyberpunkVR_ConstantBankBytes = 0;
        CyberpunkVR_ConstantBankDraws = 0;
        CyberpunkVR_ConstantBankPreparedDraws = 0;
        CyberpunkVR_ConstantBankInstanceBytes = 0;
        CyberpunkVR_ConstantBankGeometryResult = E_PENDING.0;
        CyberpunkVR_ConstantBankPassResult = E_PENDING.0;
        CyberpunkVR_ConstantBankPassDraws = 0;
        CyberpunkVR_ConstantBankPassStage = 0;
        CyberpunkVR_ConstantBankPassValidation = [0; 3];
        let slot = &mut comparing.0;
        if std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| bank(compare_depth, slot))).is_err() {
            CyberpunkVR_ConstantBankResult = E_OUTOFMEMORY.0;
        }
        if compare_depth && CyberpunkVR_ConstantCompareState.load(SeqCst) == 0 {
            CyberpunkVR_ConstantCompareSeq.fetch_add(1, SeqCst);
            CyberpunkVR_ConstantCompareResult = E_FAIL.0;
            CyberpunkVR_ConstantCompareState.store(3, SeqCst);
            CyberpunkVR_ConstantCompareSeq.fetch_add(1, SeqCst);
        }
    }
    CyberpunkVR_ConstantBankDone.store(request, Release);
}
/// The request: the pairs' bank; with geometry asked, their commands; over their shared targets the pass; with the
/// depth comparison asked, its begin.
///
/// # Safety
/// Under Poll (the bank exports are its own).
unsafe fn bank(compare_depth: bool, comparison: &mut Option<DepthComparison>) {
    let count = CyberpunkVR_ConstantBankPairCount.load(SeqCst).max(1);
    if count > BANK_PAIR_CAPACITY {
        return;
    }
    let sequences = sequences();
    let mut inputs: Vec<PairInput> = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let Some(input) = copy_constant_pair(sequences[2 * i], sequences[2 * i + 1]) else { return };
        inputs.push(input);
    }
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: the first family's live paired root; a local output.
    let mut hr = match unsafe { inputs[0].family.paired_root.root.GetDevice(&mut device) } {
        Ok(()) => S_OK,
        Err(e) => e.code(),
    };
    let device = match (hr.is_ok(), device) {
        (true, Some(device)) => device,
        _ => {
            // SAFETY: as documented.
            unsafe { CyberpunkVR_ConstantBankResult = if hr.is_ok() { E_FAIL.0 } else { hr.0 } };
            return;
        }
    };
    {
        let draws: Vec<BankDraw> = inputs.iter().map(|i| BankDraw { family: &*i.family, pairs: &i.pairs, source_root: false }).collect();
        match create_bank(&device, &draws) {
            Ok(bank) => {
                let counts = bank_counts(&bank);
                // SAFETY: as documented.
                unsafe {
                    CyberpunkVR_ConstantBankDescriptors = counts.descriptors;
                    CyberpunkVR_ConstantBankBytes = counts.constant_bytes;
                    CyberpunkVR_ConstantBankDraws = counts.draws;
                }
            }
            Err(e) => hr = e,
        }
    }
    // SAFETY: as documented.
    unsafe { CyberpunkVR_ConstantBankResult = hr.0 };
    if hr.is_err() || CyberpunkVR_ConstantBankPrepareGeometry.load(SeqCst) == 0 {
        return;
    }
    let mut geometry: Vec<GeometryPair> = Vec::with_capacity(inputs.len());
    let mut complete = true;
    for (i, input) in inputs.iter().enumerate() {
        let (left, right) = (sequences[2 * i], sequences[2 * i + 1]);
        let pair = if CyberpunkVR_GeometryFreezeCount.load(SeqCst) != 0 {
            let pair = crate::geometry_freeze::copy_pair_resources(left, right).map(|eyes| GeometryPair { eyes, instance_rebase_safe: true });
            for (eye, sequence) in [left, right].into_iter().enumerate() {
                let original = crate::geometry_leases::natives::copy_draw_resources(sequence);
                complete &= match (&original, &pair) {
                    (Some(original), Some(pair)) => original.instances == pair.eyes[eye].instances,
                    _ => false,
                };
            }
            pair
        } else {
            match (crate::geometry_leases::natives::copy_draw_resources(left), crate::geometry_leases::natives::copy_draw_resources(right)) {
                (Some(a), Some(b)) => Some(GeometryPair { eyes: [a, b], instance_rebase_safe: input.family.instance_rebase_safe }),
                _ => None,
            }
        };
        match pair {
            Some(pair) => geometry.push(pair),
            None => complete = false,
        }
    }
    let commands = if complete { create_geometry(&device, &geometry) } else { Err(E_FAIL) };
    // SAFETY: as documented.
    unsafe {
        CyberpunkVR_ConstantBankGeometryResult = match &commands {
            Ok(_) => S_OK.0,
            Err(e) => e.0,
        };
    }
    let Ok(commands) = commands else { return };
    let (draws, bytes) = geometry_counts(&commands);
    // SAFETY: as documented.
    unsafe {
        CyberpunkVR_ConstantBankPreparedDraws = draws;
        CyberpunkVR_ConstantBankInstanceBytes = bytes;
    }
    drop(commands);
    let _internal = cvr_host::probe::Internal::new();
    let native = &geometry[0].eyes[0];
    let color_count = native.draw.rt_count;
    let mut supported = color_count == 0 || color_count == 3;
    // SAFETY: as documented.
    unsafe { CyberpunkVR_ConstantBankPassStage = 1 };
    let mut arrays: [cvr_gpu::target_array::TargetArray; 4] = Default::default();
    let mut colors: Vec<ID3D12Resource> = Vec::new();
    let mut depth: Option<ID3D12Resource> = None;
    let mut pass_result = E_INVALIDARG;
    let mut slot = if color_count != 0 { 0 } else { 3 };
    while supported && slot < 4 {
        let Some(resource) = native.state.targets[slot].as_ref() else {
            supported = false;
            break;
        };
        // SAFETY: a live resource.
        let desc = unsafe { resource.GetDesc() };
        for pair in &geometry {
            for eye in &pair.eyes {
                let Some(candidate) = eye.state.targets[slot].as_ref() else {
                    supported = false;
                    continue;
                };
                // SAFETY: as above.
                let d = unsafe { candidate.GetDesc() };
                // SAFETY: two plain descriptions.
                supported &= unsafe { crate::constant_capture_checks::cvr_ncc_same_target(&d, &desc) };
            }
        }
        if !supported {
            break;
        }
        // SAFETY: as documented.
        unsafe { CyberpunkVR_ConstantBankPassStage = 2 };
        let state = if slot == 3 { D3D12_RESOURCE_STATE_DEPTH_WRITE } else { D3D12_RESOURCE_STATE_RENDER_TARGET };
        pass_result = arrays[slot].initialize(&device, &desc, state, None);
        if pass_result.is_err() {
            supported = false;
            break;
        }
        let made = arrays[slot].resource().cloned();
        if slot == 3 {
            depth = made;
        } else if let Some(made) = made {
            colors.push(made);
        }
        slot += 1;
    }
    if supported {
        // SAFETY: as documented.
        unsafe { CyberpunkVR_ConstantBankPassStage = 3 };
        let pass_draws: Vec<PassDraw> = geometry
            .into_iter()
            .zip(&inputs)
            .map(|(geometry, input)| PassDraw { geometry, family: input.family.clone(), constants: &input.pairs, view_mask: 3 })
            .collect();
        let mut pass = GeometryPass::default();
        pass_result = pass.create(&device, &pass_draws, &colors, depth.as_ref());
        // SAFETY: as documented.
        unsafe {
            CyberpunkVR_ConstantBankPassValidation = [pass.validation_stage, pass.validation_draw, pass.validation_bits];
            if pass_result.is_ok() {
                CyberpunkVR_ConstantBankPassDraws = pass.draw_count();
            }
        }
        if pass_result.is_ok() && compare_depth {
            let mut candidate = DepthComparison::new();
            let reference = pass_draws[0].geometry.eyes[0].state.targets[3].as_ref();
            let compare_result = match reference {
                // SAFETY: a live resource.
                Some(target) if CyberpunkVR_GeometryFreezeCount.load(SeqCst) != 0 && color_count == 0 => candidate.begin(&device, &pass_draws, &unsafe { target.GetDesc() }),
                _ => E_INVALIDARG,
            };
            CyberpunkVR_ConstantCompareSeq.fetch_add(1, SeqCst);
            // SAFETY: as documented.
            unsafe {
                CyberpunkVR_ConstantCompareResult = compare_result.0;
                CyberpunkVR_ConstantCompareStage = candidate.stage;
            }
            CyberpunkVR_ConstantCompareState.store(if compare_result.is_ok() { 1 } else { 3 }, SeqCst);
            if compare_result.is_ok() {
                *comparison = Some(candidate);
            }
            CyberpunkVR_ConstantCompareSeq.fetch_add(1, SeqCst);
        }
    } else if pass_result.is_ok() {
        pass_result = E_INVALIDARG;
    }
    // SAFETY: as documented.
    unsafe {
        CyberpunkVR_ConstantBankPassResult = pass_result.0;
        if pass_result.is_ok() {
            CyberpunkVR_ConstantBankPassStage = 4;
        }
    }
}
