cmake_minimum_required(VERSION 3.24)

project(CyberpunkVRPort LANGUAGES CXX C)

include(FetchContent)
FetchContent_Declare(
  openxr
  GIT_REPOSITORY https://github.com/KhronosGroup/OpenXR-SDK.git
  GIT_TAG release-1.0.34
)
FetchContent_MakeAvailable(openxr)

FetchContent_Declare(
  imgui
  GIT_REPOSITORY https://github.com/ocornut/imgui.git
  GIT_TAG v1.90.9
)
FetchContent_GetProperties(imgui)
if(NOT imgui_POPULATED)
  FetchContent_Populate(imgui)
endif()

set(MINHOOK_SOURCES
    externals/minhook/src/buffer.c
    externals/minhook/src/hook.c
    externals/minhook/src/trampoline.c
    externals/minhook/src/hde/hde32.c
    externals/minhook/src/hde/hde64.c
)


add_library(imgui STATIC
    ${imgui_SOURCE_DIR}/imgui.cpp
    ${imgui_SOURCE_DIR}/imgui_draw.cpp
    ${imgui_SOURCE_DIR}/imgui_tables.cpp
    ${imgui_SOURCE_DIR}/imgui_widgets.cpp
    ${imgui_SOURCE_DIR}/backends/imgui_impl_win32.cpp
    ${imgui_SOURCE_DIR}/backends/imgui_impl_dx12.cpp
    ${MINHOOK_SOURCES}
)
target_include_directories(imgui PUBLIC
    ${imgui_SOURCE_DIR}
    ${imgui_SOURCE_DIR}/backends
)



add_subdirectory(externals/RED4ext.SDK)

# ============================================================================================
# CyberpunkVR_Stereo -- the RED4ext plugin. This is the whole mod.
# ============================================================================================
#
# It used to ship as a dxgi.dll proxy sitting next to the game exe, with DllMain and the seven
# DXGI factory exports forwarding to the system dxgi.dll. None of the code ever needed to BE
# the proxy -- it only needed a device, a queue and Present, and RED4ext hands over all three.
# The proxy is gone: no DllMain, no export forwarders, no LoadLibrary of the real dxgi.dll,
# and no second copy of every hook racing the first if both DLLs were installed by mistake.
#
# CyberpunkVR_Hands.dll is a SEPARATE red4ext plugin and stays that way -- this one must not
# absorb it or share its directory.
add_library(cyberpunkvrport_stereo SHARED
    src/red4ext_stereo/plugin_main.cpp
    src/vr/core/vr_core.cpp
    src/vr/core/debug_gate.cpp
    src/vr/openxr/openxr_manager.cpp
    src/vr/openxr/openxr_frameloop.cpp
    src/vr/openxr/openxr_present.cpp
    src/vr/openxr/openxr_capture.cpp
    src/vr/openxr/openxr_calibration.cpp
    src/vr/openxr/openxr_vive_trackers.cpp
    src/vr/openxr/openvr_trackers.cpp
    src/vr/core/swapchain_hooks.cpp
    src/vr/render/depth_resolve.cpp
    src/vr/render/sharpen_pass.cpp
    src/vr/render/color_blit.cpp
    src/vr/stereo/sync_stereo.cpp
    src/vr/overlay/imgui_overlay.cpp
    src/vr/overlay/launcher_dialog.cpp
    src/vr/hooks/ngx_hook.cpp
    externals/im3d/im3d.cpp
)
target_compile_features(cyberpunkvrport_stereo PRIVATE cxx_std_20)
target_compile_definitions(cyberpunkvrport_stereo PRIVATE
    WIN32_LEAN_AND_MEAN NOMINMAX
    XR_USE_PLATFORM_WIN32 XR_USE_GRAPHICS_API_D3D12
    CPVR_RED4EXT_PLUGIN=1
    _CRT_SECURE_NO_WARNINGS=1
)
target_link_libraries(cyberpunkvrport_stereo PRIVATE
    openxr_loader
    imgui
    d3d12.lib
    d3dcompiler
    dxgi.lib        # the SYSTEM import lib -- bare "dxgi" would resolve to our own target
    psapi
    RED4ext::SDK
)
target_include_directories(cyberpunkvrport_stereo PRIVATE
    # --- module include roots (a bare #include "x.h" resolves anywhere) ---
    ${CMAKE_CURRENT_SOURCE_DIR}/src
    ${CMAKE_CURRENT_SOURCE_DIR}/src/common
    ${CMAKE_CURRENT_SOURCE_DIR}/src/vr
    ${CMAKE_CURRENT_SOURCE_DIR}/src/vr/core
    ${CMAKE_CURRENT_SOURCE_DIR}/src/vr/openxr
    ${CMAKE_CURRENT_SOURCE_DIR}/src/vr/render
    ${CMAKE_CURRENT_SOURCE_DIR}/src/vr/overlay
    ${CMAKE_CURRENT_SOURCE_DIR}/src/vr/hooks
    # --- third-party / vendored ---
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/im3d
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/minhook/include
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/minhook/src
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/minhook/src/hde
)
if(MSVC)
    target_compile_options(cyberpunkvrport_stereo PRIVATE
        $<$<COMPILE_LANGUAGE:CXX>:/W4 /permissive- /EHsc>)
    # SYMBOLS IN THE RELEASE BUILD. A crash landed inside this DLL and there was nothing to read it
    # with: no PDB here, no cdb or windbg on the machine, and dumpbin only opens PE files. Placing
    # CyberpunkVR_Stereo.dll+0x459AF took disassembling the whole image and matching the faulting
    # instruction by hand. That is affordable once.
    #
    # /Zi and /DEBUG do not change optimisation -- /OPT:REF and /OPT:ICF are restored explicitly
    # because /DEBUG turns them off by default, and without them the linker would stop folding and
    # stripping, which WOULD change the binary. The PDB ships nowhere; it stays in the build tree
    # so a dump from this machine resolves instantly and a tester's dump resolves against the build
    # that produced their DLL.
    target_compile_options(cyberpunkvrport_stereo PRIVATE $<$<CONFIG:Release>:/Zi>)
    target_link_options(cyberpunkvrport_stereo PRIVATE
        $<$<CONFIG:Release>:/DEBUG /OPT:REF /OPT:ICF>)
endif()
set_target_properties(cyberpunkvrport_stereo PROPERTIES
    PREFIX ""
    OUTPUT_NAME CyberpunkVR_Stereo
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin/red4ext/plugins/CyberpunkVR_Stereo"
)

# ============================================================================================
# Tools. Separate targets, never deployed with the plugin.
# ============================================================================================
# xr_probe_layer -- an OpenXR API layer that records what any VR application asks the runtime for
# and what it submits back. Used to read Quest 3 geometry out of R.E.A.L. VR (a dxgi.dll proxy)
# without touching its internals. Build it explicitly:
#     cmake --build build --config Release --target xr_probe_layer
add_subdirectory(tools/xr_probe)
