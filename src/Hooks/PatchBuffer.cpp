#if !CVR_RUST_VRIK   // the Rust build registers this file's hook from rust/crates/cvr-render/src/hook_table.rs
// PatchBuffer -- one hook, one file. Installed from the registry it registers itself with at the
// bottom; see Hooks/Hook.hpp for why the order is declared here rather than in a boot function.
//
// The engine copies a float buffer; this observes it.

#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Utils/AobScanner.hpp"
#include "Utils/MemorySafe.hpp"

#include <windows.h>
#include <cstdint>
#include <cstddef>

#if CVR_RUST_VRIK
// The callback and the stub are rust/crates/cvr-rig/src/vrik/photo_capture.rs; the registration (gated off) stays.
extern "C" bool cvr_hook_patch_buffer_install();
extern "C" bool cvr_hook_patch_buffer_wanted();
CVR_HOOK_IF("PatchBuffer", ::cvr::hooks::Stage::Boot, 80, cvr_hook_patch_buffer_install, cvr_hook_patch_buffer_wanted);
#else
static uint64_t g_patchBufferHits = 0;

extern "C" void __fastcall OnPatchBufferCallback(float* dest, float* src, size_t count) {
    g_patchBufferHits++;

}

bool InstallPatchBufferHook() {
    // 48 8B C1 4C 8D 15 ?? ?? ?? ?? 49 83 F8 0F
    const char* pattern = "\x48\x8B\xC1\x4C\x8D\x15\x00\x00\x00\x00\x49\x83\xF8\x0F";
    const char* mask = "xxxxxx????xxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 14; 
    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // --- CALL C++ CALLBACK ---
    code[pos++] = 0x9C; // pushfq
    code[pos++] = 0x50; // push rax
    code[pos++] = 0x51; // push rcx
    code[pos++] = 0x52; // push rdx
    code[pos++] = 0x41; code[pos++] = 0x50; // push r8
    code[pos++] = 0x41; code[pos++] = 0x51; // push r9
    code[pos++] = 0x41; code[pos++] = 0x52; // push r10
    code[pos++] = 0x41; code[pos++] = 0x53; // push r11
    code[pos++] = 0x55; // push rbp

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40; // sub rsp, 40h
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24; // movups [rsp+00h], xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups [rsp+10h], xmm1
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups [rsp+20h], xmm2
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups [rsp+30h], xmm3

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5; // mov rbp, rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0; // and rsp, -16
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20; // sub rsp, 20h

    // args: rcx=dest, rdx=src, r8=count (already set!)
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnPatchBufferCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0; // call rax

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC; // mov rsp, rbp

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24; // movups xmm0, [rsp+00h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups xmm1, [rsp+10h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups xmm2, [rsp+20h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups xmm3, [rsp+30h]
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40; // add rsp, 40h

    code[pos++] = 0x5D; // pop rbp
    code[pos++] = 0x41; code[pos++] = 0x5B; // pop r11
    code[pos++] = 0x41; code[pos++] = 0x5A; // pop r10
    code[pos++] = 0x41; code[pos++] = 0x59; // pop r9
    code[pos++] = 0x41; code[pos++] = 0x58; // pop r8
    code[pos++] = 0x5A; // pop rdx
    code[pos++] = 0x59; // pop rcx
    code[pos++] = 0x58; // pop rax
    code[pos++] = 0x9D; // popfq

    // Original instructions:
    // mov rax, rcx
    code[pos++] = 0x48; code[pos++] = 0x8B; code[pos++] = 0xC1;
    // lea r10, [rip+...]
    // We cannot copy RIP-relative LEA easily if it's pointing to something in the game executable.
    // found+3 is the start of lea r10, [rip+offset]. It's 7 bytes long.
    // The offset is *(int32_t*)(found+6).
    // Absolute address of target = (found + 3) + 7 + offset.
    int32_t leaOffset = *reinterpret_cast<int32_t*>(found + 6);
    uintptr_t targetAddr = reinterpret_cast<uintptr_t>(found) + 10 + leaOffset;
    
    // We can replace LEA with MOV R10, absolute_addr
    code[pos++] = 0x49; code[pos++] = 0xBA; // mov r10, imm64
    *reinterpret_cast<uint64_t*>(code + pos) = targetAddr;
    pos += 8;

    // cmp r8, 0Fh
    code[pos++] = 0x49; code[pos++] = 0x83; code[pos++] = 0xF8; code[pos++] = 0x0F;

    // jmp back
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

// THE GUARD THIS HOOK ALWAYS HAD. It is a tracer, off in every shipped build, and the boot used
// to wrap the call in `if (kEnablePatchBufferTracer != 0)`. Moving it to the registry dropped that
// condition for one commit and installed it unconditionally -- restored here, where the hook can
// state its own precondition instead of a boot function remembering it.
namespace { constexpr int kEnablePatchBufferTracer = 0;
            bool PatchBufferWanted() { return kEnablePatchBufferTracer != 0; } }

CVR_HOOK_IF("PatchBuffer", ::cvr::hooks::Stage::Boot, 80, InstallPatchBufferHook, PatchBufferWanted);
#endif
#endif
