// The port's own sounds (Audio/PortSound.hpp).
#include "Audio/PortSound.hpp"
#include "Core/Paths.hpp"

#include <windows.h>
#include <xaudio2.h>

#define STB_VORBIS_HEADER_ONLY
#include <stb_vorbis.c>

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <memory>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>

extern void Log(const char*, ...);

namespace cvr::sound {
namespace {

// A sound as XAudio2 plays it: 16-bit interleaved PCM, and the region it loops (frames; length 0 = no loop).
struct Pcm {
    WAVEFORMATEX format{};
    std::vector<int16_t> samples;
    uint32_t loopBegin = 0, loopLength = 0;
    uint32_t Frames() const { return format.nChannels ? uint32_t(samples.size() / format.nChannels) : 0; }
};

std::vector<uint8_t> ReadFile(const std::filesystem::path& path) {
    std::ifstream f(path, std::ios::binary);
    if (!f) return {};
    return std::vector<uint8_t>((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
}

// RIFF / WAVE, 16-bit PCM only: the 'fmt ' chunk and the 'data' chunk, whatever else sits between them.
bool DecodeWav(const std::vector<uint8_t>& b, Pcm& pcm) {
    if (b.size() < 12 || std::memcmp(b.data(), "RIFF", 4) != 0 || std::memcmp(b.data() + 8, "WAVE", 4) != 0) return false;
    bool haveFormat = false;
    const uint8_t* data = nullptr;
    uint32_t dataSize = 0;
    for (size_t at = 12; at + 8 <= b.size();) {
        uint32_t size = 0;
        std::memcpy(&size, b.data() + at + 4, 4);
        const uint8_t* body = b.data() + at + 8;
        if (at + 8 + size > b.size()) break;
        if (std::memcmp(b.data() + at, "fmt ", 4) == 0 && size >= 16) {
            std::memcpy(&pcm.format, body, 16);
            haveFormat = pcm.format.wFormatTag == WAVE_FORMAT_PCM && pcm.format.wBitsPerSample == 16;
        } else if (std::memcmp(b.data() + at, "data", 4) == 0) {
            data = body;
            dataSize = size;
        }
        at += 8 + size + (size & 1);
    }
    if (!haveFormat || !data || !pcm.format.nChannels) return false;
    pcm.samples.resize(dataSize / 2);
    std::memcpy(pcm.samples.data(), data, pcm.samples.size() * 2);
    pcm.format.cbSize = 0;
    return !pcm.samples.empty();
}

bool DecodeOgg(const std::vector<uint8_t>& b, Pcm& pcm) {
    int channels = 0, rate = 0;
    short* out = nullptr;
    const int frames = stb_vorbis_decode_memory(b.data(), int(b.size()), &channels, &rate, &out);
    if (frames <= 0 || !out || channels <= 0) {
        std::free(out);
        return false;
    }
    pcm.samples.assign(out, out + size_t(frames) * channels);
    std::free(out);
    pcm.format.wFormatTag = WAVE_FORMAT_PCM;
    pcm.format.nChannels = WORD(channels);
    pcm.format.nSamplesPerSec = DWORD(rate);
    pcm.format.wBitsPerSample = 16;
    pcm.format.nBlockAlign = WORD(channels * 2);
    pcm.format.nAvgBytesPerSec = DWORD(rate) * pcm.format.nBlockAlign;
    pcm.format.cbSize = 0;
    return true;
}

// The loop from `from` to `to` (the file's end when not after `from`), its seam blended: the last `crossfade` before
// `to` is faded out while what comes just before `from` fades in under it (equal power), so that reaching `to` and
// going back to `from` is one continuous sound.
void MakeLoop(Pcm& pcm, float from, float to, float crossfade) {
    const uint32_t frames = pcm.Frames(), channels = pcm.format.nChannels, rate = pcm.format.nSamplesPerSec;
    const uint32_t begin = uint32_t(std::clamp(double(from) * rate, 0.0, double(frames)));
    uint32_t end = uint32_t(std::clamp(double(to) * rate, 0.0, double(frames)));
    if (end <= begin) end = frames;
    if (begin + 1 >= end) return;
    const uint32_t blend = std::min({uint32_t(std::max(0.0, double(crossfade) * rate)), begin, end - begin});
    for (uint32_t i = 0; i < blend; ++i) {
        const double t = (i + 0.5) / blend;
        const double in = std::sin(t * 1.5707963267948966), out = std::cos(t * 1.5707963267948966);
        int16_t* tail = &pcm.samples[size_t(end - blend + i) * channels];
        const int16_t* lead = &pcm.samples[size_t(begin - blend + i) * channels];
        for (uint32_t c = 0; c < channels; ++c)
            tail[c] = int16_t(std::clamp(std::lround(tail[c] * out + lead[c] * in), -32768L, 32767L));
    }
    pcm.loopBegin = begin;
    pcm.loopLength = end - begin;
}

// A relative path is under the plugin's Lua mods folder (PLUGIN\lua).
std::filesystem::path SoundPath(const char* file) {
    std::wstring w(MultiByteToWideChar(CP_UTF8, 0, file, -1, nullptr, 0), L'\0');
    if (!w.empty()) MultiByteToWideChar(CP_UTF8, 0, file, -1, w.data(), static_cast<int>(w.size()));
    if (!w.empty() && w.back() == L'\0') w.pop_back();
    std::filesystem::path p(w);
    if (p.is_absolute()) return p;
    return std::filesystem::path(cvr::paths::Dir(cvr::paths::Folder::Lua)) / p;
}

std::mutex g_mutex;
IXAudio2* g_audio = nullptr;
IXAudio2MasteringVoice* g_master = nullptr;
bool g_failed = false;
// Decoded once per file and loop: a loop's seam is part of its samples.
std::unordered_map<std::string, std::shared_ptr<Pcm>> g_cache;

// Its key in the cache: the file and its loop.
std::string Key(const char* file, float loopFrom, float loopTo, float crossfade) {
    char loop[96];
    std::snprintf(loop, sizeof(loop), "|%.4f|%.4f|%.4f", loopFrom, loopTo, crossfade);
    return std::string(file) + loop;
}

std::shared_ptr<Pcm> Load(const char* file, float loopFrom, float loopTo, float crossfade) {
    const std::string key = Key(file, loopFrom, loopTo, crossfade);
    auto& pcm = g_cache[key];
    if (pcm) return pcm;
    const std::filesystem::path path = SoundPath(file);
    const std::vector<uint8_t> bytes = ReadFile(path);
    auto decoded = std::make_shared<Pcm>();
    const std::wstring ext = path.extension().wstring();
    const bool ok = (_wcsicmp(ext.c_str(), L".ogg") == 0) ? DecodeOgg(bytes, *decoded) : DecodeWav(bytes, *decoded);
    if (!ok) {
        Log("[sound] %s: not a 16-bit WAV or an Ogg Vorbis to play\n", file);
        g_cache.erase(key);
        return nullptr;
    }
    if (loopFrom >= 0.0f) MakeLoop(*decoded, loopFrom, loopTo, crossfade);
    pcm = decoded;
    return pcm;
}

bool StartAudio() {
    if (g_audio) return true;
    if (g_failed) return false;
    // The calling thread (the game's script thread) has COM already; this only adds a reference, or answers that the
    // thread is in another apartment, which XAudio2 does not mind.
    CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    if (FAILED(XAudio2Create(&g_audio, 0, XAUDIO2_DEFAULT_PROCESSOR)) || FAILED(g_audio->CreateMasteringVoice(&g_master))) {
        if (g_audio) g_audio->Release();
        g_audio = nullptr;
        g_failed = true;
        Log("[sound] no XAudio2 device: the port's own sounds are off\n");
        return false;
    }
    return true;
}

// A voice playing one sound, with its volume and its filter on their way to where they are told to go.
struct Voice {
    IXAudio2SourceVoice* voice = nullptr;
    std::shared_ptr<Pcm> pcm;
    std::string key;
    float volume = 0.0f, volumeTarget = 0.0f, volumeRate = 0.0f;
    float lowpass = 0.0f, lowpassTarget = 0.0f, lowpassRate = 0.0f;
    bool ending = false;
};

std::vector<IXAudio2SourceVoice*> g_once;
std::unordered_map<std::string, Voice> g_channels;
std::vector<Voice> g_fading;  // what a channel held before its sound changed, on its way out
ULONGLONG g_lastTick = 0;

// The filter's cutoff as XAudio2 takes it (2 sin(pi f / rate), at most 1: none).
void ApplyFilter(Voice& v) {
    XAUDIO2_FILTER_PARAMETERS p{LowPassFilter, XAUDIO2_MAX_FILTER_FREQUENCY, 1.0f};
    if (v.lowpass > 0.0f && v.pcm)
        p.Frequency = std::min(XAUDIO2_MAX_FILTER_FREQUENCY,
                               float(2.0 * std::sin(3.141592653589793 * v.lowpass / v.pcm->format.nSamplesPerSec)));
    v.voice->SetFilterParameters(&p);
}

bool Start(Voice& v, const std::shared_ptr<Pcm>& pcm) {
    if (FAILED(g_audio->CreateSourceVoice(&v.voice, &pcm->format, XAUDIO2_VOICE_USEFILTER))) {
        v.voice = nullptr;
        return false;
    }
    v.pcm = pcm;
    XAUDIO2_BUFFER buf{};
    buf.AudioBytes = UINT32(pcm->samples.size() * 2);
    buf.pAudioData = reinterpret_cast<const BYTE*>(pcm->samples.data());
    buf.Flags = XAUDIO2_END_OF_STREAM;
    if (pcm->loopLength) {
        buf.LoopBegin = pcm->loopBegin;
        buf.LoopLength = pcm->loopLength;
        buf.LoopCount = XAUDIO2_LOOP_INFINITE;
    }
    v.voice->SetVolume(v.volume);
    ApplyFilter(v);
    if (FAILED(v.voice->SubmitSourceBuffer(&buf)) || FAILED(v.voice->Start(0))) {
        v.voice->DestroyVoice();
        v.voice = nullptr;
        return false;
    }
    return true;
}

// Toward `target` over `fade` seconds from where it is (at once when no fade).
void Aim(float& value, float& target, float& rate, float to, float fade) {
    target = to;
    if (fade <= 0.0f) {
        value = to;
        rate = 0.0f;
    } else {
        rate = std::fabs(to - value) / fade;
    }
}

bool Finished(IXAudio2SourceVoice* voice) {
    XAUDIO2_VOICE_STATE st{};
    voice->GetState(&st, XAUDIO2_VOICE_NOSAMPLESPLAYED);
    return st.BuffersQueued == 0;
}

void Step(float& value, float target, float rate, float dt) {
    if (value == target) return;
    const float step = rate * dt;
    value = (rate <= 0.0f || std::fabs(target - value) <= step) ? target : value + (target > value ? step : -step);
}

// Advances a voice; false when it is done with (let go).
bool Advance(Voice& v, float dt) {
    if (!v.voice) return false;
    if (Finished(v.voice) || (v.ending && v.volume <= 0.0f)) {
        v.voice->DestroyVoice();
        v.voice = nullptr;
        return false;
    }
    const float volume = v.volume, lowpass = v.lowpass;
    Step(v.volume, v.volumeTarget, v.volumeRate, dt);
    Step(v.lowpass, v.lowpassTarget, v.lowpassRate, dt);
    if (v.volume != volume) v.voice->SetVolume(v.volume);
    if (v.lowpass != lowpass) ApplyFilter(v);
    return true;
}

void FadeOut(Voice& v, float fade) {
    v.ending = true;
    Aim(v.volume, v.volumeTarget, v.volumeRate, 0.0f, fade);
    if (v.voice) v.voice->SetVolume(v.volume);
}

}  // namespace

bool PlayOnce(const char* file, float volume) {
    std::lock_guard lock(g_mutex);
    if (!file || !StartAudio()) return false;
    for (size_t i = 0; i < g_once.size();) {
        if (Finished(g_once[i])) {
            g_once[i]->DestroyVoice();
            g_once[i] = g_once.back();
            g_once.pop_back();
        } else {
            ++i;
        }
    }
    auto pcm = Load(file, -1.0f, 0.0f, 0.0f);
    IXAudio2SourceVoice* voice = nullptr;
    if (!pcm || g_once.size() >= 16 || FAILED(g_audio->CreateSourceVoice(&voice, &pcm->format))) return false;
    XAUDIO2_BUFFER buf{};
    buf.AudioBytes = UINT32(pcm->samples.size() * 2);
    buf.pAudioData = reinterpret_cast<const BYTE*>(pcm->samples.data());
    buf.Flags = XAUDIO2_END_OF_STREAM;
    voice->SetVolume(std::clamp(volume, 0.0f, 1.0f));
    if (FAILED(voice->SubmitSourceBuffer(&buf)) || FAILED(voice->Start(0))) {
        voice->DestroyVoice();
        return false;
    }
    g_once.push_back(voice);
    return true;
}

bool Channel(const char* name, const Sound& sound, float fade) {
    std::lock_guard lock(g_mutex);
    if (!name || !StartAudio()) return false;
    const float volume = std::clamp(sound.volume, 0.0f, 1.0f), lowpass = std::max(0.0f, sound.lowpass);
    auto it = g_channels.find(name);
    if (!sound.file || !*sound.file) {
        if (it != g_channels.end()) {
            g_fading.push_back(it->second);
            FadeOut(g_fading.back(), fade);
            g_channels.erase(it);
        }
        return true;
    }
    const std::string id = Key(sound.file, sound.loopFrom, sound.loopTo, sound.crossfade);
    if (it != g_channels.end() && it->second.key == id && it->second.voice) {
        Voice& v = it->second;
        v.ending = false;
        Aim(v.volume, v.volumeTarget, v.volumeRate, volume, fade);
        Aim(v.lowpass, v.lowpassTarget, v.lowpassRate, lowpass, fade);
        v.voice->SetVolume(v.volume);
        ApplyFilter(v);
        return true;
    }
    if (it != g_channels.end()) {
        g_fading.push_back(it->second);
        FadeOut(g_fading.back(), fade);
        g_channels.erase(it);
    }
    auto pcm = Load(sound.file, sound.loopFrom, sound.loopTo, sound.crossfade);
    if (!pcm) return false;
    Voice v;
    v.key = id;
    v.volume = fade > 0.0f ? 0.0f : volume;
    v.lowpass = lowpass;
    Aim(v.volume, v.volumeTarget, v.volumeRate, volume, fade);
    v.lowpassTarget = lowpass;
    if (!Start(v, pcm)) return false;
    g_channels[name] = v;
    return true;
}

void Tick() {
    std::lock_guard lock(g_mutex);
    const ULONGLONG now = GetTickCount64();
    const float dt = g_lastTick ? std::min(0.25f, (now - g_lastTick) / 1000.0f) : 0.0f;
    g_lastTick = now;
    if (!g_audio) return;
    for (auto it = g_channels.begin(); it != g_channels.end();)
        it = Advance(it->second, dt) ? std::next(it) : g_channels.erase(it);
    for (size_t i = 0; i < g_fading.size();) {
        if (Advance(g_fading[i], dt)) {
            ++i;
        } else {
            g_fading[i] = g_fading.back();
            g_fading.pop_back();
        }
    }
}

}  // namespace cvr::sound
