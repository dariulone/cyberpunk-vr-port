cmake_minimum_required(VERSION 3.24)

project(CyberpunkVRPort LANGUAGES CXX)

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

add_library(dxgi SHARED
    src/dxgi_proxy.cpp
    src/openxr_manager.cpp
    src/optical_flow_d3d12.cpp
    src/dxgi_factory_wrapper.cpp
    src/imgui_overlay.cpp
    src/launcher_dialog.cpp
    src/ngx_hook.cpp
    src/mv_warp.cpp
    src/depth_resolve.cpp
    src/stereo_reproject.cpp
    src/sharpen_pass.cpp
    src/color_blit.cpp
    src/aer_v2/cuda_interop.cpp
    src/aer_v2/nvof_instance.cpp
    src/aer_v2/warp_kernel.cpp
    src/aer_v2/aer_v2_pipeline.cpp
    src/aer_v2/pose_predictor.cpp
    externals/im3d/im3d.cpp
    externals/Optical_Flow_SDK_5.0.7/Common/NvOFBase/NvOF.cpp
    externals/Optical_Flow_SDK_5.0.7/Common/NvOFBase/NvOFD3D12.cpp
    externals/Optical_Flow_SDK_5.0.7/Common/NvOFBase/NvOFD3DCommon.cpp
)

add_library(d3d12 SHARED
    src/d3d12_proxy.cpp
)
add_subdirectory(externals/RED4ext.SDK)


target_compile_features(dxgi PRIVATE cxx_std_20)
target_compile_definitions(dxgi PRIVATE WIN32_LEAN_AND_MEAN NOMINMAX XR_USE_PLATFORM_WIN32 XR_USE_GRAPHICS_API_D3D12)
target_link_libraries(dxgi PRIVATE 
    openxr_loader 
    imgui 
    d3d12 
    d3dcompiler
    RED4ext::SDK
)
target_include_directories(dxgi PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/im3d
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/Optical_Flow_SDK_5.0.7/Common/NvOFBase
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/Optical_Flow_SDK_5.0.7/NvOFInterface
    ${CMAKE_CURRENT_SOURCE_DIR}/src/aer_v2
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/minhook/include
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/minhook/include
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/minhook/src
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/minhook/src/hde
)

# ---- Optional CUDA / NvOF CUDA backend (AER V2 "эталон" path) ----
# When the CUDA Toolkit is present we add the NvOFCuda backend + compile the
# warp kernel (.cu). Without CUDA the project still builds — the AER V2
# pipeline stub returns false and OpenXRManager falls back to AER legacy.
option(AER_V2_NVOF "Enable NvOF CUDA backend for AER V2" ON)
if(AER_V2_NVOF)
    find_package(CUDAToolkit QUIET)
    if(CUDAToolkit_FOUND)
        enable_language(CUDA)
        target_sources(dxgi PRIVATE
            externals/Optical_Flow_SDK_5.0.7/Common/NvOFBase/NvOFCuda.cpp
            src/aer_v2/warp_kernel.cu
        )
        # Distribution build: real SASS for every common gaming/datacenter arch
        # (Turing sm_75 is the NvOF floor) PLUS a compute_75 PTX fallback, so any
        # GPU >= Turing that lacks a matching cubin (Jetson sm_87, future Blackwell
        # variants, etc.) JITs from PTX instead of failing. Was "native" -> that
        # baked in ONLY the build machine's GPU arch, so on any user with a different
        # GPU the warp kernel had no image and cudaLaunchKernel failed every frame
        # with cudaErrorNoKernelImageForDevice (err=209) -> AER V2 synthesis silently
        # died (raw stale eye, "eyes not superposed"), e.g. on an RTX 4090 (sm_89).
        # NOTE: do NOT use "all-major" -- on CUDA 13 it emits real SASS only for the
        # newest minor of each major ({75,89,90,120}) + compute_120 PTX, which leaves
        # RTX 30-series (sm_86) with no cubin and no usable (forward-only) PTX -> 209.
        # 86-real is required explicitly. Override with -DCPVR_CUDA_ARCHITECTURES=native
        # for fast local iteration.
        if(NOT DEFINED CPVR_CUDA_ARCHITECTURES)
            set(CPVR_CUDA_ARCHITECTURES "75-real;80-real;86-real;89-real;90-real;120-real;75-virtual")
        endif()
        set_property(TARGET dxgi PROPERTY CUDA_ARCHITECTURES "${CPVR_CUDA_ARCHITECTURES}")
        target_compile_definitions(dxgi PRIVATE AER_V2_NVOF_ENABLED=1)
        target_link_libraries(dxgi PRIVATE CUDA::cudart CUDA::cuda_driver)
        target_include_directories(dxgi PRIVATE ${CUDAToolkit_INCLUDE_DIRS})
        file(GLOB AER_V2_CUDART_DLLS
            "${CUDAToolkit_BIN_DIR}/cudart64_*.dll"
            "${CUDAToolkit_BIN_DIR}/x64/cudart64_*.dll")
        foreach(AER_V2_CUDART_DLL IN LISTS AER_V2_CUDART_DLLS)
            add_custom_command(TARGET dxgi POST_BUILD
                COMMAND ${CMAKE_COMMAND} -E copy_if_different
                    "${AER_V2_CUDART_DLL}"
                    "$<TARGET_FILE_DIR:dxgi>")
        endforeach()
        message(STATUS "AER V2 NvOF: ENABLED (CUDA ${CUDAToolkit_VERSION})")
    else()
        message(STATUS "AER V2 NvOF: DISABLED (CUDA Toolkit not found; fallback to D3D12-compute)")
    endif()
endif()

target_compile_features(d3d12 PRIVATE cxx_std_20)
target_compile_definitions(d3d12 PRIVATE WIN32_LEAN_AND_MEAN NOMINMAX)

if(MSVC)
    target_compile_options(dxgi PRIVATE $<$<COMPILE_LANGUAGE:CXX>:/W4 /permissive- /EHsc>)
    target_compile_options(d3d12 PRIVATE /W4 /permissive- /EHsc)
endif()

set_target_properties(dxgi PROPERTIES
    OUTPUT_NAME dxgi
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin"
)

set_target_properties(d3d12 PROPERTIES
    OUTPUT_NAME d3d12
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin"
)
