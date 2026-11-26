//! The probe's internal-command flag (include/Render/RenderProbeScope.hpp, `stereo::probe::internalCommands`): a
//! thread-local of the C++ half that marks the port's own D3D12 commands, so the device and command-list hooks leave
//! them alone. While C++ code of the build reads it, the flag is C++'s: RenderProbeScope.hpp hands Rust its two
//! accessors while the DLL initialises (any number of times, always the same pair, from every translation unit that
//! includes it). Unattached -- no such translation unit left -- the flag is a thread-local of Rust's own.
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

thread_local! {
    /// The calling thread's flag while no C++ accessors are attached.
    static OWN: Cell<bool> = const { Cell::new(false) };
}

static GET: AtomicUsize = AtomicUsize::new(0);
static SET: AtomicUsize = AtomicUsize::new(0);

/// The C++ accessors of the calling thread's flag.
#[no_mangle]
pub extern "C" fn cvr_probe_internal_attach(get: extern "C" fn() -> bool, set: extern "C" fn(bool) -> bool) {
    GET.store(get as usize, Ordering::Release);
    SET.store(set as usize, Ordering::Release);
}

/// Whether the calling thread is issuing the port's own commands.
pub fn internal() -> bool {
    match GET.load(Ordering::Acquire) {
        0 => OWN.with(Cell::get),
        // SAFETY: the accessor RenderProbeScope.hpp attached.
        f => unsafe { core::mem::transmute::<usize, extern "C" fn() -> bool>(f)() },
    }
}
/// Sets the calling thread's flag; the previous value returned.
pub fn set_internal(on: bool) -> bool {
    set(on)
}
fn set(on: bool) -> bool {
    match SET.load(Ordering::Acquire) {
        0 => OWN.with(|flag| flag.replace(on)),
        // SAFETY: the accessor RenderProbeScope.hpp attached.
        f => unsafe { core::mem::transmute::<usize, extern "C" fn(bool) -> bool>(f)(on) },
    }
}

/// stereo::probe::Scope: the calling thread's commands are the port's own until it drops.
pub struct Internal(bool);
impl Internal {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Internal(set(true))
    }
}
impl Drop for Internal {
    fn drop(&mut self) {
        set(self.0);
    }
}
