#pragma once
#include "Anim/VehiclePosePolicy.hpp"
#include <cmath>

namespace cvr::input {
inline bool VehicleOwnsTurn(bool mounted,int vehicleState) {
    return mounted || cvr::anim::IsPassengerWindowCombat(vehicleState);
}
inline bool NativeOwnsTurn(bool mounted,int vehicleState,bool onLadder) {
    return onLadder || VehicleOwnsTurn(mounted,vehicleState);
}
// Snap thresholds are raw stick travel, independent of the smooth-look curve.
inline constexpr float kSnapTurnStickFire=.80f;
inline constexpr float kSnapTurnStickRearm=.50f;
struct TurnInput { float axis{},snapDegrees{}; bool clearPending{}; };
inline TurnInput RouteTurn(float axis,bool snapEnabled,int& armedDirection,
                           float angle,float fire,float rearm) {
    if (!std::isfinite(axis)) return {0,0,!snapEnabled};
    if (!(fire>.05f) || fire>1.0f) fire=kSnapTurnStickFire;
    if (!(rearm>=0.0f) || rearm>=fire) rearm=fire*.55f;
    if (!snapEnabled) {
        // Native vehicle look keeps the axis. A held stick must also be released
        // before it can snap after leaving the vehicle or another native mode.
        armedDirection=std::abs(axis)<=rearm ? 0 : (axis>0 ? 1 : -1);
        return {axis,0,true};
    }
    const int direction=axis>=fire ? 1 : (axis<=-fire ? -1 : 0);
    if (std::abs(axis)<=rearm) armedDirection=0;
    float delta=0;
    if (direction && direction!=armedDirection) {
        armedDirection=direction;
        delta=-float(direction)*(std::isfinite(angle) && angle>0 ? angle : 30.0f);
    }
    return {0,delta,false};
}
inline TurnInput RouteControllerTurn(float rawAxis,float lookAxis,bool snapEnabled,int& armedDirection,
                                    float angle,float fire=kSnapTurnStickFire,float rearm=kSnapTurnStickRearm) {
    auto turn=RouteTurn(rawAxis,snapEnabled,armedDirection,angle,fire,rearm);
    if (!snapEnabled) turn.axis=std::isfinite(lookAxis) ? lookAxis : 0;
    return turn;
}
}
