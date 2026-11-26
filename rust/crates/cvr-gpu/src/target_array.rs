//! Single pass's two-layer target arrays (formerly src/Render/StereoTargetArray.cpp): the array output of a common
//! geometry pass, with explicit copies between its layers (one per eye; a depth/stencil format has two planes, layer
//! `eye + plane * 2`) and the engine's own single-view targets read by its unmodified lighting and temporal shaders.
//! The owner retains this object and the native targets until their queue's fence passes; nothing here waits. The
//! caller proves native access and states; these helpers grant no authority of their own.
use core::ffi::c_void;

use windows::core::{Interface, HRESULT};
use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_D32_FLOAT_S8X24_UINT, DXGI_FORMAT_R32G8X24_TYPELESS};

/// StereoTargetArray. Clones share the array (as copies of the C++ object shared its COM reference).
#[derive(Clone, Default)]
pub struct TargetArray {
    resource: Option<ID3D12Resource>,
    native: D3D12_RESOURCE_DESC,
    planes: u32,
}

type Barriers = Vec<D3D12_RESOURCE_BARRIER>;
/// A transition borrowing `resource` (no reference of its own), skipped when the states agree.
fn transition(barriers: &mut Barriers, resource: *mut c_void, subresource: u32, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) {
    if before == after {
        return;
    }
    barriers.push(D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                pResource: unsafe { core::mem::transmute_copy(&resource) },
                Subresource: subresource,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    });
}
/// The same barriers with their states swapped (the way back).
fn reversed(barriers: &Barriers) -> Barriers {
    barriers
        .iter()
        .map(|b| {
            // SAFETY: every barrier here is a transition.
            let t = unsafe { &b.Anonymous.Transition };
            let mut back = Barriers::new();
            transition(
                &mut back,
                t.pResource.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw()),
                t.Subresource,
                t.StateAfter,
                t.StateBefore,
            );
            back.pop().unwrap_or_else(|| unreachable!("a transition's states differ"))
        })
        .collect()
}
fn submit(list: &ID3D12GraphicsCommandList, barriers: &Barriers) {
    if !barriers.is_empty() {
        // SAFETY: an open list and transitions of live resources.
        unsafe { list.ResourceBarrier(barriers) };
    }
}
fn copy(list: &ID3D12GraphicsCommandList, from: *mut c_void, from_subresource: u32, to: *mut c_void, to_subresource: u32) {
    let location = |resource: *mut c_void, subresource: u32| D3D12_TEXTURE_COPY_LOCATION {
        // SAFETY: the same pointer without an AddRef; the location is never dropped as an owner.
        pResource: unsafe { core::mem::transmute_copy(&resource) },
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: subresource },
    };
    let (source, destination) = (
        core::mem::ManuallyDrop::new(location(from, from_subresource)),
        core::mem::ManuallyDrop::new(location(to, to_subresource)),
    );
    // SAFETY: an open list; both resources live for the recording.
    unsafe { list.CopyTextureRegion(&*destination, 0, 0, 0, &*source, None) };
}
fn plane_count(device: &ID3D12Device, format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT) -> Result<u32, HRESULT> {
    let mut info = D3D12_FEATURE_DATA_FORMAT_INFO { Format: format, PlaneCount: 0 };
    // SAFETY: the format-info struct and its size.
    unsafe {
        device.CheckFeatureSupport(
            D3D12_FEATURE_FORMAT_INFO,
            &mut info as *mut _ as *mut c_void,
            core::mem::size_of_val(&info) as u32,
        )
    }
    .map_err(|e| e.code())?;
    Ok(info.PlaneCount as u32)
}
fn device_of<T: Interface>(object: &T) -> Result<ID3D12Device, HRESULT> {
    let child: ID3D12DeviceChild = object.cast().map_err(|e| e.code())?;
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live device child writing its device.
    unsafe { child.GetDevice(&mut device) }.map_err(|e| e.code())?;
    device.ok_or(E_INVALIDARG)
}

fn hr(result: Result<(), HRESULT>) -> HRESULT {
    result.err().unwrap_or(S_OK)
}

impl TargetArray {
    pub fn resource(&self) -> Option<&ID3D12Resource> {
        self.resource.as_ref()
    }
    pub fn plane_count(&self) -> u32 {
        self.planes
    }
    fn raw(&self) -> *mut c_void {
        self.resource.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw())
    }
    /// The native target's shape: one 2D layer of the array's size and format (or D32S8 for an R32G8X24 array).
    fn native_shape(&self, desc: &D3D12_RESOURCE_DESC) -> bool {
        let depth_family = self.native.Format == DXGI_FORMAT_R32G8X24_TYPELESS && desc.Format == DXGI_FORMAT_D32_FLOAT_S8X24_UINT;
        desc.Dimension == self.native.Dimension
            && desc.Width == self.native.Width
            && desc.Height == self.native.Height
            && desc.DepthOrArraySize == 1
            && desc.MipLevels == 1
            && desc.SampleDesc.Count == 1
            && (desc.Format == self.native.Format || depth_family)
    }

    /// Initialize: once, a two-layer array of `desc`'s single-view target, in `state`.
    pub fn initialize(
        &mut self,
        device: &ID3D12Device,
        desc: &D3D12_RESOURCE_DESC,
        state: D3D12_RESOURCE_STATES,
        clear: Option<&D3D12_CLEAR_VALUE>,
    ) -> HRESULT {
        if self.resource.is_some()
            || desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
            || desc.DepthOrArraySize != 1
            || desc.MipLevels != 1
            || desc.SampleDesc.Count != 1
            || desc.Width == 0
            || desc.Height == 0
            || desc.Layout != D3D12_TEXTURE_LAYOUT_UNKNOWN
        {
            return E_INVALIDARG;
        }
        let planes = match plane_count(device, desc.Format) {
            Ok(p) => p,
            Err(e) => return e,
        };
        if planes == 0 || planes > 2 {
            return E_NOTIMPL;
        }
        let array = D3D12_RESOURCE_DESC {
            DepthOrArraySize: 2,
            Alignment: 0,
            ..*desc
        };
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };
        let mut resource: Option<ID3D12Resource> = None;
        // SAFETY: the live device; descriptions on the stack.
        if let Err(e) = unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &array, state, clear.map(|c| c as *const _), &mut resource) } {
            return e.code();
        }
        self.resource = resource;
        self.native = *desc;
        self.planes = planes;
        S_OK
    }

    fn validate_layer(&self, list: &ID3D12GraphicsCommandList, eye: u32, native: Option<&ID3D12Resource>) -> Result<(), HRESULT> {
        let Some(resource) = &self.resource else { return Err(E_INVALIDARG) };
        // SAFETY: a live list.
        if eye > 1
            || self.planes == 0
            || self.planes > 2
            || native.is_some_and(|n| n.as_raw() == resource.as_raw())
            || unsafe { list.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT
        {
            return Err(E_INVALIDARG);
        }
        let device = device_of(resource)?;
        if device_of(list)?.as_raw() != device.as_raw() {
            return Err(E_INVALIDARG);
        }
        // SAFETY: a live resource.
        let array = unsafe { resource.GetDesc() };
        if array.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
            || array.Width != self.native.Width
            || array.Height != self.native.Height
            || array.DepthOrArraySize != 2
            || array.MipLevels != 1
            || array.SampleDesc.Count != 1
            || array.SampleDesc.Quality != 0
            || array.Format != self.native.Format
            || array.Layout != D3D12_TEXTURE_LAYOUT_UNKNOWN
        {
            return Err(E_INVALIDARG);
        }
        if plane_count(&device, array.Format)? != self.planes {
            return Err(E_INVALIDARG);
        }
        let Some(native) = native else { return Ok(()) };
        if device_of(native)?.as_raw() != device.as_raw() {
            return Err(E_INVALIDARG);
        }
        // SAFETY: a live resource.
        let desc = unsafe { native.GetDesc() };
        let depth_family = self.native.Format == DXGI_FORMAT_R32G8X24_TYPELESS && desc.Format == DXGI_FORMAT_D32_FLOAT_S8X24_UINT;
        if desc.Dimension != array.Dimension
            || desc.Width != array.Width
            || desc.Height != array.Height
            || desc.DepthOrArraySize != 1
            || desc.MipLevels != 1
            || desc.SampleDesc.Count != 1
            || desc.SampleDesc.Quality != 0
            || desc.Layout != array.Layout
            || (desc.Format != self.native.Format && !depth_family)
        {
            return Err(E_INVALIDARG);
        }
        if plane_count(&device, desc.Format)? == self.planes {
            Ok(())
        } else {
            Err(E_INVALIDARG)
        }
    }

    /// TransitionEye: one layer (every plane of it); the other eye's state is left alone.
    pub fn transition_eye(&self, list: &ID3D12GraphicsCommandList, eye: u32, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> HRESULT {
        hr((|| {
            self.validate_layer(list, eye, None)?;
            let mut barriers = Barriers::new();
            for plane in 0..self.planes {
                transition(&mut barriers, self.raw(), eye + plane * 2, before, after);
            }
            submit(list, &barriers);
            Ok(())
        })())
    }

    /// ImportEyeLayer: the native source into one layer; the other eye's contents are left alone.
    pub fn import_eye_layer(
        &self,
        list: &ID3D12GraphicsCommandList,
        eye: u32,
        array_state: D3D12_RESOURCE_STATES,
        source: Option<&ID3D12Resource>,
        source_state: D3D12_RESOURCE_STATES,
    ) -> HRESULT {
        let Some(source) = source else { return E_INVALIDARG };
        hr((|| {
            self.validate_layer(list, eye, Some(source))?;
            let mut barriers = Barriers::new();
            transition(
                &mut barriers,
                source.as_raw(),
                D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                source_state,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            );
            for plane in 0..self.planes {
                transition(&mut barriers, self.raw(), eye + plane * 2, array_state, D3D12_RESOURCE_STATE_COPY_DEST);
            }
            submit(list, &barriers);
            for plane in 0..self.planes {
                copy(list, source.as_raw(), plane, self.raw(), eye + plane * 2);
            }
            submit(list, &reversed(&barriers));
            Ok(())
        })())
    }

    /// CopyEyeLayer: one layer into the native target, with one state per native plane (in subresource order). Copies
    /// honour the caller's predication; barriers stay unconditional.
    pub fn copy_eye_layer(
        &self,
        list: &ID3D12GraphicsCommandList,
        eye: u32,
        array_state: D3D12_RESOURCE_STATES,
        target: Option<&ID3D12Resource>,
        before: &[D3D12_RESOURCE_STATES],
        after: &[D3D12_RESOURCE_STATES],
    ) -> HRESULT {
        let Some(target) = target else { return E_INVALIDARG };
        if before.len() != self.planes as usize || after.len() != self.planes as usize {
            return E_INVALIDARG;
        }
        hr((|| {
            self.validate_layer(list, eye, Some(target))?;
            let mut barriers = Barriers::new();
            for plane in 0..self.planes {
                transition(&mut barriers, self.raw(), eye + plane * 2, array_state, D3D12_RESOURCE_STATE_COPY_SOURCE);
            }
            for (plane, &state) in before.iter().enumerate() {
                transition(&mut barriers, target.as_raw(), plane as u32, state, D3D12_RESOURCE_STATE_COPY_DEST);
            }
            submit(list, &barriers);
            for plane in 0..self.planes {
                copy(list, self.raw(), eye + plane * 2, target.as_raw(), plane);
            }
            let mut back = Barriers::new();
            for plane in 0..self.planes {
                transition(&mut back, self.raw(), eye + plane * 2, D3D12_RESOURCE_STATE_COPY_SOURCE, array_state);
            }
            for (plane, &state) in after.iter().enumerate() {
                transition(&mut back, target.as_raw(), plane as u32, D3D12_RESOURCE_STATE_COPY_DEST, state);
            }
            submit(list, &back);
            Ok(())
        })())
    }

    /// CopyEyeLayer with one state for every native plane.
    pub fn copy_eye_layer_uniform(
        &self,
        list: &ID3D12GraphicsCommandList,
        eye: u32,
        array_state: D3D12_RESOURCE_STATES,
        target: Option<&ID3D12Resource>,
        before: D3D12_RESOURCE_STATES,
        after: D3D12_RESOURCE_STATES,
    ) -> HRESULT {
        if self.planes == 0 || self.planes > 2 {
            return E_INVALIDARG;
        }
        let n = self.planes as usize;
        self.copy_eye_layer(list, eye, array_state, target, &[before; 2][..n], &[after; 2][..n])
    }

    /// ImportEye: the native source into one layer.
    pub fn import_eye(
        &self,
        list: &ID3D12GraphicsCommandList,
        eye: u32,
        array_state: D3D12_RESOURCE_STATES,
        source: Option<&ID3D12Resource>,
        source_state: D3D12_RESOURCE_STATES,
    ) -> HRESULT {
        if eye > 1 {
            E_INVALIDARG
        } else {
            self.import_eyes(list, 1 << eye, array_state, source, source_state)
        }
    }

    /// ImportEyes: either or both layers from one native source, with one source/array transition pair.
    pub fn import_eyes(
        &self,
        list: &ID3D12GraphicsCommandList,
        eye_mask: u32,
        array_state: D3D12_RESOURCE_STATES,
        source: Option<&ID3D12Resource>,
        source_state: D3D12_RESOURCE_STATES,
    ) -> HRESULT {
        let (Some(resource), Some(source)) = (&self.resource, source) else {
            return E_INVALIDARG;
        };
        if source.as_raw() == resource.as_raw() || eye_mask == 0 || eye_mask > 3 {
            return E_INVALIDARG;
        }
        // SAFETY: a live resource.
        if !self.native_shape(&unsafe { source.GetDesc() }) {
            return E_INVALIDARG;
        }
        let mut barriers = Barriers::new();
        transition(
            &mut barriers,
            source.as_raw(),
            D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            source_state,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
        );
        transition(
            &mut barriers,
            self.raw(),
            D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            array_state,
            D3D12_RESOURCE_STATE_COPY_DEST,
        );
        submit(list, &barriers);
        for eye in (0..2).filter(|e| eye_mask & (1 << e) != 0) {
            for plane in 0..self.planes {
                copy(list, source.as_raw(), plane, self.raw(), eye + plane * 2);
            }
        }
        let mut back = Barriers::new();
        transition(
            &mut back,
            source.as_raw(),
            D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
            source_state,
        );
        transition(
            &mut back,
            self.raw(),
            D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            D3D12_RESOURCE_STATE_COPY_DEST,
            array_state,
        );
        submit(list, &back);
        S_OK
    }

    /// CopyEye: one eye (every plane: depth and stencil both, or MAIN's stencil masks would stay) into the native
    /// target, with the whole array transitioned.
    pub fn copy_eye(
        &self,
        list: &ID3D12GraphicsCommandList,
        eye: u32,
        source_state: D3D12_RESOURCE_STATES,
        target: Option<&ID3D12Resource>,
        before: D3D12_RESOURCE_STATES,
        after: D3D12_RESOURCE_STATES,
    ) -> HRESULT {
        let (Some(resource), Some(target)) = (&self.resource, target) else {
            return E_INVALIDARG;
        };
        if target.as_raw() == resource.as_raw() || eye > 1 {
            return E_INVALIDARG;
        }
        // SAFETY: a live resource.
        if !self.native_shape(&unsafe { target.GetDesc() }) {
            return E_INVALIDARG;
        }
        let mut barriers = Barriers::new();
        transition(
            &mut barriers,
            self.raw(),
            D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            source_state,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
        );
        transition(
            &mut barriers,
            target.as_raw(),
            D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            before,
            D3D12_RESOURCE_STATE_COPY_DEST,
        );
        submit(list, &barriers);
        for plane in 0..self.planes {
            copy(list, self.raw(), eye + plane * 2, target.as_raw(), plane);
        }
        let mut back = Barriers::new();
        transition(
            &mut back,
            self.raw(),
            D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
            source_state,
        );
        transition(
            &mut back,
            target.as_raw(),
            D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            D3D12_RESOURCE_STATE_COPY_DEST,
            after,
        );
        submit(list, &back);
        S_OK
    }
}

// The header's wrapper (StereoTargetArray.hpp in the Rust build). Every list argument is checked for null first.

fn list_of(list: &*mut c_void) -> Option<&ID3D12GraphicsCommandList> {
    // SAFETY: the caller's live list (or null), borrowed for the call.
    unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(list) }
}
fn resource_of(resource: &*mut c_void) -> Option<&ID3D12Resource> {
    // SAFETY: the caller's live resource (or null), borrowed for the call.
    unsafe { ID3D12Resource::from_raw_borrowed(resource) }
}
fn array<'a>(a: *const TargetArray) -> Option<&'a TargetArray> {
    // SAFETY: the wrapper's array.
    unsafe { a.as_ref() }
}

#[unsafe(no_mangle)]
pub extern "C" fn cvr_target_array_new() -> *mut TargetArray {
    Box::into_raw(Box::default())
}
/// # Safety
/// `a` came from cvr_target_array_new or _clone.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_target_array_clone(a: *const TargetArray) -> *mut TargetArray {
    Box::into_raw(Box::new(array(a).cloned().unwrap_or_default()))
}
/// # Safety
/// `a` came from cvr_target_array_new or _clone (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_target_array_free(a: *mut TargetArray) {
    if !a.is_null() {
        // SAFETY: the wrapper's array, dropped once.
        drop(unsafe { Box::from_raw(a) });
    }
}
/// # Safety
/// `a` is a live array; `device` live or null; `desc` points to a description; `clear` null or a clear value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_target_array_initialize(
    a: *mut TargetArray,
    device: *mut c_void,
    desc: *const D3D12_RESOURCE_DESC,
    state: D3D12_RESOURCE_STATES,
    clear: *const D3D12_CLEAR_VALUE,
) -> HRESULT {
    // SAFETY: the wrapper's array and the caller's values.
    match (unsafe { a.as_mut() }, unsafe { ID3D12Device::from_raw_borrowed(&device) }, unsafe {
        desc.as_ref()
    }) {
        (Some(a), Some(device), Some(desc)) => a.initialize(device, desc, state, unsafe { clear.as_ref() }),
        _ => E_INVALIDARG,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_target_array_resource(a: *const TargetArray) -> *mut c_void {
    array(a).map_or(core::ptr::null_mut(), |a| a.raw())
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_target_array_planes(a: *const TargetArray) -> u32 {
    array(a).map_or(0, TargetArray::plane_count)
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_target_array_copy_eye(
    a: *const TargetArray,
    list: *mut c_void,
    eye: u32,
    source_state: D3D12_RESOURCE_STATES,
    target: *mut c_void,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> HRESULT {
    match (array(a), list_of(&list)) {
        (Some(a), Some(list)) => a.copy_eye(list, eye, source_state, resource_of(&target), before, after),
        _ => E_INVALIDARG,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_target_array_import_eye(
    a: *const TargetArray,
    list: *mut c_void,
    eye: u32,
    array_state: D3D12_RESOURCE_STATES,
    source: *mut c_void,
    source_state: D3D12_RESOURCE_STATES,
) -> HRESULT {
    match (array(a), list_of(&list)) {
        (Some(a), Some(list)) => a.import_eye(list, eye, array_state, resource_of(&source), source_state),
        _ => E_INVALIDARG,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_target_array_import_eyes(
    a: *const TargetArray,
    list: *mut c_void,
    eye_mask: u32,
    array_state: D3D12_RESOURCE_STATES,
    source: *mut c_void,
    source_state: D3D12_RESOURCE_STATES,
) -> HRESULT {
    match (array(a), list_of(&list)) {
        (Some(a), Some(list)) => a.import_eyes(list, eye_mask, array_state, resource_of(&source), source_state),
        _ => E_INVALIDARG,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_target_array_transition_eye(
    a: *const TargetArray,
    list: *mut c_void,
    eye: u32,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> HRESULT {
    match (array(a), list_of(&list)) {
        (Some(a), Some(list)) => a.transition_eye(list, eye, before, after),
        _ => E_INVALIDARG,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_target_array_import_eye_layer(
    a: *const TargetArray,
    list: *mut c_void,
    eye: u32,
    array_state: D3D12_RESOURCE_STATES,
    source: *mut c_void,
    source_state: D3D12_RESOURCE_STATES,
) -> HRESULT {
    match (array(a), list_of(&list)) {
        (Some(a), Some(list)) => a.import_eye_layer(list, eye, array_state, resource_of(&source), source_state),
        _ => E_INVALIDARG,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_target_array_copy_eye_layer_uniform(
    a: *const TargetArray,
    list: *mut c_void,
    eye: u32,
    array_state: D3D12_RESOURCE_STATES,
    target: *mut c_void,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> HRESULT {
    match (array(a), list_of(&list)) {
        (Some(a), Some(list)) => a.copy_eye_layer_uniform(list, eye, array_state, resource_of(&target), before, after),
        _ => E_INVALIDARG,
    }
}
/// # Safety
/// `before` and `after` point to `before_count` and `after_count` states (or are null with a count of 0).
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_target_array_copy_eye_layer(
    a: *const TargetArray,
    list: *mut c_void,
    eye: u32,
    array_state: D3D12_RESOURCE_STATES,
    target: *mut c_void,
    before: *const D3D12_RESOURCE_STATES,
    before_count: usize,
    after: *const D3D12_RESOURCE_STATES,
    after_count: usize,
) -> HRESULT {
    // SAFETY: the caller's state arrays.
    let slice = |p: *const D3D12_RESOURCE_STATES, n: usize| {
        if n == 0 || p.is_null() {
            &[][..]
        } else {
            unsafe { core::slice::from_raw_parts(p, n) }
        }
    };
    match (array(a), list_of(&list)) {
        (Some(a), Some(list)) => a.copy_eye_layer(
            list,
            eye,
            array_state,
            resource_of(&target),
            slice(before, before_count),
            slice(after, after_count),
        ),
        _ => E_INVALIDARG,
    }
}
