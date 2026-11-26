cmake_minimum_required(VERSION 3.24)
project(SpaceQuadTests LANGUAGES CXX)
enable_testing()
set(CVR_OPENXR_INCLUDE "${CMAKE_CURRENT_SOURCE_DIR}/../../build/_deps/openxr-src/include" CACHE PATH "OpenXR-SDK headers")
add_executable(space_quad_tests main.cpp ../../src/Render/SpaceQuad.cpp)
target_include_directories(space_quad_tests PRIVATE ../../include ${CVR_OPENXR_INCLUDE})
target_compile_features(space_quad_tests PRIVATE cxx_std_20)
target_compile_definitions(space_quad_tests PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
target_link_libraries(space_quad_tests PRIVATE d3d12 dxgi d3dcompiler)
add_test(NAME space_quad COMMAND space_quad_tests)

# The same program against the Rust drawer (rust/crates/cvr-gpu through cvr_rust.lib), and both traces compared bit for
# bit.
option(CVR_SPACE_QUAD_TESTS_RUST "Also run the test against the Rust drawer" ON)
if(CVR_SPACE_QUAD_TESTS_RUST)
    include(../../cmake/RustStaticlib.cmake)
    add_executable(space_quad_tests_rust main.cpp)
    target_include_directories(space_quad_tests_rust PRIVATE ../../include ${CVR_OPENXR_INCLUDE})
    target_compile_features(space_quad_tests_rust PRIVATE cxx_std_20)
    target_compile_definitions(space_quad_tests_rust PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1)
    target_link_libraries(space_quad_tests_rust PRIVATE d3d12 dxgi d3dcompiler)
    cvr_link_rust(space_quad_tests_rust)
    add_test(NAME space_quad_rust COMMAND space_quad_tests_rust)
    add_test(NAME space_quad_parity
             COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:space_quad_tests> -DRUST=$<TARGET_FILE:space_quad_tests_rust>
                     -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
endif()
