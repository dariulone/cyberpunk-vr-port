#pragma once
#include <cstdint>

// The fingers closed on what a hand holds, from the thing's shape and size (src/Anim/GraspPose.cpp).
namespace cvr::anim {
// Each player pose pass: measures both palms, and writes the grasp of a hand that holds something.
void GraspFingers(uint8_t* boneBuf);
}
