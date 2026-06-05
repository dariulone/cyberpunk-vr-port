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

add_library(imgui STATIC
    ${imgui_SOURCE_DIR}/imgui.cpp
    ${imgui_SOURCE_DIR}/imgui_draw.cpp
    ${imgui_SOURCE_DIR}/imgui_tables.cpp
    ${imgui_SOURCE_DIR}/imgui_widgets.cpp
    ${imgui_SOURCE_DIR}/backends/imgui_impl_win32.cpp
    ${imgui_SOURCE_DIR}/backends/imgui_impl_dx12.cpp
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
    third_party/im3d/im3d.cpp
    third_party/Optical_Flow_SDK_5.0.7/Common/NvOFBase/NvOF.cpp
    third_party/Optical_Flow_SDK_5.0.7/Common/NvOFBase/NvOFD3D12.cpp
    third_party/Optical_Flow_SDK_5.0.7/Common/NvOFBase/NvOFD3DCommon.cpp
)

add_library(d3d12 SHARED
    src/d3d12_proxy.cpp
)

target_compile_features(dxgi PRIVATE cxx_std_20)
target_compile_definitions(dxgi PRIVATE WIN32_LEAN_AND_MEAN NOMINMAX XR_USE_PLATFORM_WIN32 XR_USE_GRAPHICS_API_D3D12)
target_link_libraries(dxgi PRIVATE openxr_loader imgui d3d12 d3dcompiler)
target_include_directories(dxgi PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/third_party/im3d
    ${CMAKE_CURRENT_SOURCE_DIR}/third_party/Optical_Flow_SDK_5.0.7/Common/NvOFBase
    ${CMAKE_CURRENT_SOURCE_DIR}/third_party/Optical_Flow_SDK_5.0.7/NvOFInterface
)

target_compile_features(d3d12 PRIVATE cxx_std_20)
target_compile_definitions(d3d12 PRIVATE WIN32_LEAN_AND_MEAN NOMINMAX)

if(MSVC)
    target_compile_options(dxgi PRIVATE /W4 /permissive- /EHsc)
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
