#pragma once
#include "Anim/VehiclePosePolicy.hpp"
#include <cmath>
#if CVR_RUST_ROOM
#include "Rust/CvrRust.h"
#endif

namespace cvr::input {
inline bool VehicleOwnsTurn(bool mounted,int vehicleState) {
    return mounted || cvr::anim::IsPassengerWindowCombat(vehicleState);
}
inline bool NativeOwnsTurn(bool mounted,int vehicleState,bool onLadder) {
    return onLadder || VehicleOwnsTurn(mounted,vehicleState);
}
struct TurnInput { float axis{},snapDegrees{}; bool clearPending{}; };
#if CVR_RUST_ROOM
static_assert(sizeof(TurnInput) == 12);
inline TurnInput RouteTurn(float axis,bool snapEnabled,int& armedDirection,
                           float angle,float fire,float rearm) {
    TurnInput out;   // rust/crates/cvr-body/src/policy.rs
    int32_t armed=armedDirection;
    cvr_route_turn(axis,snapEnabled,&armed,angle,fire,rearm,&out);
    armedDirection=armed;
    return out;
}
#else
inline TurnInput RouteTurn(float axis,bool snapEnabled,int& armedDirection,
                           float angle,float fire,float rearm) {
    if (!(fire>.05f) || fire>1.0f) fire=.90f;
    if (!(rearm>=0.0f) || rearm>=fire) rearm=fire*.55f;
    if (!snapEnabled) {
        // Native vehicle look keeps the axis. A held stick must also be released
        // before it can snap after leaving the vehicle or another native mode.
        armedDirection=std::abs(axis)<rearm ? 0 : (axis>0 ? 1 : -1);
        return {axis,0,true};
    }
    const int direction=axis>fire ? 1 : (axis<-fire ? -1 : 0);
    if (std::abs(axis)<rearm) armedDirection=0;
    float delta=0;
    if (direction && direction!=armedDirection) {
        armedDirection=direction;
        delta=-float(direction)*(angle>0 ? angle : 30.0f);
    }
    return {0,delta,false};
}
#endif
}
