#if !CVR_RUST_VRIK   // the Rust build's tracer: rust/crates/cvr-render/src/setter_tracers.rs (registered in hook_table.rs)
// NativeSetterMetaWrite -- one hook, one file. It registers itself at the bottom; Hooks/Hook.hpp says why the
// stage and order are declared here rather than in a boot function.
//
// A tracer over the engine's native-setter metadata write. Off in shipped builds.

#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Utils/AobScanner.hpp"
#include "Utils/MemorySafe.hpp"
#include "Core/Telemetry.hpp"

#include <windows.h>
#include <cstdint>
#include <cstddef>

bool InstallNativeSetterMetaWriteHook() {
    const char* pattern = "\x48\x8B\x07\x48\x89\x4F\x08\x48\x8D\x4D\xE7\x48\x89\x45\xE7";
    const char* mask = "xxxxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 11; // mov rax,[rdi] / mov [rdi+08],rcx / lea rcx,[rbp-19]
    void* tramp = AllocateTrampoline(found, 256);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    code[pos++] = 0x50; // push rax
    code[pos++] = 0x41; code[pos++] = 0x53; // push r11
    WriteMovR11Imm64(code, pos, reinterpret_cast<uintptr_t>(g_setterTrace) + kMetaWriteTraceOffset);
    code[pos++] = 0x48; code[pos++] = 0x8D; code[pos++] = 0x44; code[pos++] = 0x24; code[pos++] = 0x10; // lea rax,[rsp+10h]
    code[pos++] = 0x41; code[pos++] = 0xFF; code[pos++] = 0x03; // inc dword ptr [r11]
    code[pos++] = 0x49; code[pos++] = 0x89; code[pos++] = 0x7B; code[pos++] = 0x08; // mov [r11+08],rdi
    code[pos++] = 0x49; code[pos++] = 0x89; code[pos++] = 0x4B; code[pos++] = 0x10; // mov [r11+10],rcx
    code[pos++] = 0x49; code[pos++] = 0x89; code[pos++] = 0x43; code[pos++] = 0x18; // mov [r11+18],rax
    code[pos++] = 0x41; code[pos++] = 0x5B; // pop r11
    code[pos++] = 0x58; // pop rax

    code[pos++] = 0x48; code[pos++] = 0x8B; code[pos++] = 0x07; // mov rax,[rdi]
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0x4F; code[pos++] = 0x08; // mov [rdi+08],rcx
    code[pos++] = 0x48; code[pos++] = 0x8D; code[pos++] = 0x4D; code[pos++] = 0xE7; // lea rcx,[rbp-19]

    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}
// THE GUARD THIS HOOK ALWAYS HAD -- the boot wrapped all three of these in
// `if (kEnableNativeSetterTracers != 0)`. It travels with the hook now, so the condition cannot be
// lost by a boot function forgetting it.
namespace {
constexpr int kEnableNativeSetterTracers = 0;
bool NativeSetterTracersWanted() { return kEnableNativeSetterTracers != 0; }
}  // namespace

CVR_HOOK_IF("NativeSetterMetaWrite", ::cvr::hooks::Stage::Boot, 95, InstallNativeSetterMetaWriteHook, NativeSetterTracersWanted);
#endif
