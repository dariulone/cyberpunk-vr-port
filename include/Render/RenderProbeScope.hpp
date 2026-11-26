#pragma once
namespace cvr::stereo::probe {
inline thread_local bool internalCommands=false;
struct Scope {
    bool previous=internalCommands;
    Scope(){internalCommands=true;}
    ~Scope(){internalCommands=previous;}
};
#if CVR_RUST_VRIK
// Rust's D3D12 passes mark their own commands through this thread-local too (rust/crates/cvr-host/src/probe.rs): every
// translation unit that includes this header hands Rust the same two accessors while the DLL initialises.
namespace detail {
inline bool InternalCommands(){return internalCommands;}
inline bool SetInternalCommands(bool on){const bool previous=internalCommands;internalCommands=on;return previous;}
}
}
extern "C" void cvr_probe_internal_attach(bool (*get)(),bool (*set)(bool));
namespace cvr::stereo::probe::detail {
[[maybe_unused]] inline const bool attached=(cvr_probe_internal_attach(&InternalCommands,&SetInternalCommands),true);
#endif
}
