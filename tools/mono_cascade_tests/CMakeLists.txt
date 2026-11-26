cmake_minimum_required(VERSION 3.20)
project(MonoCascadeTests LANGUAGES CXX)
enable_testing()
add_executable(cascade_policy policy.cpp)
target_include_directories(cascade_policy PRIVATE ../../include)
target_compile_features(cascade_policy PRIVATE cxx_std_20)
add_test(NAME cascade_policy COMMAND cascade_policy)
add_executable(cascade_gpu gpu.cpp ../../src/Render/MonoCascades.cpp ../../src/Render/MonoCascadeImage.cpp ../../src/Render/CommandResources.cpp ../../src/Render/SceneGpuProfile.cpp)
# MonoCascades.cpp reaches EngineStereo.hpp, which includes OpenXR.
set(CVR_OPENXR_INCLUDE "${CMAKE_CURRENT_SOURCE_DIR}/../../build/_deps/openxr-src/include")
target_include_directories(cascade_gpu PRIVATE ../../include ${CVR_OPENXR_INCLUDE})
target_compile_features(cascade_gpu PRIVATE cxx_std_20)
target_compile_definitions(cascade_gpu PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
target_link_libraries(cascade_gpu PRIVATE d3d12 dxgi)
add_test(NAME cascade_gpu COMMAND cascade_gpu)

# The cascades against Rust (rust/crates/cvr-gpu through cvr_rust.lib): MonoCascades.cpp keeps only the exports and hands
# them to Rust. Both traces compared bit for bit.
option(CVR_CASCADE_TESTS_RUST "Also run the GPU test against the Rust cascades" ON)
if(CVR_CASCADE_TESTS_RUST)
    include(../../cmake/RustStaticlib.cmake)
    add_executable(cascade_gpu_rust gpu.cpp ../../src/Render/MonoCascades.cpp)
    target_include_directories(cascade_gpu_rust PRIVATE ../../include ${CVR_OPENXR_INCLUDE})
    target_compile_features(cascade_gpu_rust PRIVATE cxx_std_20)
    target_compile_definitions(cascade_gpu_rust PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1)
    target_link_libraries(cascade_gpu_rust PRIVATE d3d12 dxgi)
    cvr_link_rust(cascade_gpu_rust)
    add_test(NAME cascade_gpu_rust COMMAND cascade_gpu_rust)
    add_test(NAME cascade_gpu_parity
             COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:cascade_gpu> -DRUST=$<TARGET_FILE:cascade_gpu_rust>
                     -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
endif()
