#include "xrfg/bridge_flight_logger.hpp"
#include "xrfg/ini_file.hpp"

#include <windows.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <cstdio>
#include <cwchar>
#include <deque>
#include <limits>
#include <mutex>
#include <string>
#include <system_error>
#include <vector>

namespace xrfg {
namespace {

#ifndef XRFG_IMPLEMENTATION_VERSION
#define XRFG_IMPLEMENTATION_VERSION 0
#endif

constexpr wchar_t kConfigurationFileName[] = L"ofxr_bridge.ini";
constexpr wchar_t kConfigurationSection[] = L"diagnostics";
constexpr std::uint64_t kMegabyte = 1024ull * 1024ull;

[[nodiscard]] const char* operation_name(
    BridgeFlightOperation operation) noexcept {
    switch (operation) {
    case BridgeFlightOperation::logger: return "logger";
    case BridgeFlightOperation::negotiation: return "negotiation";
    case BridgeFlightOperation::instance_create: return "instance_create";
    case BridgeFlightOperation::instance_destroy: return "instance_destroy";
    case BridgeFlightOperation::session_create: return "session_create";
    case BridgeFlightOperation::session_destroy: return "session_destroy";
    case BridgeFlightOperation::session_begin: return "session_begin";
    case BridgeFlightOperation::session_end: return "session_end";
    case BridgeFlightOperation::application_wait_frame: return "app_wait_frame";
    case BridgeFlightOperation::application_begin_frame: return "app_begin_frame";
    case BridgeFlightOperation::application_end_frame: return "app_end_frame";
    case BridgeFlightOperation::application_swapchain_acquire:
        return "app_swapchain_acquire";
    case BridgeFlightOperation::application_swapchain_wait:
        return "app_swapchain_wait";
    case BridgeFlightOperation::application_swapchain_release:
        return "app_swapchain_release";
    case BridgeFlightOperation::private_swapchain_acquire:
        return "private_swapchain_acquire";
    case BridgeFlightOperation::private_swapchain_wait:
        return "private_swapchain_wait";
    case BridgeFlightOperation::private_swapchain_release:
        return "private_swapchain_release";
    case BridgeFlightOperation::synthesis_initialize: return "synthesis_initialize";
    case BridgeFlightOperation::synthesis_prime: return "synthesis_prime";
    case BridgeFlightOperation::synthesis_pair: return "synthesis_pair";
    case BridgeFlightOperation::downstream_first_end_frame:
        return "downstream_first_end_frame";
    case BridgeFlightOperation::internal_wait_frame: return "internal_wait_frame";
    case BridgeFlightOperation::internal_begin_frame: return "internal_begin_frame";
    case BridgeFlightOperation::internal_end_frame: return "internal_end_frame";
    case BridgeFlightOperation::continuity_reset: return "continuity_reset";
    case BridgeFlightOperation::gpu_drain: return "gpu_drain";
    case BridgeFlightOperation::swapchain_create: return "swapchain_create";
    case BridgeFlightOperation::swapchain_eligibility: return "swapchain_eligibility";
    case BridgeFlightOperation::projection_mapping: return "projection_mapping";
    case BridgeFlightOperation::generation_prepare: return "generation_prepare";
    case BridgeFlightOperation::session_binding: return "session_binding";
    case BridgeFlightOperation::swapchain_image: return "swapchain_image";
    case BridgeFlightOperation::d3d11_capture: return "d3d11_capture";
    case BridgeFlightOperation::d3d11_publish: return "d3d11_publish";
    case BridgeFlightOperation::runtime_identity: return "runtime_identity";
    case BridgeFlightOperation::presenter_submission:
        return "presenter_submission";
    case BridgeFlightOperation::presenter_transition:
        return "presenter_transition";
    case BridgeFlightOperation::nvidia_gpu_stages:
        return "nvidia_gpu_stages";
    case BridgeFlightOperation::nvidia_gpu_total:
        return "nvidia_gpu_total";
    case BridgeFlightOperation::presenter_pace:
        return "presenter_pace";
    case BridgeFlightOperation::runtime_entry_section:
        return "runtime_entry_section";
    case BridgeFlightOperation::steamvr_delivery_attach:
        return "steamvr_delivery_attach";
    case BridgeFlightOperation::steamvr_delivery:
        return "steamvr_delivery";
    case BridgeFlightOperation::vram_usage:
        return "vram_usage";
    case BridgeFlightOperation::projection_pose:
        return "projection_pose";
    case BridgeFlightOperation::reference_space:
        return "reference_space";
    case BridgeFlightOperation::view_locate:
        return "view_locate";
    case BridgeFlightOperation::view_configuration:
        return "view_configuration";
    case BridgeFlightOperation::projection_view_rect:
        return "projection_view_rect";
    case BridgeFlightOperation::process_excluded:
        return "process_excluded";
    case BridgeFlightOperation::vulkan_bridge:
        return "vulkan_bridge";
    case BridgeFlightOperation::presenter_vsync_lock:
        return "presenter_vsync_lock";
    case BridgeFlightOperation::presenter_frame_presented:
        return "presenter_frame_presented";
    case BridgeFlightOperation::vulkan_negotiation:
        return "vulkan_negotiation";
    case BridgeFlightOperation::vulkan_interop:
        return "vulkan_interop";
    case BridgeFlightOperation::d3d11_bridge:
        return "d3d11_bridge";
    case BridgeFlightOperation::synthesis_frame_start_wait:
        return "synthesis_frame_start_wait";
    case BridgeFlightOperation::embedded_configuration: return "embedded_configuration";
    case BridgeFlightOperation::synthesis_gpu_span:
        return "synthesis_gpu_span";
    case BridgeFlightOperation::virtual_clock_clamp:
        return "virtual_clock_clamp";
    case BridgeFlightOperation::session_state:
        return "session_state";
    case BridgeFlightOperation::presenter_pair_release:
        return "presenter_pair_release";
    case BridgeFlightOperation::recording: return "recording";
    case BridgeFlightOperation::history_gpu_ready: return "history_gpu_ready";
    case BridgeFlightOperation::synthesis_input_source: return "synthesis_input_source";
    case BridgeFlightOperation::deferred_capture: return "deferred_capture";
    case BridgeFlightOperation::history_capture_wait: return "history_capture_wait";
    case BridgeFlightOperation::arm_control: return "arm_control";
    }
    return "unknown";
}

// The records that say what a session is, as against what it did each frame:
// written once or on a change, tens of them in a session. These are kept in
// memory so a file opened in the middle of a session still begins with them.
// presenter_transition is both - its 600-604 and 700-range selectors describe
// the session (its shape, and the presenter's starts and stops, a handful in
// a session), its others are per pair.
[[nodiscard]] bool session_record(
    BridgeFlightOperation operation, std::int64_t result) noexcept {
    switch (operation) {
    case BridgeFlightOperation::logger:
    case BridgeFlightOperation::negotiation:
    case BridgeFlightOperation::instance_create:
    case BridgeFlightOperation::instance_destroy:
    case BridgeFlightOperation::session_create:
    case BridgeFlightOperation::session_destroy:
    case BridgeFlightOperation::session_begin:
    case BridgeFlightOperation::session_end:
    case BridgeFlightOperation::synthesis_initialize:
    case BridgeFlightOperation::swapchain_create:
    case BridgeFlightOperation::swapchain_eligibility:
    case BridgeFlightOperation::session_binding:
    case BridgeFlightOperation::swapchain_image:
    case BridgeFlightOperation::runtime_identity:
    case BridgeFlightOperation::embedded_configuration:
    case BridgeFlightOperation::session_state:
    case BridgeFlightOperation::steamvr_delivery_attach:
    case BridgeFlightOperation::vulkan_negotiation:
    case BridgeFlightOperation::d3d11_bridge:
    case BridgeFlightOperation::view_configuration:
    case BridgeFlightOperation::projection_view_rect:
    case BridgeFlightOperation::process_excluded:
    case BridgeFlightOperation::vulkan_bridge:
    case BridgeFlightOperation::recording:
    case BridgeFlightOperation::arm_control:
        return true;
    case BridgeFlightOperation::presenter_transition:
        return (result >= 600 && result <= 604) ||
            (result >= 700 && result < 800);
    default:
        return false;
    }
}

// A begin record has no result yet, so a presenter_transition - which is
// only ever written as an event - is not asked about here.
[[nodiscard]] bool session_record(BridgeFlightOperation operation) noexcept {
    return operation != BridgeFlightOperation::presenter_transition &&
        session_record(operation, 0);
}

[[nodiscard]] std::filesystem::path fallback_log_directory() {
    std::array<wchar_t, 32768> buffer{};
    const DWORD length = GetEnvironmentVariableW(
        L"LOCALAPPDATA", buffer.data(), static_cast<DWORD>(buffer.size()));
    if (length == 0 || length >= buffer.size()) {
        return {};
    }
    return std::filesystem::path(buffer.data()) / L"OFXR Bridge" / L"Logs";
}

// `attempt` above zero adds a suffix: the recorder can be switched off and on
// again inside the second the name resolves to, and the second file must not
// replace the first.
[[nodiscard]] std::wstring log_file_name(unsigned attempt) {
    SYSTEMTIME time{};
    GetLocalTime(&time);
    std::array<wchar_t, 128> name{};
    _snwprintf_s(
        name.data(),
        name.size(),
        _TRUNCATE,
        L"ofxr-bridge-flight-%04u%02u%02u-%02u%02u%02u-pid%lu",
        static_cast<unsigned>(time.wYear),
        static_cast<unsigned>(time.wMonth),
        static_cast<unsigned>(time.wDay),
        static_cast<unsigned>(time.wHour),
        static_cast<unsigned>(time.wMinute),
        static_cast<unsigned>(time.wSecond),
        static_cast<unsigned long>(GetCurrentProcessId()));
    std::wstring result = name.data();
    if (attempt != 0) {
        result += L"-" + std::to_wstring(attempt + 1);
    }
    return result + L".log";
}

[[nodiscard]] HANDLE create_log_file(
    const std::filesystem::path& directory,
    std::filesystem::path* output_path) noexcept {
    try {
        if (directory.empty() || output_path == nullptr) {
            return INVALID_HANDLE_VALUE;
        }
        std::error_code error;
        std::filesystem::create_directories(directory, error);
        if (error) {
            return INVALID_HANDLE_VALUE;
        }
        // FILE_FLAG_OVERLAPPED is what makes the positioned writes in
        // Impl::write concurrent. Without it the kernel serialises every
        // WriteFile on the file object no matter which offset is passed, and
        // the two threads queue behind each other exactly as they did when a
        // lock held the file pointer: measured at 46.5 us p99 either way,
        // against 19.5 us with the flag. Removing it silently reverts this
        // whole change.
        HANDLE file = INVALID_HANDLE_VALUE;
        for (unsigned attempt = 0; attempt < 16; ++attempt) {
            const auto path = directory / log_file_name(attempt);
            file = CreateFileW(
                path.c_str(),
                GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                nullptr,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED,
                nullptr);
            if (file != INVALID_HANDLE_VALUE) {
                *output_path = path;
                break;
            }
            if (GetLastError() != ERROR_FILE_EXISTS) {
                break;
            }
        }
        return file;
    } catch (...) {
        return INVALID_HANDLE_VALUE;
    }
}

[[nodiscard]] std::filesystem::path current_module_directory() noexcept {
    try {
        HMODULE module = nullptr;
        if (!GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                reinterpret_cast<LPCWSTR>(&initialize_bridge_flight_logger),
                &module)) {
            return {};
        }
        std::array<wchar_t, 32768> path{};
        const DWORD length = GetModuleFileNameW(
            module, path.data(), static_cast<DWORD>(path.size()));
        if (length == 0 || length >= path.size()) {
            return {};
        }
        return std::filesystem::path(path.data()).parent_path();
    } catch (...) {
        return {};
    }
}

} // namespace

[[nodiscard]] HANDLE write_completion_event() noexcept {
    // One event per thread, so two writes in flight never share completion
    // state. Deliberately never closed: a thread_local with a destructor is
    // destroyed at thread exit, and for the main thread that happens BEFORE
    // static destructors run - so the close record written from
    // ~BridgeFlightLogger would reach an already-closed handle and vanish,
    // leaving every log looking truncated. Measured exactly that way before
    // the handle was made to leak. A handful of logging threads each leak one
    // event for the life of the process, which the OS reclaims at exit.
    thread_local HANDLE handle = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    return handle;
}

struct BridgeFlightLogger::Impl {
    // wrap_lock, exclusive, to change; shared to write through.
    HANDLE file{INVALID_HANDLE_VALUE};
    LARGE_INTEGER frequency{};
    // The process's first use of the recorder, not the file's creation: a
    // file opened later carries records from before it existed, and they
    // keep the times they happened at.
    LARGE_INTEGER origin{};
    // Held SHARED by every write and EXCLUSIVE by a wrap, a start and a stop.
    // Shared holders do not block each other, so the two threads no longer
    // queue: the point of this lock is to stop the file changing under
    // writes in flight, not to serialise the writes themselves. At 32 MB the
    // wrap happens about once every 105 seconds.
    SRWLOCK wrap_lock = SRWLOCK_INIT;
    std::atomic<std::uint64_t> next_sequence{1};
    std::filesystem::path module_directory;
    std::filesystem::path config_path;
    std::filesystem::path path; // control_mutex
    // The byte range a writer owns is claimed with one fetch_add; nothing
    // touches the handle's own file pointer, which is what used to need a
    // lock. Reservation order is the file's order.
    std::atomic<std::uint64_t> write_offset{0};
    std::uint64_t maximum_bytes{32 * kMegabyte}; // wrap_lock
    bool flush_each_event{}; // wrap_lock
    // Read without a lock on every record; written under wrap_lock.
    std::atomic<bool> active{};

    // The session records, as the lines they are in a file. Bounded: the
    // first ones say what the process and the runtime are and are never
    // dropped, the rest is the most recent, which is what the running
    // session looks like after a title has rebuilt its swapchains many times.
    static constexpr std::size_t kKeptFirst = 128;
    static constexpr std::size_t kKeptRecent = 896;
    // V540: records a session has a handful of that say how it was set up,
    // and that the recent ones would push out (the alternating eyes'
    // anchors, 862, and their lag, 864: Cyberpunk 2077's 3042 probe records
    // left none of the anchor's; V546, a stream pair's measured eye order,
    // 868).
    static constexpr std::size_t kKeptPinned = 64;
    std::mutex kept_mutex;
    std::vector<std::string> kept_first; // kept_mutex
    std::vector<std::string> kept_pinned; // kept_mutex
    std::deque<std::string> kept_recent; // kept_mutex
    std::uint64_t kept_dropped{}; // kept_mutex

    // Start, stop and the poll that decides between them.
    std::mutex control_mutex;
    std::atomic<std::int64_t> next_poll_counter{0};
    // The setting has to read the same on two polls before it is acted on:
    // the tray replaces the ini while this reads it, and one failed read
    // would otherwise close the file and open another a quarter second later.
    bool pending_setting{}; // control_mutex
    bool pending_setting_seen{}; // control_mutex

    void keep(const char* line, std::size_t length, bool pinned) {
        std::scoped_lock lock(kept_mutex);
        if (kept_first.size() < kKeptFirst) {
            kept_first.emplace_back(line, length);
            return;
        }
        if (pinned && kept_pinned.size() < kKeptPinned) {
            kept_pinned.emplace_back(line, length);
            return;
        }
        if (kept_recent.size() >= kKeptRecent) {
            kept_recent.pop_front();
            ++kept_dropped;
        }
        kept_recent.emplace_back(line, length);
    }

    // wrap_lock held exclusive, or the file not yet published.
    void put(std::uint64_t at, const char* data, DWORD length) noexcept {
        OVERLAPPED overlapped{};
        overlapped.Offset = static_cast<DWORD>(at & 0xFFFFFFFFull);
        overlapped.OffsetHigh = static_cast<DWORD>(at >> 32);
        overlapped.hEvent = write_completion_event();
        DWORD written = 0;
        if (!WriteFile(file, data, length, &written, &overlapped) &&
            GetLastError() == ERROR_IO_PENDING) {
            static_cast<void>(GetOverlappedResult(file, &overlapped, &written, TRUE));
        }
    }

    // The session records at the head of an empty file. wrap_lock held
    // exclusive. Returns how many were written; leaves write_offset after
    // them. They never fill the file: a thousand lines against megabytes.
    std::uint64_t write_kept(std::uint64_t* dropped) noexcept {
        std::uint64_t count = 0;
        std::uint64_t at = 0;
        try {
            std::scoped_lock lock(kept_mutex);
            const auto write_line = [&](const std::string& line) {
                put(at, line.data(), static_cast<DWORD>(line.size()));
                at += line.size();
                ++count;
            };
            for (const auto& line : kept_first) write_line(line);
            for (const auto& line : kept_pinned) write_line(line);
            for (const auto& line : kept_recent) write_line(line);
            if (dropped != nullptr) *dropped = kept_dropped;
        } catch (...) {
        }
        write_offset.store(at, std::memory_order_relaxed);
        return count;
    }

    void write(
        std::uint64_t sequence,
        std::int64_t counter,
        char phase,
        BridgeFlightOperation operation,
        std::int64_t result,
        std::uint64_t duration_microseconds,
        std::uint64_t a,
        std::uint64_t b,
        std::uint64_t c) noexcept {
        const bool kept = phase == 'B'
            ? session_record(operation)
            : session_record(operation, result);
        const bool recording = active.load(std::memory_order_acquire);
        if (!recording && !kept) {
            return;
        }

        const double elapsed_ms = frequency.QuadPart > 0
            ? static_cast<double>(counter - origin.QuadPart) * 1000.0 /
                  static_cast<double>(frequency.QuadPart)
            : 0.0;
        std::array<char, 384> line{};
        const int length = std::snprintf(
            line.data(),
            line.size(),
            "seq=%llu ms=%.3f tid=%lu phase=%c op=%s result=%lld dur_us=%llu "
            "a=%llu b=%llu c=%llu\r\n",
            static_cast<unsigned long long>(sequence),
            elapsed_ms,
            static_cast<unsigned long>(GetCurrentThreadId()),
            phase,
            operation_name(operation),
            static_cast<long long>(result),
            static_cast<unsigned long long>(duration_microseconds),
            static_cast<unsigned long long>(a),
            static_cast<unsigned long long>(b),
            static_cast<unsigned long long>(c));
        if (length <= 0 || static_cast<std::size_t>(length) >= line.size()) {
            return;
        }

        const auto span = static_cast<std::uint64_t>(length);
        AcquireSRWLockShared(&wrap_lock);
        // Asked again under the lock: a stop takes it exclusive, so the file
        // is either open for the whole of this write or not written at all.
        if (active.load(std::memory_order_acquire) &&
            file != INVALID_HANDLE_VALUE) {
            std::uint64_t at =
                write_offset.fetch_add(span, std::memory_order_relaxed);
            if (at + span > maximum_bytes) {
                // Past the cap. Promote to exclusive so the truncate cannot
                // race a write still in flight, then re-check: another thread
                // may have wrapped while this one was waiting for the lock,
                // in which case its reservation already stands and this one
                // only needs a fresh slot at the new base.
                ReleaseSRWLockShared(&wrap_lock);
                AcquireSRWLockExclusive(&wrap_lock);
                if (active.load(std::memory_order_acquire) &&
                    file != INVALID_HANDLE_VALUE &&
                    write_offset.load(std::memory_order_relaxed) > maximum_bytes) {
                    rotate_file();
                }
                at = write_offset.fetch_add(span, std::memory_order_relaxed);
                ReleaseSRWLockExclusive(&wrap_lock);
                AcquireSRWLockShared(&wrap_lock);
            }
            // Positioned: the offset comes from the reservation, so
            // concurrent writers never contend for the file pointer.
            // Synchronous to this caller either way - the record has reached
            // the OS by the time write returns, which is what survives a
            // crash.
            if (active.load(std::memory_order_acquire) &&
                file != INVALID_HANDLE_VALUE) {
                OVERLAPPED overlapped{};
                overlapped.Offset = static_cast<DWORD>(at & 0xFFFFFFFFull);
                overlapped.OffsetHigh = static_cast<DWORD>(at >> 32);
                overlapped.hEvent = write_completion_event();
                DWORD written = 0;
                BOOL complete = WriteFile(
                    file, line.data(), static_cast<DWORD>(length), &written,
                    &overlapped);
                if (!complete && GetLastError() == ERROR_IO_PENDING) {
                    complete =
                        GetOverlappedResult(file, &overlapped, &written, TRUE);
                }
                if (complete && flush_each_event) {
                    static_cast<void>(FlushFileBuffers(file));
                }
            }
        }
        ReleaseSRWLockShared(&wrap_lock);
        // After the file, so a wrap that rewrote the kept records while this
        // one waited does not put it in the file twice.
        if (kept) {
            try {
                keep(line.data(), static_cast<std::size_t>(length),
                    operation == BridgeFlightOperation::embedded_configuration &&
                        (result == 862 || result == 864 || result == 868));
            } catch (...) {
            }
        }
    }

    // Past the cap, wrap_lock held exclusive. The full file becomes
    // `<name>.1`, replacing the one before it, and recording carries on in a
    // fresh file under the original name, which starts with what the session
    // is. A truncate in place threw away everything recorded so far the
    // moment the cap was reached, so a comparison made a few seconds earlier
    // was lost with it. Where the rename is refused - a reader holding the
    // file without delete sharing - the file is truncated in place as before.
    void rotate_file() noexcept {
        try {
            std::filesystem::path previous = path;
            previous += L".1";
            static_cast<void>(FlushFileBuffers(file));
            CloseHandle(file);
            file = INVALID_HANDLE_VALUE;
            const bool moved = MoveFileExW(
                path.c_str(), previous.c_str(), MOVEFILE_REPLACE_EXISTING) != FALSE;
            // The same sharing and flags as create_log_file, whose comment
            // says why FILE_FLAG_OVERLAPPED matters.
            file = CreateFileW(
                path.c_str(),
                GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                nullptr,
                moved ? CREATE_ALWAYS : OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED,
                nullptr);
            if (file == INVALID_HANDLE_VALUE) {
                return;
            }
            if (!moved) {
                LARGE_INTEGER beginning{};
                if (!SetFilePointerEx(file, beginning, nullptr, FILE_BEGIN) ||
                    !SetEndOfFile(file)) {
                    return;
                }
            }
            static_cast<void>(write_kept(nullptr));
        } catch (...) {
        }
    }

    // control_mutex held.
    void start() noexcept {
        try {
            if (active.load(std::memory_order_acquire)) {
                return;
            }
            const UINT maximum_mb = std::clamp<UINT>(
                ini_file::read_int(config_path, kConfigurationSection, L"max_file_mb", 32),
                1,
                256);
            const bool flush = ini_file::read_int(config_path, kConfigurationSection, L"flush_each_event", 0) != 0;
            std::filesystem::path new_path;
            HANDLE new_file = create_log_file(module_directory, &new_path);
            if (new_file == INVALID_HANDLE_VALUE) {
                new_file = create_log_file(fallback_log_directory(), &new_path);
            }
            if (new_file == INVALID_HANDLE_VALUE) {
                return;
            }
            std::uint64_t dropped = 0;
            std::uint64_t written = 0;
            AcquireSRWLockExclusive(&wrap_lock);
            file = new_file;
            path = new_path;
            maximum_bytes = static_cast<std::uint64_t>(maximum_mb) * kMegabyte;
            flush_each_event = flush;
            written = write_kept(&dropped);
            active.store(true, std::memory_order_release);
            ReleaseSRWLockExclusive(&wrap_lock);
            LARGE_INTEGER now{};
            QueryPerformanceCounter(&now);
            write(
                next_sequence.fetch_add(1, std::memory_order_relaxed),
                now.QuadPart, 'I', BridgeFlightOperation::recording, 1, 0,
                written, dropped, 0);
        } catch (...) {
        }
    }

    // control_mutex held.
    void stop() noexcept {
        if (!active.load(std::memory_order_acquire)) {
            return;
        }
        LARGE_INTEGER now{};
        QueryPerformanceCounter(&now);
        write(
            next_sequence.fetch_add(1, std::memory_order_relaxed),
            now.QuadPart, 'I', BridgeFlightOperation::recording, 0, 0, 0, 0, 0);
        // Exclusive, so this waits out any write still inside the shared
        // section before the handle it is writing through is closed.
        AcquireSRWLockExclusive(&wrap_lock);
        active.store(false, std::memory_order_release);
        if (file != INVALID_HANDLE_VALUE) {
            static_cast<void>(FlushFileBuffers(file));
            CloseHandle(file);
            file = INVALID_HANDLE_VALUE;
        }
        ReleaseSRWLockExclusive(&wrap_lock);
    }
};

BridgeFlightLogger::BridgeFlightLogger() noexcept = default;

BridgeFlightLogger::~BridgeFlightLogger() {
    shutdown();
}

void BridgeFlightLogger::initialize(
    const std::filesystem::path& module_directory) noexcept {
    try {
        if (impl_) {
            return;
        }
        auto implementation = std::make_unique<Impl>();
        implementation->module_directory = module_directory;
        implementation->config_path = module_directory / kConfigurationFileName;
        if (!QueryPerformanceFrequency(&implementation->frequency)) {
            return;
        }
        const bool wanted = ini_file::read_int(implementation->config_path, kConfigurationSection, L"logging_enabled", 0) != 0;
        const UINT maximum_mb = std::clamp<UINT>(
            ini_file::read_int(implementation->config_path, kConfigurationSection, L"max_file_mb", 32),
            1,
            256);
        const bool flush = ini_file::read_int(implementation->config_path, kConfigurationSection, L"flush_each_event", 0) != 0;
        // Last, so the record that follows is at time zero as it always was.
        if (!QueryPerformanceCounter(&implementation->origin)) {
            return;
        }
        impl_ = std::move(implementation);
        // The first record of every file, kept like the other session
        // records, so a file opened later still says which build and which
        // process it is from.
        event(
            BridgeFlightOperation::logger,
            XRFG_IMPLEMENTATION_VERSION,
            maximum_mb,
            flush ? 1u : 0u,
            GetCurrentProcessId());
        if (wanted) {
            std::scoped_lock lock(impl_->control_mutex);
            impl_->start();
        }
    } catch (...) {
        impl_.reset();
    }
}

void BridgeFlightLogger::shutdown() noexcept {
    try {
        if (!impl_) {
            return;
        }
        event(BridgeFlightOperation::logger, 0);
        {
            std::scoped_lock lock(impl_->control_mutex);
            impl_->stop();
        }
        impl_.reset();
    } catch (...) {
        impl_.reset();
    }
}

void BridgeFlightLogger::follow_setting() noexcept {
    try {
        if (!impl_ || impl_->frequency.QuadPart <= 0) {
            return;
        }
        LARGE_INTEGER now{};
        if (!QueryPerformanceCounter(&now) ||
            now.QuadPart <
                impl_->next_poll_counter.load(std::memory_order_relaxed)) {
            return;
        }
        std::unique_lock lock(impl_->control_mutex, std::try_to_lock);
        if (!lock.owns_lock()) {
            return;
        }
        impl_->next_poll_counter.store(
            now.QuadPart + impl_->frequency.QuadPart / 4,
            std::memory_order_relaxed);
        const bool wanted = ini_file::read_int(impl_->config_path, kConfigurationSection, L"logging_enabled", 0) != 0;
        if (wanted == impl_->active.load(std::memory_order_acquire)) {
            impl_->pending_setting_seen = false;
            return;
        }
        if (!impl_->pending_setting_seen || impl_->pending_setting != wanted) {
            impl_->pending_setting = wanted;
            impl_->pending_setting_seen = true;
            return;
        }
        impl_->pending_setting_seen = false;
        if (wanted) {
            impl_->start();
        } else {
            impl_->stop();
        }
    } catch (...) {
    }
}

bool BridgeFlightLogger::enabled() const noexcept {
    return impl_ && impl_->active.load(std::memory_order_acquire);
}

bool BridgeFlightLogger::keeps_session_records() const noexcept {
    return impl_ != nullptr;
}

std::int64_t BridgeFlightLogger::microseconds_for_counter(
    std::int64_t counter) const noexcept {
    if (impl_ == nullptr || impl_->frequency.QuadPart <= 0) {
        return 0;
    }
    return static_cast<std::int64_t>(
        (counter - impl_->origin.QuadPart) * 1000000ll /
        impl_->frequency.QuadPart);
}

std::filesystem::path BridgeFlightLogger::log_path() const {
    if (!impl_) {
        return {};
    }
    std::scoped_lock lock(impl_->control_mutex);
    return impl_->path;
}

BridgeFlightToken BridgeFlightLogger::begin(
    BridgeFlightOperation operation,
    std::uint64_t a,
    std::uint64_t b,
    std::uint64_t c) noexcept {
    if (!impl_ || (!enabled() && !session_record(operation))) {
        return {};
    }
    LARGE_INTEGER now{};
    if (!QueryPerformanceCounter(&now)) {
        return {};
    }
    const BridgeFlightToken token{
        impl_->next_sequence.fetch_add(1, std::memory_order_relaxed),
        now.QuadPart};
    impl_->write(
        token.sequence, token.start_counter, 'B', operation, 0, 0, a, b, c);
    return token;
}

void BridgeFlightLogger::end(
    BridgeFlightToken token,
    BridgeFlightOperation operation,
    std::int64_t result,
    std::uint64_t a,
    std::uint64_t b,
    std::uint64_t c) noexcept {
    // A token from before the recorder was switched on has no sequence, so
    // an operation that straddles the switch leaves no half record.
    if (!impl_ || token.sequence == 0) {
        return;
    }
    LARGE_INTEGER now{};
    if (!QueryPerformanceCounter(&now)) {
        return;
    }
    const std::uint64_t duration = impl_->frequency.QuadPart > 0 &&
            now.QuadPart >= token.start_counter
        ? static_cast<std::uint64_t>(
              (now.QuadPart - token.start_counter) * 1000000ll /
              impl_->frequency.QuadPart)
        : 0;
    impl_->write(
        token.sequence, now.QuadPart, 'E', operation, result, duration, a, b, c);
}

void BridgeFlightLogger::event(
    BridgeFlightOperation operation,
    std::int64_t result,
    std::uint64_t a,
    std::uint64_t b,
    std::uint64_t c) noexcept {
    if (!impl_ || (!enabled() && !session_record(operation, result))) {
        return;
    }
    LARGE_INTEGER now{};
    if (!QueryPerformanceCounter(&now)) {
        return;
    }
    impl_->write(
        impl_->next_sequence.fetch_add(1, std::memory_order_relaxed),
        now.QuadPart,
        'I',
        operation,
        result,
        0,
        a,
        b,
        c);
}

BridgeFlightLogger& bridge_flight_logger() noexcept {
    static BridgeFlightLogger logger;
    return logger;
}

void initialize_bridge_flight_logger() noexcept {
    static std::once_flag once;
    try {
        std::call_once(once, [] {
            bridge_flight_logger().initialize(current_module_directory());
        });
    } catch (...) {
    }
}

} // namespace xrfg
