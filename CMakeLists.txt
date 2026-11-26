cmake_minimum_required(VERSION 3.24)

project(CyberpunkVRPort VERSION 0.2.5.5 LANGUAGES CXX C ASM_MASM)

# One compiler process per core inside every target, the fetched and bundled libraries included: `cmake --build
# --parallel` only spreads whole projects, and a full rebuild otherwise compiles each target one file at a time.
# /FS serialises the shared /Zi PDB writes.
if(MSVC)
    add_compile_options($<$<COMPILE_LANGUAGE:C,CXX>:/MP> $<$<COMPILE_LANGUAGE:C,CXX>:/FS>)
endif()

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
# scripts with it when redscript itself is not installed (rust/crates/cvr-host/src/scripts.rs). Only the C API library is built.
# The released line (0.5.x, what redscript's mod page ships), not the 1.0 previews: 1.0.0-preview.22 put up a
# modal "requires migration" dialog on struct `new` that held the game until the engine's watchdog, and compiled the
# breach indicator's wrap so its breach anchor stayed zero (the diamond slid with the head).
set(CVR_REDSCRIPT_TAG v0.5.32)
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
# One target directory per tag: a changed tag builds its own library instead of finding the old one up to date.
set(CVR_SCC_LIB "${CMAKE_BINARY_DIR}/redscript/${CVR_REDSCRIPT_TAG}/release/scc_lib.dll")
add_custom_command(
  OUTPUT ${CVR_SCC_LIB}
  COMMAND ${CVR_CARGO} build --release --locked -p scc-lib
          --manifest-path "${redscript_SOURCE_DIR}/Cargo.toml"
          --target-dir "${CMAKE_BINARY_DIR}/redscript/${CVR_REDSCRIPT_TAG}"
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

# MinHook (externals/minhook, v1.3.4-14-gd94c64d) is compiled once, into cvr_rust.lib (rust/crates/cvr-engine/build.rs):
# the Rust hook manager and the C++ half's MH_* calls share that one instance.


add_library(imgui STATIC
    ${imgui_SOURCE_DIR}/imgui.cpp
    ${imgui_SOURCE_DIR}/imgui_draw.cpp
    ${imgui_SOURCE_DIR}/imgui_tables.cpp
    ${imgui_SOURCE_DIR}/imgui_widgets.cpp
    ${imgui_SOURCE_DIR}/backends/imgui_impl_win32.cpp
    ${imgui_SOURCE_DIR}/backends/imgui_impl_dx12.cpp
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
# main's new C++ not yet ported to Rust (cmake/MainPendingRust.txt): out of every build of this branch until it is.
file(STRINGS ${CMAKE_CURRENT_SOURCE_DIR}/cmake/MainPendingRust.txt CVR_MAIN_PENDING REGEX "^src/")
foreach(path IN LISTS CVR_MAIN_PENDING)
    list(REMOVE_ITEM CPVR_SOURCES ${CMAKE_CURRENT_SOURCE_DIR}/${path})
endforeach()
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS ${CMAKE_CURRENT_SOURCE_DIR}/cmake/MainPendingRust.txt)
add_subdirectory(src/Lua)
# The player's solve: src/Anim/CharacterRig.cpp and HandSnapshot.cpp, or with CVR_RUST_VRIK their Rust port (rust/crates/cvr-rig/src/vrik,
# the same C names over the same globals, src/Anim/CharacterRigState.cpp) and the C forms of the C++ it calls
# (src/Anim/CharacterRigRust.cpp). tools/vrik_tests builds both and compares them.
option(CVR_RUST_VRIK "The player's solve (VRIK) from Rust" ON)
if(CVR_RUST_VRIK)
    list(FILTER CPVR_SOURCES EXCLUDE REGEX "/src/Anim/(CharacterRig|HandSnapshot|ReloadPose|HandGestures|Cyberware|LeftHanded|HeadAimWeapon|FkRecorder|AdsEyeAlign|AdsMuzzleStabilizer|SmokingPose|ReloadHandOff|WeaponRig|WheelGrab|Recoil|GraspPose|TwoHandGrip)\\.cpp$")
    list(FILTER CPVR_SOURCES EXCLUDE REGEX "/src/Hooks/AnimPoseBody\\.cpp$")
    list(FILTER CPVR_SOURCES EXCLUDE REGEX "/src/Runtimes/(Haptics|FingerGrip)\\.cpp$")
    list(FILTER CPVR_SOURCES EXCLUDE REGEX "/src/Camera/(CameraLink|CameraMath|PoseIdentity)\\.cpp$")
    list(FILTER CPVR_SOURCES EXCLUDE REGEX "/src/Hooks/CyberwareChord\\.cpp$")
else()
    list(FILTER CPVR_SOURCES EXCLUDE REGEX "/src/(Anim/CharacterRigRust|Camera/CameraRust|Camera/PoseIdentityRust|Hooks/RoomscaleRust|Hooks/WaterLadderRust|Hooks/InputRust)\\.cpp$")
endif()
if(CVR_RUST_VRIK)
    # The sources whose Rust build compiles nothing (their code is Rust's; the C++ build and the Rust test variants keep
    # them): cmake/RustBuildEmptySources.txt.
    file(STRINGS ${CMAKE_CURRENT_SOURCE_DIR}/cmake/RustBuildEmptySources.txt CVR_RUST_EMPTY_SOURCES REGEX "^src/")
    foreach(path IN LISTS CVR_RUST_EMPTY_SOURCES)
        list(REMOVE_ITEM CPVR_SOURCES ${CMAKE_CURRENT_SOURCE_DIR}/${path})
    endforeach()
    set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS ${CMAKE_CURRENT_SOURCE_DIR}/cmake/RustBuildEmptySources.txt)
endif()

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
if(CVR_RUST_VRIK)
    # The Rust build's thunks are Rust's: rust/crates/cvr-render/src/asm_thunks (the same code through global_asm!).
    set(CPVR_ASM_SOURCES)
endif()
add_library(cyberpunkvrport_stereo SHARED
    ${CPVR_SOURCES}
    ${CPVR_ASM_SOURCES}
    externals/im3d/im3d.cpp
)
include(cmake/WorldMarkerShaders.cmake)
cvr_world_marker_shaders(cyberpunkvrport_stereo)
# The Rust half while both languages share this DLL (docs/rust-migration-plan.md): cvr_rust.lib from rust/, called
# through include/Rust/CvrRust.h. Since stage 2 it is not optional: it owns the plugin's entry points (exported by
# name below), the log, the plugin's folders, the build gate, the script compiler, MinHook and the stick input. The
# C++-only reference DLL is built from commit 7ae067a5 with -DCVR_WITH_RUST=OFF (docs/rust-migration/baseline.md).
set(CVR_RUST_FEATURES plugin)
if(CVR_RUST_VRIK)
    list(APPEND CVR_RUST_FEATURES vrik)
    # The hook files whose work is Rust's keep their registration (and so the install order) under this switch.
    target_compile_definitions(cyberpunkvrport_stereo PRIVATE CVR_RUST_VRIK=1)
    # The Lua host puts its InternalCall handler behind the Rust side's hook (src/Lua/cet/scripting/FunctionOverride.cpp).
    target_compile_definitions(cvr_lua PRIVATE CVR_RUST_VRIK=1)
    # One address resolver for every C++ file of the DLL (include/Core/PortAddressResolver.hpp): RED4ext's when it
    # runs, else the address library the game ships -- the port's own loader runs without RED4ext. The Lua host (cvr_lua)
    # takes it through its precompiled header (src/Lua/cet/reverse/Relocation.h).
    set(CVR_RESOLVER_FI "/FI${CMAKE_CURRENT_SOURCE_DIR}/include/Core/PortAddressResolver.hpp")
    target_compile_options(cyberpunkvrport_stereo PRIVATE $<$<COMPILE_LANGUAGE:CXX>:${CVR_RESOLVER_FI}>)
    # The SDK is built as a static library (RED4EXT_STATIC_LIB): its UniversalRelocBase::Resolve is compiled there, so
    # the override has to be seen by its own files too.
    if(TARGET RED4ext.SDK)
        target_compile_options(RED4ext.SDK PRIVATE $<$<COMPILE_LANGUAGE:CXX>:${CVR_RESOLVER_FI}>)
    endif()
endif()
include(cmake/RustStaticlib.cmake)
cvr_link_rust(cyberpunkvrport_stereo)
target_link_options(cyberpunkvrport_stereo PRIVATE /EXPORT:Main /EXPORT:Query /EXPORT:Supports)
if(CVR_RUST_VRIK)
    # The exported variables Rust defines: the tools read them by name, as they read the C++ ones (rust/exported-data.txt).
    file(STRINGS ${CMAKE_CURRENT_SOURCE_DIR}/rust/exported-data.txt CVR_RUST_EXPORTED_DATA REGEX "^[A-Za-z_][A-Za-z0-9_]*$")
    foreach(name IN LISTS CVR_RUST_EXPORTED_DATA)
        target_link_options(cyberpunkvrport_stereo PRIVATE /EXPORT:${name},DATA)
    endforeach()
    set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS ${CMAKE_CURRENT_SOURCE_DIR}/rust/exported-data.txt)
    # The exported functions Rust defines (rust/crates/cvr-render/src/api_exports.rs).
    file(STRINGS ${CMAKE_CURRENT_SOURCE_DIR}/rust/exported-functions.txt CVR_RUST_EXPORTED_FUNCTIONS REGEX "^[A-Za-z_][A-Za-z0-9_]*$")
    foreach(name IN LISTS CVR_RUST_EXPORTED_FUNCTIONS)
        target_link_options(cyberpunkvrport_stereo PRIVATE /EXPORT:${name})
    endforeach()
    set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS ${CMAKE_CURRENT_SOURCE_DIR}/rust/exported-functions.txt)
endif()
# The C++ headers whose logic Rust has taken over call it (each matched bit for bit by its test suite's trace case):
# the body's turn and bend (tools/body_rotation_tests), roomscale, the crouch, the view's anchor and the scene and input
# policies (tools/roomscale_tests).
target_compile_definitions(cyberpunkvrport_stereo PRIVATE CVR_RUST_BODY=1 CVR_RUST_ROOM=1)
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
    iphlpapi ws2_32 ole32 uuid # bridge listener detection and native vehicle audio loopback
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
    # NightCoop's SDK header (MIT), for Natives/NightCoopBridge.cpp; nothing is linked
    ${CMAKE_CURRENT_SOURCE_DIR}/externals/nightcoop-sdk/include
)
if(MSVC)
    target_compile_options(cyberpunkvrport_stereo PRIVATE
        $<$<COMPILE_LANGUAGE:CXX>:/W4 /permissive- /EHsc>
        # every core: the Visual Studio generator otherwise compiles the ~400 sources one after another (12 minutes)
        $<$<COMPILE_LANGUAGE:CXX>:/MP>)
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
if(EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/tools/rtti_dump/CMakeLists.txt")
    add_subdirectory(tools/rtti_dump)
endif()
