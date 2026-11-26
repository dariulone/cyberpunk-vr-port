//! The parts of single pass's native stereo probe (src/Render/NativeStereoProbe.cpp) moved so far: the
//! test-process switch that lets the binding observers run without the probe's GPU assets.
use std::sync::atomic::{AtomicU8, Ordering};

// Asked on every draw and descriptor copy, so a plain load rather than a lazy cell: 0 = not read yet, 1 = off,
// 2 = on. Concurrent first readers store the same value.
static OBSERVE_BINDINGS: AtomicU8 = AtomicU8::new(0);

#[cold]
#[inline(never)]
fn read_observe_bindings() -> u8 {
    let state = if std::env::var_os("CVR_STEREO_OBSERVE_BINDINGS").is_some_and(|v| v == "1") {
        2
    } else {
        1
    };
    OBSERVE_BINDINGS.store(state, Ordering::Relaxed);
    state
}

/// native_probe::ObserveBindings: CVR_STEREO_OBSERVE_BINDINGS=1 in the process environment (read once).
pub fn observe_bindings() -> bool {
    let mut state = OBSERVE_BINDINGS.load(Ordering::Relaxed);
    if state == 0 {
        state = read_observe_bindings();
    }
    state == 2
}

#[unsafe(no_mangle)]
pub extern "C" fn cvr_observe_bindings() -> bool {
    observe_bindings()
}
