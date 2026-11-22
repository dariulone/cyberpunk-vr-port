#include "Runtimes/CyberFingerHintBindings.hpp"
#include <stdexcept>

using namespace cvr::cyberfinger;
int main() {
    auto check = [](bool ok) { if (!ok) throw std::runtime_error("incorrect CF hint"); };
    check(PhysicalHintSource("Left Hand", "B (Menu: context dial) Click") == LeftMenu);
    check(PhysicalHintSource("Right Hand", "Grip (the grab button, as pressed)") == RightGrip);
    check(PhysicalHintSource("Left Hand", "Trigger Touch") == 0);
    check(PhysicalHintSource("Other", "Trigger") == 0);
    HintLayout s;
    s.seated = s.driving = s.exitGrip = s.defaultExitTime = true;
    check(ControlHint("EnterCombatMode", false, s).empty());
    s.sources = LeftMenu | LeftGrip;
    check(ControlHint("EnterCombatMode", false, s) == "Tap Left Menu");
    check(ControlHint("Exit", false, s) == "Hold Left Grip 0.4 s");
    s.exitGrip = false;
    check(ControlHint("Exit", true, s).empty());
    s = {};
    s.sources = ~0u;
    s.leftWeapon = true;
    check(ControlHint("Reload", false, s) == "Tap Right Black Wrist");
    s.leftWeapon = false;
    check(ControlHint("Reload", false, s) == "Tap Left Black Wrist");
    check(ControlHint("Choice1", false, s).empty());
    s.interaction = true;
    check(ControlHint("Choice1_Hold", false, s) == "Hold Right Trigger");
    s.leftMouse = true;
    check(ControlHint("Choice2", true, s) == "Hold Left Grip");
    s.dialogue = true;
    check(ControlHint("ChoiceApply", false, s) == "Right Trigger");
    check(ControlHint("Choice2", true, s).empty());
    check(ControlHint("Unknown", true, s).empty());
}
