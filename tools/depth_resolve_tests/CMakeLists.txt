cmake_minimum_required(VERSION 3.24)
project(DepthResolveTests LANGUAGES CXX)
enable_testing()
add_executable(depth_resolve_tests main.cpp ../../src/Render/DepthResolve.cpp)
target_include_directories(depth_resolve_tests PRIVATE ../../include)
target_compile_features(depth_resolve_tests PRIVATE cxx_std_20)
target_compile_definitions(depth_resolve_tests PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
target_link_libraries(depth_resolve_tests PRIVATE d3d12 dxgi d3dcompiler)
add_test(NAME depth_resolve COMMAND depth_resolve_tests)

# The same program against the Rust resolve (rust/crates/cvr-gpu through cvr_rust.lib), and both traces compared bit
# for bit.
option(CVR_DEPTH_RESOLVE_TESTS_RUST "Also run the test against the Rust resolve" ON)
if(CVR_DEPTH_RESOLVE_TESTS_RUST)
    include(../../cmake/RustStaticlib.cmake)
    add_executable(depth_resolve_tests_rust main.cpp)
    target_include_directories(depth_resolve_tests_rust PRIVATE ../../include)
    target_compile_features(depth_resolve_tests_rust PRIVATE cxx_std_20)
    target_compile_definitions(depth_resolve_tests_rust PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1)
    target_link_libraries(depth_resolve_tests_rust PRIVATE d3d12 dxgi d3dcompiler)
    cvr_link_rust(depth_resolve_tests_rust)
    add_test(NAME depth_resolve_rust COMMAND depth_resolve_tests_rust)
    add_test(NAME depth_resolve_parity
             COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:depth_resolve_tests> -DRUST=$<TARGET_FILE:depth_resolve_tests_rust>
                     -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
endif()
