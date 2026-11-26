cmake_minimum_required(VERSION 3.24)
project(PipelineStreamTests LANGUAGES CXX)
enable_testing()
# Pipeline state streams: src/Render/ViewInstancedPipeline.cpp as the C++ reference.
add_executable(pipeline_stream_tests main.cpp ../../src/Render/ViewInstancedPipeline.cpp)
target_include_directories(pipeline_stream_tests PRIVATE ../../include)
target_compile_features(pipeline_stream_tests PRIVATE cxx_std_20)
target_compile_definitions(pipeline_stream_tests PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN _CRT_SECURE_NO_WARNINGS)
add_test(NAME pipeline_stream COMMAND pipeline_stream_tests trace)

# The same program against the Rust port (rust/crates/cvr-render/src/pipeline_stream.rs through the same file's
# forwarders and cvr_rust.lib), and both traces compared byte for byte.
option(CVR_PIPELINE_STREAM_TESTS_RUST "Also run the test against the Rust streams" ON)
if(CVR_PIPELINE_STREAM_TESTS_RUST)
    include(../../cmake/RustStaticlib.cmake)
    add_executable(pipeline_stream_tests_rust main.cpp ../../src/Render/ViewInstancedPipeline.cpp)
    target_include_directories(pipeline_stream_tests_rust PRIVATE ../../include)
    target_compile_features(pipeline_stream_tests_rust PRIVATE cxx_std_20)
    target_compile_definitions(pipeline_stream_tests_rust PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN _CRT_SECURE_NO_WARNINGS CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1)
    cvr_link_rust(pipeline_stream_tests_rust)
    add_test(NAME pipeline_stream_rust COMMAND pipeline_stream_tests_rust trace)
    add_test(NAME pipeline_stream_parity
             COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:pipeline_stream_tests> -DRUST=$<TARGET_FILE:pipeline_stream_tests_rust>
                     -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
endif()
