cmake_minimum_required(VERSION 3.20)
project(CyberpunkVR_Hands)

set(CMAKE_CXX_STANDARD 20)

# Add RED4ext.SDK
add_subdirectory(${CMAKE_SOURCE_DIR}/../../externals/RED4ext.SDK _deps/red4ext)

# MinHook (compiled directly from source — no CMakeLists of its own)
set(MINHOOK_DIR ${CMAKE_SOURCE_DIR}/../../externals/minhook)
add_library(minhook STATIC
    ${MINHOOK_DIR}/src/buffer.c
    ${MINHOOK_DIR}/src/hook.c
    ${MINHOOK_DIR}/src/trampoline.c
    ${MINHOOK_DIR}/src/hde/hde32.c
    ${MINHOOK_DIR}/src/hde/hde64.c
)
target_include_directories(minhook PUBLIC ${MINHOOK_DIR}/include)

add_library(CyberpunkVR_Hands SHARED main.cpp)
target_link_libraries(CyberpunkVR_Hands PRIVATE RED4ext.SDK minhook)

# Restructured module include roots (bare #include "x.h" resolves regardless of subfolder)
target_include_directories(CyberpunkVR_Hands PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${CMAKE_CURRENT_SOURCE_DIR}/vrik
    ${CMAKE_CURRENT_SOURCE_DIR}/weapon
    ${CMAKE_CURRENT_SOURCE_DIR}/../common
)

# Rename to .dll but standard prefix is none
set_target_properties(CyberpunkVR_Hands PROPERTIES PREFIX "")
