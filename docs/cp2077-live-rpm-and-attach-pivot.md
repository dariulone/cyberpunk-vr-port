---
name: cp2077-live-rpm-and-attach-pivot
description: workflow pivot to live ReadProcessMemory (no rebuild loop) + direction pivot from packet-replay to component render-attach lifecycle
metadata:
  type: project
---

Two pivots decided 2026-06-08:

**Workflow — live RPM.** `tools\rpm.ps1` (PowerShell + ReadProcessMemory P/Invoke, read-only OpenProcess(VM_READ)) reads live game memory while CP2077 runs. Eliminates the build→deploy→restart→reproduce→send-log loop for ALL read-only inspection. Functions: `Rpm-Open/Base/Rva/Q/D/W/Read/Chain/Dump/Ann`. Usage: dot-source `. .\rpm.ps1`, then e.g. `Rpm-Dump (Rpm-Rva 0x3427C00) 0x40`. Caveat: read-only — calling engine functions (the render-attach itself) still needs the RED4ext plugin; external process cannot safely call game code. NOTE: no IDA MCP available; Ghidra is headless-CLI only.

**Direction — stop the packet-replay chase.** The texMgr command-queue / RTT-packet / "wrapper object with callable +0x98 to replay enroll-packet" path is the WRONG ALTITUDE: manually driving the render-thread command stream is fragile (lifetime/threading), bypasses engine resource mgmt, and keeps crashing (caf-chain, base−5, aborted scans). See [[cp2077-952xx-is-generic-task-scheduler]] for a prior similar over-low dead-end.

Right lever is one level UP: the **component render-attach lifecycle**. Root cause (already established): script AddComponent skips render-attach → no generator@comp+0x88 → no GPU backing [[cp2077-resref-autoload-dead]]. Plan: use inventory preview as live reference via RPM — compare a live preview RTT component (has +0x88 generator) vs our script component (+0x88 null), find the render-attach function that fills +0x88, then drive that ONE attach call from the plugin. Robust because it uses the engine's own end-to-end path, not a hand-replayed tail.

Diagnostics already shipped (can be retired in favor of RPM): Game.DumpRenderTexRegistry/DumpRenderTexEntries/DumpTexMgrQueues/DumpTexMgrRttTargets/FindRttWrapperObjects. Last FindRttWrapperObjects run stalled at sample[5] (44 targets), never reached wrapper match — packet path abandoned, no loss.
