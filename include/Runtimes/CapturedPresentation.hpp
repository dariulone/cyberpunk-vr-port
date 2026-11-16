#pragma once
#include <cstdint>

namespace cvr::capture {
// Travels with the pixels under the capture lock. Live game/menu flags may
// already describe a different frame when the XR thread submits this one.
enum class Presentation : uint8_t { Scene, Menu, Loading, Notice, Dark };
constexpr Presentation Classify(bool menu, bool loadingScreen, bool notice, bool dark) {
    if (loadingScreen) return notice ? Presentation::Notice : Presentation::Loading;
    if (dark) return Presentation::Dark;
    return menu ? Presentation::Menu : Presentation::Scene;
}
constexpr bool IsLoading(Presentation kind) {
    return kind==Presentation::Loading || kind==Presentation::Notice || kind==Presentation::Dark;
}
constexpr bool CanGenerate(Presentation selected, Presentation newest) {
    // The runtime queue may select an older completed capture. Do not carry a
    // pending generated scene endpoint across a newly published menu/load.
    return selected==Presentation::Scene && newest==Presentation::Scene;
}
constexpr bool CanHold(Presentation kind, int64_t ageNs) {
    // A loading pause can last seconds. Its stationary screen/black curtain
    // remains valid; dropping the layer lets the runtime show its fallback.
    return ageNs>=0 && (IsLoading(kind) || ageNs<=1000000000);
}
constexpr bool CanPair(Presentation frame, Presentation eye, uint64_t serial, uint64_t eyeSerial, uint32_t reuse) {
    if (!eyeSerial || eyeSerial>serial || frame!=eye) return false;
    // Non-scene images require their exact partner, even across a transition
    // back to gameplay. Never show an old street/loading screen in one eye.
    return eyeSerial==serial || (frame==Presentation::Scene && serial-eyeSerial<=reuse);
}
} // namespace cvr::capture
