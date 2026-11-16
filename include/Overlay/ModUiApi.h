#pragma once
// Optional, versioned C ABI for independently built UI renderers. No ImGui,
// STL, OpenXR, or allocator objects cross the DLL boundary. See docs/mod-ui-vr.md.
#include <stdint.h>
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif
#define CVR_MOD_UI_VERSION 1u
#define CVR_MOD_UI_VISIBLE 1u
#define CVR_MOD_UI_INTERACTIVE 2u
#define CVR_MOD_UI_PRIMARY 1u
#define CVR_MOD_UI_SECONDARY 2u
#define CVR_MOD_UI_BACK 1u
#define CVR_MOD_UI_ACTIVE 2u
typedef uint64_t CvrModUiHandle;
typedef struct CvrModUiViewport {
    uint32_t size, width, height, reserved;
} CvrModUiViewport;
typedef struct CvrModUiTarget {
    uint32_t size, width, height, format;
    uint64_t lease;
    void* texture; // borrowed ID3D12Resource*, valid until Submit/Cancel
    uintptr_t rtv; // borrowed D3D12_CPU_DESCRIPTOR_HANDLE.ptr
} CvrModUiTarget;
typedef struct CvrModUiInput {
    float x, y, wheel;
    uint32_t buttons, flags;
    int32_t tab;
} CvrModUiInput;
typedef struct CvrModUiApi {
    uint32_t size, version;
    CvrModUiHandle (*Register)(const char* name);
    void (*Unregister)(CvrModUiHandle);
    int (*Viewport)(CvrModUiHandle, CvrModUiViewport*);
    void (*State)(CvrModUiHandle, uint32_t flags);
    uint32_t (*Poll)(CvrModUiHandle, CvrModUiInput*, uint32_t capacity);
    int (*Begin)(CvrModUiHandle, void* device, uint32_t format, CvrModUiTarget*);
    // Signal this fence on the producer queue BEFORE submitting. End the
    // texture in PIXEL_SHADER_RESOURCE. Never touch it after Submit.
    int (*Submit)(CvrModUiHandle, uint64_t lease, void* fence, uint64_t value);
    // Cancel ONLY if no GPU work references this lease.
    void (*Cancel)(CvrModUiHandle, uint64_t lease);
} CvrModUiApi;
typedef const CvrModUiApi* (*CvrGetModUiApi)(uint32_t version, uint32_t size);
// Discover CyberpunkVR_GetModUiApi with GetProcAddress. Missing/incompatible
// host or Viewport==0 means the renderer keeps its normal desktop behavior.
#ifdef __cplusplus
}
#endif
