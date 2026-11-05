// What the speakers play, as levels over time: the default render endpoint captured in WASAPI loopback, one line
// per window -- GetTickCount64() at its end, RMS and peak in dBFS over all channels, then each audio session that
// is sounding as pid:peak_db (the endpoint's sessions and their own peak meters: which process it is). The tick is
// the same clock KUSER_SHARED_DATA keeps (0x7FFE0320 TickCountQuad * TickCountMultiplier >> 24), so a debugger's log
// of the game's audio events stamped with it lines up with these windows.
//
//   loopback_meter.exe OUT.txt [seconds=600] [window_ms=100] [stop_file]
//
// Ends after `seconds`, or as soon as stop_file exists.
#include <windows.h>
#include <audioclient.h>
#include <audiopolicy.h>
#include <endpointvolume.h>
#include <mmdeviceapi.h>
#include <mmreg.h>
#include <ks.h>
#include <ksmedia.h>

#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>

#pragma comment(lib, "ole32.lib")

namespace {

template <class T> void Release(T*& p) {
    if (p) p->Release();
    p = nullptr;
}

double Db(double linear) { return linear > 1e-9 ? 20.0 * std::log10(linear) : -180.0; }

// Each session of the endpoint that is sounding now, appended as " pid:peak_db".
void Sessions(IAudioSessionManager2* manager, FILE* out) {
    IAudioSessionEnumerator* sessions = nullptr;
    if (!manager || FAILED(manager->GetSessionEnumerator(&sessions))) return;
    int n = 0;
    sessions->GetCount(&n);
    for (int i = 0; i < n; ++i) {
        IAudioSessionControl* control = nullptr;
        IAudioSessionControl2* control2 = nullptr;
        IAudioMeterInformation* meter = nullptr;
        if (SUCCEEDED(sessions->GetSession(i, &control)) &&
            SUCCEEDED(control->QueryInterface(__uuidof(IAudioSessionControl2), reinterpret_cast<void**>(&control2))) &&
            SUCCEEDED(control->QueryInterface(__uuidof(IAudioMeterInformation), reinterpret_cast<void**>(&meter)))) {
            float peak = 0.0f;
            DWORD pid = 0;
            meter->GetPeakValue(&peak);
            control2->GetProcessId(&pid);
            if (peak > 1e-6f) std::fprintf(out, " %lu:%.1f", static_cast<unsigned long>(pid), Db(peak));
        }
        Release(meter);
        Release(control2);
        Release(control);
    }
    Release(sessions);
}

bool IsFloat(const WAVEFORMATEX* f) {
    if (f->wFormatTag == WAVE_FORMAT_IEEE_FLOAT) return true;
    if (f->wFormatTag == WAVE_FORMAT_EXTENSIBLE)
        return reinterpret_cast<const WAVEFORMATEXTENSIBLE*>(f)->SubFormat == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
    return false;
}

}  // namespace

int main(int argc, char** argv) {
    if (argc < 2) {
        std::fprintf(stderr, "usage: loopback_meter OUT.txt [seconds] [window_ms] [stop_file]\n");
        return 2;
    }
    const double seconds = argc > 2 ? std::atof(argv[2]) : 600.0;
    const int windowMs = argc > 3 ? std::atoi(argv[3]) : 100;
    const char* stopFile = argc > 4 ? argv[4] : nullptr;
    FILE* out = std::fopen(argv[1], "w");
    if (!out) return 3;

    CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    IMMDeviceEnumerator* devices = nullptr;
    IMMDevice* device = nullptr;
    IAudioClient* client = nullptr;
    IAudioCaptureClient* capture = nullptr;
    WAVEFORMATEX* format = nullptr;
    HRESULT hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL, __uuidof(IMMDeviceEnumerator),
                                  reinterpret_cast<void**>(&devices));
    if (SUCCEEDED(hr)) hr = devices->GetDefaultAudioEndpoint(eRender, eConsole, &device);
    if (SUCCEEDED(hr)) hr = device->Activate(__uuidof(IAudioClient), CLSCTX_ALL, nullptr, reinterpret_cast<void**>(&client));
    if (SUCCEEDED(hr)) hr = client->GetMixFormat(&format);
    if (SUCCEEDED(hr)) hr = client->Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 10000000, 0, format, nullptr);
    if (SUCCEEDED(hr)) hr = client->GetService(__uuidof(IAudioCaptureClient), reinterpret_cast<void**>(&capture));
    if (SUCCEEDED(hr)) hr = client->Start();
    if (FAILED(hr)) {
        std::fprintf(stderr, "WASAPI loopback failed: 0x%08lX\n", static_cast<unsigned long>(hr));
        return 4;
    }
    IAudioSessionManager2* manager = nullptr;
    device->Activate(__uuidof(IAudioSessionManager2), CLSCTX_ALL, nullptr, reinterpret_cast<void**>(&manager));
    const bool isFloat = IsFloat(format);
    const int channels = format->nChannels;
    const int bits = format->wBitsPerSample;
    std::fprintf(out, "# %u Hz, %d channels, %d-bit %s; tick_ms rms_db peak_db [pid:session_peak_db ...]\n",
                 static_cast<unsigned>(format->nSamplesPerSec), channels, bits, isFloat ? "float" : "pcm");
    std::fflush(out);

    const ULONGLONG start = GetTickCount64();
    ULONGLONG windowEnd = start + windowMs;
    double sumSquares = 0.0, peak = 0.0;
    unsigned long long count = 0;
    while (GetTickCount64() - start < static_cast<ULONGLONG>(seconds * 1000.0)) {
        if (stopFile && GetFileAttributesA(stopFile) != INVALID_FILE_ATTRIBUTES) break;
        UINT32 packet = 0;
        while (SUCCEEDED(capture->GetNextPacketSize(&packet)) && packet > 0) {
            BYTE* data = nullptr;
            UINT32 frames = 0;
            DWORD flags = 0;
            if (FAILED(capture->GetBuffer(&data, &frames, &flags, nullptr, nullptr))) break;
            const unsigned long long samples = static_cast<unsigned long long>(frames) * channels;
            if (!(flags & AUDCLNT_BUFFERFLAGS_SILENT)) {
                for (unsigned long long i = 0; i < samples; ++i) {
                    double v = 0.0;
                    if (isFloat && bits == 32) v = reinterpret_cast<const float*>(data)[i];
                    else if (bits == 16) v = reinterpret_cast<const short*>(data)[i] / 32768.0;
                    else if (bits == 32) v = reinterpret_cast<const int*>(data)[i] / 2147483648.0;
                    sumSquares += v * v;
                    peak = (std::fabs(v) > peak) ? std::fabs(v) : peak;
                }
            }
            count += samples;
            capture->ReleaseBuffer(frames);
        }
        const ULONGLONG now = GetTickCount64();
        if (now >= windowEnd) {
            const double rms = count ? std::sqrt(sumSquares / static_cast<double>(count)) : 0.0;
            std::fprintf(out, "%llu %.1f %.1f", static_cast<unsigned long long>(now), Db(rms), Db(peak));
            Sessions(manager, out);
            std::fputc('\n', out);
            std::fflush(out);
            sumSquares = 0.0;
            peak = 0.0;
            count = 0;
            windowEnd = now + windowMs;
        }
        Sleep(10);
    }
    client->Stop();
    CoTaskMemFree(format);
    Release(manager);
    Release(capture);
    Release(client);
    Release(device);
    Release(devices);
    CoUninitialize();
    std::fclose(out);
    return 0;
}
