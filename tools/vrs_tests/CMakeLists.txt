cmake_minimum_required(VERSION 3.20)
project(FixedVrsTests LANGUAGES CXX)
enable_testing()

add_executable(vrs_settings settings.cpp ../../src/Render/FixedVrsSettings.cpp)
target_include_directories(vrs_settings PRIVATE ../../include)
target_compile_features(vrs_settings PRIVATE cxx_std_20)
target_compile_definitions(vrs_settings PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
add_test(NAME vrs_settings COMMAND vrs_settings)

add_executable(vrs_gpu gpu.cpp ../../src/Render/FixedVrs.cpp ../../src/Render/FixedVrsSettings.cpp ../../src/Render/ViewInstancedPipeline.cpp ../../src/Render/CommandResources.cpp)
target_include_directories(vrs_gpu PRIVATE ../../include)
target_compile_features(vrs_gpu PRIVATE cxx_std_20)
target_compile_definitions(vrs_gpu PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
target_link_libraries(vrs_gpu PRIVATE d3d12 dxgi d3dcompiler)
add_test(NAME vrs_gpu COMMAND vrs_gpu)

add_executable(vrs_single_pass_gpu single_pass_gpu.cpp ../../src/Stereo/SinglePass.cpp ../../src/Render/FixedVrs.cpp ../../src/Render/FixedVrsSettings.cpp ../../src/Render/ViewInstancedPipeline.cpp ../../src/Render/CommandResources.cpp)
target_include_directories(vrs_single_pass_gpu PRIVATE ../../include)
target_compile_features(vrs_single_pass_gpu PRIVATE cxx_std_20)
target_compile_definitions(vrs_single_pass_gpu PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
target_link_libraries(vrs_single_pass_gpu PRIVATE d3d12 dxgi d3dcompiler)
add_test(NAME vrs_single_pass_gpu COMMAND vrs_single_pass_gpu)

file(GLOB VRS_SDK_DXC_HINTS "C:/Program Files (x86)/Windows Kits/10/bin/*/x64")
list(SORT VRS_SDK_DXC_HINTS COMPARE NATURAL)
list(REVERSE VRS_SDK_DXC_HINTS)
find_program(VRS_DXC dxc HINTS ${VRS_SDK_DXC_HINTS} "C:/Program Files (x86)/Windows Kits/10/bin/10.0.26100.0/x64")
if(VRS_DXC)
    set(VI_HLSL "${CMAKE_CURRENT_BINARY_DIR}/vi_fixture.hlsl")
    file(WRITE "${VI_HLSL}"
        "float4 vs(uint id:SV_VertexID, uint vid:SV_ViewID):SV_Position { return float4(id==2?3:-1,id==1?3:-1,.5+(vid*0.0),1); }\n"
        "float4 ps(float4 p:SV_Position):SV_Target { return float4(frac(p.x/31),frac(p.y/29),frac((p.x+p.y)/23),1); }\n")
    add_custom_command(OUTPUT "${CMAKE_CURRENT_BINARY_DIR}/vi_vs.h" "${CMAKE_CURRENT_BINARY_DIR}/vi_ps.h"
        COMMAND "${VRS_DXC}" -T vs_6_1 -E vs -O3 -Fh "${CMAKE_CURRENT_BINARY_DIR}/vi_vs.h" -Vn g_vi_vs "${VI_HLSL}"
        COMMAND "${VRS_DXC}" -T ps_6_1 -E ps -O3 -Fh "${CMAKE_CURRENT_BINARY_DIR}/vi_ps.h" -Vn g_vi_ps "${VI_HLSL}"
        DEPENDS "${VI_HLSL}" VERBATIM)
    add_custom_target(vrs_vi_shaders DEPENDS "${CMAKE_CURRENT_BINARY_DIR}/vi_vs.h" "${CMAKE_CURRENT_BINARY_DIR}/vi_ps.h")
    set_source_files_properties("${CMAKE_CURRENT_BINARY_DIR}/vi_vs.h" "${CMAKE_CURRENT_BINARY_DIR}/vi_ps.h" PROPERTIES GENERATED TRUE HEADER_FILE_ONLY TRUE)
    foreach(vrs_target IN ITEMS vrs_gpu vrs_single_pass_gpu)
        add_dependencies(${vrs_target} vrs_vi_shaders)
        target_sources(${vrs_target} PRIVATE "${CMAKE_CURRENT_BINARY_DIR}/vi_vs.h" "${CMAKE_CURRENT_BINARY_DIR}/vi_ps.h")
        target_include_directories(${vrs_target} PRIVATE "${CMAKE_CURRENT_BINARY_DIR}")
        target_compile_definitions(${vrs_target} PRIVATE VRS_HAVE_VI_SHADERS=1)
    endforeach()
endif()

add_executable(vrs_mt_stress_gpu mt_stress_gpu.cpp ../../src/Render/FixedVrs.cpp ../../src/Render/FixedVrsSettings.cpp ../../src/Render/ViewInstancedPipeline.cpp ../../src/Render/CommandResources.cpp)
target_include_directories(vrs_mt_stress_gpu PRIVATE ../../include)
target_compile_features(vrs_mt_stress_gpu PRIVATE cxx_std_20)
target_compile_definitions(vrs_mt_stress_gpu PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
target_link_libraries(vrs_mt_stress_gpu PRIVATE d3d12 dxgi d3dcompiler)
add_test(NAME vrs_mt_stress_gpu COMMAND vrs_mt_stress_gpu)

if(MSVC)
    foreach(vrs_target IN ITEMS vrs_settings vrs_gpu vrs_single_pass_gpu vrs_mt_stress_gpu)
        target_compile_options(${vrs_target} PRIVATE /W4)
    endforeach()
endif()

set_tests_properties(vrs_gpu vrs_single_pass_gpu vrs_mt_stress_gpu PROPERTIES SKIP_RETURN_CODE 77)
