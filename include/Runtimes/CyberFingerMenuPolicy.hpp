#pragma once
#include <cmath>
#include <cstdint>
#include "Hooks/WeaponTrigger.hpp"
#include "Hooks/Handedness.hpp"

namespace cvr::cyberfinger {
// Keep interactions on the hand free of locomotion, including mirrored sticks.
constexpr int MouseHand(bool leftHanded,int bindings) {
    return leftHanded && cvr::input::TradesSticks(bindings)?0:1;
}
struct InteractionButtons {
    cvr::input::WeaponTrigger primary,secondary;
    bool claimTrigger{},claimGrip{};
    uint16_t Step(bool configured,bool enabled,float trigger,bool grip) {
        if(!configured){*this={};return 0;}
        if(enabled){claimTrigger=claimGrip=true;}
        else {if(trigger<.2f)claimTrigger=false;if(!grip)claimGrip=false;}
        return (primary.Update(enabled,trigger)?0x4000:0)|(secondary.Update(enabled,grip?1.f:0.f)?0x8000:0);
    }
};
// Every context requires a release/neutral before it accepts input. Opening a
// menu with a held button or walking stick must not activate its first item.
class ButtonEdge {
    bool armed{},held{};
public:
    void Reset(){armed=held=false;}
    bool Step(bool down) {
        if(!down){armed=true;held=false;return false;}
        const bool edge=armed && !held;held=true;return edge;
    }
};
class ToggleButton {
    ButtonEdge edge;bool value{};uint64_t last{};
public:
    bool& Step(bool enabled,bool down,uint64_t now) {
        if(!enabled || (last && (now<last || now-last>250))){edge.Reset();value=false;last=now;return value;}
        last=now;if(edge.Step(down))value=!value;return value;
    }
};
class AxisStep {
    int direction{};bool armed{};uint64_t next{};
public:
    void Reset(){*this={};}
    int Step(float x,float y,uint64_t now,bool repeat=true) {
        if(!std::isfinite(x) || !std::isfinite(y)){Reset();return 0;}
        const float magnitude=std::fmax(std::abs(x),std::abs(y));
        if(magnitude<.35f){armed=true;direction=0;next=0;return 0;}
        if(!armed || magnitude<.65f)return 0;
        const int d=std::abs(x)>std::abs(y)?(x<0?3:4):(y>0?1:2); // up/down/left/right
        if(d!=direction){direction=d;next=now+400;return d;}
        if(repeat && now>=next){next=now+120;return d;}
        return 0;
    }
};
inline uint16_t DirectionButton(int direction) {
    return direction==1?0x0001:direction==2?0x0002:direction==3?0x0004:direction==4?0x0008:0;
}
struct MenuSteps { int direction{},tab{};bool back{};unsigned grabMask{}; };
class MenuNavigation {
    AxisStep left,right;ButtonEdge grabs[2];bool active{};
public:
    MenuSteps Step(bool enabled,float lx,float ly,float rx,bool leftGrab,bool rightGrab,uint64_t now) {
        if(!enabled){*this={};return {};}
        if(!active){left.Reset();right.Reset();for(auto& g:grabs)g.Reset();active=true;}
        const int tab=right.Step(rx,0,now,false);
        const bool l=grabs[0].Step(leftGrab),r=grabs[1].Step(rightGrab);
        return {left.Step(lx,ly,now),tab==3?-1:tab==4?1:0,l||r,unsigned(l)|(unsigned(r)<<1)};
    }
};
class MenuFocus {
    bool focus{};float x{},y{};
public:
    void Reset(){*this={};}
    bool Active() const{return focus;}
    bool Step(bool directional,bool hit,float u,float v) {
        if(directional){focus=true;x=u;y=v;}
        else if(focus && hit && ((u-x)*(u-x)+(v-y)*(v-y)>.0016f))focus=false;
        return focus;
    }
};
// Defer Back until release so holding both Menu buttons can toggle the HUD
// without first exiting the native screen. Overlay menus use MenuNavigation.
class MenuBackButton {
    bool menuArmed[2]{},menuHeld[2]{},chord{};
public:
    bool Step(bool leftMenu,bool rightMenu) {
        const bool menus[]={leftMenu,rightMenu};bool back=false;
        if(leftMenu && rightMenu)chord=true;
        if(chord) {
            for(int i=0;i<2;++i){menuHeld[i]=false;menuArmed[i]=!menus[i];}
            if(!leftMenu && !rightMenu)chord=false;
            return false;
        }
        for(int i=0;i<2;++i) {
            if(!menus[i]) {
                if(menuHeld[i])back=true;
                menuArmed[i]=true;menuHeld[i]=false;
            } else if(menuArmed[i])menuHeld[i]=true;
        }
        return back;
    }
};
class NativeMenuButtons {
    MenuBackButton back;
    bool grabArmed[2]{};
public:
    uint16_t Step(bool leftMenu,bool rightMenu,bool leftGrab,bool rightGrab) {
        const bool grabs[]={leftGrab,rightGrab};
        uint16_t buttons=back.Step(leftMenu,rightMenu)?0x2000:0;
        for(int i=0;i<2;++i) {
            if(!grabs[i])grabArmed[i]=true;
            if(grabs[i] && grabArmed[i])buttons|=0x4000;
        }
        return buttons;
    }
};
// A shard can open over gameplay or a native menu. Reserve its closing press
// through release, even if the widget disappears while the button is down.
struct ShardCloseGrip {
    bool armed{};
    bool claimed{};

    bool Update(bool reader, bool active, bool down) {
        if (!active) {
            armed = false;
        } else if (!down) {
            armed = reader;
            claimed = false;
        } else if (reader) {
            claimed = true;
            const bool pressed = armed;
            armed = false;
            return pressed;
        } else {
            armed = false;
        }
        return false;
    }
};
// One menu button: tap opens inventory; hold activates cyberware. No simultaneous
// grip/stick/finger gesture, and a context change cancels the pending tap.
class MenuTapHold {
    bool armed{},held{},fired{};uint64_t since{},last{};
public:
    struct Result{bool tap{},hold{};};
    Result Step(bool down,bool allowed,uint64_t now) {
        if(!allowed || (last && (now<last || now-last>250))){*this={};last=now;return {};}
        last=now;
        if(!down){const bool tap=held && !fired;armed=true;held=fired=false;return {tap,false};}
        if(!armed)return {};
        if(!held){held=true;since=now;}
        if(!fired && now-since>=650){fired=true;return {false,true};}
        return {};
    }
};
// The XInput hook and the reload script poll on different clocks. Hold the result
// briefly so each sees the tap or hold once even when it misses the first poll.
class FirearmGrip {
    MenuTapHold gesture;
    uint64_t crouchUntil{}, slideUntil{};
public:
    struct Result { bool crouch{}, slide{}; };
    Result Step(bool down, bool allowed, uint64_t now) {
        const auto edges=gesture.Step(down,allowed,now);
        if (!allowed) { crouchUntil=slideUntil=0; return {}; }
        if (edges.tap) crouchUntil=now+90;
        if (edges.hold) slideUntil=now+90;
        return {now<crouchUntil,now<slideUntil};
    }
};
// Hold both physical Menu buttons for half a second. Claim until BOTH are up,
// so neither Back/inventory nor a weapon-wheel release follows the HUD toggle.
class HudChord {
    bool armed{},claimed{},latched{};float held{};
public:
    bool Step(bool left,bool right,bool enabled,float dt) {
        if(!enabled){*this={};return false;}
        if(!left && !right){armed=true;claimed=latched=false;held=0;return false;}
        if(!armed)return false;
        if(!left || !right){held=0;return false;}
        claimed=true;
        if(latched)return false;
        if(std::isfinite(dt) && dt>0 && dt<=.25f)held+=dt;else held=0;
        if(held>=.5f){latched=true;return true;}
        return false;
    }
    bool Claimed()const{return claimed;}
    float Held()const{return held;}
};
class ConversationNavigation {
    struct TriggerPress {
        bool armed{};
        bool Step(float value) {
            if (!std::isfinite(value)) armed=false;
            else if (value<=.35f) armed=true;
            else if (armed && value>.65f) { armed=false; return true; }
            return false;
        }
    };
    AxisStep stick;
    TriggerPress left, right;
    ButtonEdge driverClick;
    uint64_t last{};
public:
    int Step(bool enabled,float rightY,float leftTrigger,float rightTrigger,bool stickClick,uint64_t now) {
        if(!enabled || (last && (now<last || now-last>250))){*this={};last=now;return 0;}
        last=now;
        const int direction=stick.Step(0,rightY,now);
        // Sample every input even when two presses arrive in the same frame.
        const bool leftPressed=left.Step(leftTrigger);
        const bool rightPressed=right.Step(rightTrigger);
        const bool clicked=driverClick.Step(stickClick);
        return leftPressed || rightPressed || clicked ? 3 : direction;
    }
};
// Reserve physical right Y and both triggers. Keep each reserved until released
// after the choice hub closes, so confirmation cannot become a shot or dodge.
struct ConversationCapture {
    bool vertical{},leftTrigger{},rightTrigger{};
    void Step(bool configured,bool active,float rightY,float left,float right) {
        if(!configured){*this={};return;}
        if(active){vertical=leftTrigger=rightTrigger=true;return;}
        if(std::isfinite(rightY) && std::abs(rightY)<.35f)vertical=false;
        if(std::isfinite(left) && left<=.35f)leftTrigger=false;
        if(std::isfinite(right) && right<=.35f)rightTrigger=false;
    }
    template<class State> State Apply(State state)const {
        if(vertical)state.rightThumbY=0;
        if(leftTrigger)state.leftTrigger=0;
        if(rightTrigger)state.rightTrigger=0;
        return state;
    }
    template<class Pad> void ApplyGamepad(Pad& pad,bool leftHanded,bool mirroredSticks)const {
        if(vertical) {
            if(mirroredSticks)pad.sThumbLY=0;
            else pad.sThumbRY=0;
        }
        if(leftHanded ? rightTrigger : leftTrigger)pad.bLeftTrigger=0;
        if(leftHanded ? leftTrigger : rightTrigger)pad.bRightTrigger=0;
    }
};
}
