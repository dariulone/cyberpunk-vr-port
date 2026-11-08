#pragma once
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>

namespace cvr::camera {
// SCENE CAMERA COMFORT: how much of a scene camera's own turning reaches the headset.
//
// A scene that drives the camera puts its orientation under the head, and the head composes on top. On a
// flat screen a slow pan, a handheld sway or a push-in tilt is framing. In the headset it turns the world
// round a player who is not turning, and that is what makes them sick. The post-prologue montage
// (q000_08_half_year_montage.scene) is 23 camera animations with a cut every 0.5-1.7 s, every shot moving.
// 2026-11-06: "smoothing up to 30 degrees for the scene camera when it drives the camera, it makes me sick".
//
// So the view holds its orientation while the scene camera turns within deadDeg of it. Past that, the view
// eases after the scene camera at catchUpSec, and the gap never grows past deadDeg + maxExtraDeg, so a whip
// pan still brings the subject into view.
//
// A cut is not a turn. A turn larger than cutDeg between two steps, or a camera jump of cutMetres, starts a
// new shot. The view takes that shot's orientation at once, as the game does. Without this, every cut in
// the montage would have been eased in as a fast spin.
//
// Position is not filtered: the eye stays where the scene puts it.
struct SceneComfortSettings {
    float deadDeg{30.0f};      // xr_scene_cam_comfort_deg; 0 switches the filter off
    float catchUpSec{.35f};    // time constant of the catch-up past deadDeg
    float maxExtraDeg{15.0f};  // the view never trails by more than deadDeg + this
    float cutDeg{35.0f};       // a turn this large between two steps is a cut
    float cutMetres{1.0f};     // and so is a camera jump this far
};

class SceneCameraComfort {
public:
    using Quat=std::array<float,4>;   // x, y, z, w

    void Reset() { m_valid=false;m_havePos=false;m_lagDeg=0; }
    bool Valid() const { return m_valid; }
    float LagDeg() const { return m_lagDeg; }
    uint32_t Cuts() const { return m_cuts; }

    // scene: the scene camera's world orientation this step. position: its world position in metres, or
    // nullptr when unknown (a cut is then told by the turn alone). dt: seconds since the previous step.
    // Returns the orientation to compose the head onto.
    Quat Step(const Quat& scene,const float* position,float dt,const SceneComfortSettings& s) {
        Quat q{};
        if(!Normalize(scene,q))return scene;
        if(!(s.deadDeg>0.0f) || !std::isfinite(s.deadDeg)) { Reset();return q; }
        if(!std::isfinite(dt) || dt<0.0f)dt=0.0f;
        dt=std::min(dt,.1f);   // a hitch or a load screen is not a long catch-up
        bool cut=!m_valid;
        if(m_valid && AngleDeg(m_prevScene,q)>s.cutDeg)cut=true;
        if(m_valid && position && m_havePos) {
            const float dx=position[0]-m_prevPos[0],dy=position[1]-m_prevPos[1],dz=position[2]-m_prevPos[2];
            const float d2=dx*dx+dy*dy+dz*dz;
            if(!std::isfinite(d2) || d2>s.cutMetres*s.cutMetres)cut=true;
        }
        m_prevScene=q;
        m_havePos=position!=nullptr;
        if(position)for(int i=0;i<3;++i)m_prevPos[i]=position[i];
        if(cut) {
            if(m_valid)++m_cuts;
            m_held=q;m_valid=true;m_lagDeg=0;
            return q;
        }
        float lag=AngleDeg(m_held,q);
        if(lag>s.deadDeg) {
            const float excess=lag-s.deadDeg;
            const float tau=std::max(s.catchUpSec,1e-3f);
            float step=excess*(1.0f-std::exp(-dt/tau));
            const float cap=s.deadDeg+std::max(s.maxExtraDeg,0.0f);
            if(lag-step>cap)step=lag-cap;
            m_held=Slerp(m_held,q,std::clamp(step/lag,0.0f,1.0f));
            lag=AngleDeg(m_held,q);
        }
        m_lagDeg=lag;
        return m_held;
    }

    // The same filter for a heading alone (radians about the vertical), for scenes whose camera base is a yaw.
    float StepYaw(float yaw,float dt,const SceneComfortSettings& s) {
        const Quat q{0,0,std::sin(yaw*.5f),std::cos(yaw*.5f)};
        const Quat out=Step(q,nullptr,dt,s);
        return 2.0f*std::atan2(out[2],out[3]);
    }

    // The relative rotation's angle, from its vector part: acos of a float dot loses about 0.02 deg near
    // identity, as much as a whole frame of a slow pan at 144 fps.
    static float AngleDeg(const Quat& a,const Quat& b) {
        const float x=a[3]*b[0]-a[0]*b[3]-a[1]*b[2]+a[2]*b[1];
        const float y=a[3]*b[1]+a[0]*b[2]-a[1]*b[3]-a[2]*b[0];
        const float z=a[3]*b[2]-a[0]*b[1]+a[1]*b[0]-a[2]*b[3];
        const float w=a[3]*b[3]+a[0]*b[0]+a[1]*b[1]+a[2]*b[2];
        return 2.0f*std::atan2(std::sqrt(x*x+y*y+z*z),std::fabs(w))*57.29577951f;
    }

private:
    static bool Normalize(const Quat& in,Quat& out) {
        const float n=in[0]*in[0]+in[1]*in[1]+in[2]*in[2]+in[3]*in[3];
        if(!std::isfinite(n) || n<.25f || n>4.0f)return false;
        const float inv=1.0f/std::sqrt(n);
        for(int i=0;i<4;++i)out[i]=in[i]*inv;
        return true;
    }
    static Quat Slerp(const Quat& a,Quat b,float t) {
        float d=a[0]*b[0]+a[1]*b[1]+a[2]*b[2]+a[3]*b[3];
        if(d<0) { d=-d;for(auto& v:b)v=-v; }
        Quat r{};
        if(d>.9995f) {
            for(int i=0;i<4;++i)r[i]=a[i]+t*(b[i]-a[i]);
        } else {
            const float th=std::acos(std::min(d,1.0f)),sn=std::sin(th);
            const float wa=std::sin((1.0f-t)*th)/sn,wb=std::sin(t*th)/sn;
            for(int i=0;i<4;++i)r[i]=wa*a[i]+wb*b[i];
        }
        Quat out{};
        return Normalize(r,out) ? out : b;
    }

    Quat m_held{0,0,0,1},m_prevScene{0,0,0,1};
    float m_prevPos[3]{};
    float m_lagDeg{};
    uint32_t m_cuts{};
    bool m_valid{},m_havePos{};
};
}
