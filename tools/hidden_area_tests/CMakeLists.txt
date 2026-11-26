cmake_minimum_required(VERSION 3.20)
project(HiddenAreaTests LANGUAGES CXX)
enable_testing()
add_executable(hidden_area_settings settings.cpp ../../src/Render/HiddenAreaSettings.cpp)
target_include_directories(hidden_area_settings PRIVATE ../../include)
target_compile_features(hidden_area_settings PRIVATE cxx_std_20)
target_compile_definitions(hidden_area_settings PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
add_test(NAME hidden_area_settings COMMAND hidden_area_settings)
add_executable(hidden_area_gpu gpu.cpp ../../src/Render/HiddenAreaSettings.cpp)
target_include_directories(hidden_area_gpu PRIVATE ../../include)
target_compile_features(hidden_area_gpu PRIVATE cxx_std_20)
target_compile_definitions(hidden_area_gpu PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
target_link_libraries(hidden_area_gpu PRIVATE d3d12 dxgi)
add_test(NAME hidden_area_gpu COMMAND hidden_area_gpu batched)
add_executable(hidden_area_dxil_patch dxil_patch.cpp ../../src/Render/DxilPatch.cpp)
target_include_directories(hidden_area_dxil_patch PRIVATE ../../include)
target_compile_features(hidden_area_dxil_patch PRIVATE cxx_std_20)
target_compile_definitions(hidden_area_dxil_patch PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
target_link_libraries(hidden_area_dxil_patch PRIVATE d3d12 dxgi)
add_test(NAME hidden_area_dxil_patch COMMAND hidden_area_dxil_patch)
set(CVR_LIFETIME_FIXTURE "${CMAKE_CURRENT_SOURCE_DIR}/../../src/Shaders/view_dir/068561d7611c882c.dxil")
# HiddenAreaShaders.cpp reaches EngineStereo.hpp, which includes OpenXR.
set(CVR_OPENXR_INCLUDE "${CMAKE_CURRENT_SOURCE_DIR}/../../build/_deps/openxr-src/include")
add_executable(hidden_area_pipeline_lifetime pipeline_lifetime.cpp ../../src/Render/HiddenAreaShaders.cpp ../../src/Render/DxilPatch.cpp)
target_include_directories(hidden_area_pipeline_lifetime PRIVATE ../../include ${CVR_OPENXR_INCLUDE})
target_compile_features(hidden_area_pipeline_lifetime PRIVATE cxx_std_20)
target_compile_definitions(hidden_area_pipeline_lifetime PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN CVR_FIXTURE="${CVR_LIFETIME_FIXTURE}")
add_test(NAME hidden_area_pipeline_lifetime COMMAND hidden_area_pipeline_lifetime "${CVR_LIFETIME_FIXTURE}")

# The geometry and the DXIL patches against Rust (rust/crates/cvr-render through cvr_rust.lib), as the DLL calls them, and
# the traces of both builds compared bit for bit.
option(CVR_HIDDEN_AREA_TESTS_RUST "Also run the settings test against the Rust geometry" ON)
if(CVR_HIDDEN_AREA_TESTS_RUST)
    include(../../cmake/RustStaticlib.cmake)
    add_executable(hidden_area_settings_rust settings.cpp ../rust_test_support/plugin_stubs.cpp)
    target_include_directories(hidden_area_settings_rust PRIVATE ../../include)
    target_compile_features(hidden_area_settings_rust PRIVATE cxx_std_20)
    target_compile_definitions(hidden_area_settings_rust PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1)
    cvr_link_rust(hidden_area_settings_rust)
    add_test(NAME hidden_area_settings_rust COMMAND hidden_area_settings_rust)
    add_executable(hidden_area_dxil_patch_rust dxil_patch.cpp ../rust_test_support/plugin_stubs.cpp)
    target_include_directories(hidden_area_dxil_patch_rust PRIVATE ../../include)
    target_compile_features(hidden_area_dxil_patch_rust PRIVATE cxx_std_20)
    target_compile_definitions(hidden_area_dxil_patch_rust PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1)
    target_link_libraries(hidden_area_dxil_patch_rust PRIVATE d3d12 dxgi)
    cvr_link_rust(hidden_area_dxil_patch_rust)
    add_test(NAME hidden_area_dxil_patch_rust COMMAND hidden_area_dxil_patch_rust)
    add_test(NAME hidden_area_dxil_parity
             COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:hidden_area_dxil_patch> -DRUST=$<TARGET_FILE:hidden_area_dxil_patch_rust>
                     -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
    # The twins and the census against Rust: HiddenAreaShaders.cpp keeps only the exports and hands Rust their addresses.
    add_executable(hidden_area_pipeline_lifetime_rust pipeline_lifetime.cpp shader_exports_rust.cpp ../rust_test_support/plugin_stubs.cpp)
    target_include_directories(hidden_area_pipeline_lifetime_rust PRIVATE ../../include ${CVR_OPENXR_INCLUDE})
    target_compile_features(hidden_area_pipeline_lifetime_rust PRIVATE cxx_std_20)
    target_compile_definitions(hidden_area_pipeline_lifetime_rust PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1
                               CVR_FIXTURE="${CVR_LIFETIME_FIXTURE}")
    cvr_link_rust(hidden_area_pipeline_lifetime_rust)
    add_test(NAME hidden_area_pipeline_lifetime_rust COMMAND hidden_area_pipeline_lifetime_rust "${CVR_LIFETIME_FIXTURE}")
    add_test(NAME hidden_area_pipeline_parity
             COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:hidden_area_pipeline_lifetime>
                     -DRUST=$<TARGET_FILE:hidden_area_pipeline_lifetime_rust>
                     -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
    add_test(NAME hidden_area_parity
             COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:hidden_area_settings> -DRUST=$<TARGET_FILE:hidden_area_settings_rust>
                     -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
endif()
