CyberpunkVRProfiler 1.0.0 — portable tester resource logger
Windows 10/11 x64. No install, Python, .NET or VC++ Redistributable required.

1. Extract CyberpunkVRProfiler anywhere, including beside the game.
2. Run RUN_PROFILER.bat, then start/play the game normally.
3. Reproduce the issue. Press Q, Esc or Ctrl+C in the profiler window to finish.
4. ZIP and send the entire ProfilerLogs\<UTC timestamp-PID> session directory.
   Its exact path is printed in the console.

RUN_5_MINUTES.bat stops after five minutes.
RUN_FULL_THREADS.bat records every thread at every sample, including idle threads.
The EXE also works alone. Logs are written beside it; if that is not writable,
the default falls back to %LOCALAPPDATA%\CyberpunkVRProfiler\ProfilerLogs.
Administrator access is normally unnecessary. Unsupported data stays N/A.
The profiler neither starts nor stops the game; recording continues after game exit.

FILES
summary.txt: all processes, peak CPU/GPU/VRAM/RAM, 100 hottest CPU threads.
summary.csv: all process peaks, average CPU, valid observation counts.
thread_summary.csv: every observed thread, sampled CPU time and context switches.
processes.csv: all processes, CPU, working set/private commit, I/O rate, GPU/VRAM.
threads.csv: per-thread user/kernel CPU, names, context switches, priority/state/wait reason.
gpu_engines.csv: per-process/per-adapter/per-engine GPU utilization.
gpu_memory.csv: per-process/per-adapter dedicated and shared GPU memory.
adapters.csv: adapter-wide memory and busiest-engine utilization.
system.csv: system RAM/commit, process/thread counts and collection cost sample_work_ms.
session.log: configuration, GPU/driver inventory, counter errors, lifecycle and notable processes.

CSV uses UTF-8, comma separator and decimal point; memory units are MiB.
All timestamps are UTC. Files flush each sample. Summaries refresh every 30 seconds
and on normal exit. Partial logs are usable after a crash or forced termination.

INTERPRETATION
Process cpu_machine_pct: 100% = all logical processors. Thread cpu_one_core_pct:
100% = one logical processor. Process cpu_one_core_pct may exceed 100%.
CPU is elapsed execution time, not clock-frequency-scaled Task Manager utility.
The first CPU observation is blank because a rate requires two observations.
CPU accounting granularity can cause small spikes above 100% of one core on short
intervals; raw measured rates are retained rather than clipped.
Default --threads active covers all processes; it writes new threads and threads
with CPU activity or context switches. Unchanged idle threads are omitted from
the time series. --threads all writes the complete grid. Identity includes creation
time to distinguish reused PIDs/TIDs. Thread names may be blank when inaccessible.

GPU = busiest engine, never sum engine percentages. Dedicated and shared GPU memory
are separate; shared GPU memory is system RAM accessible by the GPU. On integrated
GPUs dedicated memory need not be physical VRAM. Per-process shared allocations
may be counted in multiple processes; use adapters.csv for device totals.
These memory counters alone do not prove eviction or VRAM exhaustion.

GPU/VRAM uses Windows WDDM counters and depends on driver support. No GPU-to-CPU-thread
attribution, frame times/FPS, call stacks or shader-thread profiling is claimed.
Polling can miss short-lived processes/threads between observations. Process CPU
can include work of threads that disappear before their next thread sample.

Blank/N/A is not zero. Status: ok = valid (including actual zero), no_instance = no
matching GPU instance, unavailable = unsupported/warming up, invalid_or_partial =
incomplete data (no total is published). The initial GPU sample normally needs warm-up.

OPTIONS
CyberpunkVRProfiler.exe --duration 300
CyberpunkVRProfiler.exe --interval-ms 500 --threads all
CyberpunkVRProfiler.exe --threads target --target Cyberpunk2077.exe
CyberpunkVRProfiler.exe --output "D:\Test logs" --duration 60
CyberpunkVRProfiler.exe --help

--threads active|all|target|none only changes thread detail; process CPU/GPU/VRAM
always covers every program. Default interval: 1000 ms; allowed: 250..60000 ms.
--duration: 1..86400 seconds; without it, stop manually. Shorter intervals generate
more data and overhead. --target also highlights that executable in console/logs.

Logs contain process/thread names and hardware details. No network uploads,
injection, driver installation or game modifications. Delete the folder to uninstall.
