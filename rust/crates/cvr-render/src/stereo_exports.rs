//! Exported variables of the stereo module defined on the Rust side (formerly definitions in src/Stereo/SyncStereo.cpp).
//! The C++ headers keep declaring them `extern "C" __declspec(dllexport) extern`, and that declaration is what puts
//! them in the DLL's export table, as it did when C++ defined them.
use core::sync::atomic::AtomicU32;

/// 1 when the VRCAM view key came from vrcam.json, 0 while the built-in default is in use.
#[no_mangle]
pub static CyberpunkVR_DebugVrcamConfigLoaded: AtomicU32 = AtomicU32::new(0);
