cmake_minimum_required(VERSION 3.24)
project(CaptionTests LANGUAGES CXX)
enable_testing()
add_executable(caption_tests main.cpp ../../src/Render/Caption.cpp)
target_include_directories(caption_tests PRIVATE ../../include)
target_compile_features(caption_tests PRIVATE cxx_std_20)
target_compile_definitions(caption_tests PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE)
target_compile_options(caption_tests PRIVATE /utf-8)
target_link_libraries(caption_tests PRIVATE d3d12 dxgi d2d1 dwrite)
add_test(NAME caption COMMAND caption_tests)

# The same program against the Rust caption (rust/crates/cvr-gpu through cvr_rust.lib), and both traces compared bit for
# bit.
option(CVR_CAPTION_TESTS_RUST "Also run the test against the Rust caption" ON)
if(CVR_CAPTION_TESTS_RUST)
    include(../../cmake/RustStaticlib.cmake)
    add_executable(caption_tests_rust main.cpp)
    target_include_directories(caption_tests_rust PRIVATE ../../include)
    target_compile_features(caption_tests_rust PRIVATE cxx_std_20)
    target_compile_definitions(caption_tests_rust PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1)
    target_compile_options(caption_tests_rust PRIVATE /utf-8)
    target_link_libraries(caption_tests_rust PRIVATE d3d12 dxgi d2d1 dwrite)
    cvr_link_rust(caption_tests_rust)
    add_test(NAME caption_rust COMMAND caption_tests_rust)
    add_test(NAME caption_parity
             COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:caption_tests> -DRUST=$<TARGET_FILE:caption_tests_rust>
                     -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
endif()
