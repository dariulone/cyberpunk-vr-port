cmake_minimum_required(VERSION 3.24)

project(CyberpunkVRPort VERSION 0.2.1 LANGUAGES CXX C ASM_MASM)

include(FetchContent)
FetchContent_Declare(
  openxr
  GIT_REPOSITORY https://github.com/KhronosGroup/OpenXR-SDK.git
  GIT_TAG release-1.0.34
)
FetchContent_MakeAvailable(openxr)

# The version CET's ImGui bindings are made for (src/Lua/cet/sol_imgui), which the Lua mods' windows go through.
FetchContent_Declare(
  imgui
  GIT_REPOSITORY https://github.com/ocornut/imgui.git
  GIT_TAG v1.91.1-docking
)
FetchContent_GetProperties(imgui)
if(NOT imgui_POPULATED)
  FetchContent_Populate(imgui)
endif()

# XML for the port's input bindings, merged into the game's own input configuration (Core/InputConfig.cpp).
FetchContent_Declare(
  pugixml
  GIT_REPOSITORY https://github.com/zeux/pugixml.git
  GIT_TAG v1.14
)
FetchContent_GetProperties(pugixml)
if(NOT pugixml_POPULATED)
  FetchContent_Populate(pugixml)
endif()
add_library(cvr_pugixml STATIC ${pugixml_SOURCE_DIR}/src/pugixml.cpp)
target_include_directories(cvr_pugixml PUBLIC ${pugixml_SOURCE_DIR}/src)

# YAML for the port's TweakDB records, applied by the plugin itself (Core/TweakRecords.cpp).
set(YAML_CPP_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(YAML_CPP_BUILD_TOOLS OFF CACHE BOOL "" FORCE)
set(YAML_CPP_BUILD_CONTRIB OFF CACHE BOOL "" FORCE)
set(YAML_CPP_INSTALL OFF CACHE BOOL "" FORCE)
set(YAML_BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)
FetchContent_Declare(
  yaml-cpp
  GIT_REPOSITORY https://github.com/jbeder/yaml-cpp.git
  GIT_TAG 0.8.0
)
# yaml-cpp 0.8.0 still asks for CMake 3.4, which CMake 4 refuses: its policies are taken as of 3.5, for it alone.
set(CMAKE_POLICY_VERSION_MINIMUM 3.5)
FetchContent_MakeAvailable(yaml-cpp)
unset(CMAKE_POLICY_VERSION_MINIMUM)

# redscript's script compiler (scc_lib.dll, MIT), built from its sources with cargo: the plugin compiles the game's
# scripts with it when redscript itself is not installed (Core/ScriptCompiler.cpp). Only the C API library is built;
# redscript-dotfile goes with it so the workspace's features unify as in redscript's own release build.
set(CVR_REDSCRIPT_TAG v1.0.0-preview.22)
FetchContent_Declare(
  redscript
  GIT_REPOSITORY https://github.com/jac3km4/redscript.git
  GIT_TAG ${CVR_REDSCRIPT_TAG}
)
FetchContent_GetProperties(redscript)
if(NOT redscript_POPULATED)
  FetchContent_Populate(redscript)
endif()
find_program(CVR_CARGO cargo HINTS "$ENV{USERPROFILE}/.cargo/bin" REQUIRED)
set(CVR_SCC_LIB "${CMAKE_BINARY_DIR}/redscript/release/scc_lib.dll")
add_custom_command(
  OUTPUT ${CVR_SCC_LIB}
  COMMAND ${CVR_CARGO} build --release --locked -p scc-lib -p redscript-dotfile
          --manifest-path "${redscript_SOURCE_DIR}/Cargo.toml" --target-dir "${CMAKE_BINARY_DIR}/redscript"
  WORKING_DIRECTORY ${redscript_SOURCE_DIR}
  COMMENT "Building redscript's scc_lib ${CVR_REDSCRIPT_TAG}"
  VERBATIM
)
add_custom_target(cvr_scc_lib ALL DEPENDS ${CVR_SCC_LIB})

# The TweakDB's record names, for the Lua host's TDBID.ToStringDEBUG and TweakDB API (src/Lua): WolvenKit's resource
# files (GPL-3.0, shipped beside the plugin as data with their licence, as CET ships them), at the commit that last
# changed them.
set(CVR_TWEAKDB_NAMES_COMMIT 95074158ed971aea39fc8abc50c2574381f4aad2)
set(CVR_TWEAKDB_NAMES_DIR "${CMAKE_BINARY_DIR}/tweakdb-names")
foreach(entry
    "WolvenKit.Common/Resources/tweakdbstr.kark|tweakdbstr.kark|ea74ca46375c414514d5b8577702c51c1ba55db06d24aaf73901541d212f5ebb"
    "WolvenKit.Common/Resources/usedhashes.kark|usedhashes.kark|e1f2bf54f5d24660a4a8ecff5694b39d2ca9db4a4a8ca3c3a8900dc71477a9e2"
    "LICENSE|LICENSE.txt|589ed823e9a84c56feb95ac58e7cf384626b9cbf4fda2a907bc36e103de1bad2")
  string(REPLACE "|" ";" entry "${entry}")
  list(GET entry 0 CVR_NAMES_SOURCE)
  list(GET entry 1 CVR_NAMES_FILE)
  list(GET entry 2 CVR_NAMES_SHA256)
  file(DOWNLOAD "https://raw.githubusercontent.com/WolvenKit/WolvenKit/${CVR_TWEAKDB_NAMES_COMMIT}/${CVR_NAMES_SOURCE}"
       "${CVR_TWEAKDB_NAMES_DIR}/${CVR_NAMES_FILE}" EXPECTED_HASH SHA256=${CVR_NAMES_SHA256} TLS_VERIFY ON)
endforeach()

set(MINHOOK_SOURCES
    externals/minhook/src/buffer.c
    externals/minhook/src/hook.c
    externals/minhook/src/trampoline.c
    externals/minhook/src/hde/hde32.c
    externals/minhook/src/hde/hde64.c
)


add_library(imgui STATIC
    ${imgui_SOURCE_DIR}/imgui.cpp
    ${imgui_SOURCE_DIR}/imgui_draw.cpp
    ${imgui_SOURCE_DIR}/imgui_tables.cpp
    ${imgui_SOURCE_DIR}/imgui_widgets.cpp
    ${imgui_SOURCE_DIR}/backends/imgui_impl_win32.cpp
    ${imgui_SOURCE_DIR}/backends/imgui_impl_dx12.cpp
    ${MINHOOK_SOURCES}
)
target_include_directories(imgui PUBLIC
    ${imgui_SOURCE_DIR}
    ${imgui_SOURCE_DIR}/backends
)



# The submodule, unless a trial build points elsewhere (tools/patchday/sdk_layouts.py build: a newer SDK
# checked out under build/patchday/, in its own build folder).
set(CVR_RED4EXT_SDK_DIR "${CMAKE_CURRENT_SOURCE_DIR}/externals/RED4ext.SDK" CACHE PATH "RED4ext.SDK source tree")
add_subdirectory(${CVR_RED4EXT_SDK_DIR} ${CMAKE_BINARY_DIR}/externals/RED4ext.SDK)

# The full-body IK: the wrpuppets-fbt-ik library (MIT), engine-independent; the game side is src/Fbt/Game.
set(CVR_FBT_IK_DIR "${CMAKE_CURRENT_SOURCE_DIR}/externals/wrpuppets-fbt-ik" CACHE PATH "wrpuppets-fbt-ik source tree")
add_subdirectory(${CVR_FBT_IK_DIR} ${CMAKE_BINARY_DIR}/externals/wrpuppets-fbt-ik)

# ============================================================================================
# cyberpunkvrport.dll (target cyberpunkvrport_stereo; CyberpunkVR_Stereo.dll up to 0.1.9) -- the RED4ext
# plugin. This is the whole mod.
# ============================================================================================
#
# It used to ship as a dxgi.dll proxy sitting next to the game exe, with DllMain and the seven
# DXGI factory exports forwarding to the system dxgi.dll. None of the code ever needed to BE
# the proxy -- it only needed a device, a queue and Present, and RED4ext hands over all three.
# The proxy is gone: no DllMain, no export forwarders, no LoadLibrary of the real dxgi.dll,
# and no second copy of every hook racing the first if both DLLs were installed by mistake.
#
# ONE PLUGIN, NOT TWO. CyberpunkVR_Hands.dll used to be a separate red4ext plugin, and this comment
# used to say it stayed that way. It does not: the glob below takes ALL of src/, which now includes the
# VRIK pose path (src/Hooks/AnimPose.cpp, src/Anim/, src/Natives/) that used to build into Hands. The
# single DLL registers all 219 natives and installs every hook.
#
# THE OLD CyberpunkVR_Hands.dll MUST BE REMOVED FROM red4ext\plugins. Both binaries register the same
# 219 natives and detour the same addresses, and two plugins detouring one address is the crash that
# reads as a fault at FFFFFFFFFFFFFFFF with no usable stack -- checked, not assumed: the deployed
# Hands.dll contains all 219 native name strings.
#
# The single file is also SMALLER than the two it replaces (2,199,552 against 1,291,776 + 1,425,920),
# because the code they shared is now emitted once.
# THE FILE LIST IS A GLOB, and deliberately so. This tree is mid-restructure: the monoliths are
# being split into one-hook-per-file units under src/Hooks, src/Anim and the rest, and a hand-kept
# list would be edited on every one of those splits and forgotten on one of them. CONFIGURE_DEPENDS
# makes the build re-glob when a file appears, so the usual objection to globbing does not apply.
file(GLOB_RECURSE CPVR_SOURCES CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/include/*.hpp"
)
# The Lua host is its own library (src/Lua/CMakeLists.txt), with its own definitions and dependencies.
list(FILTER CPVR_SOURCES EXCLUDE REGEX "/src/Lua/")
add_subdirectory(src/Lua)

# Keep the folder shape in the IDE -- Hooks/, Runtimes/, Anim/, Utils/ are the reading order.
source_group(TREE ${CMAKE_CURRENT_SOURCE_DIR} FILES ${CPVR_SOURCES})

set(CPVR_ASM_SOURCES
    src/Hooks/CameraPoseLeafThunks.asm
    src/Hooks/SwimmingActionLeafThunk.asm
    src/Hooks/WorldMarkerTextThunk.asm
    src/Hooks/WorldTouchTriangleThunk.asm
    src/Hooks/PhotoCaptureClampThunk.asm
    src/Hooks/SurveillanceInputThunk.asm
    src/Hooks/StoryAttentionThunk.asm
    src/Hooks/RenderGraphCacheThunk.asm
    src/Hooks/NativeBoundsThunk.asm
    src/Hooks/NativeDrawOriginThunk.asm
    src/Hooks/NativeSceneUpdateThunk.asm
    src/Hooks/NativeUploadReservationThunk.asm
    src/Hooks/NativeInstanceReservationThunk.asm
    src/Hooks/NativeSourceSrvBindingThunk.asm
)
# MASM, said outright: LuaJIT's build (src/Lua) enables CMake's generic ASM language, and an enabled language is
# global -- by extension alone these would go to it, not to ml64.
set_source_files_properties(${CPVR_ASM_SOURCES} PROPERTIES LANGUAGE ASM_MASM)
add_library(cyberpunkvrport_stereo SHARED
    ${CPVR_SOURCES}
    ${CPVR_ASM_SOURCES}
    externals/im3d/im3d.cpp
)
include(cmake/WorldMarkerShaders.cmake)
cvr_world_marker_shaders(cyberpunkvrport_stereo)
include(cmake/NativeFramegen.cmake)
target_compile_features(cyberpunkvrport_stereo PRIVATE cxx_std_20)
string(REGEX REPLACE "^v" "" CVR_REDSCRIPT_VERSION ${CVR_REDSCRIPT_TAG})
target_compile_definitions(cyberpunkvrport_stereo PRIVATE
    CVR_REDSCRIPT_VERSION="${CVR_REDSCRIPT_VERSION}"
    # the version RED4ext lists the plugin under: project() above
    CVR_VERSION_MAJOR=${PROJECT_VERSION_MAJOR} CVR_VERSION_MINOR=${PROJECT_VERSION_MINOR}
    CVR_VERSION_PATCH=${PROJECT_VERSION_PATCH}
    WIN32_LEAN_AND_MEAN NOMINMAX
    XR_USE_PLATFORM_WIN32 XR_USE_GRAPHICS_API_D3D12
    CPVR_RED4EXT_PLUGIN=1
    _CRT_SECURE_NO_WARNINGS=1
)
# The notice's word in every language the game is translated into, as wide string literals.
set_source_files_properties(src/Lobby/Notice.cpp PROPERTIES COMPILE_OPTIONS /utf-8)
target_link_libraries(cyberpunkvrport_stereo PRIVATE
    wrpuppets_fbt_ik
    openxr_loader
    imgui
    d3d12.lib
    d3dcompiler
    bcrypt
    dxgi.lib        # the SYSTEM import lib -- bare "dxgi" would resolve to our own target
    psapi
    xaudio2.lib     # the port's own sounds (Audio/PortSound.cpp)
    d2d1.lib        # lines of text set for a hung screen (Render/Caption.cpp)
    dwrite.lib
    cvr_pugixml
    yaml-cpp::yaml-cpp
    RED4ext::SDK
    cvr_lua
)
# Use the JSON parser already selected for the statically linked OpenXR loader.
if(BUILD_WITH_SYSTEM_JSONCPP)
    target_link_libraries(cyberpunkvrport_stereo PRIVATE JsonCpp::JsonCpp)
else()
    target_include_directories(cyberpunkvrport_stereo PRIVATE ${openxr_SOURCE_DIR}/src/external/jsoncpp/include)
endif()
target_include_directories(cyberpunkvrport_stereo PRIVATE
    # ONE root for our own headers. Every internal include reads "Area/Name.hpp", so a header's
    # path states which area owns it -- which is the whole point of the layout. The old set of
    # eleven roots let any header be reached by its bare name from anywhere, and that is precisely
    # what let the monoliths grow: nothing about an include said where the thing belonged.
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/streamline/include
    # --- third-party / vendored ---
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/im3d
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/stb
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/minhook/include
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/minhook/src
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/minhook/src/hde
)
if(MSVC)
    target_compile_options(cyberpunkvrport_stereo PRIVATE
        $<$<COMPILE_LANGUAGE:CXX>:/W4 /permissive- /EHsc>)
    # SYMBOLS IN THE RELEASE BUILD. A crash landed inside this DLL and there was nothing to read it
    # with: no PDB here, no cdb or windbg on the machine, and dumpbin only opens PE files. Placing
    # CyberpunkVR_Stereo.dll+0x459AF took disassembling the whole image and matching the faulting
    # instruction by hand. That is affordable once.
    #
    # /Zi and /DEBUG do not change optimisation -- /OPT:REF and /OPT:ICF are restored explicitly
    # because /DEBUG turns them off by default, and without them the linker would stop folding and
    # stripping, which WOULD change the binary. The PDB ships nowhere; it stays in the build tree
    # so a dump from this machine resolves instantly and a tester's dump resolves against the build
    # that produced their DLL.
    target_compile_options(cyberpunkvrport_stereo PRIVATE $<$<CONFIG:Release>:/Zi>)
    target_link_options(cyberpunkvrport_stereo PRIVATE
        $<$<CONFIG:Release>:/DEBUG /OPT:REF /OPT:ICF>)
endif()
set_target_properties(cyberpunkvrport_stereo PROPERTIES
    PREFIX ""
    OUTPUT_NAME cyberpunkvrport
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin/red4ext/plugins/cyberpunkvrport"
)
# The compiler and its licence beside the plugin, in its redscript\ folder; the TweakDB's names in config\tweakdb.
add_dependencies(cyberpunkvrport_stereo cvr_scc_lib)
add_custom_command(TARGET cyberpunkvrport_stereo POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E make_directory "$<TARGET_FILE_DIR:cyberpunkvrport_stereo>/redscript"
    COMMAND ${CMAKE_COMMAND} -E copy_if_different ${CVR_SCC_LIB} "$<TARGET_FILE_DIR:cyberpunkvrport_stereo>/redscript/scc_lib.dll"
    COMMAND ${CMAKE_COMMAND} -E copy_if_different "${redscript_SOURCE_DIR}/LICENSE"
            "$<TARGET_FILE_DIR:cyberpunkvrport_stereo>/redscript/LICENSE.txt"
    COMMAND ${CMAKE_COMMAND} -E copy_directory "${CVR_TWEAKDB_NAMES_DIR}"
            "$<TARGET_FILE_DIR:cyberpunkvrport_stereo>/config/tweakdb"
    VERBATIM
)

# ============================================================================================
# Tools. Separate targets, never deployed with the plugin.
# ============================================================================================
# xr_probe_layer -- an OpenXR API layer that records what any VR application asks the runtime for
# and what it submits back, without touching application internals. Build it explicitly:
#     cmake --build build --config Release --target xr_probe_layer
add_subdirectory(tools/xr_probe)
# rtti_dump_plugin -- patch-day RED4ext plugin: the running game's RTTI layouts as JSON (tools/rtti_dump).
add_subdirectory(tools/rtti_dump)
