#pragma once

// The handful of helpers the natives share across their now-separate files. Small on purpose: if
// this grows into a second dumping ground, the family split has failed the same way
// Core/VrCoreShared.hpp's banner warns about.

#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/Generated/Quaternion.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/IOrientationProvider.hpp>

#include <fstream>
#include <cstddef>
#include <cstdint>

// Is this address safe to dereference? The natives walk engine pointers, so every dump guards.
// Signature copied from the definition in src/Natives/Natives.cpp. I first wrote
// `const void*` here from memory; it takes a uintptr_t, because the callers are walking
// raw engine addresses rather than holding pointers.
bool IsReadable(uintptr_t a, size_t n);

// Maps the shared block for the CET bridge; idempotent.
void EnsureSharedMemory();

// A readable name for an RTTI type, for the diagnostic dumps.
const char* GetTypeNameForDump(RED4ext::rtti::IType* aType);

// A provider that always answers with one fixed quaternion -- how a world-space orientation is
// handed to the engine's own IK.
RED4ext::Handle<RED4ext::ent::IOrientationProvider> CreateStaticOrientationProviderQ(const RED4ext::Quaternion& aQuat);

// ---- shared between the native families, signatures copied from the definitions ----
RED4ext::CClass* SafeGetObjectType(void* aPtr);
RED4ext::anim::AnimatedObject* FindAnimatedObjectForEntity(RED4ext::ent::Entity* aEntity, const char* aComponentName, bool aAnyComponent);
RED4ext::anim::AnimatedObject* FindPlayerAnimatedObjectByComponentName(const char* aComponentName);
RED4ext::ent::Entity* FindPlayerEntity();
bool ClassIsA(RED4ext::CClass* type, RED4ext::CName className);
bool ContainsInsensitive(const char* haystack, const char* needle);
bool EqualsInsensitive(const char* a, const char* b);
const char* ClassifyQword(uint64_t v);
int ListAnimatedComponents(RED4ext::ent::Entity* aEntity, char* aOut, size_t aCap, RED4ext::anim::AnimatedObject** aFirstNonRoot);
#if CVR_RUST_VRIK
// rust/crates/cvr-rig/src/vrik/vrik_arming.rs
extern "C" int VRIK_DoArmPlayer();
extern "C" bool VRIK_PlayerBindingChanged();
#else
int VRIK_DoArmPlayer();
bool VRIK_PlayerBindingChanged();
#endif
RED4ext::world::AnimationSystem* ScanForAnimationSystemInBlock(uint8_t* aBase, size_t aSize, std::ofstream* aOut);
bool IsLikelyFppArmComponent(const char* componentName);
extern RED4ext::world::AnimationSystem* g_cachedAnimationSystem;
inline constexpr int VR_CAMTRACE_CAP = 256;   // the camera-trace ring's bound
extern float g_camTrace[VR_CAMTRACE_CAP][22];
extern int   g_camTraceFreeze;
extern int   g_camTraceN;
float SafeReadFloat(uint8_t* base, size_t off);
uint32_t SafeReadU32(uint8_t* base, size_t off);
uint64_t SafeReadQword(uint8_t* base, size_t off);
void PollVRCalibFromShared();

// Writes the VR diagnostic dump. Defined with the calibration family, called from the preamble
#if CVR_RUST_VRIK
// rust/crates/cvr-rig/src/vrik/calibration.rs
extern "C" void WriteVRDiagCore(float camX, float camY, float camZ,
                                float qi, float qj, float qk, float qr);
#else
void WriteVRDiagCore(float camX, float camY, float camZ,
                     float qi, float qj, float qk, float qr);
#endif
