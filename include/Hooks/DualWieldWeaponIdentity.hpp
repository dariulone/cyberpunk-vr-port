#pragma once
#include <atomic>
#include <cstdint>

namespace cvr::input {
enum class WeaponSide { Unknown = -1, Left = 0, Right = 1 };
inline std::atomic<uintptr_t> dualWeaponLeft{0}, dualWeaponRight{0};

inline WeaponSide ShotWeaponSide(uintptr_t shotWeapon, uintptr_t left, uintptr_t right) {
    if (!shotWeapon || !left || !right || left == right) return WeaponSide::Unknown;
    if (shotWeapon == left) return WeaponSide::Left;
    if (shotWeapon == right) return WeaponSide::Right;
    return WeaponSide::Unknown;
}
// Published attachment instances are compared only, never dereferenced or held
// alive. AttackData owns the shot's weapon identity, even when its triggers have
// since been released (Heywood Hurricane) or both records name the same model.
inline void PublishDualWeapons(uintptr_t left, uintptr_t right) {
    dualWeaponLeft.store(left, std::memory_order_release);
    dualWeaponRight.store(right, std::memory_order_release);
}
inline WeaponSide ShotWeaponSide(uintptr_t shotWeapon) {
    return ShotWeaponSide(shotWeapon, dualWeaponLeft.load(std::memory_order_acquire),
        dualWeaponRight.load(std::memory_order_acquire));
}
} // namespace cvr::input
