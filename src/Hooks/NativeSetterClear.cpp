#if !CVR_RUST_VRIK   // the Rust build's tracer: rust/crates/cvr-render/src/setter_tracers.rs (registered in hook_table.rs)
// NativeSetterClear -- one hook, one file. It registers itself at the bottom; Hooks/Hook.hpp says why the
// stage and order are declared here rather than in a boot function.
//
// The clear half of the same tracer. Off in shipped builds.

#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Utils/AobScanner.hpp"
#include "Utils/MemorySafe.hpp"
#include "Core/Telemetry.hpp"

#include <windows.h>
#include <cstdint>
#include <cstddef>

bool InstallNativeSetterClearHook() {
    const char* pattern = "\xCC\x48\x83\x22\x00\x48\x83\x62\x08\x00\xC3\xCC";
    const char* mask = "xxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    uint8_t* hookAt = found + 1;
    constexpr int replaceLen = 9; // and qword ptr [rdx],00 / and qword ptr [rdx+08],00
    void* tramp = AllocateTrampoline(hookAt, 256);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    code[pos++] = 0x50; // push rax
    code[pos++] = 0x41; code[pos++] = 0x53; // push r11
    WriteMovR11Imm64(code, pos, reinterpret_cast<uintptr_t>(g_setterTrace) + kClearTraceOffset);
    code[pos++] = 0x41; code[pos++] = 0xFF; code[pos++] = 0x03; // inc dword ptr [r11]
    code[pos++] = 0x49; code[pos++] = 0x89; code[pos++] = 0x53; code[pos++] = 0x08; // mov [r11+08],rdx
    code[pos++] = 0x48; code[pos++] = 0x8B; code[pos++] = 0x44; code[pos++] = 0x24; code[pos++] = 0x10; // mov rax,[rsp+10h]
    code[pos++] = 0x49; code[pos++] = 0x89; code[pos++] = 0x43; code[pos++] = 0x10; // mov [r11+10],rax
    code[pos++] = 0x41; code[pos++] = 0x5B; // pop r11
    code[pos++] = 0x58; // pop rax

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0x22; code[pos++] = 0x00; // and qword ptr [rdx],00
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0x62; code[pos++] = 0x08; code[pos++] = 0x00; // and qword ptr [rdx+08],00

    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((hookAt + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(hookAt, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    hookAt[0] = 0xE9;
    *reinterpret_cast<int32_t*>(hookAt + 1) = static_cast<int32_t>(code - (hookAt + 5));
    for (int i = 5; i < replaceLen; ++i) hookAt[i] = 0x90;
    VirtualProtect(hookAt, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), hookAt, replaceLen);
    return true;
}
// THE GUARD THIS HOOK ALWAYS HAD -- the boot wrapped all three of these in
// `if (kEnableNativeSetterTracers != 0)`. It travels with the hook now, so the condition cannot be
// lost by a boot function forgetting it.
namespace {
constexpr int kEnableNativeSetterTracers = 0;
bool NativeSetterTracersWanted() { return kEnableNativeSetterTracers != 0; }
}  // namespace

CVR_HOOK_IF("NativeSetterClear", ::cvr::hooks::Stage::Boot, 97, InstallNativeSetterClearHook, NativeSetterTracersWanted);
#endif
