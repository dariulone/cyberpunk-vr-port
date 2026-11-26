cmake_minimum_required(VERSION 3.24)
project(FramegenOverlayTests LANGUAGES CXX)
enable_testing()
add_executable(framegen_overlay_tests main.cpp ../../src/Framegen/StatsOverlayDraw.cpp)
target_include_directories(framegen_overlay_tests PRIVATE ../../include)
target_compile_features(framegen_overlay_tests PRIVATE cxx_std_20)
target_compile_definitions(framegen_overlay_tests PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE)
target_compile_options(framegen_overlay_tests PRIVATE /utf-8)
add_test(NAME framegen_overlay COMMAND framegen_overlay_tests)

# The same program against the Rust panel (rust/crates/cvr-gpu through cvr_rust.lib), and both traces compared pixel for
# pixel.
option(CVR_FRAMEGEN_OVERLAY_TESTS_RUST "Also run the test against the Rust panel" ON)
if(CVR_FRAMEGEN_OVERLAY_TESTS_RUST)
    include(../../cmake/RustStaticlib.cmake)
    add_executable(framegen_overlay_tests_rust main.cpp)
    target_include_directories(framegen_overlay_tests_rust PRIVATE ../../include)
    target_compile_features(framegen_overlay_tests_rust PRIVATE cxx_std_20)
    target_compile_definitions(framegen_overlay_tests_rust PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1)
    target_compile_options(framegen_overlay_tests_rust PRIVATE /utf-8)
    cvr_link_rust(framegen_overlay_tests_rust)
    add_test(NAME framegen_overlay_rust COMMAND framegen_overlay_tests_rust)
    add_test(NAME framegen_overlay_parity
             COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:framegen_overlay_tests> -DRUST=$<TARGET_FILE:framegen_overlay_tests_rust>
                     -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
endif()
