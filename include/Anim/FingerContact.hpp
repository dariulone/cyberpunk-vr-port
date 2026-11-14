#pragma once
#include <cstdint>

// The fingers bent on the world (src/Anim/FingerContact.cpp; solved in Natives/PhysHands.cpp on its own thread).
namespace cvr::anim {
// Each player pose pass, after every finger layer: publishes both hands' fingers as drawn and turns their joints by the
// latest solve. `busy` per side (0 left, 1 right): the fingers belong to something else this pass (a carried body, a
// native reload's hand-off), so they are left alone.
void FingerContact(uint8_t* boneBuf, const bool* busy);
}
