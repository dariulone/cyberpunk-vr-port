# cmake -DCPP=<C++ test exe> -DRUST=<Rust test exe> [-DCASE=trace] -P ParityTrace.cmake
# Runs the trace case of both builds of a test suite and fails unless they print the same output: the C++ reference and
# its Rust port agreeing bit for bit on the same inputs (docs/rust-migration-plan.md).
if(NOT DEFINED CASE)
    set(CASE trace)
endif()
execute_process(COMMAND ${CPP} ${CASE} OUTPUT_VARIABLE cpp_out RESULT_VARIABLE cpp_rc)
execute_process(COMMAND ${RUST} ${CASE} OUTPUT_VARIABLE rust_out RESULT_VARIABLE rust_rc)
string(STRIP "${cpp_out}" cpp_out)
string(STRIP "${rust_out}" rust_out)
message(STATUS "C++:  ${cpp_out}")
message(STATUS "Rust: ${rust_out}")
if(NOT cpp_rc EQUAL 0 OR NOT rust_rc EQUAL 0)
    message(FATAL_ERROR "a trace run failed (C++ ${cpp_rc}, Rust ${rust_rc})")
endif()
if(NOT cpp_out STREQUAL rust_out OR cpp_out STREQUAL "")
    message(FATAL_ERROR "C++ and Rust differ")
endif()
