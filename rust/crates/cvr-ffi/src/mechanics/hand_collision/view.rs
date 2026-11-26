//! THE COLLISION VIEW (main's debugDraw): the capsules this mod knows, the people as the physics hands have them, V's
//! own body as the hands meet it (orange), the palms, and what PhysX itself has around the hands. The rays draw
//! themselves (carry's cast_ray).
use super::{carry, data, mul, nat, qrot, v4, vp, Frame, State};
use crate::script_call::Value;

#[derive(Default)]
pub(super) struct View {
    /// the view is on this frame
    pub on: bool,
}
pub(super) fn debug_draw(s: &mut State, f: &Frame) {
    let was = s.view.on;
    s.view.on = false;
    s.people.drawn_on = false;
    if nat::<bool>("VRDebugOn", &[]) != Some(true) {
        if was {
            nat::<bool>("VRDebugFlush", &[]);
        }
        return;
    }
    s.view.on = true;
    let d = data::data();
    for i in 0..d.caps.len() {
        if let Some((a, e)) = s.endpoints(i) {
            let color = if d.caps[i].slot.contains("vrp_f_") { 5 } else { 3 };
            nat::<bool>("VRDebugCapsule", &[vp(&v4(&f.to_world(&a), 1.0)), vp(&v4(&f.to_world(&e), 1.0)), Value::Float(d.caps[i].r), Value::Int32(color)]);
        }
    }
    // the people as the physics hands have them (world space already); a part a hand holds in red
    s.people.drawn_on = true;
    let held: Vec<i32> = (0..2).map(|side| cvr_rig::vrik::physhands::grab::grabbed_person(side).0).filter(|o| *o > 0).collect();
    for c in &s.people.drawn {
        let color = if held.contains(&(c[7] as i32)) { 2 } else { 6 };
        nat::<bool>("VRDebugCapsule", &[vp(&[c[0], c[1], c[2], 1.0]), vp(&[c[3], c[4], c[5], 1.0]), Value::Float(c[6]), Value::Int32(color)]);
    }
    // V's own body as the hands meet it (orange)
    for i in 0..8 {
        let Some((a, e, r, _)) = cvr_rig::vrik::physhands::people::self_shape(i) else { break };
        nat::<bool>("VRDebugCapsule", &[vp(&v4(&a, 1.0)), vp(&v4(&e, 1.0)), Value::Float(r), Value::Int32(7)]);
    }
    let (mut c, mut n) = ([0f32; 3], 0);
    for side in 0..2 {
        let Some(h) = d.hand[side] else { continue };
        let Some((hp, hq)) = s.bone(d.caps[h].index) else { continue };
        c = super::add(&c, &hp);
        n += 1;
        carry::palm_normal(s, side);
        let skin = qrot(&hq, &carry::palm_skin(s, side));
        let at = super::add(&hp, &skin);
        nat::<bool>("VRDebugPoint", &[vp(&v4(&f.to_world(&at), 1.0)), Value::Float(0.015), Value::Int32(0)]);
        if let Some(nl) = s.carry.palm_local[side] {
            let o = qrot(&hq, &mul(&nl, 0.1));
            nat::<bool>("VRDebugLine", &[vp(&v4(&f.to_world(&at), 1.0)), vp(&v4(&f.to_world(&super::add(&at, &o)), 1.0)), Value::Int32(0)]);
        }
    }
    if n > 0 {
        let centre = f.to_world(&mul(&c, 1.0 / n as f32));
        let shapes = nat::<i32>("VRDebugWorld", &[vp(&v4(&centre, 1.0)), Value::Float(2.0)]);
        if let Some(k) = shapes.filter(|k| *k > 0) {
            s.say_once(&format!("the view found shapes: {k}"));
        }
    }
    nat::<bool>("VRDebugFlush", &[]);
}
