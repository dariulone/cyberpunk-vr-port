//! cvr_rust.lib: the Rust half of cyberpunkvrport.dll while the C++ half still links it (docs/rust-migration-plan.md:
//! until stage 7 CMake links the DLL, from stage 2 Rust owns its entry points).
//!
//!   plugin  Main / Query / Supports, exported from the DLL (/EXPORT in CMakeLists.txt): the start and stop of the
//!           plugin in the order src/Main.cpp had, calling the C++ subsystems not yet in Rust through `cvr_cpp_*`
//!   host    the log, the plugin's folders, for the C++ half (its Log() and cvr::paths forward here)
//!   engine  the game build gate, for the C++ half (cvr::game_build forwards here)
//!   input   the stick, keypad and trigger input (stage 1a)
//!   body    the body's turn and bend (stage 3)
//!   room    roomscale, the crouch, the view's anchor and frames, scene and input policies (stage 3)
//!   rig     (feature vrik) the player's solve, cvr-rig's exports under the C++ names (stage 3)
//!   settings  vrport.ini read as the C++ half read it (stage 3)
//!   xr      OpenXR, handed over from OpenXRManager part by part (stage 4)
//!
//! Every export has its declaration in include/Rust/CvrRust.h. Nothing unwinds into C++: entry points run under
//! cvr_host::guard, and the plain functions cannot panic.
pub mod body;
pub mod engine;
pub mod host;
pub mod input;
// The port's input bindings in the game's input configuration (formerly src/Core/InputConfig.cpp).
pub mod input_config;
pub mod room;
// The plugin's first launch: the game settings preset and the borderless window (formerly src/Core/FirstLaunch.cpp).
pub mod first_launch;
// The SDK class layouts against the game's RTTI (formerly src/Core/SdkLayouts.cpp).
pub mod sdk_layouts;
// The port's TweakDB records from config\tweaks (formerly src/Core/TweakRecords.cpp's logic).
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod tweak_records;
// Its native tweaks: the on-foot capsule, the launcher's ready state (formerly src/Hooks/RoomscaleTweaks.cpp,
// src/Natives/LauncherReadyState.cpp).
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod native_tweaks;
// The port's archive without ArchiveXL (formerly src/Core/ArchiveExtensions.cpp's logic).
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod archive_ext;
// The entities the port spawns and their natives (formerly src/Natives/SpawnSystem.cpp's logic).
// A native's arguments and the handle arrays it fills.
#[cfg(all(feature = "plugin", feature = "vrik"))]
mod native_args;
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod spawn_system;
// What Codeware added to engine classes, under the port's names (formerly src/Natives/EngineObjects.cpp's logic).
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod engine_objects;
// The lobby intro's natives (formerly src/Lobby/Intro.cpp).
#[cfg(all(feature = "plugin", feature = "vrik"))]
mod lobby_intro_natives;
// The hand natives, the overlay's calibration, the FPP component dump and the helpers the C++ natives share
// (formerly src/Natives/Natives.cpp).
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod hand_natives;
// The VRIK and weapon-aim ABI's globals (formerly src/Natives/NativeState.cpp).
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod vrik_state_exports;
// Finding the player, its controller, the world's animation system and an entity's animated objects (formerly
// src/Natives/AnimLookup.cpp's lookups).
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod anim_lookup;
// The Osprey's screen hinge requested by the optics (VRPhysicalOpticsHinge; the pose pass's half is cvr-rig's).
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod optic_hinge;
// The animation inputs: graph variables, tracks, features, UpdateVRIKAnimInputs (formerly src/Natives/AnimGraph.cpp's and
// AnimFeatures.cpp's natives).
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod anim_inputs;
// The animation dumps (formerly src/Natives/AnimDumps.cpp's natives and AnimGraph.cpp's dumps).
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod anim_dumps;
// The player's flags for the camera from RTTI (formerly src/Camera/PlayerCameraState.cpp).
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod player_camera_state;
// The HUD panel's natives and hooks (formerly src/Natives/HudPanel.cpp).
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod hud_natives;
// The first-launch setup's and the VR overlay's natives (formerly src/Natives/BodySetup.cpp, VrOverlay.cpp).
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod body_setup;
#[cfg(feature = "rig")]
pub use cvr_rig as rig;
// NVIDIA Reflex's options policy (include/Hooks/ReflexOptions.hpp), in every build; its hook is rig::vrik::reflex_hook.
pub use cvr_rig::reflex;
pub use cvr_settings as settings;
pub use cvr_xr as xr;
// The Direct3D 12 helpers (include/Render/CommandResources.hpp).
pub use cvr_gpu as gpu;
// The render passes' policies (include/Render/FixedVrs.hpp, FoveatedDlss.hpp).
pub use cvr_render as render;
#[cfg(all(feature = "plugin"))]
pub mod plugin;
// The port's own loader: RED4ext's part built in (bin/x64/powrprof.dll calls it).
#[cfg(all(feature = "plugin"))]
pub mod loader;
// HookServices.cpp's RTTI and engine-object services (the plugin's; vrik_tests keeps its stubs).
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod hook_services;
// The natives' registration (formerly src/Natives/Registration.cpp): the DLL with the Rust solve.
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod natives;
// The NPC grips' natives (main's NpcArmGrip.cpp): the person's animated object found by the animation lookup.
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod npc_grip_natives;
// The blade's natives (main's BladeContactNative.cpp): the hit representation's shapes, the sweeps, the lodged joint.
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod blade_natives;
// The blade's marks on the struck person's own mesh (main's BladeMesh*.cpp).
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod blade_mesh;
// main's VRVehiclePartNear (VehicleHands.cpp): the panel of a car at a point.
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod vehicle_parts;
// The CET mods' mechanics moved to Rust (stage 6), and the script calls they make.
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod mechanics;
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub mod script_call;
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub(crate) mod script_hooks;
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub(crate) mod audit;
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub(crate) mod class_probe;
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub(crate) mod script_classes;
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub(crate) mod tdb_names;
#[cfg(all(feature = "plugin", feature = "vrik"))]
pub(crate) mod cet_migration;

/// Bumped whenever a signature or struct layout in include/Rust/CvrRust.h changes; C++ checks CVR_RUST_ABI_VERSION.
pub const ABI_VERSION: u32 = 2;

#[no_mangle]
pub extern "C" fn cvr_rust_abi_version() -> u32 {
    ABI_VERSION
}
