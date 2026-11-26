//! Framegen's frame capture (formerly src/Framegen/FrameCapture.cpp; the file keeps the switch
//! `CyberpunkVR_FramegenCapture` and `CaptureFrame`, which hands each eye's inputs over as the fields read here).
//!
//! With the switch at 1 the next three consecutive real frames -- both eyes' colour, the engine depth and motion
//! vectors, the engine camera and the eye camera -- are copied into readbacks on the presenting queue and written by a
//! thread of their own to log\framegen-capture\<time>\ (capture.json and f<N>_e<eye>_{color,depth,motion}.raw) once
//! the copies are done: offline replays of every generator (tools/framegen_tests) and the benchmark's stereo check
//! (tools/vr_bench) read them. The switch reads 2 once written, -1 on a failure. A frame shown again is no break in the
//! sequence; any other frame that does not follow the last one restarts it, but only once the last copies are done
//! (clearing first freed readbacks the GPU was still writing: a device removed by a page fault on the copy's write).
//! Motion is optional: without frame generation it is not captured.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, Ordering::Relaxed};
use cvr_crt::{format, Arg};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Mutex;
use windows::core::Interface;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows_sys::Win32::System::Performance::QueryPerformanceCounter;

/// include/Framegen/Inputs.hpp's CameraData.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CameraData {
    pub jitter: [f32; 2],
    pub motion_scale: [f32; 2],
    pub position: [f32; 3],
    pub up: [f32; 3],
    pub right: [f32; 3],
    pub forward: [f32; 3],
    pub near_plane: f32,
    pub far_plane: f32,
    pub vertical_fov: f32,
    pub aspect: f32,
    pub shift: [f32; 2],
    pub inverted: bool,
    pub infinite: bool,
    pub jittered: bool,
    pub reset: bool,
    pub camera_motion: bool,
    pub motion3d: bool,
}
/// include/Framegen/Generator.hpp's EyeCamera: orientation (x, y, z, w) in the XR space, the field of view's angles
/// (left, right, up, down).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct EyeCamera {
    pub orientation: [f32; 4],
    pub fov: [f32; 4],
}
/// One eye of a frame as `CaptureFrame` hands it over: its `Inputs`' fields and the eye camera it was shown with.
#[repr(C)]
pub struct CaptureEye {
    pub depth: *mut c_void,
    pub motion: *mut c_void,
    pub camera: *const CameraData,
    pub eye: *const EyeCamera,
    pub pose_id: u64,
    pub frame_id: u32,
    pub has_motion: bool,
}

#[derive(Default)]
struct Image {
    readback: Option<ID3D12Resource>,
    footprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    rows: u32,
    row_bytes: u64,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
}
#[derive(Default)]
struct Frame {
    color: [Image; 2],
    depth: [Image; 2],
    motion: [Image; 2],
    camera: [CameraData; 2],
    eye: [EyeCamera; 2],
    frame_id: u32,
    // The head-pose sample each eye was drawn from (Inputs::poseId), and when the frame was captured (QPC): two frames
    // with one pose sample are the port reusing a pose; two samples with one orientation are the runtime's.
    pose_id: [u64; 2],
    /// The rifle elbow fixture's right arm for this pose sample (main's ce2a00cad), when both eyes share one.
    arm: Option<cvr_host::rifle_elbow::Pose>,
    captured_qpc: i64,
}
struct State {
    device: usize,
    allocator: Option<ID3D12CommandAllocator>,
    list: Option<ID3D12GraphicsCommandList>,
    fence: Option<ID3D12Fence>,
    value: u64,
    frames: Vec<Frame>,
    writing: bool,
}
// SAFETY: D3D12 objects are free-threaded; the state is used under STATE's lock.
unsafe impl Send for State {}
static STATE: Mutex<State> = Mutex::new(State {
    device: 0,
    allocator: None,
    list: None,
    fence: None,
    value: 0,
    frames: Vec::new(),
    writing: false,
});

/// The frames handed to the writing thread.
struct Job {
    frames: Vec<Frame>,
    fence: ID3D12Fence,
    value: u64,
    armed: &'static AtomicI32,
}
// SAFETY: as State; the readbacks are only mapped once the fence has passed their copies.
unsafe impl Send for Job {}

unsafe fn barrier(
    list: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    a: D3D12_RESOURCE_STATES,
    b: D3D12_RESOURCE_STATES,
) {
    let v = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: core::mem::transmute_copy(resource),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: a,
                StateAfter: b,
            }),
        },
    };
    list.ResourceBarrier(&[v]);
}

/// `source`'s first subresource copied on `list` into a readback of its own.
unsafe fn copy(
    device: &ID3D12Device,
    list: &ID3D12GraphicsCommandList,
    source: &ID3D12Resource,
    image: &mut Image,
) -> bool {
    let desc = source.GetDesc();
    let mut bytes = 0u64;
    device.GetCopyableFootprints(
        &desc,
        0,
        1,
        0,
        Some(&mut image.footprint),
        Some(&mut image.rows),
        Some(&mut image.row_bytes),
        Some(&mut bytes),
    );
    let buffer = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: bytes,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_READBACK,
        ..Default::default()
    };
    let mut readback: Option<ID3D12Resource> = None;
    if device
        .CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_NONE,
            &buffer,
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
            &mut readback,
        )
        .is_err()
    {
        return false;
    }
    let Some(readback) = readback else {
        return false;
    };
    let from = D3D12_TEXTURE_COPY_LOCATION {
        pResource: core::mem::transmute_copy(source),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            SubresourceIndex: 0,
        },
    };
    let to = D3D12_TEXTURE_COPY_LOCATION {
        pResource: core::mem::transmute_copy(&readback),
        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            PlacedFootprint: image.footprint,
        },
    };
    list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
    image.readback = Some(readback);
    image.width = desc.Width as u32;
    image.height = desc.Height;
    image.format = desc.Format;
    true
}

/// The readback's rows without their padding, as a raw file.
unsafe fn write_image(image: &Image, path: &Path) -> bool {
    let Some(readback) = image.readback.as_ref() else {
        return false;
    };
    let mut data: *mut c_void = core::ptr::null_mut();
    if readback.Map(0, None, Some(&mut data)).is_err() {
        return false;
    }
    let mut ok = false;
    if let Ok(file) = File::create(path) {
        let mut out = BufWriter::with_capacity(1 << 20, file);
        ok = true;
        let base = data as *const u8;
        for row in 0..image.rows as usize {
            let start = base.add(
                image.footprint.Offset as usize + row * image.footprint.Footprint.RowPitch as usize,
            );
            if out
                .write_all(core::slice::from_raw_parts(start, image.row_bytes as usize))
                .is_err()
            {
                ok = false;
                break;
            }
        }
        ok = out.flush().is_ok() && ok;
    }
    readback.Unmap(0, Some(&D3D12_RANGE::default()));
    ok
}

/// `%.9g`, as the C runtime prints it.
fn g9(v: f32) -> String {
    format(c"%.9g", &[Arg::F(v as f64)])
        .to_string_lossy()
        .into_owned()
}
fn floats(out: &mut String, name: &str, v: &[f32], comma: bool) {
    out.push('"');
    out.push_str(name);
    out.push_str("\":[");
    for (i, x) in v.iter().enumerate() {
        if i != 0 {
            out.push(',');
        }
        out.push_str(&g9(*x));
    }
    out.push(']');
    if comma {
        out.push(',');
    }
}
fn shape(image: &Image) -> String {
    format!("[{},{},{}]", image.width, image.height, image.format.0)
}

fn write_all(job: Job) {
    let Job {
        frames,
        fence,
        value,
        armed,
    } = job;
    // SAFETY: plain Win32 and D3D12 calls on objects this thread holds.
    let mut ok = unsafe {
        if fence.GetCompletedValue() < value {
            if let Ok(event) = CreateEventW(None, false, false, None) {
                if fence.SetEventOnCompletion(value, event).is_ok() {
                    WaitForSingleObject(event, 10000);
                }
                let _ = CloseHandle(event);
            }
        }
        fence.GetCompletedValue() >= value
    };
    // SAFETY: plain Win32 call.
    let t = unsafe { GetLocalTime() };
    let stamp = format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    );
    let directory = cvr_host::paths::dir(cvr_host::paths::Folder::Log)
        .join("framegen-capture")
        .join(&stamp);
    ok = std::fs::create_dir_all(&directory).is_ok() && ok;
    let json = if ok {
        File::create(directory.join("capture.json")).ok()
    } else {
        None
    };
    ok = json.is_some();
    // The C++ capture wrote capture.json in text mode: every line ends with "\r\n".
    let mut text = String::new();
    if ok {
        text.push_str("{\"frames\":[\r\n");
    }
    let mut k = 0;
    while ok && k < frames.len() {
        let frame = &frames[k];
        text.push_str(&format!(
            "{}{{\"frame\":{},\"eyes\":[",
            if k != 0 { ",\r\n" } else { "" },
            frame.frame_id
        ));
        let mut e = 0;
        while e < 2 && ok {
            // SAFETY: the fence has passed the copies (ok).
            unsafe {
                ok = write_image(
                    &frame.color[e],
                    &directory.join(format!("f{k}_e{e}_color.raw")),
                );
                ok = ok
                    && write_image(
                        &frame.depth[e],
                        &directory.join(format!("f{k}_e{e}_depth.raw")),
                    );
                ok = ok
                    && (frame.motion[e].readback.is_none()
                        || write_image(
                            &frame.motion[e],
                            &directory.join(format!("f{k}_e{e}_motion.raw")),
                        ));
            }
            let (c, v) = (&frame.camera[e], &frame.eye[e]);
            text.push_str(&format!(
                "{}{{\"color\":{},\"depth\":{},\"motion\":{},",
                if e != 0 { "," } else { "" },
                shape(&frame.color[e]),
                shape(&frame.depth[e]),
                shape(&frame.motion[e])
            ));
            floats(&mut text, "jitter", &c.jitter, true);
            floats(&mut text, "motionScale", &c.motion_scale, true);
            floats(&mut text, "position", &c.position, true);
            floats(&mut text, "up", &c.up, true);
            floats(&mut text, "right", &c.right, true);
            floats(&mut text, "forward", &c.forward, true);
            floats(
                &mut text,
                "planes",
                &[c.near_plane, c.far_plane, c.vertical_fov, c.aspect],
                true,
            );
            text.push_str(&format!(
                "\"inverted\":{},\"infinite\":{},\"jittered\":{},\"pose\":{},\"qpc\":{},",
                c.inverted as i32,
                c.infinite as i32,
                c.jittered as i32,
                frame.pose_id[e],
                frame.captured_qpc
            ));
            if let Some(arm) = frame.arm.filter(|a| a.pose_id != 0 && a.pose_id == frame.pose_id[e]) {
                text.push_str(&format!("\"elbowMode\":{},", arm.mode));
                floats(&mut text, "rightArmModel", &arm.joints, true);
            }
            floats(&mut text, "orientation", &v.orientation, true);
            floats(&mut text, "fov", &v.fov, false);
            text.push('}');
            e += 1;
        }
        text.push_str("]}");
        k += 1;
    }
    if let Some(mut json) = json {
        text.push_str("\r\n]}\r\n");
        let _ = json.write_all(text.as_bytes());
    }
    cvr_host::log::write(&format!(
        "[framegen] capture of {} frames {}: {}\n",
        frames.len(),
        if ok { "written" } else { "FAILED" },
        directory.display()
    ));
    if let Ok(mut state) = STATE.lock() {
        state.writing = false;
    }
    armed.store(if ok { 2 } else { -1 }, Relaxed);
}

/// One real frame offered to the capture: copied when `armed` is 1 and the frame continues the sequence, the
/// sequence handed to the writing thread at three frames.
///
/// # Safety
/// `armed` is the switch (alive for the process), `device`, `queue`, `colors[0..2]` live objects or null (the colours
/// in COPY_SOURCE), `eyes[0..2]` valid with their depth and motion in NON_PIXEL_SHADER_RESOURCE.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_framegen_capture_frame(
    armed: *const AtomicI32,
    device: *mut c_void,
    queue: *mut c_void,
    colors: *const *mut c_void,
    eyes: *const CaptureEye,
) {
    let armed: &'static AtomicI32 = &*armed;
    if armed.load(Relaxed) != 1 {
        return;
    }
    let Ok(mut state) = STATE.lock() else {
        return;
    };
    let colors = [*colors, *colors.add(1)];
    let (Some(device), Some(queue), Some(color0), Some(color1)) = (
        ID3D12Device::from_raw_borrowed(&device),
        ID3D12CommandQueue::from_raw_borrowed(&queue),
        ID3D12Resource::from_raw_borrowed(&colors[0]),
        ID3D12Resource::from_raw_borrowed(&colors[1]),
    ) else {
        return;
    };
    if state.writing {
        return;
    }
    let eyes = &*(eyes as *const [CaptureEye; 2]);
    // A real frame shown again (no newer one yet) is no break in the sequence.
    let last = state.frames.last().map(|f| f.frame_id);
    if last == Some(eyes[0].frame_id) {
        return;
    }
    // The frames kept so far are released only once their copies are done.
    if state.device == device.as_raw() as usize
        && state.value != 0
        && state
            .fence
            .as_ref()
            .is_some_and(|f| f.GetCompletedValue() < state.value)
    {
        return;
    }
    if last.is_some_and(|id| eyes[0].frame_id != id.wrapping_add(1)) {
        state.frames.clear();
    }
    if state.device != device.as_raw() as usize {
        state.device = device.as_raw() as usize;
        state.frames.clear();
        state.list = None;
        state.allocator = None;
        state.fence = None;
        state.value = 0;
        let allocator: Option<ID3D12CommandAllocator> = device
            .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
            .ok();
        let list: Option<ID3D12GraphicsCommandList> = allocator.as_ref().and_then(|a| {
            device
                .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, a, None)
                .ok()
        });
        let fence: Option<ID3D12Fence> = list
            .as_ref()
            .and_then(|_| device.CreateFence(0, D3D12_FENCE_FLAG_NONE).ok());
        state.allocator = allocator;
        state.list = list;
        state.fence = fence;
        let Some(list) = state.list.as_ref().filter(|_| state.fence.is_some()) else {
            state.device = 0;
            armed.store(-1, Relaxed);
            return;
        };
        let _ = list.Close();
    }
    let (Some(allocator), Some(list), Some(fence)) = (
        state.allocator.clone(),
        state.list.clone(),
        state.fence.clone(),
    ) else {
        return;
    };
    if allocator.Reset().is_err() || list.Reset(&allocator, None).is_err() {
        return;
    }
    let mut frame = Frame {
        frame_id: eyes[0].frame_id,
        ..Default::default()
    };
    if eyes[0].pose_id == eyes[1].pose_id {
        frame.arm = cvr_host::rifle_elbow::find(eyes[0].pose_id);
    }
    let mut ok = true;
    let read = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    let source = D3D12_RESOURCE_STATE_COPY_SOURCE;
    let mut e = 0;
    while e < 2 && ok {
        let input = &eyes[e];
        frame.camera[e] = *input.camera;
        frame.eye[e] = *input.eye;
        frame.pose_id[e] = input.pose_id;
        if e == 0 {
            QueryPerformanceCounter(&mut frame.captured_qpc);
        }
        ok = copy(
            device,
            &list,
            if e == 0 { color0 } else { color1 },
            &mut frame.color[e],
        ); // the captures rest in COPY_SOURCE
        let motion = if input.has_motion {
            ID3D12Resource::from_raw_borrowed(&input.motion)
        } else {
            None
        };
        let Some(depth) = ID3D12Resource::from_raw_borrowed(&input.depth) else {
            ok = false;
            break;
        };
        barrier(&list, depth, read, source);
        if let Some(m) = motion {
            barrier(&list, m, read, source);
        }
        ok = ok
            && copy(device, &list, depth, &mut frame.depth[e])
            && motion.is_none_or(|m| copy(device, &list, m, &mut frame.motion[e]));
        barrier(&list, depth, source, read);
        if let Some(m) = motion {
            barrier(&list, m, source, read);
        }
        e += 1;
    }
    if list.Close().is_err() || !ok {
        armed.store(-1, Relaxed);
        state.frames.clear();
        return;
    }
    queue.ExecuteCommandLists(&[Some(
        list.cast::<ID3D12CommandList>()
            .unwrap_or_else(|_| unreachable!()),
    )]);
    state.value += 1;
    let _ = queue.Signal(&fence, state.value);
    state.frames.push(frame);
    if state.frames.len() < 3 {
        return;
    }
    state.writing = true;
    let job = Job {
        frames: core::mem::take(&mut state.frames),
        fence,
        value: state.value,
        armed,
    };
    std::thread::spawn(move || write_all(job));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_match_the_headers() {
        assert_eq!(core::mem::size_of::<CameraData>(), 96);
        assert_eq!(core::mem::offset_of!(CameraData, shift), 80);
        assert_eq!(core::mem::offset_of!(CameraData, inverted), 88);
        assert_eq!(core::mem::offset_of!(CameraData, motion3d), 93);
        assert_eq!(core::mem::size_of::<EyeCamera>(), 32);
        assert_eq!(core::mem::offset_of!(CaptureEye, pose_id), 32);
        assert_eq!(core::mem::offset_of!(CaptureEye, frame_id), 40);
        assert_eq!(core::mem::offset_of!(CaptureEye, has_motion), 44);
    }

    #[test]
    fn floats_print_as_the_c_runtime() {
        let mut s = String::new();
        floats(&mut s, "v", &[0.1, -0.0, 1e-30, 3.4e38, 16777216.0], false);
        assert_eq!(
            s,
            "\"v\":[0.100000001,-0,1e-30,3.39999995e+38,16777216]"
        );
    }
}
