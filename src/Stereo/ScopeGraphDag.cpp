// ScopeGraphDag -- the camera scope's render graph built with MAIN's dependencies, not as one chain.
//
// The scene graph builder sub_142276E60(owner, graph, builder, features = view+0x25B0, view data) adds a
// view's passes and then wires the jobs that run them. With feature bit 64 (f1 bit 0, the bit that also
// adds the RTT colour extraction pass) it calls sub_1408EA568, which makes every node wait for the one
// before it, and skips its dependency rules (0x14227B6FF: sub_1403C1C00(features, 64)). The scope's view
// is an RTT view with that bit, so its 63 nodes ran as one chain: longest chain 63, widest level 1
// (tools/hitch_tests/graph_shape.py), one worker at a time for the whole 12 ms window while MAIN's 87
// nodes form a graph 23 deep and 23 wide. The rules cover every node the scope adds: its one extra node
// (the extraction pass, sub_1408EAF74) has its own rule at the end of the builder (0x14227BCB1).
//
// With xr_scope_graph_dag (CyberpunkVR_ScopeGraphDag, default on), that one call answers no when the view being built is the scope's
// (its camera name, [features-2588h], as the G-buffer partition patch in Render/StageReplay.cpp reads
// it), and the builder takes its dependency rules. The call goes to a stub instead of a detour of
// sub_1403C1C00: callers keep values in volatile registers across that leaf function (0x1405C4B67 keeps
// r9), so only a stub that touches nothing but rax and the flags, as the function itself, can stand in
// for it. Every other test of the bit (the extraction pass itself) is left alone. A graph is built when
// its view's features change, so the switch applies to the next build (drawing the weapon again).

#include "Stereo/VrcamConfig.hpp"
#include "Utils/StereoLog.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include <windows.h>
#include <atomic>
#include <cstdint>
#include <cstring>
#include <initializer_list>

extern "C" {
// 1 = the scope's graph gets MAIN's dependency rules (next build). Read by the stub, live; set from
// xr_scope_graph_dag (Core/LiveControlsPoll.cpp).
__declspec(dllexport) volatile uint32_t CyberpunkVR_ScopeGraphDag = 1;
// 0 not installed, 1 installed, 2 the site did not match.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ScopeGraphDagState{0};
}

namespace {

constexpr uintptr_t SERIAL_TEST_CALL_RVA = 0x227B6FF;   // call sub_1403C1C00, in sub_142276E60
constexpr uintptr_t FEATURE_TEST_RVA = 0x3C1C00;

bool InstallScopeGraphDag() {
    auto* exe = reinterpret_cast<uint8_t*>(GetModuleHandleW(nullptr));
    auto* site = exe + SERIAL_TEST_CALL_RVA;
    // mov edx, 40h ; mov rcx, r12 ; call sub_1403C1C00
    static const uint8_t expected[8] = {0xBA, 0x40, 0x00, 0x00, 0x00, 0x49, 0x8B, 0xCC};
    if (std::memcmp(site - 8, expected, sizeof(expected)) != 0 || site[0] != 0xE8 ||
        site + 5 + *reinterpret_cast<const int32_t*>(site + 1) != exe + FEATURE_TEST_RVA) {
        CyberpunkVR_ScopeGraphDagState.store(2);
        return false;
    }
    auto* code = static_cast<uint8_t*>(AllocateTrampoline(site, 64));
    if (!code) { CyberpunkVR_ScopeGraphDagState.store(2); return false; }
    int pos = 0;
    int toOriginal[2];
    const auto put = [&](std::initializer_list<uint8_t> bytes) { for (uint8_t b : bytes) code[pos++] = b; };
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(&CyberpunkVR_ScopeGraphDag));
    put({0x83, 0x38, 0x00});                                                            // cmp dword [rax], 0
    code[pos++] = 0x74; toOriginal[0] = pos++;                                          // je original
    WriteMovRaxImm64(code, pos, cvr::cname_hash("vr_optics_feed"));
    put({0x48, 0x3B, 0x81, 0x78, 0xDA, 0xFF, 0xFF});                                    // cmp rax, [rcx-2588h]
    code[pos++] = 0x75; toOriginal[1] = pos++;                                          // jne original
    put({0x31, 0xC0, 0xC3});                                                            // xor eax, eax ; ret
    for (int k = 0; k < 2; ++k) code[toOriginal[k]] = static_cast<uint8_t>(pos - (toOriginal[k] + 1));
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(exe + FEATURE_TEST_RVA)); // original:
    put({0xFF, 0xE0});                                                                  // jmp rax
    DWORD old;
    VirtualProtect(site, 5, PAGE_EXECUTE_READWRITE, &old);
    *reinterpret_cast<int32_t*>(site + 1) = static_cast<int32_t>(code - (site + 5));
    VirtualProtect(site, 5, old, &old);
    FlushInstructionCache(GetCurrentProcess(), site, 5);
    CyberpunkVR_ScopeGraphDagState.store(1);
    return true;
}

}  // namespace

CVR_HOOK("ScopeGraphDag", ::cvr::hooks::Stage::Boot, 50, InstallScopeGraphDag);
