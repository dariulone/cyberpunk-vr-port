#include "Hooks/FarInlineHook.hpp"
#include <Windows.h>
#include <TlHelp32.h>
#include <cstdint>
#include <cstring>
#include <vector>
#include "hde64.h"

namespace cvr::hooks {
namespace {

constexpr size_t kJump = 14;  // FF 25 00000000 <address64>
constexpr size_t kMaxThreads = 8192;

void WriteJump(uint8_t* at, const void* destination) {
    const uint8_t jump[6] = {0xFF, 0x25, 0, 0, 0, 0};
    std::memcpy(at, jump, sizeof(jump));
    const auto address = reinterpret_cast<uintptr_t>(destination);
    std::memcpy(at + 6, &address, sizeof(address));
}

// Whole instructions covering at least kJump bytes that run the same from any address.
bool Movable(const uint8_t* code, size_t& length) {
    length = 0;
    while (length < kJump) {
        hde64s hs{};
        const unsigned size = hde64_disasm(code + length, &hs);
        if (!size || (hs.flags & F_ERROR) || (hs.flags & F_RELATIVE)) return false;
        if ((hs.flags & F_MODRM) && hs.modrm_mod == 0 && hs.modrm_rm == 5) return false;  // [rip+disp32]
        const uint8_t op = hs.opcode;
        if (op == 0xC3 || op == 0xC2 || op == 0xCB || op == 0xCA || op == 0xCC || op == 0xCD) return false;
        if (op == 0xFF && (hs.flags & F_MODRM) && hs.modrm_reg >= 2 && hs.modrm_reg <= 5) return false;  // call/jmp r/m
        length += size;
    }
    return true;
}

void Thaw(std::vector<HANDLE>& threads) {
    for (HANDLE thread : threads) {
        ResumeThread(thread);
        CloseHandle(thread);
    }
    threads.clear();
}

// Suspends every other thread of the process. No heap use while threads are held: the list's storage is reserved
// before the first suspension. False when a thread sits in [from, to) or the list would outgrow its storage.
bool Freeze(std::vector<HANDLE>& threads, uintptr_t from, uintptr_t to) {
    threads.reserve(kMaxThreads);
    const HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
    if (snapshot == INVALID_HANDLE_VALUE) return false;
    const DWORD process = GetCurrentProcessId(), self = GetCurrentThreadId();
    THREADENTRY32 entry{};
    entry.dwSize = sizeof(entry);
    bool clear = true;
    for (BOOL more = Thread32First(snapshot, &entry); more && clear; more = Thread32Next(snapshot, &entry)) {
        if (entry.th32OwnerProcessID != process || entry.th32ThreadID == self) continue;
        const HANDLE thread = OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION, FALSE,
                                         entry.th32ThreadID);
        if (!thread) continue;
        if (threads.size() >= kMaxThreads || SuspendThread(thread) == static_cast<DWORD>(-1)) {
            CloseHandle(thread);
            clear = threads.size() < kMaxThreads;
            continue;
        }
        threads.push_back(thread);
        CONTEXT context{};
        context.ContextFlags = CONTEXT_CONTROL;
        if (GetThreadContext(thread, &context) && context.Rip >= from && context.Rip < to) clear = false;
    }
    CloseHandle(snapshot);
    if (!clear) Thaw(threads);
    return clear;
}

}  // namespace

FarHookResult FarInlineHook(void* target, void* detour, void** original) {
    if (!target || !detour || !original) return FarHookResult::Failed;
    auto* code = static_cast<uint8_t*>(target);
    size_t length = 0;
    if (!Movable(code, length)) return FarHookResult::Unsupported;

    // The moved instructions, then the way back to the rest of the function.
    auto* trampoline = static_cast<uint8_t*>(
        VirtualAlloc(nullptr, length + kJump, MEM_RESERVE | MEM_COMMIT, PAGE_EXECUTE_READWRITE));
    if (!trampoline) return FarHookResult::Failed;
    std::memcpy(trampoline, code, length);
    WriteJump(trampoline + length, code + length);
    DWORD previous{};
    VirtualProtect(trampoline, length + kJump, PAGE_EXECUTE_READ, &previous);
    FlushInstructionCache(GetCurrentProcess(), trampoline, length + kJump);

    DWORD protection{};
    if (!VirtualProtect(code, length, PAGE_EXECUTE_READWRITE, &protection)) {
        VirtualFree(trampoline, 0, MEM_RELEASE);
        return FarHookResult::Failed;
    }
    uint8_t patch[32];
    WriteJump(patch, detour);
    std::memset(patch + kJump, 0xCC, length - kJump);
    std::vector<HANDLE> threads;
    if (!Freeze(threads, reinterpret_cast<uintptr_t>(code), reinterpret_cast<uintptr_t>(code) + length)) {
        DWORD ignored{};
        VirtualProtect(code, length, protection, &ignored);
        VirtualFree(trampoline, 0, MEM_RELEASE);
        return FarHookResult::Busy;
    }
    *original = trampoline;
    std::memcpy(code, patch, length);
    FlushInstructionCache(GetCurrentProcess(), code, length);
    Thaw(threads);
    DWORD ignored{};
    VirtualProtect(code, length, protection, &ignored);
    return FarHookResult::Ok;
}

const char* ToString(FarHookResult result) {
    switch (result) {
        case FarHookResult::Ok: return "ok";
        case FarHookResult::Unsupported: return "prologue not movable";
        case FarHookResult::Busy: return "a thread is inside the prologue";
        default: return "allocation or protection failed";
    }
}

}  // namespace cvr::hooks
