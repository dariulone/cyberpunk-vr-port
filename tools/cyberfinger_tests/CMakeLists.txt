cmake_minimum_required(VERSION 3.24)
project(CyberFingerTests LANGUAGES C CXX)
enable_testing()
set(CVR_FBT_IK_DIR "${CMAKE_CURRENT_LIST_DIR}/../../externals/wrpuppets-fbt-ik" CACHE PATH "Pinned IK source")
set(OPENXR_INCLUDE_DIR "${CMAKE_CURRENT_LIST_DIR}/../../build/_deps/openxr-src/include" CACHE PATH "OpenXR headers")
set(RED4EXT_INCLUDE_DIR "${CMAKE_CURRENT_LIST_DIR}/../../externals/RED4ext.SDK/include" CACHE PATH "Pinned RED4ext SDK headers")
set(IMGUI_INCLUDE_DIR "${CMAKE_CURRENT_LIST_DIR}/../../build/_deps/imgui-src" CACHE PATH "Pinned ImGui headers")
set(RED4EXT_HEADER_ONLY ON CACHE BOOL "" FORCE)
add_subdirectory(${RED4EXT_INCLUDE_DIR}/.. sdk EXCLUDE_FROM_ALL)
add_subdirectory(${CVR_FBT_IK_DIR} ik)
add_executable(cyberfinger_tests main.cpp)
target_include_directories(cyberfinger_tests PRIVATE ../../include ${OPENXR_INCLUDE_DIR})
target_compile_features(cyberfinger_tests PRIVATE cxx_std_17)
target_link_libraries(cyberfinger_tests PRIVATE wrpuppets_fbt_ik)
add_test(NAME cyberfinger_policy_and_ik COMMAND cyberfinger_tests)
add_executable(cyberfinger_menu_tests menu_test.cpp)
target_include_directories(cyberfinger_menu_tests PRIVATE ../../include)
target_compile_features(cyberfinger_menu_tests PRIVATE cxx_std_17)
add_test(NAME cyberfinger_menu_policy COMMAND cyberfinger_menu_tests)
add_executable(cyberfinger_melee_tests melee_input_test.cpp)
target_include_directories(cyberfinger_melee_tests PRIVATE ../../include)
target_compile_features(cyberfinger_melee_tests PRIVATE cxx_std_17)
add_test(NAME cyberfinger_melee_input COMMAND cyberfinger_melee_tests)
add_executable(cyberfinger_game_menu_tests game_menu_test.cpp ../../src/Runtimes/CyberFingerGameMenu.cpp)
target_include_directories(cyberfinger_game_menu_tests PRIVATE ../../include ${OPENXR_INCLUDE_DIR})
target_compile_features(cyberfinger_game_menu_tests PRIVATE cxx_std_20)
target_compile_definitions(cyberfinger_game_menu_tests PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
add_test(NAME cyberfinger_game_menu_bridge COMMAND cyberfinger_game_menu_tests)
add_executable(cyberfinger_binding_activity_tests binding_activity_test.cpp ../../src/Runtimes/CyberFingerInput.cpp)
target_include_directories(cyberfinger_binding_activity_tests PRIVATE ../../include ${OPENXR_INCLUDE_DIR})
target_compile_features(cyberfinger_binding_activity_tests PRIVATE cxx_std_17)
target_compile_definitions(cyberfinger_binding_activity_tests PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
add_test(NAME cyberfinger_binding_activity COMMAND cyberfinger_binding_activity_tests)
add_executable(cyberfinger_ui_tests ui_test.cpp ../../src/Runtimes/CyberFingerUi.cpp)
target_include_directories(cyberfinger_ui_tests PRIVATE ../../include ${OPENXR_INCLUDE_DIR})
target_compile_features(cyberfinger_ui_tests PRIVATE cxx_std_20)
target_compile_definitions(cyberfinger_ui_tests PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
add_test(NAME cyberfinger_hud_conversation COMMAND cyberfinger_ui_tests)

# Compile the actual adapters as well as the engine-independent policies. This
# does not link a game DLL and needs no game installation or running VR runtime.
add_library(cyberfinger_adapters OBJECT
    ../../src/Runtimes/CyberFingerInput.cpp ../../src/Runtimes/FingerGrip.cpp
    ../../src/Fbt/Game/Trackers.cpp ../../src/Fbt/Game/Body.cpp ../../src/Fbt/Game/Calibration.cpp
    ../../src/Fbt/Game/Menu.cpp ../../src/Natives/Reach.cpp ../../src/Natives/Registration.cpp
    ../../src/Runtimes/OpenXRManager.cpp ../../src/Runtimes/OpenXRFrameLoop.cpp
    ../../src/Hooks/XInput.cpp)
target_include_directories(cyberfinger_adapters PRIVATE ../../include ${OPENXR_INCLUDE_DIR} ${RED4EXT_INCLUDE_DIR} ${IMGUI_INCLUDE_DIR})
target_compile_features(cyberfinger_adapters PRIVATE cxx_std_20)
target_compile_definitions(cyberfinger_adapters PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN RED4EXT_STATIC_LIB
    XR_USE_PLATFORM_WIN32 XR_USE_GRAPHICS_API_D3D12)
target_link_libraries(cyberfinger_adapters PRIVATE wrpuppets_fbt_ik RED4ext.SDK)

# Optional offline execution of the real world-interaction script. Lua 5.4 is
# sufficient here; the test supplies LuaJIT's math.atan2 alias explicitly.
set(LUA_SOURCE_DIR "" CACHE PATH "Official Lua source directory containing lua.c")
if(LUA_SOURCE_DIR)
    add_test(NAME cyberfinger_stock_arm_shortcut COMMAND cyberfinger_lua
        ${CMAKE_CURRENT_LIST_DIR}/arm_shortcut_test.lua
        ${CMAKE_CURRENT_LIST_DIR}/../../mods/cet/CyberpunkVRPort_ArmCyberwareB/init.lua)
    add_test(NAME cyberfinger_melee_strength COMMAND cyberfinger_lua
        ${CMAKE_CURRENT_LIST_DIR}/melee_strength_test.lua
        ${CMAKE_CURRENT_LIST_DIR}/../../mods/cet/CyberpunkVRPort_Weapon/)
    add_test(NAME cyberfinger_melee_equipment COMMAND cyberfinger_lua
        ${CMAKE_CURRENT_LIST_DIR}/melee_toggle_test.lua
        ${CMAKE_CURRENT_LIST_DIR}/../../mods/cet/CyberpunkVRPort_CyberFinger/melee_toggle.lua)
    file(GLOB LUA_SOURCES "${LUA_SOURCE_DIR}/*.c")
    list(FILTER LUA_SOURCES EXCLUDE REGEX "/(onelua|luac|ltests)\\.c$")
    add_executable(cyberfinger_lua ${LUA_SOURCES})
    target_compile_definitions(cyberfinger_lua PRIVATE _CRT_SECURE_NO_WARNINGS)
    add_test(NAME cyberfinger_nearby_world COMMAND cyberfinger_lua
        ${CMAKE_CURRENT_LIST_DIR}/world_test.lua
        ${CMAKE_CURRENT_LIST_DIR}/../../mods/cet/CyberpunkVRPort_Reach/modules/world.lua)
    add_test(NAME cyberfinger_menu_navigation COMMAND cyberfinger_lua
        ${CMAKE_CURRENT_LIST_DIR}/menu_nav_test.lua
        ${CMAKE_CURRENT_LIST_DIR}/../../mods/cet/CyberpunkVRPort_Menu/modules/cyberfinger_nav.lua)
    add_test(NAME cyberfinger_conversation_bridge COMMAND cyberfinger_lua
        ${CMAKE_CURRENT_LIST_DIR}/conversation_test.lua
        ${CMAKE_CURRENT_LIST_DIR}/../../mods/cet/CyberpunkVRPort_CyberFinger/init.lua)
endif()
