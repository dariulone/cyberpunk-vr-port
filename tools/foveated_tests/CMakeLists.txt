cmake_minimum_required(VERSION 3.24)
project(FoveatedTests LANGUAGES C CXX)
enable_testing()

set(CMAKE_RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin")

get_filename_component(CVR_REPO_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../.." ABSOLUTE)
set(CVR_OPENXR_SOURCE_DIR "${CVR_REPO_ROOT}/build/_deps/openxr-src" CACHE PATH "Path to OpenXR-SDK source tree")
if(NOT EXISTS "${CVR_OPENXR_SOURCE_DIR}/include/openxr/openxr.h")
    message(FATAL_ERROR "OpenXR sources not found at CVR_OPENXR_SOURCE_DIR=${CVR_OPENXR_SOURCE_DIR}; pass -DCVR_OPENXR_SOURCE_DIR=...")
endif()

add_library(foveated_json STATIC
    "${CVR_OPENXR_SOURCE_DIR}/src/external/jsoncpp/src/lib_json/json_reader.cpp"
    "${CVR_OPENXR_SOURCE_DIR}/src/external/jsoncpp/src/lib_json/json_value.cpp"
    "${CVR_OPENXR_SOURCE_DIR}/src/external/jsoncpp/src/lib_json/json_writer.cpp"
)
target_include_directories(foveated_json
    PUBLIC "${CVR_OPENXR_SOURCE_DIR}/src/external/jsoncpp/include"
    PRIVATE "${CVR_OPENXR_SOURCE_DIR}/src/external/jsoncpp/src/lib_json"
)
target_compile_features(foveated_json PRIVATE cxx_std_20)
target_compile_definitions(foveated_json PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN _CRT_SECURE_NO_WARNINGS=1)

add_library(foveated_minhook STATIC
    ../../externals/minhook/src/buffer.c
    ../../externals/minhook/src/hook.c
    ../../externals/minhook/src/trampoline.c
    ../../externals/minhook/src/hde/hde64.c
)
target_include_directories(foveated_minhook
    PUBLIC ../../externals/minhook/include
    PRIVATE ../../externals/minhook/src ../../externals/minhook/src/hde
)
target_compile_definitions(foveated_minhook PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
if(MSVC)
    target_compile_options(foveated_minhook PRIVATE /W0)
endif()

add_executable(foveated_tests main.cpp ../../src/Render/FoveatedDlssSettings.cpp)
target_include_directories(foveated_tests PRIVATE ../../include)
target_compile_features(foveated_tests PRIVATE cxx_std_20)
target_compile_definitions(foveated_tests PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
target_compile_options(foveated_tests PRIVATE $<$<AND:$<BOOL:${MSVC}>,$<COMPILE_LANGUAGE:CXX>>:/W4>)
add_test(NAME foveated_settings COMMAND foveated_tests)

add_executable(foveated_ngx ngx.cpp)
target_include_directories(foveated_ngx PRIVATE ../../include)
target_compile_features(foveated_ngx PRIVATE cxx_std_20)
target_compile_definitions(foveated_ngx PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
target_compile_options(foveated_ngx PRIVATE $<$<AND:$<BOOL:${MSVC}>,$<COMPILE_LANGUAGE:CXX>>:/W4>)
add_test(NAME foveated_ngx COMMAND foveated_ngx)

add_executable(foveated_gpu gpu.cpp ../../src/Render/FoveatedGpu.cpp ../../src/Render/CommandResources.cpp)
target_include_directories(foveated_gpu PRIVATE ../../include)
target_compile_features(foveated_gpu PRIVATE cxx_std_20)
target_compile_definitions(foveated_gpu PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
target_compile_options(foveated_gpu PRIVATE $<$<AND:$<BOOL:${MSVC}>,$<COMPILE_LANGUAGE:CXX>>:/W4>)
target_link_libraries(foveated_gpu PRIVATE d3d12 dxgi d3dcompiler)
add_test(NAME foveated_gpu COMMAND foveated_gpu)

add_executable(foveated_eyegaze eyegaze.cpp ../../src/Runtimes/EyeGaze.cpp)
target_include_directories(foveated_eyegaze PRIVATE ../../include "${CVR_OPENXR_SOURCE_DIR}/include")
target_compile_features(foveated_eyegaze PRIVATE cxx_std_20)
target_compile_definitions(foveated_eyegaze PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
target_compile_options(foveated_eyegaze PRIVATE $<$<AND:$<BOOL:${MSVC}>,$<COMPILE_LANGUAGE:CXX>>:/W4>)
add_test(NAME foveated_eyegaze COMMAND foveated_eyegaze)

add_library(nvngx_dlss SHARED mock_nvngx_dlss.cpp mock_nvngx_dlss.h)
set_target_properties(nvngx_dlss PROPERTIES
    PREFIX ""
    OUTPUT_NAME "nvngx_dlss"
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin"
)
target_include_directories(nvngx_dlss PRIVATE ../../include)
target_compile_features(nvngx_dlss PRIVATE cxx_std_20)
target_compile_definitions(nvngx_dlss PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN MOCK_NVNGX_EXPORTS)
target_compile_options(nvngx_dlss PRIVATE $<$<AND:$<BOOL:${MSVC}>,$<COMPILE_LANGUAGE:CXX>>:/W4>)
target_link_libraries(nvngx_dlss PRIVATE d3d12)

add_executable(foveated_process
    process.cpp
    ../../src/Render/FoveatedDlss.cpp
    ../../src/Render/FoveatedDlssSettings.cpp
    ../../src/Render/FoveatedGpu.cpp
    ../../src/Render/CommandResources.cpp
    ../../src/Runtimes/EyeGaze.cpp
    ../../src/Hooks/FarInlineHook.cpp
)
set_target_properties(foveated_process PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin"
)
target_include_directories(foveated_process PRIVATE
    ../../include
    ../../externals/streamline/include
    ../../externals/minhook/src/hde
    "${CVR_OPENXR_SOURCE_DIR}/include"
)
target_compile_features(foveated_process PRIVATE cxx_std_20)
target_compile_definitions(foveated_process PRIVATE
    NOMINMAX
    WIN32_LEAN_AND_MEAN
    XR_USE_PLATFORM_WIN32
    XR_USE_GRAPHICS_API_D3D12
    _CRT_SECURE_NO_WARNINGS=1
)
target_compile_options(foveated_process PRIVATE $<$<AND:$<BOOL:${MSVC}>,$<COMPILE_LANGUAGE:CXX>>:/W4>)
target_link_libraries(foveated_process PRIVATE nvngx_dlss foveated_json foveated_minhook d3d12 dxgi d3dcompiler)
add_test(NAME foveated_process COMMAND foveated_process)

set_tests_properties(foveated_gpu foveated_process PROPERTIES SKIP_RETURN_CODE 77)
