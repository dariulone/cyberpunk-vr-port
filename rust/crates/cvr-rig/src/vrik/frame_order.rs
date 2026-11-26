//! FrameOrderProbe (main's src/Natives/FrameOrderProbe.cpp): the order of one frame's events across threads, for a few
//! frames, into the log. Measured on the simulator (2026-11-08), every frame in this order: the hand collision script's
//! tick, the player's hands solved and published (~3 ms before the step), PxScene::simulate on a job thread,
//! fetchResults ~1.3 ms later, the instances' pose records sent to the renderer, the frame submit.
//!
//! The notes: 'K' the character rig's pose pass, 'L' the VR menu, 'S'/'s' PxScene::simulate in and out, 'F'/'f'
//! fetchResults in and out, 'R' the instances' records sent, 'U' the frame submit (a frame's end).
use core::sync::atomic::{AtomicI32, Ordering::*};
use cvr_host::log;
use red4ext_abi::stack::StackFrame;
use std::sync::Mutex;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

const MOST_EVENTS: usize = 1 << 16;
const MOST_FRAMES: i32 = 120;

#[derive(Clone, Copy)]
struct Event {
    qpc: i64,
    thread: u32,
    what: u8,
}
static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());
static FRAMES_LEFT: AtomicI32 = AtomicI32::new(0);

fn dump() {
    // Late notes from other threads may still land; a short wait lets them, and they are past the last submit anyway.
    std::thread::sleep(std::time::Duration::from_millis(1));
    let mut events = core::mem::take(&mut *EVENTS.lock().unwrap_or_else(|e| e.into_inner()));
    events.sort_by_key(|e| e.qpc);
    let n = events.len();
    let (_, frequency) = super::physhands::qpc();
    let to_ms = 1000.0 / frequency as f64;
    let mut threads: Vec<u32> = Vec::new();
    let game = events.iter().find(|e| e.what == b'U').map_or(0, |e| e.thread);
    let mut label = |t: u32| -> char {
        if t == game {
            return 'G';
        }
        if let Some(k) = threads.iter().position(|&x| x == t) {
            return (b'a' + k as u8) as char;
        }
        if threads.len() < 26 {
            threads.push(t);
            return (b'a' + (threads.len() - 1) as u8) as char;
        }
        '?'
    };
    #[derive(Clone, Copy, Default)]
    struct Seen {
        first: f64,
        last: f64,
        count: i32,
        thread: char,
    }
    let mut start = 0i64;
    let mut frame = 0;
    let mut seen = [Seen::default(); 128];
    for e in &events {
        if start == 0 {
            if e.what == b'U' {
                start = e.qpc;
            }
            continue;
        }
        let t = (e.qpc - start) as f64 * to_ms;
        let s = &mut seen[(e.what & 127) as usize];
        if s.count == 0 {
            s.first = t;
            s.thread = label(e.thread);
        }
        s.last = t;
        s.count += 1;
        if e.what != b'U' {
            continue;
        }
        frame += 1;
        let mut line = format!("[frameorder] frame {}, {:.2} ms:", frame, t);
        let mut order: Vec<u8> = (0..128u8).filter(|&c| seen[c as usize].count != 0).collect();
        order.sort_by(|&a, &b| seen[a as usize].first.total_cmp(&seen[b as usize].first));
        for c in order {
            if line.len() >= 1024 - 48 {
                break;
            }
            let s = seen[c as usize];
            if s.count > 1 {
                line += &format!(" {} {:.2}..{:.2} x{} ({})", c as char, s.first, s.last, s.count, s.thread);
            } else {
                line += &format!(" {} {:.2} ({})", c as char, s.first, s.thread);
            }
        }
        log!("{}\n", line);
        seen = [Seen::default(); 128];
        start = e.qpc;
    }
    let mut names = format!("[frameorder] {} events; threads: G the submit's ({})", n, game);
    for (k, t) in threads.iter().enumerate() {
        if names.len() >= 512 - 24 {
            break;
        }
        names += &format!(", {} {}", (b'a' + k as u8) as char, t);
    }
    log!("{}\n", names);
}

/// Note: one event of this frame, while frames are being recorded.
pub fn note(what: u8) {
    if FRAMES_LEFT.load(Relaxed) <= 0 {
        return;
    }
    let (qpc, _) = super::physhands::qpc();
    // SAFETY: the calling thread's id.
    let thread = unsafe { GetCurrentThreadId() };
    let mut events = EVENTS.lock().unwrap_or_else(|e| e.into_inner());
    if events.len() < MOST_EVENTS {
        events.push(Event { qpc, thread, what });
    }
}
/// cvr_frame_order_note: the C++ half's notes (the character rig, the VR menu).
#[no_mangle]
pub extern "C" fn cvr_frame_order_note(what: u8) {
    note(what);
}
/// NoteSubmit: the frame submit, a frame's end; the last one writes the order out.
pub fn note_submit() {
    if FRAMES_LEFT.load(Relaxed) <= 0 {
        return;
    }
    note(b'U');
    if FRAMES_LEFT.fetch_sub(1, SeqCst) == 1 {
        dump();
    }
}
/// Start: records that many frames' order of events into the log (the step hooked first).
pub fn start(frames: i32) -> bool {
    if !super::physx_step::ensure() {
        return false;
    }
    {
        let mut events = EVENTS.lock().unwrap_or_else(|e| e.into_inner());
        events.clear();
        events.reserve(MOST_EVENTS);
    }
    // one more than asked: the first submit only opens the first frame
    FRAMES_LEFT.store(frames.clamp(1, MOST_FRAMES) + 1, SeqCst);
    true
}
/// VRFrameOrderProbe(frames) -> Bool: records that many frames' order of events into the log.
#[no_mangle]
pub unsafe extern "C" fn VRFrameOrderProbe(_this: *mut core::ffi::c_void, frame: *mut StackFrame, out: *mut bool, _: i64) {
    let mut frames = 0i32;
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut frames);
        frame.finish_params();
    }
    let ok = start(frames);
    if !out.is_null() {
        *out = ok;
    }
}
