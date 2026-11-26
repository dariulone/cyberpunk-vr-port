#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/PhotoModeCapture.hpp"
#include "Hooks/Trampoline.hpp"
#include "Utils/AobScanner.hpp"
#include "Utils/DebugGate.hpp"
#include "Utils/MemorySafe.hpp"

#include <MinHook.h>
#include <atomic>
#include <cstdint>
#include <cstring>
#include <limits>

#if !CVR_RUST_VRIK
extern "C" UINT GetForcedWindowWidth();
extern "C" UINT GetForcedWindowHeight();
#endif

#if !CVR_RUST_VRIK
extern "C" {
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PhotoCaptureSizeOverrides{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PhotoCaptureClampOverrides{};
#endif
#if !CVR_RUST_VRIK
}
#endif
#if CVR_RUST_VRIK
// The policies, the size detour's install and the clamp call's relay are rust/crates/cvr-rig/src/vrik/photo_capture.rs
// (with the thunks' originals); the thunks (src/Hooks/PhotoCaptureClampThunk.asm), the counters above and the
// registration stay.
#if !CVR_RUST_VRIK   // registered from rust/crates/cvr-render/src/hook_table.rs
CVR_HOOK("PhotoModeCapture", ::cvr::hooks::Stage::Boot, 86, cvr_hook_photo_mode_capture_install);
#endif
#else
extern "C" {
void* CvrPhotoClampOriginal{};
void CvrPhotoClampDetour();
void* CvrPhotoSizeOriginal{};
void CvrPhotoSizeDetour();   // PhotoCaptureClampThunk.asm: the native query, then CvrPhotoSizePolicy, register-neutral
}

namespace {
struct CaptureSize {
    uint32_t width;
    uint32_t height;
};
const auto gameBase = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
constexpr uintptr_t ReadbackClampReturn = 0x20B5BE2;

uint32_t CurrentCaptureType() {
    uintptr_t renderer{}, capture{};
    uint32_t type{};
    // The renderer owns the capture manager. Read it afresh so a saved pointer
    // cannot outlive an engine/session reset. The descriptor starts at +190.
    if (!ReadPtrSafe(gameBase + 0x3BBFB38, &renderer) || !renderer ||
        !ReadPtrSafe(renderer + 0x320, &capture) || !capture ||
        !ReadU32Safe(capture + 0x1C4, &type)) return 0;
    return type;
}

bool SkipReadbackClamp(uint32_t* width, uint32_t* height) {
    // The native screenshot path also fits its CPU image into 3840x2160.
    // VR keeps the actual texture at the launcher size: at 2560x2560 that
    // produced a 2160x2160 PNG containing rows with a 2560-pixel stride.
    // Called only from the readback CALL site, through a register-preserving
    // thunk. Never detour the shared helper: the world-UI mesh picker keeps its
    // window pointer in R8 across that leaf call (RVA2446763). An ordinary C++
    // entry detour clobbered it even when it chose the original function.
    const UINT forcedWidth = GetForcedWindowWidth();
    const UINT forcedHeight = GetForcedWindowHeight();
    if (width && height && cvr::capture::PreserveReadbackSize(
        true, CurrentCaptureType(), *width, *height, forcedWidth, forcedHeight)) {
        CVR_DIAGNOSTIC(CyberpunkVR_PhotoCaptureClampOverrides.fetch_add(1, std::memory_order_relaxed));
        return true;
    }
    return false;
}

}  // namespace

// After the native capture-size query returned `requested` (CvrPhotoSizeDetour restores its registers afterwards).
extern "C" void CvrPhotoSizePolicy(uintptr_t capture, void* sizeOut, bool requested) {
    auto* size = static_cast<CaptureSize*>(sizeOut);
    uint8_t active{};
    uint32_t type{};
    if (!requested || !size || !ReadU8Safe(capture + 0x1C0, &active) ||
        !ReadU32Safe(capture + 0x1C4, &type))
        return;
    // Only Photo Mode's own request is resized; the savegame thumbnail (type 3) and every other query keep the
    // native size and never reach the VR size code.
    if (!active || type != cvr::capture::PhotoModeRequest) return;

    // Native Photo Mode capture (2.31, +224DEF4) asks for its screenshot preset.
    // The render job (+1E4E66D) retries a resize and skips the frame until that
    // size matches the active render size. SettingsRes / DXGI keep the VR size,
    // so a 1920x1080 request against 2560x2560 never reaches image readback.
    // Use the same dimensions as those overrides, preserving the game's capture,
    // warm-up, encoding and completion callback. Other size queries stay native.
    const UINT width = GetForcedWindowWidth();
    const UINT height = GetForcedWindowHeight();
    if (cvr::capture::OverrideSize(requested, active != 0, type, width, height) &&
        (size->width != width || size->height != height)) {
        size->width = width;
        size->height = height;
        CVR_DIAGNOSTIC(CyberpunkVR_PhotoCaptureSizeOverrides.fetch_add(1, std::memory_order_relaxed));
    }
}

namespace {

bool InstallPhotoModeCapture() {
    // 2.5 sub_1420B5AE0 (2.31 sub_14224DEF4): the same fields (+1C0 active, +210 preset, +124 size), recompiled
    // to keep the size pointer in r9 only.
    const char* pattern = "\x48\x83\xEC\x28\x80\xB9\xC0\x01\x00\x00\x00\x4C\x8B\xCA\x74\x2B\x8B\x89\x10";
    const char* mask = "xxxxxxxxxxxxxxxxxxx";
    void* target = FindPattern("Cyberpunk2077.exe", pattern, mask);
    if (!target) return false;
    const char* clampPattern = "\x81\x39\x00\x0F\x00\x00\x77\x08\x81\x3A\x70\x08\x00\x00\x76\x50";
    // Verify the complete entry before using the 2.31 RVA; several unrelated
    // image utilities compare against 3840 too.
    auto* clamp = reinterpret_cast<uint8_t*>(GetModuleHandleW(nullptr)) + 0x27FEB44;
    auto* call = reinterpret_cast<uint8_t*>(gameBase + ReadbackClampReturn - 5);
    int32_t displacement{};
    std::memcpy(&displacement, call + 1, sizeof(displacement));
    if (std::memcmp(clamp, clampPattern, 16) != 0 || call[0] != 0xE8 ||
        gameBase + ReadbackClampReturn + displacement != reinterpret_cast<uintptr_t>(clamp)) return false;

    // A CALL to this nearby register-neutral relay retains the original return
    // address. The DLL thunk has normal unwind metadata and preserves the leaf
    // register contract around its C++ policy, then tail-calls the real helper.
    auto* relay = static_cast<uint8_t*>(AllocateTrampoline(call, 14));
    if (!relay) return false;
    const auto relative = reinterpret_cast<intptr_t>(relay) - reinterpret_cast<intptr_t>(call + 5);
    if (relative < std::numeric_limits<int32_t>::min() || relative > std::numeric_limits<int32_t>::max()) return false;
    const uint8_t jump[] = {0xff,0x25,0,0,0,0};
    std::memcpy(relay,jump,sizeof(jump));
    const auto thunk = reinterpret_cast<uintptr_t>(&CvrPhotoClampDetour);
    std::memcpy(relay+6,&thunk,sizeof(thunk));
    FlushInstructionCache(GetCurrentProcess(),relay,14);
    CvrPhotoClampOriginal = clamp;
    if (MH_CreateHook(target,reinterpret_cast<void*>(&CvrPhotoSizeDetour),&CvrPhotoSizeOriginal) != MH_OK) return false;
    if (MH_EnableHook(target) != MH_OK) { MH_RemoveHook(target); return false; }
    uint8_t before[5], after[5] = {0xe8};
    std::memcpy(before,call,sizeof(before));
    const auto callOffset = static_cast<int32_t>(relative);
    std::memcpy(after+1,&callOffset,sizeof(callOffset));
    DWORD protection{};
    if (!VirtualProtect(call,sizeof(after),PAGE_EXECUTE_READWRITE,&protection)) {
        MH_DisableHook(target); MH_RemoveHook(target); return false;
    }
    std::memcpy(call,after,sizeof(after));
    DWORD ignored{};
    if (!VirtualProtect(call,sizeof(after),protection,&ignored)) {
        std::memcpy(call,before,sizeof(before));
        VirtualProtect(call,sizeof(before),protection,&ignored);
        FlushInstructionCache(GetCurrentProcess(),call,sizeof(before));
        MH_DisableHook(target); MH_RemoveHook(target); return false;
    }
    FlushInstructionCache(GetCurrentProcess(),call,sizeof(after));
    Log("[photo-capture] readback-only clamp call installed; native UI clamp untouched\n");
    return true;
}

CVR_HOOK("PhotoModeCapture", ::cvr::hooks::Stage::Boot, 86, InstallPhotoModeCapture);
}

extern "C" bool CvrPhotoClampSkip(uint32_t* width, uint32_t* height) {
    return SkipReadbackClamp(width,height);
}
#endif
#endif
