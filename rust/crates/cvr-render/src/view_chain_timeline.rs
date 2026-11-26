//! Research probe: the frame's chain of views and jobs on the node timeline (formerly src/Stereo/ViewChainTimeline.cpp;
//! the file keeps the registrations and their wants: CVR_CHAIN_TIMELINE for the chain, CVR_JOB_TIMELINE for the jobs
//! too). Each detour times its call into the node timeline ring (stereo_profiler.rs) while CyberpunkVR_NodeTimeline is
//! on: kind 6 scene wait, 7 frame submit, 8 view flush, 9 view job, 10 game frame, 11 RenderFrameTick job, 12 sub-scene
//! job, 13 sub-scene list, 14 world tick, 15 a counter wait inside the game frame (its caller's RVA), 20 a game-thread
//! job run (its function and queue, its name's RVA), 21 the dispatcher's job run. A view is tagged by its address and
//! its render target's type, read under SEH.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::stereo_profiler::{cvr_stereo_prof_now as now, cvr_stereo_prof_timeline_add as add};
use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::cell::Cell;

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_NodeTimeline: i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

thread_local! {
    static IN_GAME_FRAME: Cell<bool> = const { Cell::new(false) };
}

fn on() -> bool {
    // SAFETY: a plain read of the C++ switch.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_NodeTimeline) != 0 }
}
fn exe_base() -> usize {
    static BASE: AtomicUsize = AtomicUsize::new(0);
    let mut base = BASE.load(Relaxed);
    if base == 0 {
        // SAFETY: the process image's own handle.
        base = unsafe { GetModuleHandleW(core::ptr::null()) } as usize;
        BASE.store(base, Relaxed);
    }
    base
}

/// The tag of one view: under SEH (cvr_engine::code::guarded_call3), its address and its render target's type.
unsafe extern "C" fn view_tag_raw(view: usize, out: *mut u32) -> bool {
    let table = *(view as *const *const usize);
    let data_of: unsafe extern "C" fn(usize) -> usize = core::mem::transmute(*table.add(4));
    let data = data_of(view);
    let mut kind = 0xFEu32;
    if data != 0 {
        let target = *((data + 0xFB0) as *const usize);
        kind = if target != 0 { *((target + 16) as *const u8) as u32 } else { 0xFD };
    }
    *out = ((view >> 4) as u32 & 0x00FF_FFFF) | (kind << 24);
    true
}
unsafe fn view_tag(view: usize) -> u32 {
    if view == 0 {
        return 0xFFFF_FFFF;
    }
    let mut tag = 0u32;
    match cvr_engine::code::guarded_call3(
        view_tag_raw as *const () as usize,
        view as *mut c_void,
        &mut tag as *mut u32 as *mut c_void,
        core::ptr::null_mut(),
    ) {
        Some(_) => tag,
        None => 0xFFFF_FFFD,
    }
}
unsafe fn view_of(view: *const i64) -> usize {
    if view.is_null() {
        0
    } else {
        *view as usize
    }
}

macro_rules! original {
    ($name:ident) => {
        #[no_mangle]
        pub static mut $name: usize = 0;
    };
}
original!(cvr_timeline_scene_wait_original);
original!(cvr_timeline_frame_submit_original);
original!(cvr_timeline_view_flush_original);
original!(cvr_timeline_view_job_original);
original!(cvr_timeline_game_frame_original);
original!(cvr_timeline_frame_tick_original);
original!(cvr_timeline_sub_scene_original);
original!(cvr_timeline_sub_scene_list_original);
original!(cvr_timeline_world_tick_original);
original!(cvr_timeline_counter_wait_original);
original!(cvr_timeline_run_job_original);

/// scene wait sub_1405D3CF0.
#[no_mangle]
pub unsafe extern "C" fn cvr_timeline_scene_wait(scene: *mut c_void) -> usize {
    let f: unsafe extern "C" fn(*mut c_void) -> usize = core::mem::transmute(cvr_timeline_scene_wait_original);
    if !on() {
        return f(scene);
    }
    let t0 = now();
    let r = f(scene);
    add(t0, now(), 0, 6, 0);
    r
}
/// frame submit sub_1403D6D84.
#[no_mangle]
pub unsafe extern "C" fn cvr_timeline_frame_submit(scene: *mut c_void, frame: *mut c_void, flag: i8) -> usize {
    let f: unsafe extern "C" fn(*mut c_void, *mut c_void, i8) -> usize = core::mem::transmute(cvr_timeline_frame_submit_original);
    if !on() {
        return f(scene, frame, flag);
    }
    let t0 = now();
    let r = f(scene, frame, flag);
    add(t0, now(), 0, 7, 0);
    r
}
/// view flush sub_1403D7030.
#[no_mangle]
pub unsafe extern "C" fn cvr_timeline_view_flush(scene: *mut c_void, view: *mut i64) -> usize {
    let f: unsafe extern "C" fn(*mut c_void, *mut i64) -> usize = core::mem::transmute(cvr_timeline_view_flush_original);
    if !on() {
        return f(scene, view);
    }
    let tag = view_tag(view_of(view));
    let t0 = now();
    let r = f(scene, view);
    add(t0, now(), tag, 8, 0);
    r
}
/// view job sub_140B60324.
#[no_mangle]
pub unsafe extern "C" fn cvr_timeline_view_job(view: *mut i64, job: *mut c_void) -> *mut c_void {
    let f: unsafe extern "C" fn(*mut i64, *mut c_void) -> *mut c_void = core::mem::transmute(cvr_timeline_view_job_original);
    if !on() {
        return f(view, job);
    }
    let tag = view_tag(view_of(view));
    let t0 = now();
    let r = f(view, job);
    add(t0, now(), tag, 9, 0);
    r
}
/// game frame sub_1405D48F0.
#[no_mangle]
pub unsafe extern "C" fn cvr_timeline_game_frame(game: *mut c_void, dt: f32, frame: *mut u32, extra: *mut c_void) -> i8 {
    let f: unsafe extern "C" fn(*mut c_void, f32, *mut u32, *mut c_void) -> i8 = core::mem::transmute(cvr_timeline_game_frame_original);
    if !on() {
        return f(game, dt, frame, extra);
    }
    let t0 = now();
    IN_GAME_FRAME.set(true);
    let r = f(game, dt, frame, extra);
    IN_GAME_FRAME.set(false);
    add(t0, now(), 0, 10, 0);
    r
}
/// RenderFrameTick job sub_1404F2170.
#[no_mangle]
pub unsafe extern "C" fn cvr_timeline_frame_tick(data: *mut u8, job: *mut c_void) -> usize {
    let f: unsafe extern "C" fn(*mut u8, *mut c_void) -> usize = core::mem::transmute(cvr_timeline_frame_tick_original);
    if !on() {
        return f(data, job);
    }
    let t0 = now();
    let r = f(data, job);
    add(t0, now(), 0, 11, 0);
    r
}
/// sub-scene job sub_140B243B8.
#[no_mangle]
pub unsafe extern "C" fn cvr_timeline_sub_scene(scene: *mut c_void, prio: u8, after: *mut i64, done: *mut i64, view: *mut i64) -> usize {
    let f: unsafe extern "C" fn(*mut c_void, u8, *mut i64, *mut i64, *mut i64) -> usize = core::mem::transmute(cvr_timeline_sub_scene_original);
    if !on() {
        return f(scene, prio, after, done, view);
    }
    let tag = view_tag(view_of(view));
    let t0 = now();
    let r = f(scene, prio, after, done, view);
    add(t0, now(), tag, 12, 0);
    r
}
/// sub-scene list sub_140B2428C.
#[no_mangle]
pub unsafe extern "C" fn cvr_timeline_sub_scene_list(scene: *mut c_void, group: *mut c_void, list: *mut u8) -> usize {
    let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut u8) -> usize = core::mem::transmute(cvr_timeline_sub_scene_list_original);
    if !on() {
        return f(scene, group, list);
    }
    let t0 = now();
    let r = f(scene, group, list);
    add(t0, now(), 0, 13, 0);
    r
}
/// world tick sub_14069C788.
#[no_mangle]
pub unsafe extern "C" fn cvr_timeline_world_tick(world: *mut c_void, a2: *mut c_void, a3: *mut c_void) -> usize {
    let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> usize = core::mem::transmute(cvr_timeline_world_tick_original);
    if !on() {
        return f(world, a2, a3);
    }
    let t0 = now();
    let r = f(world, a2, a3);
    add(t0, now(), 0, 14, 0);
    r
}
unsafe extern "C" fn counter_wait(counter: *mut c_void, return_address: usize) -> usize {
    let f: unsafe extern "C" fn(*mut c_void) -> usize = core::mem::transmute(cvr_timeline_counter_wait_original);
    if !on() || !IN_GAME_FRAME.get() {
        return f(counter);
    }
    let t0 = now();
    let r = f(counter);
    let t1 = now();
    if t1 - t0 > 0 {
        add(t0, t1, return_address.wrapping_sub(exe_base()) as u32, 15, 0);
    }
    r
}
/// counter wait sub_1405D3CFC: its return address handed on as a second argument.
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn cvr_timeline_counter_wait() {
    core::arch::naked_asm!("mov rdx, [rsp]", "jmp {body}", body = sym counter_wait);
}

/// A job name's RVA in the image, or 0xFFFFFFFF.
fn name_rva(name: *const u8) -> u32 {
    let offset = (name as usize).wrapping_sub(exe_base());
    if !name.is_null() && offset < 0x1000_0000 {
        offset as u32
    } else {
        0xFFFF_FFFF
    }
}
/// The game thread's job call (exe+0x2735DB, patched to come here): the job run and timed.
unsafe extern "C" fn job_run(data: *mut c_void, name_arg: *mut c_void, f: unsafe extern "C" fn(*mut c_void, *mut c_void), queue: u32) {
    if !on() {
        f(data, name_arg);
        return;
    }
    let t0 = now();
    f(data, name_arg);
    let rva = (f as usize).wrapping_sub(exe_base()) as u32;
    add(
        t0,
        now(),
        (rva & 0x0FFF_FFFF) | ((queue & 0xF) << 28),
        20,
        name_rva(*(name_arg as *const *const u8)),
    );
}
/// game-thread job run sub_14026E094.
#[no_mangle]
pub unsafe extern "C" fn cvr_timeline_run_job(dispatcher: usize, job: *mut usize, a3: i32, priority: u8, a5: usize, a6: usize) -> usize {
    let f: unsafe extern "C" fn(usize, *mut usize, i32, u8, usize, usize) -> usize = core::mem::transmute(cvr_timeline_run_job_original);
    if !on() || job.is_null() {
        return f(dispatcher, job, a3, priority, a5, a6);
    }
    let t0 = now();
    let r = f(dispatcher, job, a3, priority, a5, a6);
    let desc = *job.add(2) as *const *const u8;
    let name = if desc.is_null() { core::ptr::null() } else { *desc };
    add(t0, now(), (*job).wrapping_sub(exe_base()) as u32, 21, name_rva(name));
    r
}

/// JobTimeline (Boot, 5): the job call at exe+0x2735DB through job_run -- the stored dil first, then the job's data and
/// name in rcx/rdx as the call had them, its function from [rsp+30h] and the queue byte.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_job_timeline_install() -> bool {
    let site = exe_base() + 0x2735DB;
    let mut found = [0u8; 8];
    if !cvr_engine::code::read_bytes(site, &mut found) || found != [0x40, 0x88, 0x7D, 0xB0, 0xFF, 0x54, 0x24, 0x30] {
        return false;
    }
    let Some(code) = cvr_engine::trampoline::allocate(site, 64) else {
        return false;
    };
    let mut c = cvr_engine::trampoline::Code::new(code);
    c.bytes(&[0x40, 0x88, 0x7D, 0xB0]) // mov [rbp-50h], dil
        .bytes(&[0x4C, 0x8B, 0x44, 0x24, 0x30]) // mov r8, [rsp+30h]
        .bytes(&[0x44, 0x0F, 0xB6, 0xCF]) // movzx r9d, dil
        .mov_rax(job_run as *const () as u64)
        .bytes(&[0xFF, 0xD0]) // call rax
        .jmp(site + 8);
    c.commit();
    cvr_engine::trampoline::patch_jump(site, 8, code);
    true
}
