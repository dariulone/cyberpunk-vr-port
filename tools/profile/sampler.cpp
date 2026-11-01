// sampler.exe PID DURATION_MS INTERVAL_US OUT.txt [TID | all]
//
// A stack sampler for one thread of a running process: the game's main thread (the oldest thread of the process)
// unless TID is given. Every INTERVAL_US the thread is suspended, its context read and its stack walked (dbghelp, the
// unwind data of the loaded modules), then resumed. One line per sample in OUT.txt:
//
//   <qpc ticks> <frame0> <frame1> ...      each frame module!0xRVA (or 0xADDRESS outside any module)
//
// With "all", every thread that ran since the last tick (its cycle count grew) is sampled, and each line carries the
// thread: <qpc ticks> t<TID> <frame0> ...
//
// The first line is "# qpc_freq <ticks per second>". QueryPerformanceCounter is the clock Python's perf_counter reads,
// so a script can place its own events among the samples.
#include <windows.h>
#include <dbghelp.h>
#include <tlhelp32.h>

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#pragma comment(lib, "dbghelp.lib")

static std::vector<DWORD> Threads(DWORD pid) {
    std::vector<DWORD> out;
    HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
    THREADENTRY32 te{sizeof(te)};
    for (BOOL ok = Thread32First(snap, &te); ok; ok = Thread32Next(snap, &te))
        if (te.th32OwnerProcessID == pid) out.push_back(te.th32ThreadID);
    CloseHandle(snap);
    return out;
}

static DWORD OldestThread(DWORD pid) {
    DWORD best = 0;
    ULONGLONG bestTime = ~0ull;
    for (DWORD tid : Threads(pid)) {
        HANDLE t = OpenThread(THREAD_QUERY_LIMITED_INFORMATION, FALSE, tid);
        if (!t) continue;
        FILETIME c, e, k, u;
        if (GetThreadTimes(t, &c, &e, &k, &u)) {
            const ULONGLONG ct = (ULONGLONG(c.dwHighDateTime) << 32) | c.dwLowDateTime;
            if (ct < bestTime) bestTime = ct, best = tid;
        }
        CloseHandle(t);
    }
    return best;
}

struct Module { ULONG64 base, size; std::string name; };

static std::vector<Module> Modules(DWORD pid) {
    std::vector<Module> out;
    HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
    MODULEENTRY32 me{sizeof(me)};
    for (BOOL ok = Module32First(snap, &me); ok; ok = Module32Next(snap, &me))
        out.push_back({ULONG64(me.modBaseAddr), me.modBaseSize, me.szModule});
    CloseHandle(snap);
    return out;
}

struct Target { DWORD tid; HANDLE handle; ULONG64 cycles; };

// One thread's stack, suspended for the walk, as module!0xRVA frames appended to `line`.
static void Walk(HANDLE proc, HANDLE thread, const std::vector<Module>& mods, std::string& line) {
    if (SuspendThread(thread) == DWORD(-1)) return;
    ULONG64 frames[48];
    int count = 0;
    CONTEXT ctx{};
    ctx.ContextFlags = CONTEXT_FULL;
    if (GetThreadContext(thread, &ctx)) {
        STACKFRAME64 sf{};
        sf.AddrPC.Offset = ctx.Rip, sf.AddrPC.Mode = AddrModeFlat;
        sf.AddrFrame.Offset = ctx.Rbp, sf.AddrFrame.Mode = AddrModeFlat;
        sf.AddrStack.Offset = ctx.Rsp, sf.AddrStack.Mode = AddrModeFlat;
        while (count < 48 && StackWalk64(IMAGE_FILE_MACHINE_AMD64, proc, thread, &sf, &ctx, nullptr,
                                         SymFunctionTableAccess64, SymGetModuleBase64, nullptr)) {
            if (sf.AddrPC.Offset == 0) break;
            frames[count++] = sf.AddrPC.Offset;
        }
    }
    ResumeThread(thread);
    for (int i = 0; i < count; ++i) {
        const ULONG64 a = frames[i];
        const Module* m = nullptr;
        for (const Module& mod : mods)
            if (a >= mod.base && a < mod.base + mod.size) { m = &mod; break; }
        char buf[300];
        if (m) snprintf(buf, sizeof(buf), " %s!0x%llx", m->name.c_str(), a - m->base);
        else snprintf(buf, sizeof(buf), " 0x%llx", a);
        line += buf;
    }
}

int main(int argc, char** argv) {
    if (argc < 5) {
        fprintf(stderr, "sampler.exe PID DURATION_MS INTERVAL_US OUT.txt [TID | all]\n");
        return 2;
    }
    const DWORD pid = strtoul(argv[1], nullptr, 10);
    const double durationMs = atof(argv[2]);
    const double intervalUs = atof(argv[3]);
    const bool all = argc > 5 && strcmp(argv[5], "all") == 0;
    HANDLE proc = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, pid);
    if (!proc) {
        fprintf(stderr, "cannot open process %lu (%lu)\n", pid, GetLastError());
        return 1;
    }
    std::vector<Target> targets;
    const DWORD access = THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION;
    if (all) {
        for (DWORD tid : Threads(pid))
            if (HANDLE t = OpenThread(access, FALSE, tid)) targets.push_back({tid, t, 0});
    } else {
        const DWORD tid = argc > 5 ? strtoul(argv[5], nullptr, 10) : OldestThread(pid);
        if (HANDLE t = OpenThread(access, FALSE, tid)) targets.push_back({tid, t, 0});
    }
    if (targets.empty()) {
        fprintf(stderr, "no thread to sample (%lu)\n", GetLastError());
        return 1;
    }
    SymSetOptions(SYMOPT_DEFERRED_LOADS | SYMOPT_UNDNAME);
    if (!SymInitialize(proc, nullptr, TRUE)) {
        fprintf(stderr, "SymInitialize failed (%lu)\n", GetLastError());
        return 1;
    }
    for (Target& t : targets) QueryThreadCycleTime(t.handle, &t.cycles);
    const std::vector<Module> mods = Modules(pid);
    FILE* f = fopen(argv[4], "w");
    if (!f) return 1;
    LARGE_INTEGER freq, start, now;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&start);
    fprintf(f, "# qpc_freq %lld thread %s\n", freq.QuadPart, all ? "all" : std::to_string(targets[0].tid).c_str());
    const LONGLONG endTicks = start.QuadPart + LONGLONG(durationMs * 1e-3 * double(freq.QuadPart));
    const LONGLONG stepTicks = LONGLONG(intervalUs * 1e-6 * double(freq.QuadPart));
    LONGLONG next = start.QuadPart;
    std::string line;
    int samples = 0;
    timeBeginPeriod(1);
    for (;;) {
        QueryPerformanceCounter(&now);
        if (now.QuadPart >= endTicks) break;
        if (now.QuadPart < next) {
            // Short waits spin; longer ones sleep a millisecond.
            if (next - now.QuadPart > freq.QuadPart / 500) Sleep(1);
            continue;
        }
        next += stepTicks;
        for (Target& t : targets) {
            if (all) {
                // Only a thread that ran since the last tick: a parked worker has nothing to say.
                ULONG64 cycles = 0;
                if (!QueryThreadCycleTime(t.handle, &cycles)) continue;
                const ULONG64 ran = cycles - t.cycles;
                t.cycles = cycles;
                if (ran < 200000) continue;
            }
            line = std::to_string(now.QuadPart);
            if (all) line += " t" + std::to_string(t.tid);
            Walk(proc, t.handle, mods, line);
            fprintf(f, "%s\n", line.c_str());
            ++samples;
        }
    }
    timeEndPeriod(1);
    fclose(f);
    printf("%d samples of %zu thread(s)\n", samples, targets.size());
    return 0;
}
