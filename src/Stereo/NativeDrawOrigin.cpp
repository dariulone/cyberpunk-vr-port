#include "Render/NativeDrawOrigin.hpp"
#include "Render/NativeEarlyDepth.hpp"
#include "Stereo/DetourRegistry.hpp"
#include <windows.h>
#if CVR_RUST_VRIK
// The game-code contracts: rust/crates/cvr-render/src/stereo_code.rs (0 draw origin, 1 bounds, 2 particles, 3 scene
// update; through ReadProcessMemory).
#endif

extern "C" {
#if !CVR_RUST_VRIK   // the Rust build's are rust/crates/cvr-render/src/engine_detours.rs's (the thunk reads them by symbol)
void* CvrNativeDrawOriginal{};
uint64_t CvrNativeDrawExpectedCaller{};
void CvrNativeDrawDetour();
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's callbacks are rust/crates/cvr-render/src/engine_detours.rs'
void CvrNativeDrawBefore(const cvr::stereo::packets::DrawEntryFrame* frame){
    using namespace cvr::stereo;
    packets::EnterDrawOrigin(early_depth::Drawing()?packets::OriginFromEntry(*frame,reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr))):packets::DrawOrigin{});
}
void CvrNativeDrawAfter(){cvr::stereo::packets::LeaveDrawOrigin();}
#endif
}
#if !CVR_RUST_VRIK   // the Rust build registers the draw origin, with its predicate, from engine_detours.rs
namespace cvr::detail {
namespace {
bool OriginCompatible(){
    if(!GetEnvironmentVariableW(L"CVR_STEREO_PAIRED_ROOT",nullptr,0))return false;
#if CVR_RUST_VRIK
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    const bool valid=cvr_stereo_code(0,base);CvrNativeDrawExpectedCaller=valid?base+0x335496:0;
#else
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));std::array<uint8_t,0xF0> bytes{};SIZE_T copied{};
    bool valid=ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<void*>(base+0x335900),bytes.data(),bytes.size(),&copied) && copied==bytes.size();
    uint64_t hash=14695981039346656037ull;for(auto byte:bytes)hash=(hash^byte)*1099511628211ull;
    valid=valid && hash==0xCF41106E47A35853ull;
    std::array<uint8_t,0x168> caller{};copied=0;
    valid=valid && ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<void*>(base+0x3353D8),caller.data(),caller.size(),&copied) && copied==caller.size();
    hash=14695981039346656037ull;for(auto byte:caller)hash=(hash^byte)*1099511628211ull;
    valid=valid && hash==0xB1E1FF97629AB0DAull;CvrNativeDrawExpectedCaller=valid?base+0x335496:0;
#endif
    CyberpunkVR_GeometryOriginCompatible=valid?1:2;return valid;
}
CVR_DETOUR_IF("[single-pass] scoped native draw origin",0x335900,CvrNativeDrawDetour,CvrNativeDrawOriginal,OriginCompatible)
}
}
#endif
