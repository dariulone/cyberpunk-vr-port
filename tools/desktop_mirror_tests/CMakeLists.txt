cmake_minimum_required(VERSION 3.24)
project(DesktopMirrorTests LANGUAGES CXX)
enable_testing()
add_executable(desktop_mirror_tests main.cpp ../../src/Render/DesktopMirror.cpp ../../src/Render/ColorBlit.cpp)
target_include_directories(desktop_mirror_tests PRIVATE ../../include)
target_compile_features(desktop_mirror_tests PRIVATE cxx_std_20)
target_compile_definitions(desktop_mirror_tests PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
target_link_libraries(desktop_mirror_tests PRIVATE d3d12 dxgi d3dcompiler user32)
add_test(NAME desktop_mirror COMMAND desktop_mirror_tests)

# The same program against the Rust mirror (rust/crates/cvr-gpu through cvr_rust.lib): the test hands it the launcher's
# sizes itself (the plugin does it from Rust as it loads; DesktopMirror.cpp is empty in the Rust build). Both traces
# compared bit for bit.
option(CVR_DESKTOP_MIRROR_TESTS_RUST "Also run the test against the Rust mirror" ON)
if(CVR_DESKTOP_MIRROR_TESTS_RUST)
    include(../../cmake/RustStaticlib.cmake)
    add_executable(desktop_mirror_tests_rust main.cpp ../../src/Render/DesktopMirror.cpp)
    target_include_directories(desktop_mirror_tests_rust PRIVATE ../../include)
    target_compile_features(desktop_mirror_tests_rust PRIVATE cxx_std_20)
    target_compile_definitions(desktop_mirror_tests_rust PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1)
    target_link_libraries(desktop_mirror_tests_rust PRIVATE d3d12 dxgi d3dcompiler user32)
    cvr_link_rust(desktop_mirror_tests_rust)
    add_test(NAME desktop_mirror_rust COMMAND desktop_mirror_tests_rust)
    add_test(NAME desktop_mirror_parity
             COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:desktop_mirror_tests> -DRUST=$<TARGET_FILE:desktop_mirror_tests_rust>
                     -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
endif()
