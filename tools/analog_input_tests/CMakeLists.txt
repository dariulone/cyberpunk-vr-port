cmake_minimum_required(VERSION 3.24)
project(AnalogInputTests LANGUAGES CXX)
enable_testing()
set(CVR_INPUT_CASES curve ranges gestures invalid default keypad keypad_cursor weapon_trigger)

# The C++ reference implementation.
add_executable(analog_input_tests main.cpp)
target_include_directories(analog_input_tests PRIVATE ../../include ../../externals/RED4ext.SDK/include)
target_compile_features(analog_input_tests PRIVATE cxx_std_20)
foreach(case ${CVR_INPUT_CASES})
    add_test(NAME analog_input_${case} COMMAND analog_input_tests ${case})
endforeach()

# The same groups against the Rust implementation (rust/crates/cvr-input through cvr_rust.lib), plus parity: both
# implementations on the same dense inputs, compared bit for bit (docs/rust-migration-plan.md, stage 1).
option(CVR_INPUT_TESTS_RUST "Also build the Rust input implementation and compare it with C++" ON)
if(CVR_INPUT_TESTS_RUST)
    include(../../cmake/RustStaticlib.cmake)
    add_executable(analog_input_tests_rust main.cpp)
    target_compile_features(analog_input_tests_rust PRIVATE cxx_std_20)
    cvr_link_rust(analog_input_tests_rust)
    foreach(case ${CVR_INPUT_CASES} parity)
        add_test(NAME analog_input_rust_${case} COMMAND analog_input_tests_rust ${case})
    endforeach()
endif()
