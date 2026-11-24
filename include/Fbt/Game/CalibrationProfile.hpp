#pragma once

#include "Fbt/FbtCalibration.hpp"
#include <filesystem>

namespace cvr::fbt::game {

using Calibration = wrpuppets::fbt::Calibration;

struct ChestCalibration {
    bool valid = false;
    unsigned revision = 0;  // process-local generation for the pose filters
    wrpuppets::fbt::Vec3 offset;
    wrpuppets::fbt::Quat rotationInverse, referenceRotation;
};

struct CalibrationProfile {
    Calibration body;
    ChestCalibration chest;
};

enum class ProfileRead { Loaded, Missing, Invalid };
ProfileRead LoadCalibrationProfile(const std::filesystem::path& path, CalibrationProfile& profile);
bool SaveCalibrationProfile(const std::filesystem::path& path, const CalibrationProfile& profile);

}  // namespace cvr::fbt::game
