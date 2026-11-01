// LumaProbe -- reading pixels back to settle an argument about brightness.
//
// The bright/dark alternation was invisible to reasoning: the two frames were structurally identical,
// the same nodes ran, the same targets were bound. What separated them was a NUMBER, so these probes
// read the number -- an 8x8 block of the finished image (luma_probe), of the constant that feeds it
// (cb_probe), and of every large texture the tonemap node reads (ti_probe) -- each split by which of
// the two output states the frame landed in.
//
// A field with a large split between the two states is the culprit; innocent fields sit near zero.
// That is the whole method, and it is why three separate colour bugs in this module have an answer
// rather than a workaround.
//
// The half-float and packed-float decoders are here because the surfaces are R11G11B10 and half-float:
// reading them as bytes yields numbers that look like data and are not.
//
// vision_dump_write rides along. It is the same machinery -- a readback resource, a mapped pointer, a
// one-shot drain -- pointed at the outline layer instead of the tonemap output.

#include "Stereo/SyncStereo.hpp"
#include "Core/Paths.hpp"
#include "Utils/StereoLog.hpp"
#include "Stereo/VrcamConfig.hpp"   // vrcam.json access + CName hashing, shared with the launcher
#include "Render/ColorBlit.hpp"   // HUD debug overlay on the mirror image
#include <windows.h>
#include <d3d12.h>
#include <d3d11.h>
#include <d3d11on12.h>
#include <dxgi1_4.h>
#include <intrin.h>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <thread>
#include <unordered_map>
#include <vector>
#include "MinHook.h"
#include "Utils/LogThrottle.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/EngineRvas.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/EngineRvas.hpp"

namespace cvr {
namespace detail {

// --- luma oscilloscope helpers (see exports block for the rationale) ---
static float luma_dec_f11(uint32_t v) {
    const uint32_t e = (v >> 6) & 0x1F, m = v & 0x3F;
    if (e == 0)  return m * (1.0f / 64.0f) * 0.00006103515625f;   // 2^-14
    if (e == 31) return 0.0f;                                     // inf/nan -> ignore
    return (1.0f + m / 64.0f) * exp2f((int)e - 15);
}
static float luma_dec_f10(uint32_t v) {
    const uint32_t e = (v >> 5) & 0x1F, m = v & 0x1F;
    if (e == 0)  return m * (1.0f / 32.0f) * 0.00006103515625f;
    if (e == 31) return 0.0f;
    return (1.0f + m / 32.0f) * exp2f((int)e - 15);
}
// ---- one-shot dump of the outline layer ------------------------------------------------------
// The second eye shows the layer as it is: silhouette fill AND outline. MAIN shows only the
// outline, because its chain (PS587 -> PS1047 -> PS1290 -> the PS1216 composite) derives edges
// from it, and that chain does not run for the second view. Before reproducing anything, look at
// what actually distinguishes an outline texel from a fill texel in this layer -- guessing that
// from the picture is how the previous three rounds went wrong.
// Write 1 to arm; the file lands next to the exe and the knob resets itself.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_VisionDump = 1;
ID3D12Resource* g_visdump_rb = nullptr;
void*    g_visdump_map = nullptr;
uint32_t g_visdump_w = 0, g_visdump_h = 0, g_visdump_pitch = 0;
int      g_visdump_slot = -1;

void vision_dump_write() {
    if (!g_visdump_map || !g_visdump_w || !g_visdump_h) return;
    char path[MAX_PATH]{};
    if (strcpy_s(path, cvr::paths::FileA(cvr::paths::Folder::Log, "cyberpunkvr_vision.raw").c_str()) != 0) return;
    HANDLE f = CreateFileA(path, GENERIC_WRITE, FILE_SHARE_READ, nullptr,
                           CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (f == INVALID_HANDLE_VALUE) return;
    // Rows written TIGHT (no 256-byte padding), so the file is a plain w*h RGBA8 raster.
    const uint8_t* base = static_cast<const uint8_t*>(g_visdump_map);
    const uint32_t row = g_visdump_w * 4;
    DWORD wrote = 0;
    for (uint32_t y = 0; y < g_visdump_h; ++y)
        WriteFile(f, base + static_cast<size_t>(y) * g_visdump_pitch, row, &wrote, nullptr);
    CloseHandle(f);
    log("[vision] dumped %ux%u RGBA8 -> %s", g_visdump_w, g_visdump_h, path);
}

bool luma_probe_ensure(uint32_t idx) {
    if (g_luma_rb[idx]) return g_luma_map[idx] != nullptr;
    D3D12_HEAP_PROPERTIES hp{};
    hp.Type = D3D12_HEAP_TYPE_READBACK;
    hp.CreationNodeMask = 1;
    hp.VisibleNodeMask = 1;
    D3D12_RESOURCE_DESC bd{};
    bd.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    bd.Width = 2048;                    // 8 rows x 256B pitch
    bd.Height = 1;
    bd.DepthOrArraySize = 1;
    bd.MipLevels = 1;
    bd.Format = DXGI_FORMAT_UNKNOWN;
    bd.SampleDesc.Count = 1;
    bd.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ID3D12Resource* rb = nullptr;
    if (FAILED(g_game_device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &bd,
            D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&rb))) || !rb)
        return false;
    void* p = nullptr;
    if (FAILED(rb->Map(0, nullptr, &p)) || !p) { rb->Release(); return false; }
    g_luma_rb[idx] = rb;
    g_luma_map[idx] = static_cast<uint8_t*>(p);
    return true;
}
double luma_probe_collect(uint32_t idx) {
    static double   s_sum[2] = {};
    static uint32_t s_n[2] = {};
    static double   s_dsum = 0.0;
    static uint32_t s_dn = 0;
    static double   s_last = -1.0;
    static uint32_t s_last_fr = 0xFFFFFFFFu;
    double L = 0.0;
    for (int y = 0; y < 8; ++y) {
        const uint32_t* row =
            reinterpret_cast<const uint32_t*>(g_luma_map[idx] + y * 256);
        for (int x = 0; x < 8; ++x) {
            const uint32_t v = row[x];
            L += 0.2126f * luma_dec_f11(v & 0x7FF)
               + 0.7152f * luma_dec_f11((v >> 11) & 0x7FF)
               + 0.0722f * luma_dec_f10((v >> 22) & 0x3FF);
        }
    }
    L /= 64.0;
    if (CyberpunkVR_LumaWave > 0) {
        --CyberpunkVR_LumaWave;            // benign race; diagnostic only
        log("[lumaw] fr=%u L=%.4f", g_luma_frame[idx], L);
    }
    const uint32_t p = g_luma_parity[idx] & 1u;
    s_sum[p] += L;
    ++s_n[p];
    // SAFE correlation: bucket L by the fin natural index used THAT frame (carried
    // through the readback ring, no cross-thread latency). Identifies which physical
    // index = dark(correct)/bright(wrong) AND whether the correct one is a stable
    // recurring (persistent) value -> tells us exactly what to pin, crash-free.
    {
        struct FinBucket { uint32_t idx; double sum; uint32_t n; };
        static FinBucket s_fb[12] = {};
        static uint32_t  s_fb_frames = 0;
        const uint32_t fi = g_luma_finidx[idx];
        for (int k = 0; k < 12; ++k) {
            if (s_fb[k].n == 0 || s_fb[k].idx == fi) {
                s_fb[k].idx = fi; s_fb[k].sum += L; ++s_fb[k].n; break;
            }
        }
        if (++s_fb_frames >= 240) {
            for (int k = 0; k < 12; ++k) {
                if (!s_fb[k].n) continue;
                log("[fincorr] finidx=%u meanL=%.4f n=%u",
                    s_fb[k].idx, s_fb[k].sum / s_fb[k].n, s_fb[k].n);
            }
            memset(s_fb, 0, sizeof(s_fb));
            s_fb_frames = 0;
        }
    }

    if (s_last >= 0.0 && g_luma_frame[idx] == s_last_fr + 1) {
        s_dsum += (L > s_last) ? (L - s_last) : (s_last - L);
        ++s_dn;
    }
    s_last = L;
    s_last_fr = g_luma_frame[idx];
    if (s_n[0] + s_n[1] >= 120) {
        const double e = s_n[0] ? s_sum[0] / s_n[0] : 0.0;
        const double o = s_n[1] ? s_sum[1] / s_n[1] : 0.0;
        const double dm = s_dn ? s_dsum / s_dn : 0.0;
        CyberpunkVR_DebugLumaEvenMilli  = (uint32_t)(e * 1000.0 + 0.5);
        CyberpunkVR_DebugLumaOddMilli   = (uint32_t)(o * 1000.0 + 0.5);
        CyberpunkVR_DebugLumaDeltaMilli = (uint32_t)(dm * 1000.0 + 0.5);
        log("[luma] n=%u/%u even=%.4f odd=%.4f dmean=%.4f(n=%u)",
            s_n[0], s_n[1], e, o, dm, s_dn);
        s_sum[0] = s_sum[1] = 0.0;
        s_n[0] = s_n[1] = 0;
        s_dsum = 0.0;
        s_dn = 0;
    }
    return L;
}

bool cb_probe_ensure(uint32_t idx) {
    if (g_cb_rb[idx]) return g_cb_map[idx] != nullptr;
    D3D12_HEAP_PROPERTIES hp{};
    hp.Type = D3D12_HEAP_TYPE_READBACK;
    hp.CreationNodeMask = 1;
    hp.VisibleNodeMask = 1;
    D3D12_RESOURCE_DESC bd{};
    bd.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    bd.Width = 1024;
    bd.Height = 1;
    bd.DepthOrArraySize = 1;
    bd.MipLevels = 1;
    bd.Format = DXGI_FORMAT_UNKNOWN;
    bd.SampleDesc.Count = 1;
    bd.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ID3D12Resource* rb = nullptr;
    if (FAILED(g_game_device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &bd,
            D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&rb))) || !rb)
        return false;
    void* p = nullptr;
    if (FAILED(rb->Map(0, nullptr, &p)) || !p) { rb->Release(); return false; }
    g_cb_rb[idx] = rb;
    g_cb_map[idx] = static_cast<uint8_t*>(p);
    return true;
}
// Auto-detect the raced CB dword: track per-dword value sets; a candidate holds
// EXACTLY two distinct values over the window and flips between them in sync with
// the bright/normal luma state. Scene-driven fields (matrices, time) take >2 values
// and disqualify themselves.
void cb_probe_collect(uint32_t idx, double L) {
    // 232 dwords: 0..211 = tonemap CB, 214..220 = vrcam exposure accumulator (28B @
    // offset 856), 224..230 = main's (@896). 212/213/221..223/231 = padding (stale).
    struct CbDw { uint32_t v0, v1; uint8_t nv, last; uint32_t flips, agree, corr_n; };
    static CbDw     s_cb[232] = {};
    static uint32_t s_frames = 0;
    static double   s_lmin = 1e9, s_lmax = -1e9;
    if (g_cb_reset_pending.exchange(false, std::memory_order_acq_rel)) {
        memset(s_cb, 0, sizeof(s_cb));      // CB resource changed: restart window
        s_frames = 0;
        s_lmin = 1e9;
        s_lmax = -1e9;
    }
    if (L < s_lmin) s_lmin = L;
    if (L > s_lmax) s_lmax = L;
    const bool flap_active = (s_lmax - s_lmin) > 0.02;
    const bool bright = flap_active && (L > (s_lmin + s_lmax) * 0.5);
    const uint32_t* dw = reinterpret_cast<const uint32_t*>(g_cb_map[idx]);
    for (int i = 0; i < 232; ++i) {
        CbDw& c = s_cb[i];
        const uint32_t v = dw[i];
        uint8_t state;
        if (c.nv == 0)            { c.v0 = v; c.nv = 1; state = 0; }
        else if (v == c.v0)       { state = 0; }
        else if (c.nv == 1)       { c.v1 = v; c.nv = 2; state = 1; }
        else if (v == c.v1)       { state = 1; }
        else                      { c.nv = 3; continue; }      // >2 values: disqualified
        if (c.nv == 2) {
            if (state != c.last) ++c.flips;
            if (flap_active) {
                if ((state != 0) == bright) ++c.agree;         // orientation A
                ++c.corr_n;
            }
        }
        c.last = state;
    }
    if (++s_frames >= 300) {
        const float* xv = reinterpret_cast<const float*>(g_cb_map[idx] + 856);
        const float* xm = reinterpret_cast<const float*>(g_cb_map[idx] + 896);
        log("[expo] v=%.5g %.5g %.5g %.5g %.5g %.5g %.5g | m=%.5g %.5g %.5g %.5g %.5g %.5g %.5g",
            xv[0], xv[1], xv[2], xv[3], xv[4], xv[5], xv[6],
            xm[0], xm[1], xm[2], xm[3], xm[4], xm[5], xm[6]);
        int b1 = -1, b2 = -1;
        for (int i = 0; i < 232; ++i) {
            const CbDw& c = s_cb[i];
            if (c.nv != 2 || c.flips < 8) continue;
            if (b1 < 0 || c.flips > s_cb[b1].flips) { b2 = b1; b1 = i; }
            else if (b2 < 0 || c.flips > s_cb[b2].flips) { b2 = i; }
        }
        for (int k = 0; k < 2; ++k) {
            const int i = (k == 0) ? b1 : b2;
            if (i < 0) continue;
            const CbDw& c = s_cb[i];
            const uint32_t hi = (c.corr_n && c.agree > c.corr_n / 2)
                ? c.agree : c.corr_n - c.agree;
            const bool hit = (c.corr_n >= 30 && hi * 10 >= c.corr_n * 9);
            float f0, f1;
            memcpy(&f0, &c.v0, 4);
            memcpy(&f1, &c.v1, 4);
            log("[cbflap] %s off=0x%03X v0=%08X(%.4f) v1=%08X(%.4f) flips=%u sync=%u/%u",
                hit ? "HIT " : "cand", i * 4, c.v0, f0, c.v1, f1, c.flips,
                hi, c.corr_n);
        }
        if (b1 < 0)
            log("[cbflap] window done: no 2-valued dword (flap %s)",
                flap_active ? "ACTIVE" : "inactive");
        memset(s_cb, 0, sizeof(s_cb));
        s_frames = 0;
        s_lmin = 1e9;
        s_lmax = -1e9;
    }
}

bool ti_probe_ensure(uint32_t idx) {
    if (g_ti_rb[idx]) return g_ti_map[idx] != nullptr;
    D3D12_HEAP_PROPERTIES hp{};
    hp.Type = D3D12_HEAP_TYPE_READBACK;
    hp.CreationNodeMask = 1;
    hp.VisibleNodeMask = 1;
    D3D12_RESOURCE_DESC bd{};
    bd.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    bd.Width = 65536;                       // 24 x 2KB sections + slack
    bd.Height = 1;
    bd.DepthOrArraySize = 1;
    bd.MipLevels = 1;
    bd.Format = DXGI_FORMAT_UNKNOWN;
    bd.SampleDesc.Count = 1;
    bd.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ID3D12Resource* rb = nullptr;
    if (FAILED(g_game_device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &bd,
            D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&rb))) || !rb)
        return false;
    void* p = nullptr;
    if (FAILED(rb->Map(0, nullptr, &p)) || !p) { rb->Release(); return false; }
    g_ti_rb[idx] = rb;
    g_ti_map[idx] = static_cast<uint8_t*>(p);
    return true;
}
static float half2f(uint16_t h) {
    const uint32_t e = (h >> 10) & 0x1F, m = h & 0x3FF;
    float f;
    if (e == 0)       f = m * (1.0f / 1024.0f) * 0.00006103515625f;
    else if (e == 31) f = 0.0f;
    else              f = (1.0f + m / 1024.0f) * exp2f((int)e - 15);
    return (h & 0x8000) ? -f : f;
}
// Green-channel mean of an 8x8 block (flap detection needs any monotonic channel).
static double ti_block_green(const uint8_t* base, uint32_t fmt) {
    double s = 0.0;
    for (int y = 0; y < 8; ++y) {
        const uint8_t* row = base + y * 256;
        for (int x = 0; x < 8; ++x) {
            switch (fmt) {
            case DXGI_FORMAT_R11G11B10_FLOAT:
                s += luma_dec_f11((reinterpret_cast<const uint32_t*>(row)[x] >> 11)
                                  & 0x7FF);
                break;
            case DXGI_FORMAT_R10G10B10A2_UNORM:
                s += ((reinterpret_cast<const uint32_t*>(row)[x] >> 10) & 0x3FF)
                     * (1.0 / 1023.0);
                break;
            case DXGI_FORMAT_R16G16B16A16_FLOAT:
                s += half2f(reinterpret_cast<const uint16_t*>(row)[x * 4 + 1]);
                break;
            case DXGI_FORMAT_R32_FLOAT:
                s += reinterpret_cast<const float*>(row)[x];
                break;
            default:    // 8-bit RGBA variants: byte 1 = G
                s += row[x * 4 + 1] * (1.0 / 255.0);
                break;
            }
        }
    }
    return s / 64.0;
}
void ti_probe_collect(uint32_t idx, double L) {
    struct Stat {
        ID3D12Resource* res; uint32_t fmt; uint32_t tag;
        double sum[2]; uint32_t n[2];
    };
    static Stat     s_st[32] = {};
    static uint32_t s_frames = 0;
    static double   s_lmin = 1e9, s_lmax = -1e9;
    if (L < s_lmin) s_lmin = L;
    if (L > s_lmax) s_lmax = L;
    const bool flap_active = (s_lmax - s_lmin) > 0.02;
    const int bright = (flap_active && L > (s_lmin + s_lmax) * 0.5) ? 1 : 0;
    if (flap_active) {
        for (uint32_t i = 0; i < g_ti_count[idx] && i < 24; ++i) {
            ID3D12Resource* res = g_ti_src[idx][i];
            if (!res) continue;
            const double g =
                ti_block_green(g_ti_map[idx] + i * 2048, g_ti_fmt[idx][i]);
            for (int k = 0; k < 32; ++k) {
                if (s_st[k].res == res || !s_st[k].res) {
                    s_st[k].res = res;
                    s_st[k].fmt = g_ti_fmt[idx][i];
                    s_st[k].tag = g_ti_tag[idx][i];
                    s_st[k].sum[bright] += g;
                    ++s_st[k].n[bright];
                    break;
                }
            }
        }
    }
    if (++s_frames >= 300) {
        for (int k = 0; k < 32; ++k) {
            const Stat& t = s_st[k];
            if (!t.res || (t.n[0] + t.n[1]) < 30) continue;
            const double mn = t.n[0] ? t.sum[0] / t.n[0] : 0.0;
            const double mb = t.n[1] ? t.sum[1] / t.n[1] : 0.0;
            log("[chain] res=%p fmt=%u node=0x%X normal=%.4f(n=%u) bright=%.4f(n=%u) split=%+.4f",
                t.res, t.fmt, t.tag, mn, t.n[0], mb, t.n[1], mb - mn);
        }
        memset(s_st, 0, sizeof(s_st));
        s_frames = 0;
        s_lmin = 1e9;
        s_lmax = -1e9;
    }
}

}  // namespace detail
}  // namespace cvr
