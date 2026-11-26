#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// The haptics' controls and read-outs (src/Runtimes/Haptics.cpp, or with CVR_RUST_VRIK its Rust port,
// rust/crates/cvr-rig/src/vrik/haptics.rs): exported for the ini, the tuner and the tools, the same objects in both builds.
#if !CVR_RUST_VRIK  // the Rust build's are rust/crates/cvr-rig/src/vrik/state_globals.rs
#include <cstdint>

extern "C" {
// Master switch (ini xr_haptics) and the strength of everything (ini xr_haptic_scale, 0..2).
__declspec(dllexport) int   CyberpunkVR_Haptics = 1;
__declspec(dllexport) float CyberpunkVR_HapticScale = 1.0f;
// THE SECOND HAND IN A TWO-HANDED GRIP feels the round as the trigger hand does: in 2H the weapon is
// in both hands, held in one it is in one (2026-10-17). This is its share; 1 = the same.
__declspec(dllexport) float CyberpunkVR_HapticSupportMul = 1.0f;
// THE PROFILES, live-tunable: per profile three segments of (amplitude 0..1, milliseconds); an
// amplitude of 0 is a gap. Rows: 0 handgun, 1 rifle/SMG/LMG, 2 shotgun, 3 sniper/precision,
// 4 belt touch, 5 melee block, 6 melee parry, 7 bullet block, 8 bullet deflect, 9 grenade pin (the
// pulling hand), 10 grenade pin (the holding hand), 11 VR menu click, 12 VR menu hover, 13 VR menu
// slider step, 14 melee hit, 15 melee swing. A segment under ~12 ms can fall between two XR frames and never
// play, so none is shorter.
__declspec(dllexport) float CyberpunkVR_HapticProfiles[16][6] = {
    { 0.85f, 28.0f,  0.25f, 30.0f,  0.00f,  0.0f },   // handgun: one crisp snap
    { 0.60f, 22.0f,  0.00f,  0.0f,  0.00f,  0.0f },   // rifle: short, so full-auto stays a rattle of rounds
    { 1.00f, 55.0f,  0.55f, 70.0f,  0.20f, 60.0f },   // shotgun: a heavy thump that rolls off
    { 1.00f, 45.0f,  0.60f, 60.0f,  0.25f, 90.0f },   // sniper: the hardest kick and the longest tail
    { 0.30f, 14.0f,  0.00f,  0.0f,  0.00f,  0.0f },   // belt: a light tick
    { 0.90f, 40.0f,  0.40f, 50.0f,  0.00f,  0.0f },   // melee block: a dull thud
    { 1.00f, 20.0f,  0.00f, 35.0f,  0.75f, 25.0f },   // melee parry: a double "clang"
    { 0.65f, 30.0f,  0.20f, 30.0f,  0.00f,  0.0f },   // bullet block: a short knock
    { 1.00f, 15.0f,  0.45f, 25.0f,  0.00f,  0.0f },   // bullet deflect: a sharp ping
    { 1.00f, 18.0f,  0.35f, 30.0f,  0.00f,  0.0f },   // grenade pin: the snap of the ring coming free
    { 0.45f, 25.0f,  0.00f,  0.0f,  0.00f,  0.0f },   // grenade pin, felt through the grenade
    { 0.80f, 16.0f,  0.00f, 12.0f,  0.35f, 12.0f },   // menu click: a press and its release
    { 0.15f, 12.0f,  0.00f,  0.0f,  0.00f,  0.0f },   // menu hover: a faint tick on entering a control
    { 0.22f, 12.0f,  0.00f,  0.0f,  0.00f,  0.0f },   // menu slider: one tick per step crossed
    { 1.00f, 35.0f,  0.50f, 55.0f,  0.00f,  0.0f },   // melee hit: the blow landing, then its weight
    { 0.50f, 45.0f,  0.00f,  0.0f,  0.00f,  0.0f },   // melee swing: the weapon cutting the air
};
// The off hand is on the weapon (src/Anim/TwoHandGrip.cpp).
__declspec(dllexport) extern int CyberpunkVR_TwoHandActive;
// [0] events accepted, [1] segments applied left, [2] right, [3] last xrApplyHapticFeedback result,
// [4] last profile row, [5] dropped (switched off, no action, no session), [6] shots, [7] events.
__declspec(dllexport) int   CyberpunkVR_HapticStats[8] = {};
}
#endif
#endif
