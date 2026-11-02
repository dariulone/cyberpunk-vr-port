// BackgroundPad -- the pad's events reach the game while the headset has the input, whichever window is in front.
//
// The game polls the pad in the background, but hands its events on (2.5 sub_1420849A0) only while
// GetForegroundWindow() is the input system's own window ([this+0x1D0]); otherwise they are dropped. The VR
// controllers reach the game as that pad (Hooks/XInput.cpp), so with the monitor's window behind anything -- the
// runtime's desktop window, a streaming app, a click on another screen, a window minimised by the game itself --
// the sticks stopped walking and steering and the cyberware chord stopped (reported after 0.2.0, the game in a
// borderless window), while the port's own VR menu, which reads OpenXR directly, still worked. The test harness
// opens the same condition for its own runs (tools/roomscale_tests/simulator_input.py).
//
// The patch is at that one call: `call [GetForegroundWindow]; cmp rax, [rbx+window]; jne skip` goes to a
// trampoline that asks PadForegroundWindow instead -- the input system's window while the OpenXR session is
// FOCUSED (the headset is on and this game has its input), the real foreground window otherwise -- and keeps
// the game's own compare and jumps. Nothing else in the game asks this question through here: its focus, pause
// and cursor logic still see the real foreground window.

#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Utils/AobScanner.hpp"

#include <windows.h>
#include <cstdint>

extern void Log(const char* fmt, ...);

extern "C" HWND __fastcall PadForegroundWindow(HWND inputWindow) {
    if (inputWindow && OpenXRManager::Get().IsSessionFocused()) return inputWindow;
    return GetForegroundWindow();
}

namespace {
bool InstallBackgroundPadHook() {
    // cmp byte [rcx+flag],0 / mov rdi,rdx / mov rbx,rcx / jne / call [GetForegroundWindow] / cmp rax,[rbx+window] / jne
    const char* pattern = "\x80\xB9\x00\x00\x00\x00\x00\x48\x8B\xFA\x48\x8B\xD9\x75\x00\xFF\x15\x00\x00\x00\x00"
                          "\x48\x3B\x83\x00\x00\x00\x00\x75\x00";
    const char* mask = "xx??xxxxxxxxxx?xx????xxx??xxx?";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    uint8_t* site = found + 15;                       // the call: 6 bytes, then the compare (7) and its jne (2)
    const int32_t window = *reinterpret_cast<int32_t*>(found + 24);
    uint8_t* drain = found + 30;                      // after the jne: the events are handed on
    uint8_t* skip = drain + static_cast<int8_t>(found[29]);
    constexpr int replaceLen = 15;

    uint8_t* code = static_cast<uint8_t*>(AllocateTrampoline(site, 64));
    if (!code) return false;
    int pos = 0;
    // mov rcx, [rbx+window] -- the argument
    code[pos++] = 0x48; code[pos++] = 0x8B; code[pos++] = 0x8B;
    *reinterpret_cast<int32_t*>(code + pos) = window; pos += 4;
    // mov rax, PadForegroundWindow / call rax -- at the original call's stack (aligned, shadow space reserved)
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(&PadForegroundWindow));
    code[pos++] = 0xFF; code[pos++] = 0xD0;
    // cmp rax, [rbx+window] / jne skip / jmp drain -- the game's own compare and jumps
    code[pos++] = 0x48; code[pos++] = 0x3B; code[pos++] = 0x83;
    *reinterpret_cast<int32_t*>(code + pos) = window; pos += 4;
    code[pos++] = 0x0F; code[pos++] = 0x85;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>(skip - (code + pos + 4)); pos += 4;
    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>(drain - (code + pos + 4)); pos += 4;
    FlushInstructionCache(GetCurrentProcess(), code, pos);

    DWORD oldProtect;
    if (!VirtualProtect(site, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect)) return false;
    site[0] = 0xE9;
    *reinterpret_cast<int32_t*>(site + 1) = static_cast<int32_t>(code - (site + 5));
    for (int i = 5; i < replaceLen; ++i) site[i] = 0x90;
    VirtualProtect(site, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), site, replaceLen);
    Log("BackgroundPad: pad events reach the game while the headset is focused (site %p, window +0x%X)\n", site,
        static_cast<unsigned>(window));
    return true;
}
}  // namespace

CVR_HOOK("BackgroundPad", ::cvr::hooks::Stage::Boot, 86, InstallBackgroundPadHook);
