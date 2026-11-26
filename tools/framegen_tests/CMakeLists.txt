cmake_minimum_required(VERSION 3.24)
project(NativeFramegenTests LANGUAGES CXX)
enable_testing()
add_test(NAME framegen_hand_motion COMMAND framegen_tests hand_motion_test)
add_test(NAME framegen_scene_velocity_pass COMMAND framegen_tests scene_velocity_pass)
add_test(NAME framegen_presentation_period COMMAND framegen_tests presentation_period)
set(root "${CMAKE_CURRENT_SOURCE_DIR}/../..")
# The main build's pinned SDKs (cmake/NativeFramegen.cmake); pass -DCVR_SDK_BUILD=<its build dir> for another one.
set(CVR_SDK_BUILD "${root}/build/patchday/sdk25-build" CACHE PATH "Main build folder holding fidelityfx-native and dlss-sdk")
set(sdk "${CVR_SDK_BUILD}/fidelityfx-native/sdk")
# Framegen.cpp, StatsOverlay.cpp and Paths.cpp call the plugin's Rust (rust/crates/cvr-gpu, cvr-ffi) in both builds.
include(${root}/cmake/RustStaticlib.cmake)
# The C++ reference build of the framegen files. (A CVR_RUST_VRIK=1 variant needs the plugin's rust-build modules,
# whose C++ seams -- the menu, the HUD quads' GPU profile -- a test program does not define.)
set(dlss "${CVR_SDK_BUILD}/dlss-sdk")
set(framegen_cases policy cadence presenter_timeline stereo_depth settings pacing adaptive_pacing metrics metrics_disabled
    debug_gate history gpu_fidelityfx gpu_fidelityfx_rgba16 input_copy gpu_fidelityfx_flow gpu_fidelityfx_half_motion
    gpu_fidelityfx_noflow overlay overlay_follow planar_depth gpu_timer hardware)
function(framegen_test target suffix)
    add_executable(${target} main.cpp replay.cpp afw_stub.cpp
        ${root}/src/Framegen/InputCopy.cpp
        ${root}/src/Render/StereoDepth.cpp
        ${root}/src/Framegen/Framegen.cpp ${root}/src/Framegen/Generator.cpp ${root}/src/Framegen/DlssFrameGen.cpp ${root}/src/Framegen/FrameCapture.cpp
        ${root}/src/Framegen/Presenter.cpp ${root}/src/Framegen/StatsOverlay.cpp ${root}/src/Framegen/GpuTimer.cpp
        ${root}/src/Framegen/HardwareTelemetry.cpp ${root}/src/Core/Paths.cpp)
    target_compile_features(${target} PRIVATE cxx_std_20)
    target_compile_definitions(${target} PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN XR_USE_PLATFORM_WIN32 XR_USE_GRAPHICS_API_D3D12 ${ARGN})
    target_include_directories(${target} PRIVATE ${root}/include ${root}/build/_deps/openxr-src/include
        ${sdk}/include ${root}/externals/nvidia-optical-flow)
    cvr_link_rust(${target})
    target_compile_options(${target} PRIVATE /Zi)
    target_link_options(${target} PRIVATE /DEBUG)
    target_link_libraries(${target} PRIVATE d3d12 dxgi d3dcompiler dbghelp psapi advapi32 windowscodecs ole32
        ${sdk}/bin/ffx_sdk/ffx_frameinterpolation_x64.lib
        ${sdk}/bin/ffx_sdk/ffx_opticalflow_x64.lib
        ${sdk}/bin/ffx_sdk/ffx_backend_dx12_x64.lib)
    if(EXISTS "${dlss}/lib/Windows_x86_64/x64/nvsdk_ngx_d.lib")
        target_include_directories(${target} PRIVATE "${dlss}/include")
        target_link_libraries(${target} PRIVATE optimized "${dlss}/lib/Windows_x86_64/x64/nvsdk_ngx_d.lib"
            debug "${dlss}/lib/Windows_x86_64/x64/nvsdk_ngx_d_dbg.lib")
        target_compile_definitions(${target} PRIVATE CVR_NATIVE_DLSSG=1)
    endif()
    foreach(case ${framegen_cases})
        add_test(NAME framegen_${case}${suffix} COMMAND ${target} ${case})
    endforeach()
endfunction()
framegen_test(framegen_tests "")
# Hardware NVIDIA OFA is exercised explicitly, never silently replaced with WARP.
# ANR FG beside the test, as beside the plugin (cmake/NativeFramegen.cmake), for gpu_anr on real hardware.
set(anr "${CVR_SDK_BUILD}/anr-fg-V.B15/Apex Neural Rendering FG/dlssg")
if(EXISTS "${anr}/nvngx_dlssg.dll")
    add_custom_command(TARGET framegen_tests POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E copy_directory "${anr}" "$<TARGET_FILE_DIR:framegen_tests>/anr/dlssg")
    add_test(NAME framegen_gpu_anr COMMAND framegen_tests gpu_anr)
endif()
