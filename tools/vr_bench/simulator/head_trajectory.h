#pragma once
// Recorded head trajectories, evaluated at the time a pose is asked for.
//
// head_pose_command.json moves the head in steps at the rate the command directory is polled (the preview rate,
// ~30 Hz), and the pose sweep runs on GetTickCount64 (~16 ms steps). A renderer under test needs the head to move
// the way a real head does: continuously, and sampled at the predicted display time of the frame being rendered,
// the same for every run. So the trajectory is a list of keys (seconds from its start, position in metres, yaw /
// pitch / roll in radians in the simulator's own convention, as g_headYaw etc.) interpolated at the XrTime the app
// passes to xrLocateViews / xrLocateSpace.
//
// Command file: head_trajectory_command.txt in the simulator's data folder, one key per line
//     t x y z yaw pitch roll
// optional first lines "delay <seconds>" (start that long after the file is read, default 0.25) and "id <number>"
// (echoed in the status). A file with no keys stops a running trajectory. When a trajectory ends the head stays at
// its last key. Status: head_trajectory_status.json {"id", "state": "armed|running|done|stopped", "startNs",
// "endNs", "keys", "samples"} -- startNs/endNs are XrTime (QPC in ns), the clock the app's frame timing uses.
#include <windows.h>
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <string>
#include <vector>

namespace trajectory {

struct Key {
    double t{};
    float x{}, y{}, z{}, yaw{}, pitch{}, roll{};
};

struct Pose {
    float x{}, y{}, z{}, yaw{}, pitch{}, roll{};
};

inline int64_t NowNs() {
    LARGE_INTEGER f{}, v{};
    QueryPerformanceFrequency(&f);
    QueryPerformanceCounter(&v);
    return (int64_t)((double)v.QuadPart * 1000000000.0 / (double)f.QuadPart);
}

class Player {
public:
    // Parses the command text. Returns false (and leaves the player untouched) when the text is malformed.
    bool Load(const std::string& text, int64_t nowNs) {
        std::vector<Key> keys;
        double delay = 0.25;
        long long id = 0;
        const char* p = text.c_str();
        while (*p) {
            const char* end = std::strchr(p, '\n');
            std::string line(p, end ? (size_t)(end - p) : std::strlen(p));
            p = end ? end + 1 : p + line.size();
            while (!line.empty() && (line.back() == '\r' || line.back() == ' ' || line.back() == '\t')) line.pop_back();
            if (line.empty() || line[0] == '#') continue;
            if (line.rfind("delay", 0) == 0) { delay = std::strtod(line.c_str() + 5, nullptr); continue; }
            if (line.rfind("id", 0) == 0) { id = std::strtoll(line.c_str() + 2, nullptr, 10); continue; }
            double v[7]{};
            const char* s = line.c_str();
            for (int i = 0; i < 7; ++i) {
                char* next = nullptr;
                v[i] = std::strtod(s, &next);
                if (next == s) return false;
                s = next;
            }
            Key k;
            k.t = v[0];
            k.x = (float)v[1]; k.y = (float)v[2]; k.z = (float)v[3];
            k.yaw = (float)v[4]; k.pitch = (float)v[5]; k.roll = (float)v[6];
            if (!keys.empty() && !(k.t > keys.back().t)) return false;   // strictly increasing times
            keys.push_back(k);
        }
        std::lock_guard<std::mutex> lock(mutex_);
        id_ = id;
        keys_ = std::move(keys);
        samples_ = 0;
        if (keys_.empty()) {
            state_ = active_ ? "stopped" : state_;
            active_ = false;
            WriteStatusLocked();
            return true;
        }
        if (delay < 0) delay = 0;
        startNs_ = nowNs + (int64_t)(delay * 1e9);
        endNs_ = startNs_ + (int64_t)(keys_.back().t * 1e9);
        active_ = true;
        state_ = "armed";
        WriteStatusLocked();
        return true;
    }

    // The pose at XrTime t, or false when no trajectory is loaded. Before the start the first key holds, after
    // the end the last one (the caller stores it as the head's pose, so the head does not jump back).
    // Once the end has been reached (the call that crosses it still answers, with the last key) the player goes
    // quiet: the caller keeps the last pose in its own head state and keyboard / pose commands own the head again.
    bool Evaluate(int64_t t, Pose& out) {
        std::lock_guard<std::mutex> lock(mutex_);
        if (keys_.empty() || !active_) return false;
        const double s = (double)(t - startNs_) * 1e-9;
        const Key* a = &keys_.front();
        const Key* b = a;
        if (s <= keys_.front().t) {
            a = b = &keys_.front();
        } else if (s >= keys_.back().t) {
            a = b = &keys_.back();
        } else {
            auto it = std::upper_bound(keys_.begin(), keys_.end(), s, [](double v, const Key& k) { return v < k.t; });
            b = &*it;
            a = &*(it - 1);
        }
        const double span = b->t - a->t;
        const float f = span > 0 ? (float)((s - a->t) / span) : 0.0f;
        auto lerp = [f](float u, float v) { return u + (v - u) * f; };
        out.x = lerp(a->x, b->x); out.y = lerp(a->y, b->y); out.z = lerp(a->z, b->z);
        out.yaw = lerp(a->yaw, b->yaw); out.pitch = lerp(a->pitch, b->pitch); out.roll = lerp(a->roll, b->roll);
        ++samples_;
        const char* next = s < 0 ? "armed" : (t >= endNs_ ? "done" : "running");
        if (active_ && std::strcmp(next, state_) != 0) {
            state_ = next;
            if (std::strcmp(next, "done") == 0) active_ = false;
            WriteStatusLocked();
        }
        return true;
    }

    void SetDataPath(const std::string& dir) {
        std::lock_guard<std::mutex> lock(mutex_);
        dir_ = dir;
    }

private:
    void WriteStatusLocked() {
        if (dir_.empty()) return;
        const std::string path = dir_ + "\\head_trajectory_status.json";
        const std::string tmp = path + ".tmp";
        FILE* f = nullptr;
        if (fopen_s(&f, tmp.c_str(), "w") != 0 || !f) return;
        std::fprintf(f, "{\"id\":%lld,\"state\":\"%s\",\"startNs\":%lld,\"endNs\":%lld,\"keys\":%zu,\"samples\":%llu}\n",
                     id_, state_, (long long)startNs_, (long long)endNs_, keys_.size(), (unsigned long long)samples_);
        std::fclose(f);
        MoveFileExA(tmp.c_str(), path.c_str(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH);
    }

    std::mutex mutex_;
    std::vector<Key> keys_;
    std::string dir_;
    int64_t startNs_{}, endNs_{};
    long long id_{};
    unsigned long long samples_{};
    bool active_{};
    const char* state_{"idle"};
};

inline Player& Get() {
    static Player player;
    return player;
}

// Reads and deletes the command file; true when a new trajectory (or a stop) was taken.
inline bool CheckCommand(const std::string& dataDir) {
    const std::string path = dataDir + "\\head_trajectory_command.txt";
    FILE* f = nullptr;
    if (fopen_s(&f, path.c_str(), "rb") != 0 || !f) return false;
    std::string text;
    char buf[65536];
    size_t n = 0;
    while ((n = std::fread(buf, 1, sizeof(buf), f)) > 0) text.append(buf, n);
    std::fclose(f);
    DeleteFileA(path.c_str());
    Get().SetDataPath(dataDir);
    return Get().Load(text, NowNs());
}

}  // namespace trajectory
