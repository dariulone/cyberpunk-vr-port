cmake_minimum_required(VERSION 3.24)
project(ViewTwinTests LANGUAGES CXX)
enable_testing()
# The single-pass shader twins (src/Render/ViewTwinIr.cpp, ViewTwinCompiler.cpp and their Rust port) over a corpus of
# original shaders (<fnv1a>.dxil: a single-pass census dump, see tools/vr_bench/README.md). The DXC folder holds
# dxcompiler.dll and dxil.dll, as beside the plugin.
set(CVR_VIEW_TWIN_DXC "C:/Program Files (x86)/Windows Kits/10/bin/10.0.26100.0/x64" CACHE PATH "Folder with dxcompiler.dll and dxil.dll")
set(CVR_VIEW_TWIN_SHADERS "${CMAKE_CURRENT_SOURCE_DIR}/../../build/vr-bench/20261105-104840-sp-layout/cvr_shaders" CACHE PATH
    "Folder of original .dxil shaders")
file(TO_NATIVE_PATH "${CVR_VIEW_TWIN_DXC}" view_twin_dxc)
file(TO_NATIVE_PATH "${CVR_VIEW_TWIN_SHADERS}" view_twin_shaders)
string(REPLACE "\\" "\\\\" view_twin_dxc "${view_twin_dxc}")
string(REPLACE "\\" "\\\\" view_twin_shaders "${view_twin_shaders}")

function(view_twin_target name)
    target_include_directories(${name} PRIVATE ../../include)
    target_compile_features(${name} PRIVATE cxx_std_20)
    target_compile_definitions(${name} PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE
        "CVR_VIEW_TWIN_DXC=L\"${view_twin_dxc}\"" "CVR_VIEW_TWIN_SHADERS=L\"${view_twin_shaders}\"")
    target_link_libraries(${name} PRIVATE ole32)
endfunction()

add_executable(view_twin_tests main.cpp ../../src/Render/ViewTwinIr.cpp ../../src/Render/ViewTwinCompiler.cpp)
view_twin_target(view_twin_tests)
if(EXISTS "${CVR_VIEW_TWIN_SHADERS}")
    add_test(NAME view_twin COMMAND view_twin_tests trace)
endif()

# The same program against the Rust rewrite and compiler (rust/crates/cvr-render through cvr_rust.lib).
option(CVR_VIEW_TWIN_TESTS_RUST "Also run the trace against the Rust port" ON)
if(CVR_VIEW_TWIN_TESTS_RUST)
    include(../../cmake/RustStaticlib.cmake)
    add_executable(view_twin_tests_rust main.cpp ../rust_test_support/plugin_stubs.cpp)
    view_twin_target(view_twin_tests_rust)
    target_compile_definitions(view_twin_tests_rust PRIVATE CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1)
    cvr_link_rust(view_twin_tests_rust)
    if(EXISTS "${CVR_VIEW_TWIN_SHADERS}")
        add_test(NAME view_twin_rust COMMAND view_twin_tests_rust trace)
        add_test(NAME view_twin_parity
                 COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:view_twin_tests> -DRUST=$<TARGET_FILE:view_twin_tests_rust>
                         -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
    endif()
endif()
