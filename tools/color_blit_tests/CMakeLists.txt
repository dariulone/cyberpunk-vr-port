cmake_minimum_required(VERSION 3.24)
project(ColorBlitTests LANGUAGES CXX)
enable_testing()
add_executable(color_blit_tests main.cpp ../../src/Render/ColorBlit.cpp)
target_include_directories(color_blit_tests PRIVATE ../../include)
target_compile_features(color_blit_tests PRIVATE cxx_std_20)
target_compile_definitions(color_blit_tests PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
target_link_libraries(color_blit_tests PRIVATE d3d12 dxgi d3dcompiler)
add_test(NAME color_blit COMMAND color_blit_tests)

# The same program against the Rust blitter (rust/crates/cvr-gpu through cvr_rust.lib), and both traces compared bit for
# bit.
option(CVR_COLOR_BLIT_TESTS_RUST "Also run the test against the Rust blitter" ON)
if(CVR_COLOR_BLIT_TESTS_RUST)
    include(../../cmake/RustStaticlib.cmake)
    add_executable(color_blit_tests_rust main.cpp)
    target_include_directories(color_blit_tests_rust PRIVATE ../../include)
    target_compile_features(color_blit_tests_rust PRIVATE cxx_std_20)
    target_compile_definitions(color_blit_tests_rust PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1)
    target_link_libraries(color_blit_tests_rust PRIVATE d3d12 dxgi d3dcompiler)
    cvr_link_rust(color_blit_tests_rust)
    add_test(NAME color_blit_rust COMMAND color_blit_tests_rust)
    add_test(NAME color_blit_parity
             COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:color_blit_tests> -DRUST=$<TARGET_FILE:color_blit_tests_rust>
                     -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
endif()
