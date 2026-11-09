// Adapted from OFXR V557, LGPL-3.0; see docs/STEAMVR-DELIVERY.md.
#include "xrfg/steamvr_delivery.hpp"

#include "xrfg/bridge_flight_logger.hpp"

#include <openvr.h>

#include <windows.h>

#include <array>
#include <chrono>
#include <cstdio>
#include <mutex>
#include <string>

namespace xrfg {
namespace {

using PfnInitInternal2 =
    std::uint32_t(__cdecl*)(vr::EVRInitError*, vr::EVRApplicationType, const char*);
using PfnShutdownInternal = void(__cdecl*)();
using PfnGetGenericInterface = void*(__cdecl*)(const char*, vr::EVRInitError*);
using PfnGetInitToken = std::uint32_t(__cdecl*)();

// The runtime registers itself here rather than in the registry, and this is
// where SteamVR itself looks. Parsed by hand: a JSON dependency for one string
// would be the larger cost. Deliberately duplicated from the standalone probe,
// which must keep building with no layer dependency at all.
[[nodiscard]] std::string runtime_path_from_vrpath() {
    char local[MAX_PATH]{};
    if (GetEnvironmentVariableA("LOCALAPPDATA", local, MAX_PATH) == 0) {
        return {};
    }
    const std::string file = std::string(local) + "\\openvr\\openvrpaths.vrpath";
    FILE* handle = nullptr;
    if (fopen_s(&handle, file.c_str(), "rb") != 0 || handle == nullptr) {
        return {};
    }
    std::string text;
    char buffer[4096];
    for (;;) {
        const std::size_t read = fread(buffer, 1, sizeof(buffer), handle);
        if (read == 0) {
            break;
        }
        text.append(buffer, read);
    }
    fclose(handle);

    const std::size_t key = text.find("\"runtime\"");
    if (key == std::string::npos) {
        return {};
    }
    const std::size_t open = text.find('[', key);
    if (open == std::string::npos) {
        return {};
    }
    const std::size_t first = text.find('"', open);
    if (first == std::string::npos) {
        return {};
    }
    const std::size_t last = text.find('"', first + 1);
    if (last == std::string::npos) {
        return {};
    }
    std::string raw = text.substr(first + 1, last - first - 1);
    std::string path;
    for (std::size_t i = 0; i < raw.size(); ++i) {
        if (raw[i] == '\\' && i + 1 < raw.size() && raw[i + 1] == '\\') {
            path.push_back('\\');
            ++i;
        } else {
            path.push_back(raw[i]);
        }
    }
    return path;
}

// How the compositor interface was obtained, recorded so a capture says which
// path worked rather than only that one did.
enum class AttachRoute : std::uint64_t {
    none = 0,
    // The process already had an OpenVR context and we borrowed it. Costs
    // nothing and adds no state, so it is tried first.
    existing_token = 1,
    // A second, background connection of our own. Never Scene: this must not
    // become the application the compositor is serving, and must not start
    // SteamVR if it is not already running.
    background_init = 2,
};

constexpr std::int64_t kWindowNanoseconds = 1'000'000'000;
// Mispresented-above-predicted values are small; anything past this is counted
// with the top bucket and is a loss under any baseline.
constexpr std::uint32_t kExcessBuckets = 16;
// Past this the last window is too old to describe what is on screen now, so
// report nothing rather than something stale.
constexpr std::int64_t kStaleNanoseconds = 5'000'000'000;

// One OpenVR connection for the whole process, opened at most once and
// deliberately never shut down.
//
// It used to be per session, and tearing it down is what killed Assetto Corsa.
// That title recreates its OpenXR session during start-up - three times, on
// every runtime - and SteamVR ships its OpenXR runtime and its OpenVR client
// in one binary, vrclient_x64.dll. Opening a background OpenVR connection into
// that binary and then closing it leaves the next xrCreateSession through it
// dereferencing null, inside the runtime, with the layer merely forwarding the
// call. Captured with symbols: the fault is in vrclient_x64 and the only frame
// of ours below it is the downstream create.
//
// Every session create before our connection attaches succeeds and the first
// one after it dies, and when the attach happened three seconds later in one
// run the crash moved three seconds later with it. The title ran on V185,
// which is the last build with no connection at all.
//
// So the connection outlives every session. Not shutting it down is the point,
// not an oversight: VR_ShutdownInternal is what does the damage, and a
// background connection costs nothing to leave open for a process that is
// about to exit anyway.
//
// The cost is paid at the other lifecycle boundary. Outliving the session also
// means outliving the *instance*, and an application that destroys its
// XrInstance and builds another one then hangs in xrCreateInstance - in
// SteamVR, with the layer forwarding the call, the same vrclient_x64.dll and
// the same connection, one boundary over. Measured on R.E.A.L. VR, which
// probes with a throwaway instance on every launch: five runs armed at launch,
// five hangs on the second create, and no hang in any run where the connection
// had not been opened. That is what SteamVrDelivery::mark_established() is
// for - the connection still never closes, it just never opens under a session
// that was only ever a probe.
struct ProcessConnection {
    std::mutex mutex;
    bool attempted{};
    bool usable{};
    HMODULE module{};
    vr::IVRCompositor* compositor{};
    vr::IVRSystem* system{};
    AttachRoute route{AttachRoute::none};
    // Only the background_init route owns anything to close. An existing_token
    // context was borrowed from the application and is not ours to shut down.
    PfnShutdownInternal shutdown{};
    bool owns_context{};
};

[[nodiscard]] ProcessConnection& process_connection() noexcept {
    static ProcessConnection connection;
    return connection;
}

} // namespace

struct SteamVrDelivery::Impl {
    std::mutex mutex;
    bool steamvr{};
    bool attempted{};
    bool usable{};
    HMODULE module{};
    PfnShutdownInternal shutdown{};
    vr::IVRCompositor* compositor{};
    // Held separately because GetTimeSinceLastVsync is on IVRSystem, not the
    // compositor. Its absence is not fatal: the delivered rate still works.
    vr::IVRSystem* system{};
    // display_period()'s cache.
    std::optional<std::chrono::nanoseconds> display_period;
    std::chrono::steady_clock::time_point display_period_read{};
    AttachRoute route{AttachRoute::none};
    bool owns_context{};

    float delivered{};
    std::int64_t delivered_at_ns{-1};

    std::uint32_t last_frame_index{};
    bool reported_frames{};
    // settled_frames() keeps its own cursor, so it never takes a frame away
    // from last_presentation() or the other way round.
    std::uint32_t settled_last_index{};
    bool settled_reported{};

    // Distinct frames the headset received, counted on the compositor's own
    // vsync-aligned clock.
    //
    // The cumulative counters cannot give this and neither can the frame index
    // alone. m_nNumNumReprojectedFrames counts routine reprojection, not loss:
    // MSFS 2024 predicts two frames ahead and marks every frame reprojected
    // while the headset receives all 90, so presents minus dropped minus
    // reprojected reads zero through a healthy session. The index rate fixes
    // that but misses the other shape of loss - two of our submissions landing
    // in one scanout window, where the compositor still produces a frame per
    // scanout and the index keeps advancing at 90 while the headset shows 45.
    //
    // What separates the two is m_nNumMisPresented against how far ahead the
    // compositor says it is predicting. Landing on a vsync other than the one
    // first predicted is expected when it predicts ahead and a fault when it
    // does not:
    //
    //   Callisto healthy   mispres   0%   predicted 0   ->  90, matches fpsVR
    //   Callisto dropping  mispres  49%   predicted 0   ->  46, matches 45-40
    //   MSFS 2024          mispres 100%   predicted 2   ->  90, matches fpsVR
    //
    // With one exception. The frame after a repeat is mispresented *because of*
    // the repeat: it arrives a period late and so lands on a vsync other than
    // the one predicted. The index rate has already subtracted that scanout,
    // because a repeat holds its index instead of advancing it, so charging the
    // successor as well counts one lost frame twice. Where every frame is
    // repeated - MSFS holding 45 on a 90 Hz display - that takes the rate to
    // zero. The mispresent therefore only counts when the previous distinct
    // frame was scanned out once; past that the index rate covers it.
    std::uint32_t rate_first_index{};
    double rate_first_time{};
    std::uint32_t rate_last_index{};
    double rate_last_time{};
    std::uint32_t rate_seen{};
    // Frames eligible to be judged - those the index gap and the previous
    // frame's scanout count did not already exempt - and how far each one was
    // mispresented beyond the depth the compositor said it was predicting.
    std::uint32_t rate_counted{};
    std::array<std::uint32_t, kExcessBuckets> rate_excess_histogram{};
    // Scanouts the previous distinct frame occupied. One means this frame is as
    // early as it could be, so a mispresent here is a real displacement.
    std::uint32_t rate_previous_presents{1};
    bool rate_started{};

    void observe(const vr::Compositor_FrameTiming& timing) noexcept {
        if (!rate_started || timing.m_nFrameIndex < rate_last_index) {
            rate_first_index = rate_last_index = timing.m_nFrameIndex;
            rate_first_time = rate_last_time = timing.m_flSystemTimeInSeconds;
            rate_seen = rate_counted = 0;
            rate_excess_histogram.fill(0);
            rate_previous_presents = timing.m_nNumFramePresents;
            rate_started = true;
            return;
        }
        if (timing.m_nFrameIndex <= rate_last_index) {
            return;
        }
        // Vsyncs this frame is past the previous one. One means the compositor
        // composited on every vsync; more means it did not, and the scanouts it
        // skipped are already excluded by counting records rather than index
        // units. Charging the mispresent on top counts the same loss twice, and
        // where every frame skips - MSFS holding 45 on a 90 Hz display - it
        // takes the rate to zero.
        //
        // This is the general form of the presents >= 2 rule beside it, which
        // assumed a repeat always shows as one frame occupying two scanouts.
        // That is how the first MSFS capture read, but a repeat shows equally
        // as an index gap of two with a single present, and then the presents
        // rule exempts nothing at all.
        const std::uint32_t gap = timing.m_nFrameIndex - rate_last_index;
        rate_last_index = timing.m_nFrameIndex;
        rate_last_time = timing.m_flSystemTimeInSeconds;
        ++rate_seen;
        const std::uint32_t predicted =
            (timing.m_nReprojectionFlags & vr::VRCompositor_PredictionMask) >> 4;
        if (gap <= 1 && rate_previous_presents <= 1) {
            const std::uint32_t excess =
                timing.m_nNumMisPresented > predicted
                    ? timing.m_nNumMisPresented - predicted
                    : 0;
            ++rate_excess_histogram[excess < kExcessBuckets
                                        ? excess
                                        : kExcessBuckets - 1];
            ++rate_counted;
        }
        rate_previous_presents = timing.m_nNumFramePresents;
    }

    // Until the session proves it will be shown, no accessor may attach. See
    // SteamVrDelivery::mark_established().
    bool established{};

    void attach() noexcept {
        attempted = true;
        if (!steamvr) {
            return;
        }
        ProcessConnection& shared = process_connection();
        std::scoped_lock connection_lock(shared.mutex);
        if (shared.attempted) {
            // Second and later sessions borrow what the first one opened.
            compositor = shared.compositor;
            system = shared.system;
            route = shared.route;
            usable = shared.usable;
            return;
        }
        // Latched on success only, at the tail. A failure here is usually
        // SteamVR not being up yet rather than anything permanent, and
        // applications create sessions early - Assetto Corsa makes three
        // before it settles. Latching on the first attempt would give the
        // whole process no delivery interface for the rest of its life on a
        // transient miss, and that failure is silent: no delivery means
        // measured pacing never engages, so SteamVR quietly reverts to the
        // behaviour this path exists to replace with nothing in the overlay to
        // say so. The per-session latch below still stops one session
        // retrying on every frame.
        const std::string runtime = runtime_path_from_vrpath();
        if (runtime.empty()) {
            bridge_flight_logger().event(
                BridgeFlightOperation::steamvr_delivery_attach, -1);
            return;
        }
        const std::string dll = runtime + "\\bin\\win64\\openvr_api.dll";
        module = LoadLibraryA(dll.c_str());
        if (module == nullptr) {
            bridge_flight_logger().event(
                BridgeFlightOperation::steamvr_delivery_attach,
                -2,
                GetLastError());
            return;
        }
        const auto generic = reinterpret_cast<PfnGetGenericInterface>(
            reinterpret_cast<void*>(
                GetProcAddress(module, "VR_GetGenericInterface")));
        const auto token = reinterpret_cast<PfnGetInitToken>(
            reinterpret_cast<void*>(GetProcAddress(module, "VR_GetInitToken")));
        if (generic == nullptr) {
            bridge_flight_logger().event(
                BridgeFlightOperation::steamvr_delivery_attach, -3);
            return;
        }

        // Borrow an existing context before making one. A second connection
        // from a process that is already a scene application is the risk in
        // this whole path, so it is the fallback rather than the first move.
        vr::EVRInitError error = vr::VRInitError_None;
        if (token != nullptr && token() != 0) {
            compositor = static_cast<vr::IVRCompositor*>(
                generic(vr::IVRCompositor_Version, &error));
            if (compositor != nullptr) {
                route = AttachRoute::existing_token;
            }
        }
        if (compositor == nullptr) {
            const auto init = reinterpret_cast<PfnInitInternal2>(
                reinterpret_cast<void*>(
                    GetProcAddress(module, "VR_InitInternal2")));
            shutdown = reinterpret_cast<PfnShutdownInternal>(
                reinterpret_cast<void*>(
                    GetProcAddress(module, "VR_ShutdownInternal")));
            if (init == nullptr || shutdown == nullptr) {
                bridge_flight_logger().event(
                    BridgeFlightOperation::steamvr_delivery_attach, -4);
                return;
            }
            error = vr::VRInitError_None;
            init(&error, vr::VRApplication_Background, nullptr);
            if (error != vr::VRInitError_None) {
                bridge_flight_logger().event(
                    BridgeFlightOperation::steamvr_delivery_attach,
                    -5,
                    static_cast<std::uint64_t>(error));
                shutdown = nullptr;
                return;
            }
            owns_context = true;
            error = vr::VRInitError_None;
            compositor = static_cast<vr::IVRCompositor*>(
                generic(vr::IVRCompositor_Version, &error));
            route = AttachRoute::background_init;
        }
        if (compositor == nullptr) {
            bridge_flight_logger().event(
                BridgeFlightOperation::steamvr_delivery_attach,
                -6,
                static_cast<std::uint64_t>(error));
            return;
        }
        error = vr::VRInitError_None;
        system = static_cast<vr::IVRSystem*>(
            generic(vr::IVRSystem_Version, &error));
        usable = true;
        shared.module = module;
        shared.compositor = compositor;
        shared.system = system;
        shared.route = route;
        shared.usable = true;
        shared.attempted = true;
        shared.shutdown = shutdown;
        shared.owns_context = owns_context;
        bridge_flight_logger().event(
            BridgeFlightOperation::steamvr_delivery_attach,
            0,
            static_cast<std::uint64_t>(route),
            GetCurrentProcessId(),
            system != nullptr ? 1 : 0);
    }

    // The probe's accumulate(). A 64-frame window is not a detail: observe()
    // restarts whenever it sees an index below the last one, and the oldest
    // entry of an overlapping window always is one, so the rate only ever spans
    // a single window. Sixty-four frames is 0.7 s at 90 Hz and 63 usable
    // samples; the eight-frame lookback last_presentation() needs for its own
    // record is seven, and a lost share estimated from seven frames taken
    // whenever the overlay happened to ask is why this read 90 through drops
    // the probe resolved.
    bool current_process() const {
        vr::Compositor_CumulativeStats stats{};
        compositor->GetCumulativeStats(&stats, sizeof(stats));
        return stats.m_nPid == GetCurrentProcessId();
    }

    void poll_window() noexcept {
        std::array<vr::Compositor_FrameTiming, 64> window{};
        window[0].m_nSize = sizeof(vr::Compositor_FrameTiming);
        const std::uint32_t filled = compositor->GetFrameTimings(window.data(), 64);
        if (filled > window.size()) {
            return;
        }
        for (std::uint32_t i = 0; i + 1 < filled; ++i) {
            if (window[i].m_nNumFramePresents != 0) {
                observe(window[i]);
            }
        }
    }

    // Scanouts per second, less the share of them that carried nothing new.
    void close_window(std::int64_t now) noexcept {
        if (!rate_started || rate_seen == 0) {
            return;
        }
        // The probe's threshold, and it has to be short: the span is one
        // window, about 0.7 s, never longer. A one-second requirement never
        // fires at all, which leaves the overlay showing submitted frames.
        const double span = rate_last_time - rate_first_time;
        if (span < 0.05) {
            return;
        }
        // Counted, not derived from the index. m_nFrameIndex counts *vsyncs*
        // while the compositor writes one record per frame it actually
        // composited, so at 45 frames on a 90 Hz display consecutive records
        // are N, N+2, N+4 and the index gap is twice the frame count. Measured
        // on Hogwarts Legacy: 31 records spanning 62 index units over 0.689 s,
        // which the index arithmetic read as 90 while the headset received 45
        // and presenter_frame_presented fired 45.1 times a second.
        //
        // Callisto and MSFS both advanced the index by exactly one per record,
        // so last - first equalled the count there and this stayed hidden
        // through two rounds of validation against fpsVR.
        // Only excursions above the window's own baseline are losses.
        //
        // The rule was mispresented above the prediction depth, and it read a
        // constant offset as total loss. Where the grid sits inside the display
        // period decides which vsync a submission is predicted for, so a
        // schedule that lands one vsync off its prediction reports
        // mispresented 1 with predicted 0 on *every* frame while delivering all
        // of them - and the overlay showed zero exactly when delivery was
        // perfect. The gap and presents exemptions cannot catch it: at full rate
        // both are 1.
        //
        // The window's median excess is that offset, whatever it happens to be,
        // so a frame is lost only when it exceeds it. Against every shape on
        // record: Callisto healthy excess 0 throughout, median 0, none lost;
        // Callisto dropping 49% above a median of 0, all counted; MSFS healthy
        // mispresented 2 against predicted 2, excess 0; MSFS holding 45 exempt
        // on the index gap; and a full-rate grid one vsync off its prediction,
        // excess 1 throughout, median 1, none lost.
        std::uint32_t median_excess = 0;
        if (rate_counted != 0) {
            const std::uint32_t half = rate_counted / 2;
            std::uint32_t running = 0;
            for (std::uint32_t i = 0; i < kExcessBuckets; ++i) {
                running += rate_excess_histogram[i];
                if (running > half) {
                    median_excess = i;
                    break;
                }
            }
        }
        std::uint32_t lost = 0;
        for (std::uint32_t i = median_excess + 1; i < kExcessBuckets; ++i) {
            lost += rate_excess_histogram[i];
        }
        const double frames = static_cast<double>(rate_seen) / span;
        const double lost_share = rate_counted == 0
            ? 0.0
            : static_cast<double>(lost) / static_cast<double>(rate_counted);
        delivered = static_cast<float>(frames * (1.0 - lost_share));
        delivered_at_ns = now;
        bridge_flight_logger().event(
            BridgeFlightOperation::steamvr_delivery,
            0,
            static_cast<std::uint64_t>(delivered * 1000.0F),
            static_cast<std::uint64_t>(lost_share * 1000.0),
            rate_seen);
        rate_started = false;
    }
};

SteamVrDelivery::SteamVrDelivery(bool steamvr_runtime) noexcept
    : impl_(nullptr) {
    try {
        impl_ = std::make_unique<Impl>();
        impl_->steamvr = steamvr_runtime;
    } catch (...) {
    }
}

SteamVrDelivery::~SteamVrDelivery() {
    if (!impl_) {
        return;
    }
    // The connection is process-wide and is never torn down here; see
    // ProcessConnection. Shutting it down between sessions is what crashed
    // SteamVR's own runtime on the next xrCreateSession, and unloading the
    // module would do the same by another route. Only the per-session
    // accounting dies with this object.
}

void SteamVrDelivery::release_process_connection() noexcept {
    ProcessConnection& shared = process_connection();
    std::scoped_lock lock(shared.mutex);
    if (!shared.attempted) {
        return;
    }
    // Every session is destroyed before the instance that owns it, so no Impl
    // still holds these pointers by the time this runs. Both captures show the
    // order directly: session_destroy completes, then instance_destroy begins.
    const AttachRoute released = shared.route;
    const bool owned = shared.owns_context;
    if (owned && shared.shutdown != nullptr) {
        shared.shutdown();
    }
    shared.attempted = false;
    shared.usable = false;
    shared.compositor = nullptr;
    shared.system = nullptr;
    shared.route = AttachRoute::none;
    shared.shutdown = nullptr;
    shared.owns_context = false;
    // The module handle is kept: openvr_api.dll stays loaded and a reopen
    // resolves its exports again rather than reloading it.
    //
    // result=1 is a release, against 0 for an attach and negatives for the
    // ways an attach can fail. a is the route that was let go, c whether it
    // was ours to close.
    bridge_flight_logger().event(
        BridgeFlightOperation::steamvr_delivery_attach,
        1,
        static_cast<std::uint64_t>(released),
        GetCurrentProcessId(),
        owned ? 1 : 0);
}

void SteamVrDelivery::mark_established() noexcept {
    try {
        if (!impl_) {
            return;
        }
        std::scoped_lock lock(impl_->mutex);
        impl_->established = true;
    } catch (...) {
    }
}

std::optional<float> SteamVrDelivery::delivered_fps(std::int64_t now) noexcept {
    try {
        if (!impl_) {
            return std::nullopt;
        }
        std::scoped_lock lock(impl_->mutex);
        if (!impl_->established) {
            return std::nullopt;
        }
        if (!impl_->attempted) {
            impl_->attach();
        }
        if (!impl_->usable || !impl_->current_process()) {
            return std::nullopt;
        }
        impl_->poll_window();
        impl_->close_window(now);
        if (impl_->delivered_at_ns < 0 ||
            now - impl_->delivered_at_ns > kStaleNanoseconds) {
            return std::nullopt;
        }
        return impl_->delivered;
    } catch (...) {
        return std::nullopt;
    }
}

std::optional<SteamVrDelivery::VsyncAnchor>
SteamVrDelivery::vsync_anchor() noexcept {
    try {
        if (!impl_) {
            return std::nullopt;
        }
        std::scoped_lock lock(impl_->mutex);
        if (!impl_->established) {
            return std::nullopt;
        }
        if (!impl_->attempted) {
            impl_->attach();
        }
        if (!impl_->usable || impl_->system == nullptr) {
            return std::nullopt;
        }
        float seconds = 0.0F;
        std::uint64_t counter = 0;
        const auto entered = std::chrono::steady_clock::now();
        // Read the clock immediately after, not before: the value is relative
        // to the moment of the call, and anything between the two becomes
        // phase error in whatever uses it.
        const bool ok = impl_->system->GetTimeSinceLastVsync(&seconds, &counter);
        const auto now = std::chrono::steady_clock::now();
        // Documented to return false with zeroes when the runtime has no vsync
        // times. A negative or absurd value would be worse than none, since a
        // caller would treat it as a real anchor.
        if (!ok || !(seconds >= 0.0F) || seconds > 1.0F) {
            return std::nullopt;
        }
        VsyncAnchor anchor{};
        anchor.at = now - std::chrono::nanoseconds(
            static_cast<std::int64_t>(static_cast<double>(seconds) * 1e9));
        anchor.frame_counter = counter;
        anchor.cost = std::chrono::duration_cast<std::chrono::microseconds>(
            now - entered);
        return anchor;
    } catch (...) {
        return std::nullopt;
    }
}

std::optional<std::chrono::nanoseconds>
SteamVrDelivery::display_period() noexcept {
    try {
        if (!impl_) {
            return std::nullopt;
        }
        std::scoped_lock lock(impl_->mutex);
        if (!impl_->established) {
            return std::nullopt;
        }
        if (!impl_->attempted) {
            impl_->attach();
        }
        if (!impl_->usable || impl_->system == nullptr) {
            return std::nullopt;
        }
        const auto now = std::chrono::steady_clock::now();
        if (impl_->display_period_read != std::chrono::steady_clock::time_point{} &&
            now - impl_->display_period_read < std::chrono::seconds(2)) {
            return impl_->display_period;
        }
        impl_->display_period_read = now;
        vr::ETrackedPropertyError error = vr::TrackedProp_Success;
        const float hertz = impl_->system->GetFloatTrackedDeviceProperty(
            vr::k_unTrackedDeviceIndex_Hmd, vr::Prop_DisplayFrequency_Float, &error);
        if (error != vr::TrackedProp_Success || !(hertz >= 20.0F) || hertz > 500.0F) {
            impl_->display_period.reset();
            return std::nullopt;
        }
        impl_->display_period = std::chrono::nanoseconds(
            static_cast<std::int64_t>(1e9 / static_cast<double>(hertz)));
        return impl_->display_period;
    } catch (...) {
        return std::nullopt;
    }
}

std::optional<std::chrono::nanoseconds>
SteamVrDelivery::frame_time_remaining() noexcept {
    try {
        if (!impl_) {
            return std::nullopt;
        }
        std::scoped_lock lock(impl_->mutex);
        if (!impl_->established) {
            return std::nullopt;
        }
        if (!impl_->attempted) {
            impl_->attach();
        }
        if (!impl_->usable || impl_->compositor == nullptr) {
            return std::nullopt;
        }
        const float seconds = impl_->compositor->GetFrameTimeRemaining();
        if (!(seconds > -1.0F) || seconds > 1.0F) {
            return std::nullopt;
        }
        return std::chrono::nanoseconds(
            static_cast<std::int64_t>(static_cast<double>(seconds) * 1e9));
    } catch (...) {
        return std::nullopt;
    }
}

std::vector<SteamVrDelivery::FramePresentation>
SteamVrDelivery::settled_frames() noexcept {
    std::vector<FramePresentation> output;
    try {
        if (!impl_) {
            return output;
        }
        std::scoped_lock lock(impl_->mutex);
        if (!impl_->established) {
            return output;
        }
        if (!impl_->attempted) {
            impl_->attach();
        }
        if (!impl_->usable || !impl_->current_process()) {
            return output;
        }
        constexpr std::uint32_t kLookback = 64;
        std::array<vr::Compositor_FrameTiming, kLookback> timings{};
        timings[0].m_nSize = sizeof(vr::Compositor_FrameTiming);
        const std::uint32_t filled =
            impl_->compositor->GetFrameTimings(timings.data(), kLookback);
        if (filled == 0 || filled > kLookback) {
            return output;
        }
        // Ascending, oldest to newest. A frame is settled once a newer one has
        // been presented: the newest ones report zero presents only because
        // nothing has happened to them yet. Everything up to and including the
        // newest presented frame is final, and a zero there is a frame the
        // compositor had and never showed.
        std::uint32_t newest_presented = filled;
        for (std::uint32_t offset = filled; offset-- > 0;) {
            if (timings[offset].m_nNumFramePresents != 0) {
                newest_presented = offset;
                break;
            }
        }
        if (newest_presented == filled) {
            return output;
        }
        for (std::uint32_t offset = 0; offset <= newest_presented; ++offset) {
            const auto& timing = timings[offset];
            if (impl_->settled_reported &&
                timing.m_nFrameIndex <= impl_->settled_last_index) {
                continue;
            }
            FramePresentation frame{};
            frame.frame_index = timing.m_nFrameIndex;
            frame.mispresented = timing.m_nNumMisPresented;
            frame.presents = timing.m_nNumFramePresents;
            frame.dropped = timing.m_nNumDroppedFrames;
            frame.skipped = impl_->settled_reported
                ? timing.m_nFrameIndex - impl_->settled_last_index - 1
                : 0;
            frame.reprojection_flags = timing.m_nReprojectionFlags;
            frame.reprojection_flags_window = timing.m_nReprojectionFlags;
            frame.total_render_gpu_us = static_cast<uint32_t>(std::max(0.0f,timing.m_flTotalRenderGpuMs)*1000.0f);
            frame.compositor_render_gpu_us = static_cast<uint32_t>(std::max(0.0f,timing.m_flCompositorRenderGpuMs)*1000.0f);
            frame.ready_vsyncs = timing.m_nNumVSyncsReadyForUse;
            frame.vsyncs_to_first_view = timing.m_nNumVSyncsToFirstView;
            frame.system_time_seconds = timing.m_flSystemTimeInSeconds;
            frame.wait_get_poses_called_ms = timing.m_flWaitGetPosesCalledMs;
            frame.new_poses_ready_ms = timing.m_flNewPosesReadyMs;
            frame.new_frame_ready_ms = timing.m_flNewFrameReadyMs;
            frame.compositor_update_start_ms =
                timing.m_flCompositorUpdateStartMs;
            frame.compositor_update_end_ms = timing.m_flCompositorUpdateEndMs;
            frame.compositor_render_start_ms =
                timing.m_flCompositorRenderStartMs;
            frame.client_frame_interval_ms = timing.m_flClientFrameIntervalMs;
            frame.compositor_idle_cpu_ms = timing.m_flCompositorIdleCpuMs;
            output.push_back(frame);
            impl_->settled_last_index = timing.m_nFrameIndex;
            impl_->settled_reported = true;
        }
    } catch (...) {
    }
    return output;
}

std::optional<SteamVrDelivery::FramePresentation>
SteamVrDelivery::last_presentation() noexcept {
    try {
        if (!impl_) {
            return std::nullopt;
        }
        std::scoped_lock lock(impl_->mutex);
        if (!impl_->established) {
            return std::nullopt;
        }
        if (!impl_->attempted) {
            impl_->attach();
        }
        if (!impl_->usable || !impl_->current_process()) {
            return std::nullopt;
        }
        // Frames ago 0 is the one still in flight: nothing has happened to it
        // yet, so it reports zero presents and zero mispresents whatever the
        // compositor ends up doing. Reading it made this record uniformly zero
        // across 6161 submissions while frames were demonstrably being lost.
        // Walk back to the newest frame that has actually been presented.
        constexpr std::uint32_t kLookback = 8;
        std::array<vr::Compositor_FrameTiming, kLookback> timings{};
        timings[0].m_nSize = sizeof(vr::Compositor_FrameTiming);
        const std::uint32_t filled =
            impl_->compositor->GetFrameTimings(timings.data(), kLookback);
        if (filled == 0 || filled > kLookback) {
            return std::nullopt;
        }
        // Ascending, oldest to newest.
        // Every frame in the window, not just the one reported below. The
        // caller looks once per submission and the window holds more than one
        // frame, so a reason raised on a frame it does not land on would
        // otherwise never be seen.
        std::uint32_t flags_window = 0;
        for (std::uint32_t i = 0; i < filled; ++i) {
            if (timings[i].m_nNumFramePresents != 0) {
                flags_window |= timings[i].m_nReprojectionFlags;
            }
        }

        for (std::uint32_t offset = filled; offset-- > 0;) {
            const auto& timing = timings[offset];
            if (timing.m_nNumFramePresents == 0) {
                continue;
            }
            if (impl_->reported_frames &&
                timing.m_nFrameIndex <= impl_->last_frame_index) {
                return std::nullopt; // Nothing has settled since the last call.
            }
            FramePresentation presentation{};
            presentation.frame_index = timing.m_nFrameIndex;
            presentation.mispresented = timing.m_nNumMisPresented;
            presentation.presents = timing.m_nNumFramePresents;
            presentation.dropped = timing.m_nNumDroppedFrames;
            presentation.skipped = impl_->reported_frames
                ? timing.m_nFrameIndex - impl_->last_frame_index - 1
                : 0;
            presentation.reprojection_flags = timing.m_nReprojectionFlags;
            presentation.reprojection_flags_window = flags_window;
            presentation.total_render_gpu_us = static_cast<std::uint32_t>(
                timing.m_flTotalRenderGpuMs * 1000.0F);
            presentation.compositor_render_gpu_us = static_cast<std::uint32_t>(
                timing.m_flCompositorRenderGpuMs * 1000.0F);
            presentation.ready_vsyncs = timing.m_nNumVSyncsReadyForUse;
            presentation.vsyncs_to_first_view = timing.m_nNumVSyncsToFirstView;
            presentation.system_time_seconds = timing.m_flSystemTimeInSeconds;
            presentation.wait_get_poses_called_ms =
                timing.m_flWaitGetPosesCalledMs;
            presentation.new_poses_ready_ms = timing.m_flNewPosesReadyMs;
            presentation.new_frame_ready_ms = timing.m_flNewFrameReadyMs;
            presentation.compositor_update_start_ms =
                timing.m_flCompositorUpdateStartMs;
            presentation.compositor_update_end_ms =
                timing.m_flCompositorUpdateEndMs;
            presentation.compositor_render_start_ms =
                timing.m_flCompositorRenderStartMs;
            presentation.client_frame_interval_ms =
                timing.m_flClientFrameIntervalMs;
            presentation.compositor_idle_cpu_ms = timing.m_flCompositorIdleCpuMs;
            impl_->last_frame_index = timing.m_nFrameIndex;
            impl_->reported_frames = true;
            return presentation;
        }
        return std::nullopt;
    } catch (...) {
        return std::nullopt;
    }
}

} // namespace xrfg
