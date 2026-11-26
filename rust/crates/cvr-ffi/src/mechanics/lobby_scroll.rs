//! CyberpunkVRPort_Menu/modules/scroll.lua, the scroll kit the lobby's panels use: a list of rows of known heights in a
//! window of the canvas, each row's canvas built when it comes near the view (a budget of rows ahead per frame), the
//! content moved by the offset, rows faded by how much of them shows, a drag with a flick that dies down, the bar.
use super::lobby_ink::{self as ink, C3};
use red4ext_abi::handle::Handle;

const FRICTION: f64 = 4.5;
const BAR: f64 = 8.0;

pub(super) struct Style {
    pub track: C3,
    pub thumb: C3,
    pub held: C3,
}
pub(super) struct Item {
    pub y: f64,
    pub h: f64,
    pub canvas: Handle,
    pub fade: f64,
}
struct Drag {
    y0: f64,
    off0: f64,
    last_y: f64,
    last_t: Option<f64>,
}
pub(super) struct View {
    style: Style,
    pub root: Handle,
    content: Handle,
    track: Handle,
    thumb: Handle,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    cw: f64,
    pub items: Vec<Item>,
    total: f64,
    pub offset: f64,
    vel: f64,
    target: Option<f64>,
    placed: Option<f64>,
    dragging: Option<Drag>,
    mask: Handle,
    mask_sync: i32,
    pub cw_pub: f64,
}
fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    lo.max(hi.min(x))
}
impl View {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(parent: &Handle, px: f64, py: f64, ax: f64, ay: f64, w: f64, h: f64, style: Style) -> View {
        let root = ink::canvas(parent, px, py, w, h);
        let content = ink::canvas(&root, 0.0, 0.0, w, h);
        let track = ink::rect(&root, w - BAR + 2.0, 0.0, BAR - 4.0, h, style.track, 0.22);
        let thumb = ink::rect(&root, w - BAR, 0.0, BAR, h, style.thumb, 0.9);
        ink::visible(&track, false);
        ink::visible(&thumb, false);
        View {
            style,
            root,
            content,
            track,
            thumb,
            x: ax,
            y: ay,
            w,
            h,
            cw: w - BAR - 10.0,
            items: Vec::new(),
            total: 0.0,
            offset: 0.0,
            vel: 0.0,
            target: None,
            placed: None,
            dragging: None,
            mask: Handle::null(),
            mask_sync: 0,
            cw_pub: w - BAR - 10.0,
        }
    }
    pub(super) fn add(&mut self, h: f64) -> usize {
        self.items.push(Item { y: self.total, h, canvas: Handle::null(), fade: 0.0 });
        self.total += h;
        self.items.len() - 1
    }
    pub(super) fn max_offset(&self) -> f64 {
        (self.total - self.h).max(0.0)
    }
    pub(super) fn scrollable(&self) -> bool {
        self.total > self.h + 1.0
    }
    pub(super) fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
    pub(super) fn visible_at(&self, x: f64, y: f64) -> bool {
        if !self.contains(x, y) {
            return false;
        }
        let cy = y - self.y + self.offset;
        for it in &self.items {
            if cy < it.y {
                return false;
            }
            if cy < it.y + it.h {
                return !it.canvas.is_null() && it.fade > 0.2;
            }
        }
        false
    }
    fn visible(&self, it: &Item) -> f64 {
        let (a, b) = (it.y - self.offset, it.y + it.h - self.offset);
        let inside = b.min(self.h) - a.max(0.0);
        clamp(inside / it.h.max(1.0), 0.0, 1.0)
    }
    pub(super) fn begin_drag(&mut self, y: f64) {
        self.dragging = Some(Drag { y0: y, off0: self.offset, last_y: y, last_t: None });
        (self.vel, self.target) = (0.0, None);
    }
    pub(super) fn drag_to(&mut self, y: f64, now: f64) {
        let mx = self.max_offset();
        let Some(d) = self.dragging.as_mut() else { return };
        self.offset = clamp(d.off0 - (y - d.y0), 0.0, mx);
        if let Some(lt) = d.last_t.filter(|lt| now > *lt) {
            let inst = -(y - d.last_y) / (now - lt);
            self.vel = self.vel * 0.6 + inst * 0.4;
        }
        (d.last_y, d.last_t) = (y, Some(now));
    }
    pub(super) fn end_drag(&mut self) {
        self.dragging = None;
        if self.vel.abs() < 60.0 {
            self.vel = 0.0;
        }
    }
    pub(super) fn stop(&mut self) {
        self.vel = 0.0;
    }
    pub(super) fn dragging(&self) -> bool {
        self.dragging.is_some()
    }
    pub(super) fn at_top(&self) -> bool {
        self.offset <= 1.0
    }
    pub(super) fn at_bottom(&self) -> bool {
        self.offset >= self.max_offset() - 1.0
    }
    pub(super) fn moving(&self) -> bool {
        self.dragging.is_some() || self.vel != 0.0 || self.target.is_some()
    }
    /// V:scrollBy: eased toward the offset `dy` further.
    pub(super) fn scroll_by(&mut self, dy: f64) {
        self.target = Some(clamp(self.target.unwrap_or(self.offset) + dy, 0.0, self.max_offset()));
        self.vel = 0.0;
    }
    /// V:enableMask: the content moved into a masking scroll area (the radio's logos must not spill out).
    pub(super) fn enable_mask(&mut self) {
        if !self.mask.is_null() {
            return;
        }
        let mask = ink::place(ink::new("inkScrollAreaWidget"), &self.root, 0.0, 0.0, self.w, self.h);
        ink::call(&mask, "SetFitToContent", &[crate::script_call::Value::Bool(false)]);
        ink::call(&mask, "SetUseInternalMask", &[crate::script_call::Value::Bool(true)]);
        // SAFETY: the scroll area's Bool property.
        if let Some(p) = unsafe { crate::script_call::property(mask.instance, "constrainContentPosition") } {
            unsafe { *(p as *mut bool) = true };
        }
        if let (Some(p), Some(v)) = (unsafe { crate::script_call::property(mask.instance, "fitToContentDirection") }, crate::script_call::enum_value("inkFitToContentDirection", "None")) {
            // SAFETY: the enum property (one byte suffices).
            unsafe { *(p as *mut u8) = v as u8 };
        }
        ink::reparent(&self.content, &mask);
        ink::move_to(&self.content, 0.0, 0.0);
        let f = self.offset / self.max_offset().max(1.0);
        ink::call(&mask, "ScrollVertical", &[crate::script_call::Value::Float(f as f32)]);
        self.mask = mask;
        self.mask_sync = 3;
        self.placed = None;
    }
    pub(super) fn scroll_to(&mut self, offset: f64) {
        self.offset = clamp(offset, 0.0, self.max_offset());
        (self.target, self.vel) = (None, 0.0);
    }
    /// V:update: the offset's motion, the rows built near the view (`build(index, canvas, y)`), the fades and the bar.
    pub(super) fn update(&mut self, dt: f64, budget: usize, build: &mut dyn FnMut(usize, &Handle, f64)) -> bool {
        let mut moving = false;
        if self.dragging.is_none() {
            if let Some(t) = self.target {
                let d = t - self.offset;
                if d.abs() < 1.0 {
                    (self.offset, self.target) = (t, None);
                } else {
                    self.offset += d * (dt * 12.0).min(1.0);
                    moving = true;
                }
            } else if self.vel != 0.0 {
                self.offset += self.vel * dt;
                self.vel *= (-FRICTION * dt).exp();
                if self.vel.abs() < 25.0 || self.offset <= 0.0 || self.offset >= self.max_offset() {
                    self.vel = 0.0;
                }
                moving = true;
            }
        } else {
            moving = true;
        }
        self.offset = clamp(self.offset, 0.0, self.max_offset());
        let (mut built, mut ahead) = (0, 0);
        let (lo, hi) = (self.offset - self.h * 0.5, self.offset + self.h * 1.5);
        for pass in 1..=2 {
            for i in 0..self.items.len() {
                if pass == 2 && ahead >= budget {
                    break;
                }
                let it = &self.items[i];
                let near = it.y + it.h > lo && it.y < hi;
                let in_view = it.y + it.h > self.offset && it.y < self.offset + self.h;
                if it.canvas.is_null() && ((pass == 1 && in_view) || (pass == 2 && near)) {
                    let c = ink::canvas(&self.content, 0.0, it.y, self.cw, it.h);
                    ink::visible(&c, false);
                    let y = it.y;
                    self.items[i].canvas = c.clone();
                    self.items[i].fade = -1.0;
                    build(i, &c, y);
                    built += 1;
                    if pass == 2 {
                        ahead += 1;
                    }
                }
            }
        }
        if self.placed != Some(self.offset) || built > 0 || self.mask_sync > 0 {
            self.placed = Some(self.offset);
            ink::size(&self.content, self.w, self.total.max(self.h));
            if self.mask.is_null() {
                ink::move_to(&self.content, 0.0, -self.offset);
            } else {
                ink::move_to(&self.content, 0.0, 0.0);
                let f = self.offset / self.max_offset().max(1.0);
                ink::call(&self.mask, "ScrollVertical", &[crate::script_call::Value::Float(f as f32)]);
                self.mask_sync = (self.mask_sync - 1).max(0);
            }
            for i in 0..self.items.len() {
                if self.items[i].canvas.is_null() {
                    continue;
                }
                let f = self.visible(&self.items[i]);
                let it = &mut self.items[i];
                if (f - it.fade).abs() > 0.02 || (f == 0.0) != (it.fade == 0.0) {
                    it.fade = f;
                    ink::visible(&it.canvas, f > 0.0);
                    if f > 0.0 {
                        ink::opacity(&it.canvas, f * f);
                    }
                }
            }
            self.bar();
        }
        moving || built > 0
    }
    fn bar(&self) {
        let show = self.scrollable();
        ink::visible(&self.track, show);
        ink::visible(&self.thumb, show);
        if !show {
            return;
        }
        let th = 60f64.max(self.h * self.h / self.total);
        let ty = (self.h - th) * (self.offset / self.max_offset().max(1.0));
        ink::move_to(&self.thumb, self.w - BAR, ty);
        ink::size(&self.thumb, BAR, th);
        ink::tint(&self.thumb, if self.dragging.is_some() { self.style.held } else { self.style.thumb });
    }
}
