//! The PSVR2 haptic channel (formerly include/Runtimes/Psvr2HapticChannel.hpp's HapticChannel, through
//! Runtimes/HapticsDevice.cpp's and OpenXRFrameLoop.cpp's services): the same payload and heartbeat contract as the
//! user's Toolkit slot-guard fork, in a different named mapping (never aliasing upstream's live input slots). The
//! mapping is made on first use, as the C++ function-local static was.

use core::ptr::{null, null_mut, read_volatile, write_volatile};
use std::sync::Mutex;

/// The mapping's name.
const HAPTIC_MAPPING: &[u8] = b"Local\\CyberpunkVR_PSVR2_Haptics_017_v1\0";
const BYTES: usize = 1024;

/// The v1 bridge polls one mailbox every 4 ms: one pending impact per hand, published at most every 8 ms so
/// simultaneous hands cannot overwrite each other (main's HapticPulseQueue).
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct PulseEvent {
    pub hand: i32,
    pub amplitude: f32,
    pub duration: i32,
    pub queued: u64,
    pub order: u64,
}
#[derive(Default)]
pub struct PulseQueue {
    pending: [PulseEvent; 2],
    order: u64,
    sent_at: u64,
    sent: bool,
}
impl PulseQueue {
    pub const fn new() -> Self {
        const E: PulseEvent = PulseEvent { hand: 0, amplitude: 0.0, duration: 0, queued: 0, order: 0 };
        Self { pending: [E; 2], order: 0, sent_at: 0, sent: false }
    }
    pub fn push(&mut self, hand: i32, amplitude: f32, duration: i32, now: u64) {
        if !(0..=1).contains(&hand) || !amplitude.is_finite() || amplitude <= 0.0 || amplitude > 1.0 || !(1..=1000).contains(&duration) {
            return;
        }
        let p = &mut self.pending[hand as usize];
        if p.order == 0 || now < p.queued || now - p.queued > 100 {
            self.order += 1;
            *p = PulseEvent { hand, amplitude, duration, queued: now, order: self.order };
        } else {
            p.amplitude = p.amplitude.max(amplitude);
            p.duration = p.duration.max(duration);
            p.queued = now;
        }
    }
    pub fn pop(&mut self, now: u64) -> Option<PulseEvent> {
        for p in &mut self.pending {
            if p.order != 0 && (now < p.queued || now - p.queued > 100) {
                *p = PulseEvent::default();
            }
        }
        if self.sent && now >= self.sent_at && now - self.sent_at < 8 {
            return None;
        }
        let i = (0..2).filter(|&i| self.pending[i].order != 0).min_by_key(|&i| self.pending[i].order)?;
        let out = self.pending[i];
        self.pending[i] = PulseEvent::default();
        self.sent = true;
        self.sent_at = now;
        Some(out)
    }
}
fn tick_ms() -> u64 {
    // SAFETY: a plain clock read.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}

/// The mapped block (null when the mapping or the view could not be made) and the impacts waiting for it.
struct Channel {
    data: *mut f32,
    pulses: PulseQueue,
}
// SAFETY: the block is only touched under the channel's mutex.
unsafe impl Send for Channel {}

static CHANNEL: Mutex<Option<Channel>> = Mutex::new(None);

/// The channel, made on first use: the mapping, its view zeroed, the contract's magic (168: 18512) and version (169: 1).
fn with<R>(f: impl FnOnce(&mut Channel) -> R) -> R {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Memory::{CreateFileMappingA, MapViewOfFile, FILE_MAP_ALL_ACCESS, PAGE_READWRITE};
    let mut guard = CHANNEL.lock().unwrap_or_else(|p| p.into_inner());
    let channel = guard.get_or_insert_with(|| {
        // SAFETY: a pagefile-backed named mapping and its view; the block zeroed before use.
        unsafe {
            let handle = CreateFileMappingA(INVALID_HANDLE_VALUE, null(), PAGE_READWRITE, 0, BYTES as u32, HAPTIC_MAPPING.as_ptr());
            let mut data: *mut f32 = null_mut();
            if !handle.is_null() {
                data = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, BYTES).Value.cast();
            }
            if !data.is_null() {
                core::ptr::write_bytes(data.cast::<u8>(), 0, BYTES);
                write_volatile(data.add(168), 18512.0);
                write_volatile(data.add(169), 1.0);
            }
            Channel { data, pulses: PulseQueue::new() }
        }
    });
    f(channel)
}
/// A counter slot advanced, wrapping to 1 past a million.
///
/// # Safety
/// `slot` is in the mapped block.
unsafe fn advance(slot: *mut f32) {
    // SAFETY: as documented.
    unsafe {
        let v = read_volatile(slot);
        write_volatile(slot, if v >= 1_000_000.0 { 1.0 } else { v + 1.0 });
    }
}

/// The heartbeat (slot 170), each display frame, and the next impact due: its payload (158-160) published before its
/// sequence (157).
#[no_mangle]
pub extern "C" fn cvr_cpp_psvr2_heartbeat() {
    with(|channel| {
        let data = channel.data;
        if data.is_null() {
            return;
        }
        // SAFETY: slots 157-170 of the 256-float block.
        unsafe {
            advance(data.add(170));
            if let Some(pulse) = channel.pulses.pop(tick_ms()) {
                write_volatile(data.add(158), pulse.hand as f32);
                write_volatile(data.add(159), pulse.amplitude);
                write_volatile(data.add(160), pulse.duration as f32);
                core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
                advance(data.add(157));
            }
        }
    });
}

/// A pulse on one hand (0 or 1): amplitude in (0, 1], 1..=1000 ms, queued for the next heartbeat.
#[no_mangle]
pub extern "C" fn cvr_cpp_psvr2_pulse(hand: i32, amplitude: f32, milliseconds: i32) {
    with(|channel| {
        if !channel.data.is_null() {
            channel.pulses.push(hand, amplitude, milliseconds, tick_ms());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_impact_per_hand_every_8_ms() {
        let mut q = PulseQueue::new();
        q.push(1, 0.5, 30, 1000);
        q.push(0, 0.4, 20, 1001);
        q.push(1, 0.7, 10, 1002); // merged into the right hand's
        let first = q.pop(1003).unwrap();
        assert_eq!((first.hand, first.amplitude, first.duration), (1, 0.7, 30));
        assert!(q.pop(1005).is_none()); // under 8 ms since the last
        assert_eq!(q.pop(1011).unwrap().hand, 0);
        assert!(q.pop(1030).is_none());
        q.push(0, 0.3, 5, 2000);
        assert!(q.pop(2200).is_none()); // stale after 100 ms
        q.push(2, 0.3, 5, 3000);
        q.push(0, 1.5, 5, 3000);
        assert!(q.pop(3000).is_none());
    }
}
