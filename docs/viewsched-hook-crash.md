---
name: viewsched-hook-crash
description: CP2077 option-C ViewSchedule hook crash root cause (racy hot-patch + r11 clobber) and fix
metadata:
  type: project
---

Hooking `sub_140952CF4` (per-view scheduler, the [[cp2077-viewfamily-iterator]] injection point) repeatedly crashed CP2077 at menu/save load — intermittently (sometimes reached world frame 433, usually died at menu).

REDEngine crash report (`%LOCALAPPDATA%\REDEngine\ReportQueue\...\stacktrace.txt`) gave the smoking gun: `EXCEPTION_ACCESS_VIOLATION (0xC0000005)` reading at `0x7FF651BCFFFB` = **module base (0x7FF651BD0000) − 5**. Deterministic near-base read, not random garbage.

Two real defects in the hand-rolled detour (`InstallViewScheduleHook` in `src/dxgi_proxy.cpp`):
1. **Non-atomic hot-patch of a function running on ~15 redDispatcher job workers.** A worker fetched a half-written 15-byte patch -> torn instruction -> AV. Explains the intermittency.
2. **r11 clobber on resume.** The displaced prologue runs `mov r11,rsp` (r11 = entry rsp = frame base the body expects), but the trampoline ended with `mov r11,imm; jmp r11`, leaving r11 wrong when resuming the body.

Fix (applied 2026-06-07):
- Return to body via RIP-relative `jmp qword ptr [rip+0]; dq found+15` (does NOT touch r11).
- New `PatchCodeSafely()` helper: CreateToolhelp32Snapshot enumerate threads, SuspendThread all but self, GetThreadContext + verify no RIP in [target,target+len); apply patch; resume. Retries 50× if a worker is stuck in range, else aborts rather than corrupt.

**Why:** any future hook on a hot job-system function (redJobs2 workers) MUST use thread-suspend patching, not a plain VirtualProtect+memcpy.
**How to apply:** reuse `PatchCodeSafely` for all detours on hot multi-threaded RedEngine functions.
