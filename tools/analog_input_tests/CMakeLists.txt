cmake_minimum_required(VERSION 3.24)
project(AnalogInputTests LANGUAGES CXX)
enable_testing()
add_executable(analog_input_tests main.cpp)
target_include_directories(analog_input_tests PRIVATE ../../include ../../externals/RED4ext.SDK/include)
target_compile_features(analog_input_tests PRIVATE cxx_std_20)
foreach(case curve ranges gestures crouch_pulse invalid default keypad keypad_cursor weapon_trigger snap_travel snap_boundary snap_invalid snap_hysteresis snap_native)
    add_test(NAME analog_input_${case} COMMAND analog_input_tests ${case})
endforeach()
# The real Lua bridge is optional when the local test interpreter has Lupa.
find_package(Python3 COMPONENTS Interpreter QUIET)
if(Python3_Interpreter_FOUND)
    execute_process(COMMAND "${Python3_EXECUTABLE}" -I -c "import lupa"
        RESULT_VARIABLE INPUT_TEST_HAS_LUPA OUTPUT_QUIET ERROR_QUIET)
    if(INPUT_TEST_HAS_LUPA EQUAL 0)
        add_test(NAME analog_input_vrik_bridge COMMAND "${Python3_EXECUTABLE}" -I -X utf8
            "${CMAKE_CURRENT_SOURCE_DIR}/vrik_input_bridge.py")
    endif()
endif()
