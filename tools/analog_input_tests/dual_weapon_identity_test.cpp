#include "Hooks/DualWieldWeaponIdentity.hpp"
#include <iostream>
#include <stdexcept>
static void Check(bool yes, const char* why) { if (!yes) throw std::runtime_error(why); }
int main() try {
    using namespace cvr::input;
    // Two instances of the SAME weapon record, automatic Hurricane shots after
    // both triggers were released. Side comes from each effect's weapon, not
    // input state, firing order, or the number of pellets/rays that preceded it.
    constexpr uintptr_t left=0x10000, right=0x20000, foreign=0x30000;
    PublishDualWeapons(left, right);
    for (uintptr_t shot : {right, right, left, left, left, right, left})
        Check(ShotWeaponSide(shot) == (shot == left ? WeaponSide::Left : WeaponSide::Right),
            "actual firing instance lost under repeated/asymmetric shots");
    Check(ShotWeaponSide(foreign) == WeaponSide::Unknown, "foreign effect claimed a hand");
    Check(ShotWeaponSide(0) == WeaponSide::Unknown, "empty effect claimed a hand");
    PublishDualWeapons(right, left);
    Check(ShotWeaponSide(left) == WeaponSide::Right && ShotWeaponSide(right) == WeaponSide::Left,
        "swapped attachments retained old identity");
    PublishDualWeapons(0,0);
    Check(ShotWeaponSide(left) == WeaponSide::Unknown, "unequip retained an old gun");
    Check(ShotWeaponSide(left,left,left) == WeaponSide::Unknown, "ambiguous attachment accepted");
    std::cout << "PASS per-effect weapon identity, repeated rays, swaps, foreign effects and release\n";
    return 0;
} catch (const std::exception& e) { std::cerr << e.what() << '\n'; return 1; }
