#pragma once

#include <cmath>
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/Generated/Vector4.hpp>
#include <RED4ext/Scripting/Natives/Generated/Quaternion.hpp>
#include <RED4ext/Scripting/Natives/Generated/QsTransform.hpp>

namespace VRIK {

    // Helper: Normalize Vector4
    inline RED4ext::Vector4 Normalize(const RED4ext::Vector4& v) {
        float length = std::sqrt(v.X * v.X + v.Y * v.Y + v.Z * v.Z);
        if (length > 0.0001f) {
            return {v.X / length, v.Y / length, v.Z / length, 0.0f};
        }
        return {0.0f, 0.0f, 0.0f, 0.0f};
    }

    // Helper: Distance between two Vector4
    inline float Distance(const RED4ext::Vector4& a, const RED4ext::Vector4& b) {
        float dx = b.X - a.X;
        float dy = b.Y - a.Y;
        float dz = b.Z - a.Z;
        return std::sqrt(dx * dx + dy * dy + dz * dz);
    }

    // Two-Bone IK Solver (Law of Cosines)
    // shoulderPos: World position of the shoulder bone
    // targetPos: World position of the VR controller (wrist target)
    // poleVector: Direction where the elbow should point
    // upperArmLength: Length from shoulder to elbow
    // forearmLength: Length from elbow to wrist
    // outElbowPos: Calculated world position of the elbow
    inline bool SolveTwoBoneIK(
        const RED4ext::Vector4& shoulderPos,
        const RED4ext::Vector4& targetPos,
        const RED4ext::Vector4& poleVector,
        float upperArmLength,
        float forearmLength,
        RED4ext::Vector4& outElbowPos) 
    {
        float targetDist = Distance(shoulderPos, targetPos);
        float maxLength = upperArmLength + forearmLength;
        
        RED4ext::Vector4 clampedTarget = targetPos;
        
        // Prevent stretching beyond arm length
        if (targetDist >= maxLength) {
            RED4ext::Vector4 dir = {targetPos.X - shoulderPos.X, targetPos.Y - shoulderPos.Y, targetPos.Z - shoulderPos.Z, 0.0f};
            dir = Normalize(dir);
            clampedTarget.X = shoulderPos.X + dir.X * (maxLength - 0.001f);
            clampedTarget.Y = shoulderPos.Y + dir.Y * (maxLength - 0.001f);
            clampedTarget.Z = shoulderPos.Z + dir.Z * (maxLength - 0.001f);
            targetDist = maxLength - 0.001f;
        }

        // Law of Cosines to find angle at the shoulder
        // c^2 = a^2 + b^2 - 2ab*cos(C)
        // C = acos((a^2 + b^2 - c^2) / 2ab)
        float a = upperArmLength;
        float b = targetDist;
        float c = forearmLength;

        float cosAngle = (a * a + b * b - c * c) / (2.0f * a * b);
        cosAngle = std::fmax(-1.0f, std::fmin(1.0f, cosAngle)); // Clamp between -1 and 1
        float shoulderAngle = std::acos(cosAngle);

        // Direction from shoulder to target
        RED4ext::Vector4 shoulderToTarget = {
            clampedTarget.X - shoulderPos.X,
            clampedTarget.Y - shoulderPos.Y,
            clampedTarget.Z - shoulderPos.Z,
            0.0f
        };
        shoulderToTarget = Normalize(shoulderToTarget);

        // Find right vector relative to pole vector and target direction
        // Cross product: Pole x ShoulderToTarget
        RED4ext::Vector4 right = {
            poleVector.Y * shoulderToTarget.Z - poleVector.Z * shoulderToTarget.Y,
            poleVector.Z * shoulderToTarget.X - poleVector.X * shoulderToTarget.Z,
            poleVector.X * shoulderToTarget.Y - poleVector.Y * shoulderToTarget.X,
            0.0f
        };
        right = Normalize(right);

        // Orthogonalize pole vector
        // Cross product: ShoulderToTarget x right
        RED4ext::Vector4 up = {
            shoulderToTarget.Y * right.Z - shoulderToTarget.Z * right.Y,
            shoulderToTarget.Z * right.X - shoulderToTarget.X * right.Z,
            shoulderToTarget.X * right.Y - shoulderToTarget.Y * right.X,
            0.0f
        };
        up = Normalize(up);

        // Calculate elbow position in the plane defined by (shoulderToTarget, up)
        float elbowXDistance = std::cos(shoulderAngle) * a;
        float elbowYDistance = std::sin(shoulderAngle) * a;

        outElbowPos.X = shoulderPos.X + (shoulderToTarget.X * elbowXDistance) + (up.X * elbowYDistance);
        outElbowPos.Y = shoulderPos.Y + (shoulderToTarget.Y * elbowXDistance) + (up.Y * elbowYDistance);
        outElbowPos.Z = shoulderPos.Z + (shoulderToTarget.Z * elbowXDistance) + (up.Z * elbowYDistance);
        outElbowPos.W = 1.0f;

        return true;
    }
}
