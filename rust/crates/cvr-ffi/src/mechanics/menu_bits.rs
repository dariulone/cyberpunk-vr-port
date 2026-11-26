//! Small parts of the VR menu: modules/hud_fade.lua (the HUD layer faded out while the menu is placed, back to its
//! own opacity after), stick_scroll.lua (the right stick held over a list scrolls it with the hand's motion) and
//! space.lua's view (where the eyes are and the level heading they face).
use super::lobby_ink as ink;
use super::lobby_scroll::View;
use super::lobby_spawn::{self as sp, V3};
use red4ext_abi::handle::Handle;
use std::cell::RefCell;
use std::rc::Rc;

// ---------------------------------------------------------------- hud_fade.lua
#[derive(Default)]
pub(super) struct HudFade {
    root: Handle,
    base: f64,
    hidden: Option<bool>,
    anim: Option<(f64, f64, f64, f64)>,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for HudFade {}
impl HudFade {
    pub(super) fn restore(&mut self) {
        if !self.root.is_null() {
            ink::opacity(&self.root, self.base);
        }
        *self = HudFade::default();
    }
    pub(super) fn update(&mut self, hidden: bool, dt: f64) {
        if self.root.is_null() {
            if !hidden {
                return;
            }
            let layer = sp::native_handle("VRInkLayer", &[sp::cn("inkHUDLayer")]);
            let root = ink::child(&layer, "GetVirtualWindow", &[]);
            if root.is_null() {
                return;
            }
            self.base = ink::get::<f32>(&root, "GetOpacity", &[]).map_or(1.0, f64::from);
            (self.root, self.hidden) = (root, Some(false));
        }
        if Some(hidden) != self.hidden {
            self.hidden = Some(hidden);
            let from = ink::get::<f32>(&self.root, "GetOpacity", &[]).map_or(1.0, f64::from);
            self.anim = Some((from, if hidden { 0.0 } else { self.base }, 0.0, if hidden { 0.14 } else { 0.18 }));
        }
        if let Some((from, to, time, dur)) = self.anim {
            let time = dur.min(time + dt.clamp(0.0, 0.1));
            let t = time / dur;
            let e = t * t * (3.0 - 2.0 * t);
            ink::opacity(&self.root, from + (to - from) * e);
            self.anim = Some((from, to, time, dur));
            if t >= 1.0 {
                self.anim = None;
                if !hidden {
                    (self.root, self.hidden) = (Handle::null(), None);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- stick_scroll.lua
pub(super) type ViewRef = Rc<RefCell<View>>;
#[derive(Default)]
pub(super) struct Stick {
    pub down: bool,
    pub used: bool,
    pub swipe_until: f64,
    pub scroll: Option<(ViewRef, &'static str)>,
}
impl Stick {
    pub(super) fn held(&self, value: f64) -> bool {
        value > if self.down { 0.4 } else { 0.7 }
    }
    pub(super) fn cancel(&mut self) {
        if let Some((v, _)) = self.scroll.take() {
            let mut v = v.borrow_mut();
            v.end_drag();
            v.stop();
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn update(&mut self, value: f64, tracked: bool, view: Option<ViewRef>, mode: Option<&'static str>, ray_y: Option<f64>, tip_y: Option<f64>, now: f64) {
        if !tracked {
            self.cancel();
            return;
        }
        self.down = self.held(value);
        if !self.down {
            if let Some((v, _)) = self.scroll.take() {
                v.borrow_mut().end_drag();
            }
            self.used = false;
            return;
        }
        if !self.used {
            if let Some(v) = view.filter(|v| v.borrow().scrollable() && !v.borrow().dragging()) {
                let y = if mode == Some("tip") { tip_y } else { ray_y };
                if let Some(y) = y {
                    v.borrow_mut().begin_drag(y);
                    self.scroll = Some((v, mode.unwrap_or("ray")));
                    self.used = true;
                }
            }
        }
        if self.used {
            self.swipe_until = now + 0.6;
        }
        if let Some((v, m)) = self.scroll.clone() {
            let y = if m == "tip" { tip_y } else { ray_y };
            match y {
                Some(y) => v.borrow_mut().drag_to(y, now),
                None => self.cancel(),
            }
        }
    }
    pub(super) fn blocks_swipe(&self, now: f64) -> bool {
        self.scroll.is_some() || (self.down && self.used) || now < self.swipe_until
    }
}

// ---------------------------------------------------------------- space.lua's view
pub(super) struct Cam {
    pub p: V3,
    pub f: V3,
    pub r: V3,
}
/// Space.view: the eyes and the level heading they face (forward, right).
pub(super) fn camera() -> Option<Cam> {
    let (p, q) = sp::view_world().or_else(|| {
        let cs = super::game_system("GetCameraSystem");
        let mut t = [[0f32; 4]; 2];
        if cs.is_null() {
            return None;
        }
        ink::call(&cs, "GetActiveCameraWorldTransform", &[crate::script_call::Value::Out(t.as_mut_ptr().cast())]);
        Some(([f64::from(t[0][0]), f64::from(t[0][1]), f64::from(t[0][2])], t[1].map(f64::from)))
    })?;
    let f = super::belt::qrot(q, [0.0, 1.0, 0.0]);
    let l = (f[0] * f[0] + f[1] * f[1]).sqrt();
    let fh = if l < 1e-6 { [0.0; 3] } else { [f[0] / l, f[1] / l, 0.0] };
    Some(Cam { p, f: fh, r: [fh[1], -fh[0], 0.0] })
}
