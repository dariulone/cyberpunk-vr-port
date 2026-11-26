#pragma once
// The Rust half of the transitional DLL (rust/crates/cvr-ffi, built as cvr_rust.lib), as the C++ half calls it.
// docs/rust-migration-plan.md: a C++ implementation stays beside its Rust one until the two agree on the same inputs,
// then the C++ callers go through here and the C++ copy is removed.
//
// Every declaration has its twin in rust/crates/cvr-ffi/src/*.rs. A changed signature or layout bumps
// CVR_RUST_ABI_VERSION on both sides.
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
#include <cstddef>
extern "C" {
#else
#include <stdbool.h>
#endif

#define CVR_RUST_ABI_VERSION 2u

// ---- host (rust/crates/cvr-ffi/src/host.rs) --------------------------------------------------------------------
// The plugin's log: `len` bytes of text, lines ending in '\n' within it. Log() in Core/Log.cpp formats and calls this.
void cvr_log_write(const char* text, size_t len);
void cvr_log_flush(void);
// A plugin folder (cvr::paths::Folder order) with a trailing backslash, valid for the process: UTF-16 and ANSI.
const uint16_t* cvr_paths_dir_w(uint32_t folder);
const char* cvr_paths_dir_a(uint32_t folder);

// ---- engine (rust/crates/cvr-ffi/src/engine.rs) ------------------------------------------------------------------
// The build gate (cvr::game_build forwards here).
bool cvr_game_build_supported(void);
bool cvr_game_build_overridden(void);
bool cvr_game_build_unmapped(uintptr_t rva);
// The plugin's one trampoline arena (rust/crates/cvr-engine/src/trampoline.rs): src/Hooks/Trampoline.cpp forwards here.
void* cvr_engine_allocate_trampoline(void* target, size_t size);
const char* cvr_game_build_expected(void);
const char* cvr_game_build_running(void);

// ---- body (rust/crates/cvr-ffi/src/body.rs), used with CVR_RUST_BODY -------------------------------------------------
// The objects are the C++ classes themselves (include/Runtimes/BodyFollowZone.hpp and the rest): Rust has their layouts.
void cvr_body_follow_zone_reset(void* zone);
bool cvr_body_follow_zone_following(const void* zone);
float cvr_body_follow_zone_step(void* zone, float residual, float radius, float dt);
void cvr_body_yaw_follower_set_enabled(void* follower, bool enabled);
void cvr_body_yaw_follower_reset(void* follower);
void cvr_body_yaw_follower_observe(void* follower, float offset, uint64_t origin);
float cvr_body_yaw_follower_step(void* follower, float headYaw, float cone, uint64_t stamp, uint64_t origin);
void cvr_body_yaw_prediction_reset(void* prediction, float seed);
void cvr_body_yaw_prediction_suspend(void* prediction);
float cvr_body_yaw_prediction_update(void* prediction, float measured, float dt, bool accepted);
float cvr_body_yaw_prediction_sample(const void* prediction, float ageSeconds);
bool cvr_tracking_frame_fresh(const void* frame, uint64_t currentOrigin, uint64_t nowUs);
void cvr_tracked_body_yaw_reset(void* tracked, float seed, bool useAbsoluteInitial);
void cvr_tracked_body_yaw_suspend(void* tracked);
void cvr_tracked_body_yaw_predict(const void* tracked, uint64_t nowUs, void* estimateOut);
void cvr_tracked_body_yaw_update(void* tracked, const void* frame, void* estimateOut);
int32_t cvr_rotation_mode_from_flags(int32_t physical, int32_t tracked, int32_t hybrid, int32_t head_led);
bool cvr_needs_body_tracking(int32_t mode);
void cvr_hybrid_body_yaw_reset(void* hybrid);
void cvr_hybrid_body_yaw_suspend(void* hybrid, bool keepAlignment);
void cvr_hybrid_body_yaw_step(void* hybrid, const void* input, void* commandOut);
float cvr_head_down_degrees(float x, float y, float z, float w);
float cvr_look_down_free_look_cone(float downDegrees, float normalDegrees, float loweredDegrees, float bodyBendRadians);
float cvr_body_free_look_cone(bool swimming, float swimmingDegrees, float downDegrees, float normalDegrees,
                              float loweredDegrees, float bodyBendRadians);
float cvr_body_smooth_range(float value, float from, float to);
float cvr_body_bend_weight(float angle);
float cvr_body_hinge_drop(float angle);
void cvr_bend_tracker_set_squat_depth(void* tracker, float depth);
float cvr_bend_tracker_neck_drop(const void* tracker);
void cvr_bend_tracker_update(void* tracker, const float* head3, const float* q4, uint64_t origin, bool swimming,
                             bool walking, void* sampleOut);

// ---- room (rust/crates/cvr-ffi/src/room.rs), used with CVR_RUST_ROOM ---------------------------------------------------
// Roomscale, the crouch, the native heading, hand reacquisition, the view's anchor and frames, scene/input policies.
// Objects are the C++ classes themselves (void*); the small value types are these C mirrors of them.
typedef struct CvrVec2 { float x, y; } CvrVec2;
typedef struct CvrAnchorVector { float x, y, z; } CvrAnchorVector;
CvrVec2 cvr_room_rotate(CvrVec2 v, float yaw);
float cvr_room_tracking_yaw(float entityYaw, float bodyRealign);
void cvr_room_follow_reset(void* follow);
void cvr_room_follow_head_motion(void* follow, CvrVec2 neck, bool upright, float dt);
bool cvr_room_follow_head_still(const void* follow);
void cvr_room_follow_block(void* follow, CvrVec2 refused);
CvrVec2 cvr_room_follow_step(void* follow, CvrVec2 error, float radius, float dt);
void cvr_room_walk_gate_reset(void* gate);
void cvr_room_walk_gate_update(void* gate, CvrVec2 velocity, float dt);
CvrVec2 cvr_room_walk_gate_reported(const void* gate);
bool cvr_room_head_yaw(float x, float y, float z, float w, float* yaw);
CvrVec2 cvr_room_eye_from_neck(float x, float y, float z, float w);
void cvr_room_movement_frame_sample(const void* tracking, const void* frame, void* out);
void cvr_room_movement_reset(void* movement);
void cvr_room_movement_suspend(void* movement);
void cvr_room_movement_begin(void* movement, const void* sample, uint64_t nowUs, float dt, float scale, float trackingYaw,
                             bool allowed, float freeRadius, bool strict, void* stepOut);
void cvr_room_movement_complete(void* movement, const void* step, CvrVec2 acceptedWorld, const void* limit);
bool cvr_room_movement_rest_neck(const void* movement, uint64_t origin, CvrVec2* out);
bool cvr_room_movement_following(const void* movement);
CvrVec2 cvr_room_accepted_displacement(CvrVec2 native, CvrVec2 physical, CvrVec2 resolved);
CvrVec2 cvr_room_physical_velocity(CvrVec2 native, CvrVec2 physical, CvrVec2 resolvedVelocity, float dt);
float cvr_crouch_threshold(float value);
float cvr_crouch_depth(float neckDrop, const void* bendSample);
void cvr_crouch_reset(void* crouch);
bool cvr_crouch_update(void* crouch, float depth, float threshold, uint64_t now, uint64_t origin, uint64_t owner,
                       bool allowed);
void cvr_native_heading_reset(void* heading);
bool cvr_native_heading_matches(const void* heading, uintptr_t owner, uint64_t origin);
bool cvr_native_heading_owned_by(const void* heading, uintptr_t owner);
bool cvr_native_heading_observe(void* heading, uintptr_t owner, uint64_t origin, float bodyYaw);
void cvr_native_heading_commit(void* heading, float bodyYaw, float nativeTurn);
float cvr_native_heading_offset(const void* heading);
float cvr_native_heading_wrap(float yaw);
float cvr_smooth_damp(float current, float target, float* velocity, float smoothTime, float dt);
void cvr_reacquire_reset(void* reacquire);
void cvr_reacquire_step(void* reacquire, const void* tracked, bool valid, float dt, void* out);
float cvr_exponential_follow(float rate, float dt);
CvrAnchorVector cvr_anchor_rotate_yaw(CvrAnchorVector v, float yaw);
CvrAnchorVector cvr_anchor_compose(CvrAnchorVector headResidual, CvrAnchorVector trackingOffset,
                                   CvrAnchorVector modelOffset, float trackingYaw, float modelYaw);
CvrAnchorVector cvr_anchor_compose_view(CvrAnchorVector residual, const void* recipe, float trackingYaw, float bodyYaw);
CvrAnchorVector cvr_anchor_orbit_neck_lever(const void* recipe, const void* restNeck);
CvrAnchorVector cvr_anchor_orbit_model(const void* recipe, float bodyYaw, float trackingYaw, const void* restNeck);
void cvr_anchor_make_recipe(CvrAnchorVector manual, CvrAnchorVector vehicle, CvrAnchorVector baked, bool inVehicle,
                            float scale, float trackingYaw, float modelYaw, void* recipeOut);
CvrAnchorVector cvr_anchor_absolute_bake(CvrAnchorVector active, CvrAnchorVector target, CvrAnchorVector rendered);
void cvr_hand_view_frame(CvrAnchorVector bodyBase, const void* recipe, CvrAnchorVector residual, const void* head,
                         float bodyYaw, float trackingYaw, const void* pitch, void* frameOut);
void cvr_camera_entity_sample(const int32_t* centre3, const int32_t* bodyBase3, const int32_t* entity3,
                              const float* cameraQuat4, const float* entityQuat4, void* out);
int32_t cvr_camera_packet_status(uintptr_t owner, uint64_t origin, uint64_t stamp, uintptr_t currentOwner,
                                 uint64_t currentOrigin, uint64_t now, bool valid, bool unavailable);
bool cvr_use_native_main_heading(bool mounted, int32_t tier, bool device, bool externalScene);
bool cvr_composition_base_yaw(const float* composed4, const float* head4, float* yaw);
void cvr_vehicle_heading_set_source(void* heading, uintptr_t player, uintptr_t camera);
bool cvr_vehicle_heading_observe(void* heading, uintptr_t camera, const float* q4, const float* entityQ4);
void cvr_vehicle_heading_for_frame(void* heading, uint64_t epoch, const float* entityQ4, float publishedYaw,
                                   float nativeOffset, void* selectionOut);
void* cvr_eye_ledger_new(void);
void cvr_eye_ledger_free(void* ledger);
void cvr_eye_ledger_publish(const void* ledger, const int32_t* eye3, const int32_t* centre3, const int32_t* bodyBase3);
bool cvr_eye_ledger_read(const void* ledger, const int32_t* eye3, int32_t* centre3, int32_t* bodyBase3);
bool cvr_eye_ledger_read_world(const void* ledger, const float* query3, float* centre3, float* bodyBase3);
bool cvr_should_suspend_vrik(int32_t engineTier, int32_t configuredTier);
bool cvr_should_suspend_vrik_for_takedown(int32_t state, int32_t enabled);
bool cvr_should_suspend_vrik_for_body_carry(int32_t carrying, int32_t disposal, int32_t locomotion, int32_t enabled);
bool cvr_should_suspend_vrik_for_finisher(int32_t active, uint64_t ageMs, int32_t enabled);
bool cvr_should_hand_right_arm_to_body_carry(int32_t carrying, int32_t bothHands, int32_t enabled);
int32_t cvr_resolve_vehicle_vrik_suspend_tier(int32_t onFoot, int32_t vehicle, bool explicitlyConfigured);
void cvr_route_turn(float axis, bool snapEnabled, int32_t* armedDirection, float angle, float fire, float rearm,
                    void* turnOut);
void cvr_vehicle_button_policy(bool mounted, int32_t state, void* buttonsOut);
bool cvr_vehicle_exit_hold_update(void* hold, bool allowed, bool xHeld, uint64_t now, uint64_t holdMs);

// ---- input (rust/crates/cvr-ffi/src/input.rs) --------------------------------------------------------------------

typedef struct CvrKeypadPoint { float x, y; } CvrKeypadPoint;
typedef struct CvrKeypadCursor {
    CvrKeypadPoint position, previousHead;
    bool initialized, hadHead;
} CvrKeypadCursor;
typedef struct CvrWeaponTrigger { bool active, down, ready; } CvrWeaponTrigger;

uint32_t cvr_rust_abi_version(void);

float cvr_stick_deadzone(float value);
float cvr_stick_full_input(float value);
float cvr_analog_axis(float value, float deadzone, float fullInput);
bool cvr_use_analog_movement(int32_t mode, bool vehicle);
bool cvr_at_full_travel(float value, bool analog, float fullInput, bool negative);

bool cvr_keypad_input_fresh(uint64_t stamp, uint64_t now);
float cvr_keypad_cursor_axis(float value, float deadzone);
bool cvr_keypad_cursor_update(CvrKeypadCursor* cursor, bool headHit, CvrKeypadPoint head, CvrKeypadPoint stick,
                              CvrKeypadPoint size, float dt);

bool cvr_weapon_trigger_update(CvrWeaponTrigger* trigger, bool enabled, float value);

// ---- the full-body tracking switches (rust/crates/cvr-settings/src/fbt_settings.rs): 0 legs, 1 spine, 2 arms -------
bool cvr_fbt_switch(uint32_t which);
void cvr_fbt_set_switch(uint32_t which, bool on);

#ifdef __cplusplus
}
static_assert(sizeof(CvrKeypadPoint) == 8 && alignof(CvrKeypadPoint) == 4);
static_assert(sizeof(CvrKeypadCursor) == 20 && offsetof(CvrKeypadCursor, previousHead) == 8 &&
              offsetof(CvrKeypadCursor, initialized) == 16 && offsetof(CvrKeypadCursor, hadHead) == 17);
static_assert(sizeof(CvrWeaponTrigger) == 3);
#endif
