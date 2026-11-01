# The plugin's Lua host (stage 6 of docs/native-frameworks-plan-20261030.md): LuaJIT and the libraries CET's scripting
# core is built on, fetched at pinned tags, and the host itself -- a static library linked into the plugin, with its
# own definitions so none of them reach the rest of the plugin.
include(FetchContent)

# LuaJIT (openresty's, as CET's), built with CMake through luajit-cmake; its host tools (minilua, buildvm) are built
# from the same sources on this machine.
FetchContent_Declare(luajit2
  GIT_REPOSITORY https://github.com/openresty/luajit2.git
  GIT_TAG v2.1-20250826)
FetchContent_Declare(luajit_cmake
  GIT_REPOSITORY https://github.com/zhaozg/luajit-cmake.git
  GIT_TAG 0cf7e0e5d595441c397a65db9bb8c9d7d2519ecd)
FetchContent_GetProperties(luajit2)
if(NOT luajit2_POPULATED)
  FetchContent_Populate(luajit2)
endif()
FetchContent_GetProperties(luajit_cmake)
if(NOT luajit_cmake_POPULATED)
  FetchContent_Populate(luajit_cmake)
endif()
set(LUAJIT_DIR ${luajit2_SOURCE_DIR} CACHE PATH "" FORCE)
set(LUAJIT_DISABLE_GC64 OFF CACHE BOOL "" FORCE)
set(CVR_SAVED_SHARED ${BUILD_SHARED_LIBS})
set(BUILD_SHARED_LIBS OFF)
add_subdirectory(${luajit_cmake_SOURCE_DIR} ${CMAKE_BINARY_DIR}/luajit EXCLUDE_FROM_ALL)
set(BUILD_SHARED_LIBS ${CVR_SAVED_SHARED})

# Header-only: sol2, spdlog (as CET has it), nlohmann json, hopscotch-map, xbyak, mem.
FetchContent_Declare(sol2 GIT_REPOSITORY https://github.com/ThePhD/sol2.git GIT_TAG v3.5.0)
FetchContent_Declare(spdlog GIT_REPOSITORY https://github.com/gabime/spdlog.git GIT_TAG v1.11.0)
FetchContent_Declare(nlohmann_json GIT_REPOSITORY https://github.com/nlohmann/json.git GIT_TAG v3.11.3)
FetchContent_Declare(hopscotch GIT_REPOSITORY https://github.com/Tessil/hopscotch-map.git GIT_TAG v2.4.0)
FetchContent_Declare(xbyak GIT_REPOSITORY https://github.com/herumi/xbyak.git GIT_TAG v7.24)
FetchContent_Declare(mem GIT_REPOSITORY https://github.com/0x1F9F1/mem.git GIT_TAG 1.0.0)
foreach(dep sol2 spdlog nlohmann_json hopscotch xbyak mem)
  FetchContent_GetProperties(${dep})
  if(NOT ${dep}_POPULATED)
    FetchContent_Populate(${dep})
  endif()
endforeach()

# TiltedCore (CET's containers and allocators) on mimalloc, both static.
FetchContent_Declare(mimalloc GIT_REPOSITORY https://github.com/microsoft/mimalloc.git GIT_TAG v2.2.4)
FetchContent_Declare(tiltedcore GIT_REPOSITORY https://github.com/tiltedphoques/TiltedCore.git GIT_TAG v0.2.9)
foreach(dep mimalloc tiltedcore)
  FetchContent_GetProperties(${dep})
  if(NOT ${dep}_POPULATED)
    FetchContent_Populate(${dep})
  endif()
endforeach()
set(MI_BUILD_SHARED OFF CACHE BOOL "" FORCE)
set(MI_BUILD_OBJECT OFF CACHE BOOL "" FORCE)
set(MI_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(MI_OVERRIDE OFF CACHE BOOL "" FORCE)
add_subdirectory(${mimalloc_SOURCE_DIR} ${CMAKE_BINARY_DIR}/mimalloc EXCLUDE_FROM_ALL)
file(GLOB CVR_TILTEDCORE_SOURCES "${tiltedcore_SOURCE_DIR}/Code/core/src/*.cpp")
add_library(cvr_tiltedcore STATIC ${CVR_TILTEDCORE_SOURCES})
# Its users include it as <TiltedCore/...>, the layout its install gives the headers.
file(COPY "${tiltedcore_SOURCE_DIR}/Code/core/include/" DESTINATION "${CMAKE_BINARY_DIR}/tiltedcore_include/TiltedCore")
target_include_directories(cvr_tiltedcore PUBLIC "${tiltedcore_SOURCE_DIR}/Code/core/include"
                                                  "${CMAKE_BINARY_DIR}/tiltedcore_include"
                                                  "${hopscotch_SOURCE_DIR}/include")
target_compile_features(cvr_tiltedcore PUBLIC cxx_std_20)
target_link_libraries(cvr_tiltedcore PUBLIC mimalloc-static)

# The json module CET's scripting core gives every mod (rxi's json.lua, MIT; cet/scripts/json), inside the plugin.
file(READ "${CMAKE_CURRENT_SOURCE_DIR}/cet/scripts/json/json.lua" CVR_JSON_LUA)
file(CONFIGURE OUTPUT "${CMAKE_CURRENT_BINARY_DIR}/generated/EmbeddedJsonLua.h"
     CONTENT "#pragma once\n// cet/scripts/json/json.lua, embedded by src/Lua/CMakeLists.txt.\ninline constexpr const char* kEmbeddedJsonLua = R\"cvrjson(@CVR_JSON_LUA@)cvrjson\";\n"
     @ONLY)
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/cet/scripts/json/json.lua")

# The host.
file(GLOB_RECURSE CVR_LUA_SOURCES CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/*.cpp" "${CMAKE_CURRENT_SOURCE_DIR}/*.h"
                                                     "${CMAKE_CURRENT_SOURCE_DIR}/*.hpp")
add_library(cvr_lua STATIC ${CVR_LUA_SOURCES})
# The collector's steps from outside the VM (HostGc.cpp): C on LuaJIT's internal headers, with the library's options.
target_sources(cvr_lua PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/IdleGc.c")
set_source_files_properties("${CMAKE_CURRENT_SOURCE_DIR}/IdleGc.c" PROPERTIES SKIP_PRECOMPILE_HEADERS ON
                            COMPILE_DEFINITIONS "LUAJIT_ENABLE_LUA52COMPAT")
source_group(TREE ${CMAKE_CURRENT_SOURCE_DIR} FILES ${CVR_LUA_SOURCES})
target_compile_features(cvr_lua PUBLIC cxx_std_20)
target_include_directories(cvr_lua
  PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}/include"
  PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}"
          "${CMAKE_CURRENT_SOURCE_DIR}/cet"
          "${CMAKE_CURRENT_BINARY_DIR}/generated"
          "${CMAKE_SOURCE_DIR}/include"
          "${CMAKE_SOURCE_DIR}/externals/minhook/include"
          "${sol2_SOURCE_DIR}/include"
          "${spdlog_SOURCE_DIR}/include"
          "${nlohmann_json_SOURCE_DIR}/single_include"
          "${xbyak_SOURCE_DIR}"
          "${mem_SOURCE_DIR}/include"
          "${luajit2_SOURCE_DIR}/src"
          "${CMAKE_BINARY_DIR}/luajit")
target_compile_definitions(cvr_lua PRIVATE
  WIN32_LEAN_AND_MEAN NOMINMAX SOL_ALL_SAFETIES_ON SOL_LUAJIT=1 SOL_EXCEPTIONS_SAFE_PROPAGATION
  SPDLOG_HEADER_ONLY SPDLOG_WCHAR_TO_UTF8_SUPPORT SPDLOG_WCHAR_FILENAMES SPDLOG_WCHAR_SUPPORT)
target_compile_options(cvr_lua PRIVATE /bigobj /utf-8)
target_link_libraries(cvr_lua PRIVATE libluajit cvr_tiltedcore RED4ext::SDK imgui)
# CET's scripting core (src/Lua/cet) includes its stdafx.h first everywhere: built once.
target_precompile_headers(cvr_lua PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/cet/stdafx.h")
