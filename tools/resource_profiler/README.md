# Standalone tester profiler

Independent Windows x64 C++ executable; deliberately not added to the mod's root
CMake project or normal release package. Uses the static MSVC runtime, Windows
NtQuerySystemInformation for process/thread snapshots and PDH WDDM GPU counters.
No game launch, injection, privileged ETW session or vendor-specific dependency.

Build, run unit checks and produce the separate tester ZIP:

```powershell
pwsh -NoProfile -File tools/resource_profiler/build_package.ps1
```

End-to-end verification (12 seconds, launches a bounded C++ CPU/D3D11 workload,
does not launch the game):

```powershell
pwsh -NoProfile -File tools/resource_profiler/verify.ps1
```

See README_RU.txt and README_EN.txt for operating instructions and measurement
limits. The integration check verifies late process/GPU discovery, thread names,
one-core CPU normalization against GetThreadTimes, idle thread readings, GPU
allocation/activity, target exit and summary finalization. A suitable hardware GPU
is needed for the GPU assertion; unsupported GPU samples must remain blank.

Native process layout is checked at compile time and each returned record is
bounds checked. CPU and I/O rates use actual monotonic time between snapshots.
Rates are unavailable on first observation; PID/TID reuse is keyed by creation
time. GPU arrays are re-read every sample and retry on changing buffer sizes.
Missing/invalid GPU values are distinguished from zero, including partial totals.
Per-adapter memory uses its own counters, not a sum of per-process attribution.

API references:

- https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntquerysysteminformation
- https://github.com/winsiderss/phnt/blob/master/ntexapi.h (native time/context-switch fields)
- https://learn.microsoft.com/en-us/windows/win32/api/pdh/nf-pdh-pdhaddenglishcounterw
- https://learn.microsoft.com/en-us/windows/win32/api/pdh/nf-pdh-pdhgetformattedcounterarrayw
- https://devblogs.microsoft.com/directx/gpus-in-the-task-manager/ (busiest-engine aggregation and shared allocation accounting)
