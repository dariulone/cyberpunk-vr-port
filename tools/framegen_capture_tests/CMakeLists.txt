cmake_minimum_required(VERSION 3.24)
project(FramegenCaptureTests LANGUAGES CXX)
enable_testing()
# Both builds take the plugin's folders from Rust (src/Core/Paths.cpp calls cvr_paths_dir_w), so both link cvr_rust.lib;
# the C++ build runs src/Framegen/FrameCapture.cpp's own capture, the Rust build its facade over
# rust/crates/cvr-gpu/src/frame_capture.rs.
include(../../cmake/RustStaticlib.cmake)
foreach(variant cpp rust)
    set(target framegen_capture_tests_${variant})
    add_executable(${target} main.cpp ../../src/Framegen/FrameCapture.cpp ../../src/Core/Paths.cpp)
    target_include_directories(${target} PRIVATE ../../include)
    target_compile_features(${target} PRIVATE cxx_std_20)
    target_compile_definitions(${target} PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE)
    target_compile_options(${target} PRIVATE /utf-8)
    target_link_libraries(${target} PRIVATE d3d12 dxgi)
    cvr_link_rust(${target})
    add_test(NAME framegen_capture_${variant} COMMAND ${target})
endforeach()
target_compile_definitions(framegen_capture_tests_rust PRIVATE CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1)
add_test(NAME framegen_capture_parity
         COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:framegen_capture_tests_cpp> -DRUST=$<TARGET_FILE:framegen_capture_tests_rust>
                 -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
# The two builds write to the same log folder: one at a time.
set_tests_properties(framegen_capture_cpp framegen_capture_rust framegen_capture_parity PROPERTIES RUN_SERIAL TRUE)
