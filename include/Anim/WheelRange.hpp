#pragma once
#include <algorithm>
#include <cmath>

namespace cvr::anim {
inline constexpr float WheelFullLockMin=15.f;
inline constexpr float WheelFullLockMax=180.f;
inline constexpr float WheelFullLockDefault=180.f;
// A MOTORCYCLE'S BARS: full lock is reached after this many degrees of yaw of the line between the hands. Small, because
// arms only push and pull so far: hands 50 cm apart at 25 degrees are about 12 cm forward and 12 cm back.
inline constexpr float BikeBarLockMin=15.f;
inline constexpr float BikeBarLockMax=60.f;
inline constexpr float BikeBarLockDefault=25.f;
// LEANING THE BODY ON A BIKE. Share of full lock that a sideways lean of BikeLeanFullM adds to the arms' steering; 0 is off.
inline constexpr float BikeBodyLeanDefault=.5f;
inline constexpr float BikeLeanDeadM=.03f;
inline constexpr float BikeLeanFullM=.15f;
inline float BikeBodyLeanShare(float value) {
    return std::isfinite(value) && value>=0 ? std::min(value,1.f):BikeBodyLeanDefault;
}
// Degrees of steering for a sideways lean in metres, right positive. Never more than one full lock.
inline float BikeLeanDegrees(float sideways,float share,float limit) {
    if(!std::isfinite(sideways))return 0;
    const float amount=std::max(std::abs(sideways)-BikeLeanDeadM,0.f)/(BikeLeanFullM-BikeLeanDeadM);
    return std::copysign(std::min(amount*BikeBodyLeanShare(share)*limit,limit),sideways);
}
inline float BikeBarLockDegrees(float value) {
    return std::isfinite(value) && value>0 ? std::clamp(value,BikeBarLockMin,BikeBarLockMax):BikeBarLockDefault;
}

// Physical hand travel from the grab reference to one full steering lock.
// This normalizes input only; cabin wheel travel is authored in vehicle graphs.
inline float WheelFullLockDegrees(float value) {
    return std::isfinite(value) && value>0
        ? std::clamp(value,WheelFullLockMin,WheelFullLockMax):WheelFullLockDefault;
}
// THE CABIN WHEEL'S ANGLE FOR A STEERING INPUT. The common car graph turns steering_wheel_turn by a curve of the drive
// model's turn input, not in proportion to it: at the game's post-deadzone input n (0..1) the wheel stands at
// 89.9 n + 40.1 n^3 degrees -- measured 2026-11-05 on the Porsche 911 and the Thorton Mackinaw, within 0.2 degrees at
// every tenth of the range (docs/wheel-range-20261011.md), with the graph's gain (mulFactor) at 2: full lock 130 = the
// native 65 doubled. The gain is 2.5 since 2026-11-05 ("the wheel is too sharp, a little less";
// tools/driving_tests/wheel_ratio_graph.py): the cabin wheel, and the hands holding it, turn 25 % further for the same
// road-wheel angle, full lock 162.5. The curve scales with the gain.
inline constexpr float CarCabinGain=2.5f;
inline constexpr float CarCabinLinearDeg=89.9f*CarCabinGain/2;
inline constexpr float CarCabinCubicDeg=40.1f*CarCabinGain/2;
inline constexpr float CarCabinWheelLockDeg=CarCabinLinearDeg+CarCabinCubicDeg;
inline float CarCabinAngleForInput(float n) {
    if(!std::isfinite(n))return 0;
    const float a=std::clamp(std::abs(n),0.f,1.f);
    return std::copysign(CarCabinLinearDeg*a+CarCabinCubicDeg*a*a*a,n);
}
// The input that stands the cabin wheel at `degrees`, so it turns with the hands holding it: a linear input left the
// wheel up to 15 degrees behind them in mid-travel. Newton from the linear guess; the curve rises everywhere (its
// slope never drops below 89.9 deg per unit), so four steps land within 1e-5 of the root.
inline float CarInputForCabinAngle(float degrees) {
    if(!std::isfinite(degrees))return 0;
    const float target=std::min(std::abs(degrees),CarCabinWheelLockDeg);
    float n=target/CarCabinWheelLockDeg;
    for(int i=0;i<4;++i) {
        const float error=CarCabinLinearDeg*n+CarCabinCubicDeg*n*n*n-target;
        n-=error/(CarCabinLinearDeg+3*CarCabinCubicDeg*n*n);
    }
    return std::copysign(std::clamp(n,0.f,1.f),degrees);
}
inline float WheelFullLockForVehicle(float value,bool car) {
    // A CAR'S WHEEL TURNS WITH THE HANDS, ONE TO ONE. The hands at full lock after 180 degrees while the cabin
    // wheel turns 130 left the wheel -- and the hands the animation holds on it -- 28 % behind the controllers
    // all the way round: play the player felt as a wheel that slips (2026-11-04: "a huge play appeared"). Full
    // lock is where the cabin wheel's is. Bikes/other mounts retain the setting's valid range and the 90-degree
    // fallback, with no doubled handlebar animation.
    return car ? CarCabinWheelLockDeg:
        (std::isfinite(value)&&value>=30.f&&value<=120.f ? value:90.f);
}
}
