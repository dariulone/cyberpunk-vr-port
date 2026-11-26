#include <windows.h>
#include <winternl.h>
#include <pdh.h>
#include <pdhmsg.h>
#include <psapi.h>
#include <dxgi1_2.h>
#include <wrl/client.h>
#include <conio.h>
#include <algorithm>
#include <array>
#include <chrono>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <map>
#include <set>
#include <stdexcept>
#include <tuple>
#include <vector>
#include "metrics.hpp"

using namespace profiler;
namespace fs = std::filesystem;
using Clock = std::chrono::steady_clock;
using Microsoft::WRL::ComPtr;
constexpr double MiB = 1048576.0;
constexpr const char* Version = "1.0.0";
HANDLE stopEvent = nullptr;
HANDLE finishedEvent = nullptr;

BOOL WINAPI onConsole(DWORD signal) {
    if (signal != CTRL_C_EVENT && signal != CTRL_BREAK_EVENT && signal != CTRL_CLOSE_EVENT &&
        signal != CTRL_LOGOFF_EVENT && signal != CTRL_SHUTDOWN_EVENT) return FALSE;
    SetEvent(stopEvent);
    if (signal != CTRL_C_EVENT && signal != CTRL_BREAK_EVENT) WaitForSingleObject(finishedEvent, 4000);
    return TRUE;
}
std::string utf8(std::wstring_view s) {
    if (s.empty()) return {};
    const int len = WideCharToMultiByte(CP_UTF8, 0, s.data(), static_cast<int>(s.size()), nullptr, 0, nullptr, nullptr);
    std::string out(len, '\0');
    WideCharToMultiByte(CP_UTF8, 0, s.data(), static_cast<int>(s.size()), out.data(), len, nullptr, nullptr);
    return out;
}
std::string hexStatus(ULONG status) {
    std::ostringstream out;
    out << "0x" << std::hex << status;
    return out.str();
}
std::string timestamp(bool filename = false) {
    SYSTEMTIME t{};
    GetSystemTime(&t);
    char out[64]{};
    if (filename) snprintf(out, sizeof(out), "%04u%02u%02u-%02u%02u%02u-%03uZ", t.wYear,t.wMonth,t.wDay,t.wHour,t.wMinute,t.wSecond,t.wMilliseconds);
    else snprintf(out, sizeof(out), "%04u-%02u-%02uT%02u:%02u:%02u.%03uZ", t.wYear,t.wMonth,t.wDay,t.wHour,t.wMinute,t.wSecond,t.wMilliseconds);
    return out;
}
fs::path exeDirectory() {
    std::vector<wchar_t> path(32768);
    DWORD len = GetModuleFileNameW(nullptr, path.data(), static_cast<DWORD>(path.size()));
    if (!len || len == path.size()) throw std::runtime_error("Cannot find executable directory");
    return fs::path(std::wstring(path.data(), len)).parent_path();
}
std::ofstream output(const fs::path& path) {
    std::ofstream file;
    file.exceptions(std::ios::badbit | std::ios::failbit);
    file.open(path, std::ios::binary | std::ios::trunc);
    file.imbue(std::locale::classic());
    return file;
}
struct Options {
    unsigned interval = 1000;
    unsigned duration = 0;
    std::string threads = "active";
    std::wstring target = L"Cyberpunk2077.exe";
    fs::path out;
    bool help = false;
};
unsigned unsignedOption(const std::wstring& s, unsigned min, unsigned max) {
    if (s.empty() || s.find_first_not_of(L"0123456789") != std::wstring::npos) throw std::runtime_error("Expected an unsigned integer");
    const auto n = std::stoull(s);
    if (n < min || n > max) throw std::runtime_error("Numeric option outside permitted range");
    return static_cast<unsigned>(n);
}
Options options(int argc, wchar_t** argv) {
    Options opt;
    for (int i = 1; i < argc; ++i) {
        const std::wstring arg = argv[i];
        if (arg == L"--help" || arg == L"-h") { opt.help = true; continue; }
        if (i + 1 >= argc) throw std::runtime_error("Option requires a value: " + utf8(arg));
        const std::wstring value = argv[++i];
        if (arg == L"--interval-ms") opt.interval = unsignedOption(value, 250, 60000);
        else if (arg == L"--duration") opt.duration = unsignedOption(value, 1, 86400);
        else if (arg == L"--threads") {
            opt.threads = utf8(value);
            if (opt.threads != "active" && opt.threads != "all" && opt.threads != "target" && opt.threads != "none")
                throw std::runtime_error("--threads must be active, all, target or none");
        } else if (arg == L"--target") opt.target = value;
        else if (arg == L"--output") opt.out = fs::absolute(value);
        else throw std::runtime_error("Unknown option: " + utf8(arg));
    }
    return opt;
}
fs::path createSession(const Options& opt) {
    const std::string name = timestamp(true) + "-" + std::to_string(GetCurrentProcessId());
    const auto create = [&](const fs::path& base) {
        auto folder = base / name;
        fs::create_directories(folder);
        auto probe = output(folder / "session.log");
        return folder;
    };
    if (!opt.out.empty()) return create(opt.out);
    try { return create(exeDirectory() / "ProfilerLogs"); }
    catch (const std::exception&) {
        wchar_t local[32768]{};
        const auto len = GetEnvironmentVariableW(L"LOCALAPPDATA", local, 32768);
        if (!len || len >= 32768) throw;
        return create(fs::path(local) / "CyberpunkVRProfiler" / "ProfilerLogs");
    }
}

struct Thread {
    uint32_t tid = 0;
    uint64_t created = 0, user = 0, kernel = 0;
    uint32_t switches = 0, state = 0, wait = 0;
    LONG priority = 0;
};
struct Process {
    uint32_t pid = 0;
    uint64_t created = 0, user = 0, kernel = 0;
    uint64_t working = 0, privateBytes = 0, readBytes = 0, writeBytes = 0;
    ULONG handles = 0;
    std::wstring name;
    std::vector<Thread> threads;
};
using ProcessKey = std::pair<uint32_t, uint64_t>;
using ThreadKey = std::tuple<uint32_t, uint64_t, uint32_t, uint64_t>;
ProcessKey key(const Process& p) { return {p.pid, p.created}; }
ThreadKey key(const Process& p, const Thread& t) { return {p.pid, p.created, t.tid, t.created}; }

class NativeSnapshot {
    using Query = NTSTATUS(NTAPI*)(SYSTEM_INFORMATION_CLASS, PVOID, ULONG, PULONG);
    Query query_ = reinterpret_cast<Query>(GetProcAddress(GetModuleHandleW(L"ntdll.dll"), "NtQuerySystemInformation"));
    std::vector<unsigned char> buffer_ = std::vector<unsigned char>(1024 * 1024);
public:
    std::vector<Process> read() {
        if (!query_) throw std::runtime_error("NtQuerySystemInformation is unavailable");
        ULONG used = 0;
        NTSTATUS status = 0;
        for (unsigned retry = 0; retry < 10; ++retry) {
            status = query_(SystemProcessInformation, buffer_.data(), static_cast<ULONG>(buffer_.size()), &used);
            if (static_cast<ULONG>(status) != 0xc0000004u) break;
            const size_t size = std::max(buffer_.size() * 2, static_cast<size_t>(used) + 65536);
            if (size > 256 * 1024 * 1024) throw std::runtime_error("Unexpectedly large process snapshot");
            buffer_.resize(size);
        }
        if (status < 0) throw std::runtime_error("Process snapshot failed: " + hexStatus(static_cast<ULONG>(status)));
        if (used > buffer_.size()) throw std::runtime_error("Process snapshot exceeds allocated buffer");
        // Windows x64 SystemProcessInformation layout. The SDK calls the time
        // fields Reserved; field meanings are also documented in phnt/ntexapi.h.
        static_assert(sizeof(SYSTEM_PROCESS_INFORMATION) == 256);
        static_assert(sizeof(SYSTEM_THREAD_INFORMATION) == 80);
        static_assert(offsetof(SYSTEM_PROCESS_INFORMATION, ImageName) == 56);
        std::vector<Process> result;
        size_t offset = 0;
        while (offset < used) {
            if (used - offset < sizeof(SYSTEM_PROCESS_INFORMATION)) throw std::runtime_error("Invalid process snapshot header");
            const auto* n = reinterpret_cast<const SYSTEM_PROCESS_INFORMATION*>(buffer_.data() + offset);
            const size_t span = n->NextEntryOffset ? n->NextEntryOffset : used - offset;
            const size_t need = sizeof(*n) + static_cast<size_t>(n->NumberOfThreads) * sizeof(SYSTEM_THREAD_INFORMATION);
            if (span < need || span > used - offset) throw std::runtime_error("Invalid process snapshot layout");
            Process p;
            p.pid = static_cast<uint32_t>(reinterpret_cast<uintptr_t>(n->UniqueProcessId));
            std::memcpy(&p.created, buffer_.data() + offset + 32, 8);
            std::memcpy(&p.user, buffer_.data() + offset + 40, 8);
            std::memcpy(&p.kernel, buffer_.data() + offset + 48, 8);
            p.working = n->WorkingSetSize;
            p.privateBytes = n->PrivatePageCount;
            p.handles = n->HandleCount;
            p.readBytes = static_cast<uint64_t>(n->Reserved7[3].QuadPart);
            p.writeBytes = static_cast<uint64_t>(n->Reserved7[4].QuadPart);
            if (n->ImageName.Length) {
                const uintptr_t name = reinterpret_cast<uintptr_t>(n->ImageName.Buffer);
                const uintptr_t begin = reinterpret_cast<uintptr_t>(buffer_.data());
                if (name < begin || name - begin > used || n->ImageName.Length > used - (name - begin) || n->ImageName.Length % 2)
                    throw std::runtime_error("Invalid process name in snapshot");
                p.name.assign(n->ImageName.Buffer, n->ImageName.Length / sizeof(wchar_t));
            } else p.name = p.pid == 0 ? L"Idle" : L"System";
            auto* threads = reinterpret_cast<const SYSTEM_THREAD_INFORMATION*>(n + 1);
            for (ULONG i = 0; i < n->NumberOfThreads; ++i) {
                const auto& t = threads[i];
                p.threads.push_back({static_cast<uint32_t>(reinterpret_cast<uintptr_t>(t.ClientId.UniqueThread)),
                    static_cast<uint64_t>(t.Reserved1[2].QuadPart), static_cast<uint64_t>(t.Reserved1[1].QuadPart),
                    static_cast<uint64_t>(t.Reserved1[0].QuadPart), t.Reserved3, t.ThreadState, t.WaitReason, t.Priority});
            }
            if (p.pid != 0) result.push_back(std::move(p)); // Idle time is not CPU work.
            if (!n->NextEntryOffset) break;
            offset += n->NextEntryOffset;
        }
        return result;
    }
};

struct CounterResult {
    bool available = false;
    std::map<std::string, std::optional<double>> values;
};
class GpuCounters {
    PDH_HQUERY query_ = nullptr;
    struct Counter { const wchar_t* path; PDH_HCOUNTER handle = nullptr; ULONG lastStatus = ~0u; };
    std::array<Counter, 5> counters_{{
        {L"\\GPU Engine(*)\\Utilization Percentage"},
        {L"\\GPU Process Memory(*)\\Dedicated Usage"},
        {L"\\GPU Process Memory(*)\\Shared Usage"},
        {L"\\GPU Adapter Memory(*)\\Dedicated Usage"},
        {L"\\GPU Adapter Memory(*)\\Shared Usage"}
    }};
    std::ofstream& log_;
    void report(Counter& c, ULONG status) {
        if (status != c.lastStatus) {
            log_ << timestamp() << " COUNTER " << utf8(c.path) << " status=" << hexStatus(status) << '\n';
            c.lastStatus = status;
        }
    }
public:
    explicit GpuCounters(std::ofstream& log) : log_(log) {
        const auto status = PdhOpenQueryW(nullptr, 0, &query_);
        if (status != ERROR_SUCCESS) { log_ << "GPU query unavailable: " << hexStatus(status) << '\n'; return; }
        for (auto& c : counters_) {
            const auto added = PdhAddEnglishCounterW(query_, c.path, 0, &c.handle);
            report(c, added);
            if (added != ERROR_SUCCESS) c.handle = nullptr;
        }
    }
    ~GpuCounters() { if (query_) PdhCloseQuery(query_); }
    std::array<CounterResult, 5> sample() {
        std::array<CounterResult, 5> results;
        if (!query_) return results;
        const auto collected = PdhCollectQueryData(query_);
        for (size_t i = 0; i < counters_.size(); ++i) {
            auto& c = counters_[i];
            if (!c.handle) continue;
            if (collected != ERROR_SUCCESS) { report(c, collected); continue; }
            const DWORD format = PDH_FMT_DOUBLE | PDH_FMT_NOCAP100;
            DWORD bytes = 0, count = 0;
            ULONG status = 0;
            std::vector<unsigned char> buffer;
            // Instances may appear/disappear between sizing and read. Restart
            // sizing on PDH_MORE_DATA; its second size is not reliable.
            for (unsigned retry = 0; retry < 4; ++retry) {
                bytes = 0;
                status = PdhGetFormattedCounterArrayW(c.handle, format, &bytes, &count, nullptr);
                if (status != PDH_MORE_DATA) break;
                buffer.resize(bytes);
                status = PdhGetFormattedCounterArrayW(c.handle, format, &bytes, &count,
                    reinterpret_cast<PDH_FMT_COUNTERVALUE_ITEM_W*>(buffer.data()));
                if (status != PDH_MORE_DATA) break;
            }
            report(c, status);
            if (status != ERROR_SUCCESS || buffer.empty()) continue;
            auto& result = results[i];
            result.available = true;
            const auto* items = reinterpret_cast<const PDH_FMT_COUNTERVALUE_ITEM_W*>(buffer.data());
            for (DWORD j = 0; j < count; ++j) {
                const auto& v = items[j].FmtValue;
                std::optional<double> value;
                if ((v.CStatus == PDH_CSTATUS_VALID_DATA || v.CStatus == PDH_CSTATUS_NEW_DATA) &&
                    std::isfinite(v.doubleValue) && v.doubleValue >= 0) value = v.doubleValue;
                result.values.emplace(utf8(items[j].szName), value);
            }
        }
        return results;
    }
};

struct GpuMetrics { Aggregate busy, dedicated, shared; };
struct GpuFrame {
    std::map<uint32_t, GpuMetrics> processes;
    std::map<std::string, GpuMetrics> adapters;
};
struct ProcessStats {
    std::string name;
    uint64_t samples = 0, cpuIntervals = 0;
    double seconds = 0, cpuSeconds = 0, peakCpu = 0, peakWorking = 0, peakPrivate = 0;
    std::optional<double> peakGpu, peakDedicated, peakShared;
    uint64_t gpuSamples = 0, dedicatedSamples = 0, sharedSamples = 0;
};
struct ThreadStats {
    std::string process, name;
    double cpuSeconds = 0, peakCpu = 0;
    uint64_t switches = 0, samples = 0;
};
void peak(std::optional<double>& dest, const std::optional<double>& value) {
    if (value) dest = dest ? std::max(*dest, *value) : *value;
}
void describeAdapters(std::ofstream& log) {
    ComPtr<IDXGIFactory1> factory;
    auto status = CreateDXGIFactory1(IID_PPV_ARGS(&factory));
    if (FAILED(status)) { log << "DXGI adapter discovery failed: " << hexStatus(status) << '\n'; return; }
    for (UINT i = 0;; ++i) {
        ComPtr<IDXGIAdapter1> adapter;
        if (factory->EnumAdapters1(i, &adapter) == DXGI_ERROR_NOT_FOUND) break;
        if (!adapter) break;
        DXGI_ADAPTER_DESC1 desc{};
        if (FAILED(adapter->GetDesc1(&desc))) continue;
        char luid[80]{};
        snprintf(luid, sizeof(luid), "luid_0x%08lx_0x%08lx", static_cast<ULONG>(desc.AdapterLuid.HighPart), desc.AdapterLuid.LowPart);
        LARGE_INTEGER driver{};
        const bool driverValid = SUCCEEDED(adapter->CheckInterfaceSupport(__uuidof(IDXGIDevice), &driver));
        log << "GPU index=" << i << " name=" << utf8(desc.Description) << " adapter=" << luid
            << " dedicated_capacity_mib=" << number(static_cast<double>(desc.DedicatedVideoMemory) / MiB)
            << " shared_limit_mib=" << number(static_cast<double>(desc.SharedSystemMemory) / MiB)
            << " vendor_id=" << hexStatus(desc.VendorId) << " device_id=" << hexStatus(desc.DeviceId);
        if (driverValid) log << " driver=" << HIWORD(driver.HighPart) << '.' << LOWORD(driver.HighPart) << '.' << HIWORD(driver.LowPart) << '.' << LOWORD(driver.LowPart);
        log << '\n';
    }
}
std::string threadName(const Thread& thread) {
    HANDLE h = OpenThread(THREAD_QUERY_LIMITED_INFORMATION, FALSE, thread.tid);
    if (!h) return {};
    FILETIME created{}, exited{}, kernel{}, user{};
    std::string name;
    uint64_t actualCreated = 0;
    if (GetThreadTimes(h, &created, &exited, &kernel, &user)) std::memcpy(&actualCreated, &created, 8);
    if (actualCreated == thread.created) {
        PWSTR text = nullptr;
        if (SUCCEEDED(GetThreadDescription(h, &text)) && text) name = utf8(text);
        if (text) LocalFree(text);
    }
    CloseHandle(h);
    return name;
}

class Recorder {
    fs::path directory_;
    Options opt_;
    DWORD processors_ = std::max<DWORD>(1, GetActiveProcessorCount(ALL_PROCESSOR_GROUPS));
    std::ofstream log_, processes_, threads_, engines_, gpuMemory_, adapters_, system_;
    GpuCounters gpu_;
    NativeSnapshot native_;
    std::map<ProcessKey, Process> previous_;
    std::map<ThreadKey, Thread> previousThreads_;
    std::map<ProcessKey, ProcessStats> stats_;
    std::map<ThreadKey, ThreadStats> threadStats_;
    uint64_t sample_ = 0;
    Clock::time_point start_, lastSnapshot_;
    double nextSummary_ = 30;
    bool targetSeen_ = false;

    GpuFrame writeGpu(const std::array<CounterResult, 5>& counters, const std::string& prefix,
                      const std::map<uint32_t, std::string>& names) {
        GpuFrame frame;
        std::map<std::pair<std::string, std::string>, Aggregate> adapterEngines;
        const auto processName = [&](uint32_t pid) { const auto it = names.find(pid); return it == names.end() ? std::string{} : it->second; };
        for (const auto& [instance, value] : counters[0].values) {
            auto parsed = parseGpuInstance(instance);
            if (!parsed.pid || parsed.adapter.empty() || parsed.engine.empty()) continue;
            frame.processes[*parsed.pid].busy.add(value, true);
            adapterEngines[{parsed.adapter, parsed.engine}].add(value);
            engines_ << prefix << *parsed.pid << ',' << csv(processName(*parsed.pid)) << ',' << csv(parsed.adapter) << ','
                << csv(parsed.engine) << ',' << optionalNumber(value) << ',' << (value ? "ok" : "invalid") << '\n';
        }
        for (const auto& [engine, aggregate] : adapterEngines) frame.adapters[engine.first].busy.add(aggregate.complete(), true);
        std::set<std::string> memoryInstances;
        for (size_t i : {1u, 2u}) for (const auto& [name, unused] : counters[i].values) { (void)unused; memoryInstances.insert(name); }
        for (const auto& instance : memoryInstances) {
            auto parsed = parseGpuInstance(instance);
            if (!parsed.pid || parsed.adapter.empty()) continue;
            std::array<std::optional<double>, 2> memory;
            std::array<std::string, 2> status;
            for (size_t i = 0; i < 2; ++i) {
                const auto& counter = counters[i + 1];
                const auto it = counter.values.find(instance);
                Aggregate one;
                if (it != counter.values.end()) {
                    if (it->second) memory[i] = *it->second / MiB;
                    one.add(memory[i]);
                }
                status[i] = one.status(counter.available);
                auto& dest = i == 0 ? frame.processes[*parsed.pid].dedicated : frame.processes[*parsed.pid].shared;
                // One side present with the other missing is a partial total.
                dest.add(memory[i]);
            }
            gpuMemory_ << prefix << *parsed.pid << ',' << csv(processName(*parsed.pid)) << ',' << csv(parsed.adapter) << ','
                << optionalNumber(memory[0]) << ',' << optionalNumber(memory[1]) << ',' << status[0] << ',' << status[1] << '\n';
        }
        for (size_t i : {3u, 4u}) for (const auto& [instance, value] : counters[i].values) {
            const auto parsed = parseGpuInstance(instance);
            if (parsed.adapter.empty()) continue;
            auto& dest = i == 3 ? frame.adapters[parsed.adapter].dedicated : frame.adapters[parsed.adapter].shared;
            dest.add(value ? std::optional<double>(*value / MiB) : std::nullopt);
        }
        for (const auto& [adapter, g] : frame.adapters) {
            adapters_ << prefix << csv(adapter) << ',' << optionalNumber(g.busy.complete()) << ','
                << optionalNumber(g.dedicated.complete()) << ',' << optionalNumber(g.shared.complete()) << ','
                << g.busy.status(counters[0].available) << ',' << g.dedicated.status(counters[3].available) << ','
                << g.shared.status(counters[4].available) << '\n';
        }
        return frame;
    }
    void flush() {
        for (auto* file : {&log_, &processes_, &threads_, &engines_, &gpuMemory_, &adapters_, &system_}) file->flush();
    }
public:
    Recorder(fs::path directory, Options opt) : directory_(std::move(directory)), opt_(std::move(opt)),
        log_(output(directory_ / "session.log")), processes_(output(directory_ / "processes.csv")),
        threads_(output(directory_ / "threads.csv")), engines_(output(directory_ / "gpu_engines.csv")),
        gpuMemory_(output(directory_ / "gpu_memory.csv")), adapters_(output(directory_ / "adapters.csv")),
        system_(output(directory_ / "system.csv")), gpu_(log_) {
        log_ << "CyberpunkVRProfiler " << Version << " | " << timestamp() << " | Windows x64\n"
            << "logical_processors=" << processors_ << " interval_ms=" << opt_.interval << " threads=" << opt_.threads
            << " target=" << utf8(opt_.target) << " profiler_pid=" << GetCurrentProcessId() << '\n'
            << "All timestamps UTC; memory MiB; process CPU uses total machine capacity; thread CPU uses one core.\n"
            << "GPU = busiest engine, not a sum. GPU thread attribution is unavailable.\n"
            << "Blank means unavailable/first observation, not zero. Check *_status columns.\n"
            << "Per-process VRAM includes cross-process shared allocations; do not sum it for adapter usage.\n"
            << "Polling can miss threads/processes created and destroyed between samples. CPU uses elapsed time, not frequency-scaled utility.\n";
        using VersionQuery = LONG(WINAPI*)(PRTL_OSVERSIONINFOW);
        auto getVersion = reinterpret_cast<VersionQuery>(GetProcAddress(GetModuleHandleW(L"ntdll.dll"), "RtlGetVersion"));
        RTL_OSVERSIONINFOW os{}; os.dwOSVersionInfoSize = sizeof(os);
        if (getVersion && getVersion(&os) == 0) log_ << "Windows=" << os.dwMajorVersion << '.' << os.dwMinorVersion << '.' << os.dwBuildNumber << '\n';
        MEMORYSTATUSEX memory{}; memory.dwLength = sizeof(memory);
        if (GlobalMemoryStatusEx(&memory)) log_ << "RAM_capacity_mib=" << number(static_cast<double>(memory.ullTotalPhys) / MiB) << '\n';
        describeAdapters(log_);
        const std::string prefix = "sample,utc,elapsed_s,interval_s,";
        processes_ << prefix << "pid,process_start_100ns,process,cpu_machine_pct,cpu_one_core_pct,ram_working_set_mib,ram_private_commit_mib,threads,handles,io_read_mib_s,io_write_mib_s,gpu_busiest_engine_pct,vram_dedicated_mib,gpu_shared_mib,gpu_status,vram_status,shared_status\n";
        threads_ << prefix << "pid,process_start_100ns,process,tid,thread_start_100ns,thread_name,cpu_one_core_pct,user_cpu_pct,kernel_cpu_pct,context_switch_delta,priority,state,wait_reason\n";
        engines_ << prefix << "pid,process,adapter,engine,utilization_pct,status\n";
        gpuMemory_ << prefix << "pid,process,adapter,vram_dedicated_mib,gpu_shared_mib,vram_status,shared_status\n";
        adapters_ << prefix << "adapter,gpu_busiest_engine_pct,vram_dedicated_mib,gpu_shared_mib,gpu_status,vram_status,shared_status\n";
        system_ << prefix << "ram_used_mib,ram_available_mib,commit_used_mib,commit_limit_mib,process_count,thread_count,sample_work_ms\n";
        start_ = Clock::now();
    }
    void sample() {
        const auto begin = Clock::now();
        auto counters = gpu_.sample();
        auto current = native_.read();
        const auto snapshotTime = Clock::now();
        const double elapsed = std::chrono::duration<double>(snapshotTime - start_).count();
        const double interval = sample_ ? std::chrono::duration<double>(snapshotTime - lastSnapshot_).count() : 0;
        lastSnapshot_ = snapshotTime;
        const auto stamp = timestamp();
        const std::string prefix = std::to_string(sample_) + ',' + stamp + ',' + number(elapsed) + ',' + number(interval, 6) + ',';
        std::map<uint32_t, std::string> names;
        for (const auto& p : current) names[p.pid] = utf8(p.name);
        auto gpu = writeGpu(counters, prefix, names);
        std::map<ProcessKey, Process> next;
        std::map<ThreadKey, Thread> nextThreads;
        size_t threadCount = 0;
        bool targetNow = false;
        std::vector<std::pair<double, std::string>> notable;
        for (auto& p : current) {
            const auto pk = key(p);
            const auto name = names[p.pid];
            const bool target = _wcsicmp(p.name.c_str(), opt_.target.c_str()) == 0;
            targetNow |= target;
            threadCount += p.threads.size();
            const auto prev = previous_.find(pk);
            const bool valid = prev != previous_.end() && interval > 0;
            std::optional<double> cpu, read, write;
            if (valid) {
                cpu = cpuPercent(p.user + p.kernel, prev->second.user + prev->second.kernel, interval);
                if (p.readBytes >= prev->second.readBytes) read = static_cast<double>(p.readBytes - prev->second.readBytes) / interval / MiB;
                if (p.writeBytes >= prev->second.writeBytes) write = static_cast<double>(p.writeBytes - prev->second.writeBytes) / interval / MiB;
            }
            auto& g = gpu.processes[p.pid];
            processes_ << prefix << p.pid << ',' << p.created << ',' << csv(name) << ','
                << (cpu ? number(*cpu / processors_) : "") << ',' << optionalNumber(cpu) << ','
                << number(static_cast<double>(p.working) / MiB) << ',' << number(static_cast<double>(p.privateBytes) / MiB) << ','
                << p.threads.size() << ',' << p.handles << ',' << optionalNumber(read) << ',' << optionalNumber(write) << ','
                << optionalNumber(g.busy.complete()) << ',' << optionalNumber(g.dedicated.complete()) << ',' << optionalNumber(g.shared.complete()) << ','
                << g.busy.status(counters[0].available) << ',' << g.dedicated.status(counters[1].available) << ',' << g.shared.status(counters[2].available) << '\n';
            auto& s = stats_[pk];
            s.name = name; ++s.samples;
            if (cpu) { ++s.cpuIntervals; s.seconds += interval; s.cpuSeconds += *cpu * interval / 100; s.peakCpu = std::max(s.peakCpu, *cpu / processors_); }
            s.peakWorking = std::max(s.peakWorking, static_cast<double>(p.working) / MiB);
            s.peakPrivate = std::max(s.peakPrivate, static_cast<double>(p.privateBytes) / MiB);
            peak(s.peakGpu, g.busy.complete()); peak(s.peakDedicated, g.dedicated.complete()); peak(s.peakShared, g.shared.complete());
            s.gpuSamples += g.busy.complete().has_value(); s.dedicatedSamples += g.dedicated.complete().has_value(); s.sharedSamples += g.shared.complete().has_value();
            if (target || (g.dedicated.complete().value_or(0) > 16) || (cpu.value_or(0) / processors_ > 1)) {
                auto show = [](const std::optional<double>& v) { return v ? number(*v, 1) : "N/A"; };
                const std::string line = name + " pid=" + std::to_string(p.pid) + " CPU=" + (cpu ? number(*cpu / processors_, 1) : "N/A") +
                    "% GPU=" + show(g.busy.complete()) + "% VRAM=" + show(g.dedicated.complete()) + " MiB shared=" + show(g.shared.complete()) + " MiB";
                notable.emplace_back(target ? 1e12 : g.dedicated.complete().value_or(0), line);
            }
            if (opt_.threads != "none" && (opt_.threads != "target" || target)) for (const auto& t : p.threads) {
                const auto tk = key(p, t);
                auto old = previousThreads_.find(tk);
                const bool known = old != previousThreads_.end() && interval > 0;
                std::optional<double> tcpu, user, kernel;
                uint32_t switches = 0;
                if (known) {
                    user = cpuPercent(t.user, old->second.user, interval);
                    kernel = cpuPercent(t.kernel, old->second.kernel, interval);
                    tcpu = *user + *kernel;
                    switches = t.switches - old->second.switches; // unsigned wrap is intentional
                }
                auto& ts = threadStats_[tk];
                ts.process = name;
                if (!known || (target && sample_ % 10 == 0)) ts.name = threadName(t);
                ++ts.samples;
                if (tcpu) { ts.cpuSeconds += *tcpu * interval / 100; ts.peakCpu = std::max(ts.peakCpu, *tcpu); ts.switches += switches; }
                if (opt_.threads != "active" || !known || tcpu.value_or(0) > 0 || switches > 0) {
                    threads_ << prefix << p.pid << ',' << p.created << ',' << csv(name) << ',' << t.tid << ',' << t.created << ',' << csv(ts.name) << ','
                        << optionalNumber(tcpu) << ',' << optionalNumber(user) << ',' << optionalNumber(kernel) << ','
                        << (known ? std::to_string(switches) : "") << ',' << t.priority << ',' << t.state << ',' << t.wait << '\n';
                }
                nextThreads.emplace(tk, t);
            }
            if (prev == previous_.end()) log_ << stamp << " PROCESS_OBSERVED pid=" << p.pid << " start=" << p.created << " name=" << csv(name) << '\n';
            next.emplace(pk, std::move(p));
        }
        for (const auto& [pk, p] : previous_) if (!next.contains(pk)) log_ << stamp << " PROCESS_GONE pid=" << p.pid << " start=" << p.created << " name=" << csv(utf8(p.name)) << '\n';
        if (targetNow != targetSeen_) {
            log_ << stamp << (targetNow ? " TARGET_PRESENT " : " TARGET_EXITED ") << utf8(opt_.target) << '\n';
            std::cout << (targetNow ? "Target detected: " : "Target exited; recording continues: ") << utf8(opt_.target) << '\n';
            targetSeen_ = targetNow;
        }
        std::sort(notable.rbegin(), notable.rend());
        log_ << stamp << " SAMPLE=" << sample_ << " elapsed_s=" << number(elapsed) << " processes=" << current.size() << " threads=" << threadCount << '\n';
        for (const auto& [score, line] : notable) { (void)score; log_ << "  " << line << '\n'; }
        previous_ = std::move(next);
        previousThreads_ = std::move(nextThreads);
        PERFORMANCE_INFORMATION memory{}; memory.cb = sizeof(memory);
        system_ << prefix;
        if (GetPerformanceInfo(&memory, sizeof(memory))) {
            const double pageMiB = static_cast<double>(memory.PageSize) / MiB;
            system_ << number(static_cast<double>(memory.PhysicalTotal - memory.PhysicalAvailable) * pageMiB) << ','
                << number(static_cast<double>(memory.PhysicalAvailable) * pageMiB) << ','
                << number(static_cast<double>(memory.CommitTotal) * pageMiB) << ','
                << number(static_cast<double>(memory.CommitLimit) * pageMiB) << ',';
        } else system_ << ",,,,";
        // Includes CSV/log writes and the previous streams' flush, so overhead
        // remains visible in the capture (system.csv's own flush is excluded).
        flush();
        const double workMs = std::chrono::duration<double, std::milli>(Clock::now() - begin).count();
        system_ << current.size() << ',' << threadCount << ',' << number(workMs) << '\n';
        system_.flush();
        if (sample_ % 5 == 0) {
            std::cout << "[" << number(elapsed, 0) << "s] processes=" << current.size() << " threads=" << threadCount
                << " sample=" << number(workMs, 1) << "ms GPU=" << (counters[0].available ? "available" : "unavailable/warming up") << '\n';
            if (!notable.empty()) std::cout << "  " << notable.front().second << '\n';
        }
        ++sample_;
        if (elapsed >= nextSummary_) { summary(false); nextSummary_ = elapsed + 30; }
    }
    void summary(bool final) {
        auto text = output(directory_ / "summary.txt.tmp");
        auto csvOut = output(directory_ / "summary.csv.tmp");
        auto threadOut = output(directory_ / "thread_summary.csv.tmp");
        text << "CyberpunkVRProfiler " << Version << " | " << timestamp() << (final ? " | FINAL\n" : " | CHECKPOINT\n")
            << "Samples: " << sample_ << " | logical processors: " << processors_ << '\n'
            << "Process CPU % = whole machine; thread CPU % = one core. Memory = MiB. N/A = no valid sample.\n"
            << "VRAM is per-process attributed memory; use adapters.csv for total GPU memory.\n"
            << "CPU totals count matched polling intervals only; short-lived threads can be missed.\n\n"
            << "ALL PROCESSES (sorted by peak dedicated VRAM)\n";
        csvOut << "pid,process_start_100ns,process,samples,cpu_valid_intervals,cpu_observed_s,cpu_avg_machine_pct,cpu_peak_machine_pct,ram_peak_working_set_mib,ram_peak_private_commit_mib,gpu_peak_busiest_engine_pct,vram_peak_dedicated_mib,gpu_peak_shared_mib,gpu_valid_samples,vram_valid_samples,shared_valid_samples\n";
        std::vector<std::pair<ProcessKey, const ProcessStats*>> sorted;
        for (const auto& [pk, s] : stats_) sorted.emplace_back(pk, &s);
        std::sort(sorted.begin(), sorted.end(), [](const auto& a, const auto& b) { return a.second->peakDedicated.value_or(-1) > b.second->peakDedicated.value_or(-1); });
        for (const auto& [pk, ptr] : sorted) {
            const auto& s = *ptr;
            auto show = [](const std::optional<double>& v) { return v ? number(*v, 1) : "N/A"; };
            text << s.name << " pid=" << pk.first << " start=" << pk.second << " | CPU peak=" << (s.cpuIntervals ? number(s.peakCpu, 1) : "N/A")
                << "% | GPU peak=" << show(s.peakGpu) << "% | VRAM peak=" << show(s.peakDedicated) << " | shared peak=" << show(s.peakShared)
                << " | RAM peak=" << number(s.peakWorking, 1) << '\n';
            csvOut << pk.first << ',' << pk.second << ',' << csv(s.name) << ',' << s.samples << ',' << s.cpuIntervals << ',' << number(s.cpuSeconds) << ','
                << (s.seconds > 0 ? number(100 * s.cpuSeconds / s.seconds / processors_) : "") << ',' << (s.cpuIntervals ? number(s.peakCpu) : "") << ','
                << number(s.peakWorking) << ',' << number(s.peakPrivate) << ',' << optionalNumber(s.peakGpu) << ',' << optionalNumber(s.peakDedicated) << ','
                << optionalNumber(s.peakShared) << ',' << s.gpuSamples << ',' << s.dedicatedSamples << ',' << s.sharedSamples << '\n';
        }
        threadOut << "pid,process_start_100ns,process,tid,thread_start_100ns,thread_name,observations,cpu_observed_s,cpu_peak_one_core_pct,context_switches_observed\n";
        std::vector<std::pair<ThreadKey, const ThreadStats*>> hot;
        for (const auto& [tk, s] : threadStats_) hot.emplace_back(tk, &s);
        std::sort(hot.begin(), hot.end(), [](const auto& a, const auto& b) { return a.second->cpuSeconds > b.second->cpuSeconds; });
        text << "\nTOP 100 CPU THREADS (all observed threads in thread_summary.csv)\n";
        size_t count = 0;
        for (const auto& [tk, ptr] : hot) {
            const auto& s = *ptr;
            threadOut << std::get<0>(tk) << ',' << std::get<1>(tk) << ',' << csv(s.process) << ',' << std::get<2>(tk) << ',' << std::get<3>(tk) << ','
                << csv(s.name) << ',' << s.samples << ',' << number(s.cpuSeconds) << ',' << (s.samples > 1 ? number(s.peakCpu) : "") << ',' << s.switches << '\n';
            if (count++ < 100) text << s.process << " pid=" << std::get<0>(tk) << " tid=" << std::get<2>(tk) << ' ' << s.name
                << " | CPU observed_s=" << number(s.cpuSeconds) << " peak_core_pct=" << (s.samples > 1 ? number(s.peakCpu, 1) : "N/A") << '\n';
        }
        text.close(); csvOut.close(); threadOut.close();
        for (const auto* name : {L"summary.txt", L"summary.csv", L"thread_summary.csv"}) {
            const fs::path dest = directory_ / name;
            const fs::path temp = directory_ / (std::wstring(name) + L".tmp");
            if (!MoveFileExW(temp.c_str(), dest.c_str(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH))
                throw std::runtime_error("Cannot save summary: " + hexStatus(GetLastError()));
        }
    }
    void finish() { summary(true); log_ << timestamp() << " STOP normal samples=" << sample_ << '\n'; flush(); }
};

int wmain(int argc, wchar_t** argv) {
    SetConsoleOutputCP(CP_UTF8);
    fs::path directory;
    int exitCode = 0;
    try {
        auto opt = options(argc, argv);
        if (opt.help) {
            std::cout << "CyberpunkVRProfiler " << Version << " - portable Windows x64 resource logger\n"
                << "  --interval-ms 250..60000  Sample period (default 1000)\n"
                << "  --duration 1..86400      Stop after N seconds (default: until Q/Ctrl+C)\n"
                << "  --threads active|all|target|none (default active: all processes, omit unchanged idle threads)\n"
                << "  --target Cyberpunk2077.exe  Highlight this process; also used by --threads target\n"
                << "  --output PATH           Parent directory for a new timestamped session\n"
                << "All processes are always recorded. GPU/VRAM are per process/engine, not per CPU thread.\n";
            return 0;
        }
        stopEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
        finishedEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
        if (!stopEvent || !finishedEvent) throw std::runtime_error("Cannot create stop events");
        SetConsoleCtrlHandler(onConsole, TRUE);
        directory = createSession(opt);
        std::cout << "CyberpunkVRProfiler " << Version << "\nLogs: " << utf8(directory.wstring())
            << "\nLaunch the game normally. Recording all processes.\nPress Q, Esc or Ctrl+C here to finish.\n";
        Recorder recorder(directory, opt);
        const auto started = Clock::now();
        auto next = started;
        while (WaitForSingleObject(stopEvent, 0) != WAIT_OBJECT_0) {
            if (_kbhit()) { const int c = _getch(); if (c == 'q' || c == 'Q' || c == 27) break; }
            const auto now = Clock::now();
            if (opt.duration && now - started >= std::chrono::seconds(opt.duration)) break;
            if (now >= next) {
                recorder.sample();
                next += std::chrono::milliseconds(opt.interval);
                // Never do a burst of catch-up samples when a counter stalls.
                if (next <= Clock::now()) next = Clock::now() + std::chrono::milliseconds(opt.interval);
            } else WaitForSingleObject(stopEvent, 50);
        }
        recorder.finish();
        std::cout << "Saved. Send the entire session folder:\n" << utf8(directory.wstring()) << '\n';
    } catch (const std::exception& e) {
        std::cerr << "ERROR: " << e.what() << '\n';
        if (!directory.empty()) {
            std::ofstream error(directory / "session.log", std::ios::binary | std::ios::app);
            error << timestamp() << " ERROR " << e.what() << '\n';
            std::cerr << "Partial logs: " << utf8(directory.wstring()) << '\n';
        }
        exitCode = 1;
    }
    if (finishedEvent) SetEvent(finishedEvent);
    // Keep event handles alive until process teardown: a console-close handler
    // may still be waiting on finishedEvent on another thread.
    return exitCode;
}
