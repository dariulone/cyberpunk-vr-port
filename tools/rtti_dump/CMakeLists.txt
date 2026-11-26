# RttiDump -- patch-day RED4ext plugin that writes the game's RTTI layouts to JSON (see RttiDump.cpp).
# Never deployed with the port. Build by name:
#     cmake --build build --config Release --target rtti_dump_plugin
# and copy the DLL to <game>\red4ext\plugins\RttiDump\ for one launch.

add_library(rtti_dump_plugin SHARED RttiDump.cpp)
set_target_properties(rtti_dump_plugin PROPERTIES OUTPUT_NAME RttiDump)
target_compile_features(rtti_dump_plugin PRIVATE cxx_std_20)
target_compile_definitions(rtti_dump_plugin PRIVATE WIN32_LEAN_AND_MEAN NOMINMAX _CRT_SECURE_NO_WARNINGS=1)
target_link_libraries(rtti_dump_plugin PRIVATE RED4ext::SDK)
