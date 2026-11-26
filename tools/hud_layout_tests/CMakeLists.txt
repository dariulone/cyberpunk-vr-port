cmake_minimum_required(VERSION 3.24)
project(HudLayoutTests LANGUAGES CXX)
enable_testing()
add_executable(hud_layout_tests main.cpp ../../src/Runtimes/HudLayout.cpp)
target_include_directories(hud_layout_tests PRIVATE ../../include)
target_compile_features(hud_layout_tests PRIVATE cxx_std_20)
target_compile_definitions(hud_layout_tests PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN _CRT_SECURE_NO_WARNINGS)
target_compile_options(hud_layout_tests PRIVATE /utf-8)
add_test(NAME hud_layout COMMAND hud_layout_tests)

# The same program against the Rust layout (rust/crates/cvr-settings through cvr_rust.lib), and both traces compared byte
# for byte.
option(CVR_HUD_LAYOUT_TESTS_RUST "Also run the test against the Rust layout" ON)
if(CVR_HUD_LAYOUT_TESTS_RUST)
    include(../../cmake/RustStaticlib.cmake)
    add_executable(hud_layout_tests_rust main.cpp ../../src/Runtimes/HudLayout.cpp)
    target_include_directories(hud_layout_tests_rust PRIVATE ../../include)
    target_compile_features(hud_layout_tests_rust PRIVATE cxx_std_20)
    target_compile_definitions(hud_layout_tests_rust PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN _CRT_SECURE_NO_WARNINGS CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1)
    target_compile_options(hud_layout_tests_rust PRIVATE /utf-8)
    cvr_link_rust(hud_layout_tests_rust)
    add_test(NAME hud_layout_rust COMMAND hud_layout_tests_rust)
    add_test(NAME hud_layout_parity
             COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:hud_layout_tests> -DRUST=$<TARGET_FILE:hud_layout_tests_rust>
                     -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
endif()
