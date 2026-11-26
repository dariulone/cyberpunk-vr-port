// The haptics' one device call (both builds): a segment due on a hand, played where the session's actions are synced.
// PSVR2: the Sense controllers' actuators belong to the PlayStation VR2 Toolkit bridge, which plays what the port sends
// over its own channel (Runtimes/Psvr2HapticChannel.hpp); there is no OpenXR vibration to drive. Counted in
// CyberpunkVR_HapticStats: [1]/[2] segments applied left/right, [3] the last xrApplyHapticFeedback result, [5] dropped.
#include "Runtimes/OpenXRManager.hpp"
#include "Runtimes/Psvr2HapticChannel.hpp"

#include <algorithm>

#if !CVR_RUST_VRIK
extern "C" __declspec(dllexport) int CyberpunkVR_HapticStats[8];
#endif

#if CVR_RUST_VRIK
// The call is Rust's (rust/crates/cvr-render/src/cpp_services.rs, haptics_device); the bridge's channel stays here.
// cvr_cpp_psvr2_pulse: rust/crates/cvr-render/src/psvr2_haptics.rs.
#else
extern "C" void cvr_cpp_haptics_play(int hand, float amplitude, float seconds) {
    if (OpenXRManager::Get().IsRuntimePsvr2()) {
        cvr::psvr2::Haptics().Pulse(hand, amplitude, std::clamp(static_cast<int>(seconds * 1000.0f + 0.5f), 1, 1000));
        ++CyberpunkVR_HapticStats[hand == 0 ? 1 : 2];
        return;
    }
    const int r = OpenXRManager::Get().ApplyHaptic(hand, amplitude, seconds);
    CyberpunkVR_HapticStats[3] = r;
    if (r == 0) ++CyberpunkVR_HapticStats[hand == 0 ? 1 : 2];
    else ++CyberpunkVR_HapticStats[5];
}
#endif