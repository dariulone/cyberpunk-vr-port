//! Services every Rust DLL of the port needs before it does anything game-specific: where its folders are
//! (`paths`, formerly src/Core/Paths.cpp), the log (`log`, src/Core/Log.cpp's `Log`), a guard that keeps a Rust
//! panic from unwinding into the engine (`guard`), and the script compiler hand-off (`scripts`,
//! src/Core/ScriptCompiler.cpp), and the launcher's DEBUG gate over the diagnostic flags (`debug_gate`,
//! src/Utils/DebugGate.cpp), and the probe's internal-command flag the D3D12 hooks honour (`probe`,
//! include/Render/RenderProbeScope.hpp).
/// The release this build is (the package's version; Cargo's own stays three-part semver).
pub const RELEASE: &str = "0.2.5.5";

pub mod debug_gate;
pub mod fast;
pub mod guard;
pub mod hitch;
pub mod log;
pub mod paths;
pub mod probe;
pub mod rifle_elbow;
pub mod scripts;
pub mod snap_turn;
pub mod submission;
pub mod wide;

/// The calling thread named for debuggers and profilers (main's c41b2e23d: the VR worker threads named).
pub fn name_this_thread(name: &str) {
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: the current thread's pseudo handle and a NUL-terminated name alive for the call.
    unsafe {
        windows_sys::Win32::System::Threading::SetThreadDescription(windows_sys::Win32::System::Threading::GetCurrentThread(), wide.as_ptr());
    }
}
