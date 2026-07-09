#include <windows.h>
#include <cstdint>

void* AllocateTrampoline(void* targetAddress, size_t size) {
    SYSTEM_INFO sysInfo;
    GetSystemInfo(&sysInfo);
    
    uintptr_t target = reinterpret_cast<uintptr_t>(targetAddress);
    uintptr_t minAddr = target > 0x7FFFFFFF ? target - 0x7FFFFFFF : 0;
    uintptr_t maxAddr = target + 0x7FFFFFFF;
    if (maxAddr < target) maxAddr = UINTPTR_MAX;
    
    minAddr -= minAddr % sysInfo.dwAllocationGranularity;
    
    for (uintptr_t addr = target - sysInfo.dwAllocationGranularity; addr > minAddr; addr -= sysInfo.dwAllocationGranularity) {
        void* p = VirtualAlloc(reinterpret_cast<void*>(addr), size, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE);
        if (p) return p;
    }
    for (uintptr_t addr = target + sysInfo.dwAllocationGranularity; addr < maxAddr; addr += sysInfo.dwAllocationGranularity) {
        void* p = VirtualAlloc(reinterpret_cast<void*>(addr), size, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE);
        if (p) return p;
    }
    return nullptr;
}

extern "C" void** g_cameraPtrGlobal = nullptr;

bool HookCameraFovWrite(void* targetAddress) {
    void* trampoline = AllocateTrampoline(targetAddress, 128);
    if (!trampoline) return false;
    
    // Machine code for trampoline:
    // push rax
    // mov rax, rcx
    // mov [rip + offset_to_g_cameraPtr], rax
    // pop rax
    // movss [rcx+2C0h], xmm1   (8 bytes)
    // movss [rcx+128h], xmm1   (8 bytes)
    // ret                      (1 byte)
    
    uint8_t* code = static_cast<uint8_t*>(trampoline);
    int pos = 0;
    
    code[pos++] = 0x50; // push rax
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xC8; // mov rax, rcx
    
    // mov [rip + 0], rax -> 48 89 05 00 00 00 00
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0x05;
    code[pos++] = 0x00; code[pos++] = 0x00; code[pos++] = 0x00; code[pos++] = 0x00;
    int ripOffsetPos = pos;
    
    code[pos++] = 0x58; // pop rax
    
    // Original instructions:
    // f3 0f 11 89 c0 02 00 00
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x89;
    code[pos++] = 0xC0; code[pos++] = 0x02; code[pos++] = 0x00; code[pos++] = 0x00;
    
    // f3 0f 11 89 28 01 00 00
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x89;
    code[pos++] = 0x28; code[pos++] = 0x01; code[pos++] = 0x00; code[pos++] = 0x00;
    
    code[pos++] = 0xC3; // ret
    
    // Store camera ptr address right after code
    void** ptrAddress = reinterpret_cast<void**>(code + pos);
    g_cameraPtrGlobal = ptrAddress;
    
    // Fix rip offset
    int32_t relOffset = static_cast<int32_t>(reinterpret_cast<uint8_t*>(ptrAddress) - (code + ripOffsetPos));
    *reinterpret_cast<int32_t*>(code + ripOffsetPos - 4) = relOffset;
    
    // Now write the jump from targetAddress to trampoline
    DWORD oldProtect;
    if (!VirtualProtect(targetAddress, 17, PAGE_EXECUTE_READWRITE, &oldProtect)) return false;
    
    uint8_t* target = static_cast<uint8_t*>(targetAddress);
    target[0] = 0xE9; // jmp
    int32_t jmpOffset = static_cast<int32_t>(code - (target + 5));
    *reinterpret_cast<int32_t*>(target + 1) = jmpOffset;
    
    // NOP out the rest of the replaced bytes (total 17 bytes replaced: 8 + 8 + 1)
    for (int i = 5; i < 17; ++i) {
        target[i] = 0x90;
    }
    
    VirtualProtect(targetAddress, 17, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), targetAddress, 17);
    
    return true;
}
