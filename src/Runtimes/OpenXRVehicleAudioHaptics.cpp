#include "Runtimes/OpenXRVehicleAudioHaptics.hpp"

#include "Core/VrCoreShared.hpp"
#include "Runtimes/Haptics.hpp"

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdint>
#include <thread>

#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <audioclient.h>
#include <ks.h>
#include <ksmedia.h>
#include <mmdeviceapi.h>
#include <windows.h>

namespace cvr::haptics {
namespace {
constexpr double kPi = 3.14159265358979323846;
std::atomic<bool> g_running{false};
std::thread g_thread;

bool IsFloatFormat(const WAVEFORMATEX* format) {
    if (format->wFormatTag == WAVE_FORMAT_IEEE_FLOAT) return true;
    if (format->wFormatTag != WAVE_FORMAT_EXTENSIBLE || format->cbSize < 22) return false;
    return reinterpret_cast<const WAVEFORMATEXTENSIBLE*>(format)->SubFormat ==
           KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
}

bool IsPcmFormat(const WAVEFORMATEX* format) {
    if (format->wFormatTag == WAVE_FORMAT_PCM) return true;
    if (format->wFormatTag != WAVE_FORMAT_EXTENSIBLE || format->cbSize < 22) return false;
    return reinterpret_cast<const WAVEFORMATEXTENSIBLE*>(format)->SubFormat ==
           KSDATAFORMAT_SUBTYPE_PCM;
}

float ReadSample(const BYTE* frame, uint16_t channel, const WAVEFORMATEX* format) {
    const uint16_t bytesPerSample = static_cast<uint16_t>(format->wBitsPerSample / 8);
    const BYTE* sample = frame + static_cast<size_t>(channel) * bytesPerSample;
    if (IsFloatFormat(format) && format->wBitsPerSample == 32)
        return std::clamp(*reinterpret_cast<const float*>(sample), -1.0f, 1.0f);
    if (!IsPcmFormat(format)) return 0.0f;
    switch (format->wBitsPerSample) {
    case 8:
        return (static_cast<int>(*sample) - 128) / 128.0f;
    case 16:
        return *reinterpret_cast<const int16_t*>(sample) / 32768.0f;
    case 24: {
        int32_t value = static_cast<int32_t>(sample[0]) |
                        (static_cast<int32_t>(sample[1]) << 8) |
                        (static_cast<int32_t>(sample[2]) << 16);
        if (value & 0x800000) value |= ~0xFFFFFF;
        return value / 8388608.0f;
    }
    case 32:
        return static_cast<float>(*reinterpret_cast<const int32_t*>(sample) / 2147483648.0);
    default:
        return 0.0f;
    }
}

void CaptureThread() {
    const HRESULT comResult = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    IMMDeviceEnumerator* enumerator = nullptr;
    IMMDevice* device = nullptr;
    IAudioClient* audioClient = nullptr;
    IAudioCaptureClient* captureClient = nullptr;
    WAVEFORMATEX* format = nullptr;

    HRESULT result = CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL,
                                      IID_PPV_ARGS(&enumerator));
    if (SUCCEEDED(result)) result = enumerator->GetDefaultAudioEndpoint(eRender, eMultimedia, &device);
    if (SUCCEEDED(result))
        result = device->Activate(__uuidof(IAudioClient), CLSCTX_ALL, nullptr,
                                  reinterpret_cast<void**>(&audioClient));
    if (SUCCEEDED(result)) result = audioClient->GetMixFormat(&format);
    if (SUCCEEDED(result) && ((!IsFloatFormat(format) && !IsPcmFormat(format)) ||
        format->nSamplesPerSec == 0 || format->nChannels == 0 ||
        format->nBlockAlign < format->nChannels * (format->wBitsPerSample / 8)))
        result = AUDCLNT_E_UNSUPPORTED_FORMAT;
    if (SUCCEEDED(result))
        result = audioClient->Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
                                         0, 0, format, nullptr);
    if (SUCCEEDED(result)) result = audioClient->GetService(IID_PPV_ARGS(&captureClient));
    if (SUCCEEDED(result)) result = audioClient->Start();

    if (FAILED(result)) {
        Log("OpenXRManager[VehicleHaptics]: default-output loopback unavailable result=0x%08lX.\n",
            static_cast<unsigned long>(result));
    } else {
        Log("OpenXRManager[VehicleHaptics]: audio-derived engine/gear capture active endpoint=%uHz channels=%u.\n",
            format->nSamplesPerSec, format->nChannels);

        const double rate = static_cast<double>(format->nSamplesPerSec);
        const double hpAlpha = std::exp(-2.0 * kPi * 28.0 / rate);
        const double lpAlpha = 1.0 - std::exp(-2.0 * kPi * 320.0 / rate);
        std::array<double, 2> previousInput{};
        std::array<double, 2> highPassed{};
        std::array<double, 2> lowPassed{};
        std::array<double, 2> smoothed{};
        double fastEnvelope = 0.0;
        double slowEnvelope = 0.0;
        bool wasDriving = false;
        bool envelopeConfirmed = false;

        while (g_running.load(std::memory_order_acquire)) {
            UINT32 packetFrames = 0;
            result = captureClient->GetNextPacketSize(&packetFrames);
            if (FAILED(result)) break;
            if (packetFrames == 0) {
                Sleep(3);
                continue;
            }

            while (packetFrames > 0 && g_running.load(std::memory_order_acquire)) {
                BYTE* data = nullptr;
                UINT32 frames = 0;
                DWORD flags = 0;
                result = captureClient->GetBuffer(&data, &frames, &flags, nullptr, nullptr);
                if (FAILED(result)) break;
                // Drain without filtering while on foot or muted. Never charge the render
                // thread for audio work, and do not carry an old engine envelope into a car.
                if (!g_isDriving.load(std::memory_order_relaxed) || VehicleGain() <= 0) {
                    captureClient->ReleaseBuffer(frames);
                    previousInput = {}; highPassed = {}; lowPassed = {}; smoothed = {};
                    fastEnvelope = slowEnvelope = 0;
                    if (wasDriving) SetVehicleHaptics(0, 0);
                    wasDriving = false;
                    result = captureClient->GetNextPacketSize(&packetFrames);
                    if (FAILED(result)) break;
                    continue;
                }
                const bool silent = (flags & AUDCLNT_BUFFERFLAGS_SILENT) != 0;
                std::array<double, 2> energy{};

                for (UINT32 frameIndex = 0; frameIndex < frames; ++frameIndex) {
                    std::array<double, 2> input{};
                    if (!silent) {
                        const BYTE* frame = data + static_cast<size_t>(frameIndex) * format->nBlockAlign;
                        input[0] = ReadSample(frame, 0, format);
                        input[1] = format->nChannels > 1 ? ReadSample(frame, 1, format) : input[0];
                        // Centre/LFE/surround content carries much of the engine and shift transient.
                        for (uint16_t channel = 2; channel < format->nChannels; ++channel) {
                            const double extra = ReadSample(frame, channel, format) * 0.12;
                            input[0] += extra;
                            input[1] += extra;
                        }
                    }
                    for (size_t side = 0; side < 2; ++side) {
                        highPassed[side] = hpAlpha *
                            (highPassed[side] + input[side] - previousInput[side]);
                        previousInput[side] = input[side];
                        lowPassed[side] += lpAlpha * (highPassed[side] - lowPassed[side]);
                        energy[side] += lowPassed[side] * lowPassed[side];
                    }
                    const double fullEnergy = std::max(std::abs(input[0]), std::abs(input[1]));
                    fastEnvelope += 0.08 * (fullEnergy - fastEnvelope);
                    slowEnvelope += 0.0012 * (fullEnergy - slowEnvelope);
                }
                captureClient->ReleaseBuffer(frames);

                const bool driving = g_isDriving.load(std::memory_order_relaxed);
                float vehicleGain = VehicleGain();
                if (vehicleGain < 0.0f) vehicleGain = 0.0f;
                if (vehicleGain > 2.0f) vehicleGain = 2.0f;
                if (driving && vehicleGain > 0.0f && frames > 0) {
                    // The low-band RMS is the sustained engine/road body. Fast-minus-slow is the
                    // short shift/impact edge. This is the same evidence source as the PSVR2
                    // bridge, reduced to an OpenXR amplitude envelope because Touch has no PCM.
                    const double transient = std::clamp(
                        (fastEnvelope - slowEnvelope * 1.12) * 12.0, 0.0, 1.0);
                    std::array<float, 2> output{};
                    for (size_t side = 0; side < 2; ++side) {
                        const double rms = std::sqrt(energy[side] / static_cast<double>(frames));
                        const double engine = std::clamp((rms - 0.0012) / 0.032, 0.0, 1.0);
                        double target = std::clamp(engine * 0.42 + transient * 0.68, 0.0, 0.90);
                        target *= vehicleGain;
                        if (target > 1.0) target = 1.0;
                        const double smoothing = target > smoothed[side] ? 0.48 : 0.10;
                        smoothed[side] += smoothing * (target - smoothed[side]);
                        output[side] = static_cast<float>(smoothed[side]);
                    }
                    SetVehicleHaptics(output[0], output[1]);
                    if (!envelopeConfirmed && (output[0] > 0.03f || output[1] > 0.03f)) {
                        envelopeConfirmed = true;
                        Log("OpenXRManager[VehicleHaptics]: driving audio envelope confirmed left=%.2f right=%.2f.\n",
                            output[0], output[1]);
                    }
                    wasDriving = true;
                } else {
                    smoothed = {};
                    if (wasDriving) {
                        SetVehicleHaptics(0.0f, 0.0f);
                        wasDriving = false;
                    }
                }

                result = captureClient->GetNextPacketSize(&packetFrames);
                if (FAILED(result)) break;
            }
            if (FAILED(result)) break;
        }
        audioClient->Stop();
        if (FAILED(result) && g_running.load(std::memory_order_relaxed)) {
            Log("OpenXRManager[VehicleHaptics]: loopback capture stopped result=0x%08lX.\n",
                static_cast<unsigned long>(result));
        }
    }

    SetVehicleHaptics(0.0f, 0.0f);
    if (format) CoTaskMemFree(format);
    if (captureClient) captureClient->Release();
    if (audioClient) audioClient->Release();
    if (device) device->Release();
    if (enumerator) enumerator->Release();
    if (SUCCEEDED(comResult)) CoUninitialize();
}
} // namespace

void StartVehicleAudioCapture() {
    if (g_running.exchange(true, std::memory_order_acq_rel)) return;
    g_thread = std::thread(CaptureThread);
}

void StopVehicleAudioCapture() {
    if (!g_running.exchange(false, std::memory_order_acq_rel)) return;
    if (g_thread.joinable()) g_thread.join();
    SetVehicleHaptics(0.0f, 0.0f);
}

} // namespace cvr::haptics
