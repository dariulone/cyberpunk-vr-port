#pragma once

namespace cvr::vehiclehands {
// A short script-owned lease consumes grip input, never the hand's collisions.
bool Busy(int side);
void Claim(int side, bool on);
}
