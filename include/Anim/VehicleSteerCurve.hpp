#pragma once

// THE ROAD WHEELS AGAINST THE CABIN WHEEL, for the car the player drives with a hand on the wheel.
// See src/Anim/VehicleSteerCurve.cpp.

namespace cvr::anim {

// Once per XInput poll. `held`: the player drives a car with a hand owning its wheel. Applies the VR steering
// curve to that car's physics while held and puts the car's own values back the moment it is not.
void VehicleSteerCurveTick(bool held);

}  // namespace cvr::anim
