#pragma once
#include <array>
#include <cstdint>
#include <cstring>
#include <limits>

namespace cvr::scope_dlss {
// Separate from MAIN(0), VRCAM(1), and the SBS eye viewports(CVR01/02).
inline constexpr uint32_t kViewport = 0x43565301u;

// CP2.5 native DLSS state, verified in the camera writer and tag/eval driver.
// These ranges contain camera/SL constants and numeric options, not GPU owners.
inline constexpr size_t kCamera = 0x20, kCache = 0x3c8, kOptions = 0x3e8;
// Native handle starts at480; BaseStructure is32 bytes. 498 is its
// size_t structVersion, NOT the viewport value (the pre-2.5 offset was stale).
inline constexpr size_t kEnd = 0x40c, kViewportOffset = 0x4a0, kToken = 0x4a8;
template<class T> T Read(const uint8_t* p, size_t at) {
    T v; std::memcpy(&v, p + at, sizeof(v)); return v;
}
template<class T> void Write(uint8_t* p, size_t at, T v) {
    std::memcpy(p + at, &v, sizeof(v));
}

struct State {
    std::array<uint8_t, kOptions - kCamera> cameraAndCache{};
    std::array<uint8_t, kEnd - kOptions> options{};
    uintptr_t owner = 0, view = 0;
    uint32_t frame = 0;
    bool constants = false, reset = true;

    void Select(uintptr_t nextOwner, uintptr_t nextView) {
        if (owner != nextOwner || view != nextView) {
            *this = {};
            owner = nextOwner; view = nextView;
        }
    }
    void PrepareFrame(uint32_t next) {
        reset = !constants || (next != frame && next != frame + 1u);
        frame = next;
    }
};

// Caller holds the engine's native recursive DLSS critical section throughout.
// Restore MAIN even if the native evaluation exits early or throws C++ exceptions.
class Swap {
    uint8_t* native;
    State& scope;
    std::array<uint8_t, kEnd - kCamera> saved;
    uint32_t viewport;
    uint64_t token;
    uint8_t fgAvailable, reflexAvailable;
public:
    Swap(uint8_t* p, State& s) : native(p), scope(s),
        viewport(Read<uint32_t>(p,kViewportOffset)), token(Read<uint64_t>(p,kToken)),
        fgAvailable(p[0x43b]), reflexAvailable(p[0x43c]) {
        std::memcpy(saved.data(), p+kCamera, saved.size());
        if (s.constants) std::memcpy(p+kCamera,s.cameraAndCache.data(),s.cameraAndCache.size());
        // Never let MAIN's dimensions suppress initial scope configuration.
        if (!s.constants || std::memcmp(s.options.data(),p+kOptions,s.options.size()) != 0)
            std::memset(p+kCache,0,kOptions-kCache);
        std::memcpy(s.options.data(),p+kOptions,s.options.size());
        Write(p,kViewportOffset,kViewport);
        // RTT's native per-view token can remain zero. Always upload its camera.
        Write(p,kToken,std::numeric_limits<uint64_t>::max());
        // Native constants driver also configures FG/Reflex when available.
        // The scope is an upscale-only viewport, never a second presentation.
        p[0x43b]=0; p[0x43c]=0;
    }
    ~Swap() {
        std::memcpy(scope.cameraAndCache.data(),native+kCamera,scope.cameraAndCache.size());
        std::memcpy(native+kCamera,saved.data(),saved.size());
        Write(native,kViewportOffset,viewport);
        Write(native,kToken,token);
        native[0x43b]=fgAvailable; native[0x43c]=reflexAvailable;
    }
    Swap(const Swap&) = delete;
    Swap& operator=(const Swap&) = delete;
};
}
