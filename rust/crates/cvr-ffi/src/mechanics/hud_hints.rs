//! THE HUD'S BUTTON HINTS: where the game shows a pad button a VR controller reaches only by a chord, the HUD shows the
//! chord, at the size and in the colour of the game's own glyph beside it.
//!
//! The chord's physical glyph replaces the pad button's in every icon the game looks up (cvr-rig vrik::input_prompts
//! chord_part): the D-pad is a stick's push (the right stick's; the left one's for a left-hander whose sticks are
//! traded), RB -- the combat gadget -- the right stick's click. Its other half is put in front of that icon here: for
//! each input display (inkInputDisplayController) whose icon is a chord's (input_prompts::chord_displays), the
//! modifier's glyph and "+" go into its root (a horizontal panel) as its first child, sized as the game's icon and tinted
//! as it is:
//!
//!   the D-pad   the left thumbrest held (Touch controllers; the right one for a traded left-hander), the left stick
//!               held in (others), PSVR2's Triangle touched -- cvr_input::modifier;
//!   RB          the right grip held (cvr_input::gadget_hold: the grip and the stick's click held half a second);
//!   the zoom    the left trigger held (the scanner).
//!
//! A button that does nothing in VR (the car's paint on the D-pad's left, which is the radio there) has its icon hidden.
//! In a menu the grips stand in for LB and RB (Touch controllers have no bumpers): their icons are replaced whole.
//!
//! In the quick slots (HotkeysWidgetController) the hint takes no room of the layout, drawn to the left of the game's
//! glyph, the slots drawn apart to make that room under them; elsewhere it takes its room and the row flows. The
//! cyberware slot's LB + RB (the arm cyberware) becomes its own chord: the left grip + the left stick's click
//! (vrik::cyberware_chord). Beside the dialogue's choices (dialogWidgetGameController, a hub of two or more), out to
//! the hub's left: the modifier, "+" and the stick up / down, at the size of the choices' own glyphs.
//!
//! The glyphs: the game's own (base\gameplay\gui\common\input\icons_durango.inkatlas: the stick's directions and
//! clicks) and the port's in their style (vrport\ui\vr_hud_hints.inkatlas: vr_rest_l / vr_rest_r, vr_grip_l / vr_grip_r,
//! vr_tri; tools/controls_icons/make_hud_hints.py).
use super::lobby_ink as ink;
use super::lobby_rtti as rt;
use crate::script_call::{call_handle, enum_value, method, Value};
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::sync::Mutex;

const ATLAS: &str = "vrport\\ui\\vr_hud_hints.inkatlas";
const PAD_ATLAS: &str = "base\\gameplay\\gui\\common\\input\\icons_durango.inkatlas";
/// The HUD's cyan (the game's MainColors.Blue) when a glyph's own tint cannot be read.
const HUD_COLOUR: ink::C3 = [0.369, 0.965, 1.0];
/// The game's input icon (64 units in every HUD panel measured) when its size cannot be read.
const ICON: f64 = 64.0;
/// The "+" between the halves: its width and size, as the game's LB + RB row.
const PLUS_W: f64 = 30.0;
const PLUS_SIZE: f64 = 40.0;
/// Room made between the quick slots for the hints drawn to the left of their glyphs (the gadget's) and for the
/// cyberware's wider row.
const GADGET_ROOM: f64 = 62.0;
const CYBER_ROOM: f64 = 24.0;
/// The car's and the radio's slots and the row moved right by this (their hint's width, as the phone's hint moves its
/// slot): they sit at the HUD canvas's left edge.
const LEFT_ROOM: f64 = 98.0;

/// A selector's row (the map's filter: its D-pad's glyphs lie over its ends) and the arrow left of its first glyph.
const SELECTOR: &str = "option_subAdd";
const SELECTOR_ARROW: &str = "btnLeft";

/// One glyph half: atlas and part.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Glyph {
    atlas: &'static str,
    part: String,
}
fn ours(part: &str) -> Glyph {
    Glyph { atlas: ATLAS, part: part.to_string() }
}
fn pads(part: &str) -> Glyph {
    Glyph { atlas: PAD_ATLAS, part: part.to_string() }
}
/// The modifier of the D-pad's chord as it is set now.
fn dpad_modifier() -> Glyph {
    use core::sync::atomic::Ordering::Relaxed;
    use cvr_input::modifier as m;
    match m::SCHEME.load(Relaxed) {
        m::SCHEME_THUMBREST => ours(if m::SIDE.load(Relaxed) == 1 { "vr_rest_r" } else { "vr_rest_l" }),
        m::SCHEME_TRIANGLE => ours("vr_tri"),
        _ => pads("left_thumb"),
    }
}
/// The D-pad's direction stick: the right one, the left one when the thumbrest chord is mirrored.
fn dpad_stick() -> &'static str {
    use core::sync::atomic::Ordering::Relaxed;
    use cvr_input::modifier as m;
    if m::SCHEME.load(Relaxed) == m::SCHEME_THUMBREST && m::SIDE.load(Relaxed) == 1 {
        "left"
    } else {
        "right"
    }
}
fn modifier_for(kind: u8) -> Glyph {
    use cvr_rig::vrik::input_prompts as prompts;
    match kind {
        prompts::CHORD_GADGET => ours("vr_grip_r"),
        prompts::CHORD_ZOOM => pads("left_trigger"),
        _ => dpad_modifier(),
    }
}
/// The glyph that replaces the icon itself: LB / RB in a menu are the grips.
fn replacement(kind: u8) -> Option<&'static str> {
    use cvr_rig::vrik::input_prompts as prompts;
    match kind {
        prompts::GRIP_LEFT => Some("vr_grip_l"),
        prompts::GRIP_RIGHT => Some("vr_grip_r"),
        _ => None,
    }
}

/// [first] + [second?]: a canvas of `size`-unit glyphs, `h` tall (the glyphs centred in it).
struct Chord {
    canvas: Handle,
    images: Vec<Handle>,
    plus: Handle,
    glyphs: (Glyph, Option<Glyph>),
    width: f64,
}
fn chord(parent: &Handle, first: &Glyph, second: Option<&Glyph>, size: f64, h: f64, colour: ink::C3) -> Chord {
    let width = size + PLUS_W + second.map_or(0.0, |_| size);
    let canvas = ink::canvas(parent, 0.0, 0.0, width, h);
    let y = ((h - size) / 2.0).max(0.0);
    let mut images = vec![ink::image(&canvas, 0.0, y, size, size, first.atlas, &first.part, colour, 1.0)];
    let plus = ink::text(&canvas, size, 0.0, PLUS_W, h, "+", PLUS_SIZE * size / ICON, colour, "centre", true, None);
    if let Some(s) = second {
        images.push(ink::image(&canvas, size + PLUS_W, y, size, size, s.atlas, &s.part, colour, 1.0));
    }
    Chord { canvas, images, plus, glyphs: (first.clone(), second.cloned()), width }
}
fn tint_chord(c: &Chord, colour: [f32; 4], opacity: f32) {
    let rgb = [f64::from(colour[0]), f64::from(colour[1]), f64::from(colour[2])];
    for w in c.images.iter().chain(core::iter::once(&c.plus)) {
        ink::tint(w, rgb);
    }
    ink::opacity(&c.canvas, f64::from(opacity));
}
fn drop_chord(c: &Chord) {
    ink::visible(&c.canvas, false);
    let parent = h(&c.canvas, "GetParentWidget", &[]);
    m::<()>(&parent, "RemoveChild", &[Value::Handle(&c.canvas)]);
}

/// A chord's hint in front of an input display's icon.
struct Prefix {
    controller: usize,
    root: Handle,
    icon: Handle,
    chord: Chord,
    kind: u8,
    /// The display's margins as the game had them, when room was made in front of it.
    room: Option<[f32; 4]>,
    /// A selector's arrow moved out to the left of the hint, and its margins as the game had them.
    arrow: Option<(Handle, [f32; 4])>,
}
fn drop_prefix(p: &Prefix) {
    drop_chord(&p.chord);
    if let Some(m) = p.room {
        ink::margin(&p.root, f64::from(m[0]), f64::from(m[1]), f64::from(m[2]), f64::from(m[3]));
    }
    if let Some((w, m)) = &p.arrow {
        ink::margin(w, f64::from(m[0]), f64::from(m[1]), f64::from(m[2]), f64::from(m[3]));
    }
}
fn is_row(w: &Handle) -> bool {
    !w.is_null() && m::<bool>(w, "IsA", &[Value::U64(CName::hashed("inkHorizontalPanelWidget").0)]).unwrap_or(false)
}
/// The horizontal panel the icon stands in, from its parent up to the display's root, and the panel's child it is in.
fn row_of(icon: &Handle, root: &Handle) -> Option<(Handle, Handle)> {
    let mut child = icon.clone();
    for _ in 0..8 {
        let parent = h(&child, "GetParentWidget", &[]);
        if parent.is_null() {
            return None;
        }
        if is_row(&parent) {
            return Some((parent, child));
        }
        if parent.instance == root.instance {
            return None;
        }
        child = parent;
    }
    None
}
/// A row whose width is its parent's, not its content's.
fn fixed(row: &Handle) -> bool {
    let fill = rt::call(row, "GetAnchor", &[]).and_then(|o| rt::Val::of(&o).num()) == enum_value("inkEAnchor", "Fill").map(|v| v as f64);
    fill || !m::<bool>(row, "GetFitToContent", &[]).unwrap_or(true)
}
fn index_in(row: &Handle, child: &Handle) -> i32 {
    children(row).iter().position(|c| c.instance == child.instance).unwrap_or(0) as i32
}
#[derive(Default)]
struct Cyber {
    lbrb: Handle,
    lb_icon: Handle,
    /// The consumable slot's icon: a key's or a mouse button's part on the keyboard.
    probe: Handle,
    chord: Option<Chord>,
}
/// A row of quick slots: its controller, the widgets moved apart with their margins as the game had them, the cyberware.
#[derive(Default)]
struct Slots {
    controller: Handle,
    spread: Vec<(Handle, [f32; 4])>,
    cyber: Cyber,
}
impl Slots {
    /// What was moved put back as the game had it, and the row forgotten: its controller was replaced (the HUD's quick
    /// slots made anew after a car, a scene or a load) or the scan starts over. Kept, the old row's record stood for the
    /// new one -- its slots never drawn apart, their hints over the icons -- or, the same widgets met again with the
    /// record gone, they were drawn apart once more each time and the icons walked off to the right.
    fn restore(&mut self) {
        for (w, was) in self.spread.drain(..) {
            ink::margin(&w, f64::from(was[0]), f64::from(was[1]), f64::from(was[2]), f64::from(was[3]));
        }
        if let Some(c) = self.cyber.chord.take() {
            drop_chord(&c);
        }
        *self = Slots::default();
    }
    fn found(&mut self, controller: &Handle) {
        if self.controller.instance != controller.instance {
            self.restore();
            self.controller = controller.clone();
        }
    }
}
/// On foot (HotkeysWidgetController): the widgets moved and by how much -- the car's and the radio's slots stand at the
/// canvas's left edge, where their hint would be cut off: moved in under the phone's (whose hint moves it by as much),
/// and the row with them, spread for the gadget's hint and the cyberware's wider one.
const FOOT: &[(&str, f64)] = &[
    ("gadget", GADGET_ROOM),
    ("cyberware", CYBER_ROOM),
    ("vehicleSlot", LEFT_ROOM),
    ("radioSlot", LEFT_ROOM),
    ("inkHorizontalPanelWidget4", LEFT_ROOM),
];
/// In a vehicle (HotkeyConsumableWidgetController): the phone's and the radio's slots are moved by their hints; the
/// autodrive's, the paint's (which has none) and the row with them.
const CAR: &[(&str, f64)] = &[("autodriveSlot", LEFT_ROOM), ("vehCustomizationSlot", LEFT_ROOM), ("container", LEFT_ROOM)];

#[derive(Default)]
struct State {
    dialog: Handle,
    foot: Slots,
    car: Slots,
    dialog_hint: Option<(Chord, usize)>,
    prefixes: Vec<Prefix>,
    /// The icons replaced by the port's own (the grips in a menu): the controller and its refreshes' count then.
    replaced: Vec<(usize, u32)>,
    /// The displays whose button does nothing in VR, their icons hidden: the controller and the icon.
    hidden: Vec<(usize, Handle)>,
    acc: f64,
    said: u32,
    /// Ticks since the prefixes were last looked at whole, and the modifier's layout they were made for.
    prefix_acc: u32,
    prefix_layout: (u8, u8),
    /// The displays (controller, kind, refreshes) the prefixes were last made for whole.
    prefix_sig: Vec<(usize, u8, u32)>,
    /// The last whole pass met a display with a chord but no root yet: the next look is whole again.
    prefix_incomplete: bool,
}
// SAFETY: the handles are made, used and dropped on the game's main thread.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn h(o: &Handle, name: &str, values: &[Value]) -> Handle {
    if o.is_null() {
        return Handle::null();
    }
    // SAFETY: the caller's live object, held.
    unsafe { call_handle(o.instance, name, values) }.unwrap_or_else(Handle::null)
}
fn m<T: crate::script_call::Ret>(o: &Handle, name: &str, values: &[Value]) -> Option<T> {
    if o.is_null() {
        return None;
    }
    // SAFETY: as above.
    unsafe { method::<T>(o.instance, name, values) }
}
fn name_of(w: &Handle) -> String {
    m::<u64>(w, "GetName", &[]).map(|n| CName(n).text().to_string()).unwrap_or_default()
}
fn children(w: &Handle) -> Vec<Handle> {
    let n = m::<i32>(w, "GetNumChildren", &[]).unwrap_or(0).clamp(0, 256);
    (0..n).map(|i| h(w, "GetWidgetByIndex", &[Value::Int32(i)])).filter(|c| !c.is_null()).collect()
}
/// The first widget named `name` at most `depth` levels below `w` (widgets that are not compound have no children).
fn find(w: &Handle, name: &str, depth: u32) -> Handle {
    if w.is_null() {
        return Handle::null();
    }
    // SAFETY: the widget, held.
    if unsafe { crate::script_call::find_function(w.instance, "GetNumChildren", 0) }.is_none() {
        return Handle::null();
    }
    for c in children(w) {
        if name_of(&c) == name {
            return c;
        }
    }
    if depth == 0 {
        return Handle::null();
    }
    for c in children(w) {
        let f = find(&c, name, depth - 1);
        if !f.is_null() {
            return f;
        }
    }
    Handle::null()
}
fn margins(w: &Handle) -> [f32; 4] {
    m::<[f32; 4]>(w, "GetMargin", &[]).unwrap_or([0.0; 4])
}
fn size_of(w: &Handle) -> [f32; 2] {
    m::<[f32; 2]>(w, "GetSize", &[]).unwrap_or([0.0; 2])
}
/// The widget holds children.
fn is_compound(w: &Handle) -> bool {
    // SAFETY: the widget, held.
    !w.is_null() && unsafe { crate::script_call::find_function(w.instance, "GetNumChildren", 0) }.is_some()
}
/// In a panel: at the left, its height's middle.
fn align(w: &Handle) {
    if let Some(a) = enum_value("inkEHorizontalAlign", "Left") {
        ink::call(w, "SetHAlign", &[Value::U64(a as u64)]);
    }
    if let Some(a) = enum_value("inkEVerticalAlign", "Center") {
        ink::call(w, "SetVAlign", &[Value::U64(a as u64)]);
    }
}
fn set_anchor(w: &Handle, anchor: &str, point: [f64; 2]) {
    if let Some(a) = enum_value("inkEAnchor", anchor) {
        ink::call(w, "SetAnchor", &[Value::U64(a as u64)]);
    }
    ink::v2(w, "SetAnchorPoint", point[0], point[1]);
}
/// `w` lies inside `ancestor` (at most 16 levels up).
fn inside(w: &Handle, ancestor: &Handle) -> bool {
    if ancestor.is_null() {
        return false;
    }
    let mut at = h(w, "GetParentWidget", &[]);
    for _ in 0..16 {
        if at.is_null() {
            return false;
        }
        if at.instance == ancestor.instance {
            return true;
        }
        at = h(&at, "GetParentWidget", &[]);
    }
    false
}

/// The HUD panels' scan found a controller (hud_panel.rs, once a second).
pub(super) fn found(class: CName, controller: &Handle) {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = g.get_or_insert_with(State::default);
    if class == CName::hashed("dialogWidgetGameController") {
        s.dialog = controller.clone();
    } else if class == CName::hashed("HotkeysWidgetController") {
        s.foot.found(controller);
    } else if class == CName::hashed("HotkeyConsumableWidgetController") {
        s.car.found(controller);
    }
}
/// The scan starts over (no HUD layer this frame): the old controllers and their hints go -- what they moved put back
/// first, in case the same widgets come back with the layer.
pub(super) fn reset() {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = g.as_mut() {
        s.dialog = Handle::null();
        s.foot.restore();
        s.car.restore();
        if let Some((hint, _)) = s.dialog_hint.take() {
            drop_chord(&hint);
        }
        for p in s.prefixes.drain(..) {
            drop_prefix(&p);
        }
        s.replaced.clear();
        s.hidden.clear();
    }
}

// ---------------------------------------------------------------- the icons' prefixes
fn prefixes(s: &mut State) {
    use core::sync::atomic::Ordering::Relaxed;
    s.prefix_acc += 1;
    let (displays, changed) = cvr_rig::vrik::input_prompts::chord_displays();
    // how the modifier is held now (the scheme and its side): another one, other glyphs
    let layout = (cvr_input::modifier::SCHEME.load(Relaxed), cvr_input::modifier::SIDE.load(Relaxed));
    if !changed && layout == s.prefix_layout && s.prefix_acc < 30 {
        // the prefixes' tint and opacity follow the icons' (every third tick: 0.3 s)
        if s.prefix_acc % 3 == 0 {
            tint_prefixes(s);
        }
        return;
    }
    s.prefix_acc = 0;
    let sig: Vec<(usize, u8, u32)> = displays.iter().map(|(w, k, n)| (w.lock().instance as usize, *k, *n)).collect();
    // THE PERIODIC LOOK (every 3 s with nothing told): the whole pass walks every display's widgets (4-7 ms on the main
    // thread, a late frame each time). With the same displays at the same refreshes and the same layout, only what a
    // pass would redo is checked -- each hint still in its row, its display on the same root -- and the pass runs only
    // when one is not.
    if !changed && !s.prefix_incomplete && layout == s.prefix_layout && sig == s.prefix_sig && prefixes_standing(s, &displays) {
        tint_prefixes(s);
        return;
    }
    s.prefix_sig = sig;
    s.prefix_layout = layout;
    s.prefix_incomplete = false;
    let hotkeys_root = h(&s.foot.controller, "GetRootWidget", &[]);
    // the displays with a chord's icon now, with their roots: one whose root sits inside another's shows no modifier of
    // its own (a prompt's second icon -- ← →, ↑ ↓ -- stands after the first one's, one modifier for both)
    let mut live: Vec<(Handle, Handle, u8)> = Vec::with_capacity(displays.len());
    let mut replaced: Vec<(usize, u32)> = Vec::new();
    let mut hidden: Vec<(usize, Handle)> = Vec::new();
    for (weak, kind, serial) in displays {
        let controller = weak.lock();
        if controller.is_null() {
            continue;
        }
        if kind == 0 {
            live.push((controller, Handle::null(), 0));
            continue;
        }
        let root = h(&controller, "GetRootWidget", &[]);
        if kind == cvr_rig::vrik::input_prompts::UNREACHABLE {
            let id = controller.instance as usize;
            let icon = super::world_touch::widget_ref(&controller, "iconRef").unwrap_or_else(|_| Handle::null());
            ink::opacity(&icon, 0.0);
            hidden.push((id, icon));
            live.push((controller, root, 0));
            continue;
        }
        let Some(part) = replacement(kind) else {
            live.push((controller, root, kind));
            continue;
        };
        // the icon itself replaced, after each refresh that set the game's again
        let id = controller.instance as usize;
        if !s.replaced.contains(&(id, serial)) {
            let icon = find(&root, "inputIcon", 4);
            if !icon.is_null() {
                ink::call(&icon, "SetAtlasResource", &[Value::U64(red4ext_abi::resource::resource_path(ATLAS))]);
                ink::call(&icon, "SetTexturePart", &[Value::U64(CName::hashed(part).0)]);
            }
            if s.said < 96 {
                s.said += 1;
                log!("[hud-hints] {} shown as {part} (the grips are the shoulders in a menu)\n", name_of(&root));
            }
        }
        replaced.push((id, serial));
        live.push((controller, root, 0));
    }
    s.replaced = replaced;
    for (id, icon) in &s.hidden {
        if !hidden.iter().any(|n| n.0 == *id) {
            ink::opacity(icon, 1.0);
        }
    }
    s.hidden = hidden;
    // each display's icon and the row it stands in
    struct Seat {
        id: usize,
        kind: u8,
        root: Handle,
        icon: Handle,
        row: Option<(Handle, Handle)>,
        index: i32,
    }
    let mut seats: Vec<Seat> = Vec::with_capacity(live.len());
    for (controller, root, kind) in &live {
        let id = controller.instance as usize;
        if *kind != 0 && root.is_null() {
            s.prefix_incomplete = true;
        }
        if *kind == 0 || root.is_null() {
            if let Some(i) = s.prefixes.iter().position(|p| p.controller == id) {
                drop_prefix(&s.prefixes.remove(i));
            }
            continue;
        }
        let mut icon = super::world_touch::widget_ref(controller, "iconRef").unwrap_or_else(|_| Handle::null());
        if icon.is_null() {
            icon = find(root, "inputIcon", 4);
        }
        let row = if icon.is_null() { None } else { row_of(&icon, root) };
        let index = row.as_ref().map_or(0, |(r, c)| index_in(r, c));
        seats.push(Seat { id, kind: *kind, root: root.clone(), icon, row, index });
    }
    let mut keep = Vec::with_capacity(seats.len());
    for k in 0..seats.len() {
        let seat = &seats[k];
        let (id, kind, root) = (seat.id, seat.kind, seat.root.clone());
        let at = s.prefixes.iter().position(|p| p.controller == id);
        keep.push(id);
        // one modifier for a prompt's two icons (← →, ↑ ↓): the first one's -- a display inside another's root, or two on
        // one root
        let second = seats.iter().enumerate().any(|(n, o)| {
            n != k
                && (if o.root.instance == root.instance { (o.index, n) < (seat.index, k) } else { inside(&root, &o.root) })
        });
        // the cyberware's LB + RB is replaced as a whole (quick_slots)
        let parent = h(&root, "GetParentWidget", &[]);
        if second || name_of(&parent) == "LbRb" {
            if let Some(i) = at {
                drop_prefix(&s.prefixes.remove(i));
            }
            continue;
        }
        let first = modifier_for(kind);
        let fresh = at.is_some_and(|i| {
            let p = &s.prefixes[i];
            p.root.instance == root.instance && p.kind == kind && p.chord.glyphs.0 == first
        });
        if fresh {
            continue;
        }
        if let Some(i) = at {
            drop_prefix(&s.prefixes.remove(i));
        }
        let size = match size_of(&seat.icon) {
            [w, _] if w > 8.0 => f64::from(w),
            _ => ICON,
        };
        let quick = inside(&root, &hotkeys_root);
        let mut room = None;
        let mut arrow = None;
        let (c, how) = if let Some((row, child)) = &seat.row {
            // the game's glyph box, for the hint's height
            let height = f64::from(size_of(child)[1]).max(size);
            let c = chord(row, &first, None, size, height, HUD_COLOUR);
            // the glyph's place now: when the seats were taken the hint made for the last layout still stood in front
            // of it (dropped above), and the new one put at that index went in after the glyph -- reversed in a row,
            // laid over the glyph in the quick slots, whenever the modifier changed (the thumbrest first touched on a
            // Quest, the controls turned left-handed)
            m::<()>(row, "ReorderChild", &[Value::Handle(&c.canvas), Value::Int32(index_in(row, child))]);
            if quick || fixed(row) {
                // a row that keeps its size (and the quick slots): the hint drawn to the left of the glyph, no room taken
                // -- in a selector, its left arrow moved out in front of the hint
                let left = f64::from(margins(child)[0]) - c.width;
                ink::margin(&c.canvas, left, 0.0, -(left + c.width), 0.0);
                if name_of(row) == SELECTOR {
                    let button = find(&h(row, "GetParentWidget", &[]), SELECTOR_ARROW, 1);
                    if !button.is_null() {
                        let was = margins(&button);
                        ink::margin(&button, f64::from(was[0]) - c.width, f64::from(was[1]), f64::from(was[2]), f64::from(was[3]));
                        arrow = Some((button, was));
                    }
                }
                align(&c.canvas);
                (c, "drawn to the left of the glyph")
            } else {
                ink::margin(&c.canvas, 0.0, 0.0, 4.0, 0.0);
                align(&c.canvas);
                (c, "in the row")
            }
        } else if is_compound(&root) {
            // no row of its own (a hint's key): drawn to the left of it, room made in the row the display stands in
            let c = chord(&root, &first, None, size, size, HUD_COLOUR);
            ink::margin(&c.canvas, -(c.width + 4.0), 0.0, 0.0, 0.0);
            align(&c.canvas);
            if is_row(&parent) {
                let was = margins(&root);
                ink::margin(&root, f64::from(was[0]) + c.width + 4.0, f64::from(was[1]), f64::from(was[2]), f64::from(was[3]));
                room = Some(was);
            }
            (c, "in front of its key")
        } else {
            continue;
        };
        if s.said < 96 {
            s.said += 1;
            log!(
                "[hud-hints] {} with {} in {} in {} ({how}), {size:.0} units\n",
                first.part,
                name_of(&root),
                name_of(&parent),
                name_of(&h(&parent, "GetParentWidget", &[])),
            );
        }
        s.prefixes.push(Prefix { controller: id, root, icon: seat.icon.clone(), chord: c, kind, room, arrow });
    }
    // gone controllers: their widgets went with them
    s.prefixes.retain(|p| keep.contains(&p.controller));
    tint_prefixes(s);
}
/// Every prefix's hint still has its parent and its display the root it was made on.
fn prefixes_standing(s: &State, displays: &[(red4ext_abi::handle::WeakHandle, u8, u32)]) -> bool {
    s.prefixes.iter().all(|p| {
        let Some((w, _, _)) = displays.iter().find(|(w, _, _)| w.lock().instance as usize == p.controller) else { return false };
        let controller = w.lock();
        !controller.is_null()
            && h(&controller, "GetRootWidget", &[]).instance == p.root.instance
            && !h(&p.chord.canvas, "GetParentWidget", &[]).is_null()
    })
}
fn tint_prefixes(s: &State) {
    for p in &s.prefixes {
        let tint = m::<[f32; 4]>(&p.icon, "GetTintColor", &[]).unwrap_or([HUD_COLOUR[0] as f32, HUD_COLOUR[1] as f32, HUD_COLOUR[2] as f32, 1.0]);
        let shown = m::<bool>(&p.icon, "IsVisible", &[]).unwrap_or(true);
        tint_chord(&p.chord, tint, if shown { m::<f32>(&p.icon, "GetOpacity", &[]).unwrap_or(1.0) } else { 0.0 });
    }
}

// ---------------------------------------------------------------- the quick slots
/// A row of quick slots on the pad: moved apart for the hints, the cyberware's LB + RB shown as its own chord.
fn quick_slots(s: &mut Slots, rooms: &[(&str, f64)], probe: &str) {
    let root = h(&s.controller, "GetRootWidget", &[]);
    if root.is_null() {
        return;
    }
    if s.cyber.lbrb.is_null() {
        s.cyber.lbrb = find(&root, "LbRb", 8);
        let lb = find(&s.cyber.lbrb, "inputDisplayControllerLB", 1);
        s.cyber.lb_icon = find(&lb, "inputIcon", 4);
    }
    // the pad's glyphs shown: the consumable's own icon is a key or a mouse button on the keyboard (the cyberware's LB
    // keeps the pad's part then: it is hidden, not looked up again)
    if s.cyber.probe.is_null() {
        s.cyber.probe = find(&find(&root, probe, 3), "inputIcon", 8);
    }
    let part = m::<u64>(&s.cyber.probe, "GetTexturePart", &[]).map(|n| CName(n).text().to_string()).unwrap_or_default();
    let native = cvr_input::modifier::NATIVE_PAD.load(core::sync::atomic::Ordering::Relaxed);
    let pad = !native && !part.is_empty() && !part.starts_with("kb_") && !part.starts_with("mouse") && part != "None";
    // the row apart
    if pad && s.spread.is_empty() {
        for (name, room) in rooms {
            let w = find(&root, name, 3);
            if w.is_null() {
                continue;
            }
            let was = margins(&w);
            ink::margin(&w, f64::from(was[0]) + room, f64::from(was[1]), f64::from(was[2]), f64::from(was[3]));
            s.spread.push((w, was));
        }
        log!("[hud-hints] the quick slots' row drawn apart for the chords' hints ({} slots)\n", s.spread.len());
    } else if !pad && !s.spread.is_empty() {
        for (w, was) in s.spread.drain(..) {
            ink::margin(&w, f64::from(was[0]), f64::from(was[1]), f64::from(was[2]), f64::from(was[3]));
        }
    }
    // the cyberware's chord in place of LB + RB
    let lbrb = s.cyber.lbrb.clone();
    if lbrb.is_null() {
        return;
    }
    let want = [ours("vr_grip_l"), pads("left_thumb")];
    if pad {
        let parent = h(&lbrb, "GetParentWidget", &[]);
        if s.cyber.chord.as_ref().is_none_or(|c| c.glyphs.0 != want[0]) {
            if let Some(old) = s.cyber.chord.take() {
                drop_chord(&old);
            }
            let size = match size_of(&s.cyber.lb_icon) {
                [w, _] if w > 8.0 => f64::from(w),
                _ => ICON,
            };
            let c = chord(&parent, &want[0], Some(&want[1]), size, size, HUD_COLOUR);
            // where LB + RB was: centred at the top of the hints' box
            set_anchor(&c.canvas, "TopCenter", [0.5, 0.0]);
            ink::margin(&c.canvas, 0.0, 0.0, 0.0, 0.0);
            log!("[hud-hints] the cyberware's LB + RB shown as its chord: the left grip + the left stick's click\n");
            s.cyber.chord = Some(c);
        }
        ink::visible(&lbrb, false);
        if let Some(c) = &s.cyber.chord {
            let tint = m::<[f32; 4]>(&s.cyber.lb_icon, "GetTintColor", &[]).unwrap_or([HUD_COLOUR[0] as f32, HUD_COLOUR[1] as f32, HUD_COLOUR[2] as f32, 1.0]);
            tint_chord(c, tint, m::<f32>(&lbrb, "GetOpacity", &[]).unwrap_or(1.0));
            ink::visible(&c.canvas, true);
        }
    } else {
        // the keyboard's key is the game's own (E): LB + RB stays as the game left it
        if let Some(c) = &s.cyber.chord {
            ink::visible(&c.canvas, false);
        }
    }
}

// ---------------------------------------------------------------- the dialogue
/// The dialogue: the active hub with two or more choices gets the chord out to its left.
fn dialog(s: &mut State) {
    let c = s.dialog.clone();
    let open = !c.is_null() && rt::field(&c, "AreDialogsOpen").is_some_and(|v| v.bool());
    // the active hub's controller, else the first of the hubs shown (activeHubController is set only once a choice is
    // focused; a hub just put up has none)
    let hub = if open {
        rt::field(&c, "activeHubController").map(|v| v.handle()).filter(|h| !h.is_null()).unwrap_or_else(|| {
            rt::field(&c, "hubControllers").and_then(|list| (0..list.len()).find_map(|i| list.at(i).map(|v| v.handle()).filter(|h| !h.is_null()))).unwrap_or_else(Handle::null)
        })
    } else {
        Handle::null()
    };
    let choices = if hub.is_null() { 0 } else { rt::field(&hub, "itemControllers").map_or(0, |v| v.len()) };
    let hub_root = h(&hub, "GetRootWidget", &[]);
    let want = open && choices >= 2 && !hub_root.is_null();
    if *TEST_ON.get_or_init(|| std::env::var("CVR_HUD_HINTS_TEST").is_ok_and(|v| v == "1")) {
        static LAST: Mutex<String> = Mutex::new(String::new());
        let line = format!("controller {} open {open} hub {} choices {choices} hub root {}", !c.is_null(), !hub.is_null(), !hub_root.is_null());
        let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
        if *last != line {
            log!("[hud-hints] dialogue state: {line}\n");
            *last = line;
        }
    }
    if !want {
        if let Some((hint, _)) = &s.dialog_hint {
            ink::visible(&hint.canvas, false);
        }
        return;
    }
    let first = dpad_modifier();
    let stick = pads(&format!("{}_axis_updown", dpad_stick()));
    // the choices' own glyph (a choice row's input display), for the size and the colour
    let icon = find(&hub_root, "inputIcon", 8);
    // the choices show a key (the keyboard's F) or the controller has a D-pad of its own: no chord beside them
    let part = m::<u64>(&icon, "GetTexturePart", &[]).map(|n| CName(n).text().to_string()).unwrap_or_default();
    if part.starts_with("kb_") || part.starts_with("mouse") || cvr_input::modifier::NATIVE_PAD.load(core::sync::atomic::Ordering::Relaxed) {
        if let Some((hint, _)) = &s.dialog_hint {
            ink::visible(&hint.canvas, false);
        }
        return;
    }
    let size = match size_of(&icon) {
        [w, _] if w > 8.0 => f64::from(w),
        _ => ICON,
    };
    let stale = s.dialog_hint.as_ref().is_none_or(|(hint, parent)| {
        *parent != hub_root.instance as usize || hint.glyphs.0 != first || hint.glyphs.1.as_ref() != Some(&stick)
    });
    if stale {
        if let Some((old, _)) = s.dialog_hint.take() {
            drop_chord(&old);
        }
        // UNDER THE HUB'S TITLE, out to the left of the choices: the hub is a horizontal panel [dpadFlex, choices];
        // dpadFlex is a canvas of no width whose title hangs out to its left (anchored at its right, as wide as its
        // text). The hint hangs the same way, a row lower: right-aligned under the title, level with the second
        // choice, whatever the title's length.
        let gap = size * 0.4;
        let holder = find(&hub_root, "dpadFlex", 0);
        let hint = if holder.is_null() {
            let hint = chord(&hub_root, &first, Some(&stick), size, size, HUD_COLOUR);
            m::<()>(&hub_root, "ReorderChild", &[Value::Handle(&hint.canvas), Value::Int32(0)]);
            ink::margin(&hint.canvas, -(hint.width + gap), size, gap, 0.0);
            hint
        } else {
            let hint = chord(&holder, &first, Some(&stick), size, size, HUD_COLOUR);
            set_anchor(&hint.canvas, "TopRight", [1.0, 0.0]);
            ink::margin(&hint.canvas, 0.0, size, gap * 0.5, 0.0);
            hint
        };
        if s.said < 24 {
            s.said += 1;
            log!(
                "[hud-hints] dialogue: {choices} choices, the hint ({} + {}) out to the left of the hub, {size:.0} units\n",
                first.part,
                stick.part
            );
        }
        s.dialog_hint = Some((hint, hub_root.instance as usize));
    }
    if let Some((hint, _)) = &s.dialog_hint {
        let tint = m::<[f32; 4]>(&icon, "GetTintColor", &[]).unwrap_or([HUD_COLOUR[0] as f32, HUD_COLOUR[1] as f32, HUD_COLOUR[2] as f32, 1.0]);
        tint_chord(hint, tint, 1.0);
        ink::visible(&hint.canvas, true);
    }
}

pub(in crate::mechanics) fn tick(dt: f32) {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = g.get_or_insert_with(State::default);
    s.acc += f64::from(dt);
    if s.acc < 0.1 {
        return;
    }
    s.acc = 0.0;
    dialog(s);
    quick_slots(&mut s.foot, FOOT, "consumable");
    // SAFETY: the plugin's flag.
    if unsafe { core::ptr::read_volatile(&raw const cvr_rig::vrik::env::g_isInVehicle) } {
        quick_slots(&mut s.car, CAR, "container");
    }
    prefixes(s);
    drop(g);
    test_tick();
}

// ---------------------------------------------------------------- the self-test (CVR_HUD_HINTS_TEST=1)
/// A dialogue hub of three choices put on the UIInteractions board some seconds after the player is in the world
/// (DialogChoiceHubs and ActiveChoiceHubID, as a scene's choice hub sends them), held 15 s for the session's still of
/// the eyes, and taken away.
struct Test {
    frames: u32,
    stage: u8,
}
static TEST: Mutex<Test> = Mutex::new(Test { frames: 0, stage: 0 });
static TEST_ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
fn test_tick() {
    if !*TEST_ON.get_or_init(|| std::env::var("CVR_HUD_HINTS_TEST").is_ok_and(|v| v == "1")) {
        return;
    }
    let pl = super::menu_rt::player();
    if pl.is_null() || !super::menu_rt::b(&pl, "IsAttached", &[]) {
        return;
    }
    let mut t = TEST.lock().unwrap_or_else(|p| p.into_inner());
    t.frames += 1; // ten a second
    match t.stage {
        0 if t.frames >= 120 => {
            match fake_hub(true) {
                Ok(()) => log!("[hud-hints-test] a dialogue hub of three choices shown\n"),
                Err(e) => log!("[hud-hints-test] the hub could not be shown: {e}\n"),
            }
            t.stage = 1;
            t.frames = 0;
        }
        1 if t.frames >= 150 => {
            let _ = fake_hub(false);
            log!("[hud-hints-test] the hub taken away\n[hud-hints-test] done\n");
            t.stage = 2;
        }
        _ => {}
    }
}
fn fake_hub(show: bool) -> Result<(), String> {
    use red4ext_abi::rtti::RttiSystem;
    let defs = super::boards::defs()?;
    let def = super::boards::definition(&defs, "UIInteractions")?.clone();
    let board = super::boards::system_board(&def)?;
    if board.is_null() {
        return Err("no UIInteractions board".into());
    }
    let hubs_id = super::boards::field(&def, "DialogChoiceHubs")?;
    let active_id = super::boards::field(&def, "ActiveChoiceHubID")?;
    let hubs = super::menu_rt::new_struct("DialogChoiceHubs").ok_or("no DialogChoiceHubs")?;
    if show {
        let list = rt::Val::of(&hubs).f("choiceHubs").ok_or("no choiceHubs")?;
        let rtti = RttiSystem::get().ok_or("no RTTI")?;
        let _ = rtti;
        // SAFETY: the array's own type, one element.
        unsafe { list.ty.array_resize(list.ptr, 1) };
        let hub = list.at(0).ok_or("no hub")?;
        if let Some(f) = hub.f("title") {
            // SAFETY: a String field.
            unsafe { red4ext_abi::cstring::CString::assign(f.ptr.cast(), "HUD hints test") };
        }
        super::menu_rt::set(&hub, "id", 4242.0);
        super::menu_rt::set(&hub, "activityState", rt::enum_value("gameinteractionsvisEVisualizerActivityState", "Active"));
        let choices = hub.f("choices").ok_or("no choices")?;
        // SAFETY: as above, three elements.
        unsafe { choices.ty.array_resize(choices.ptr, 3) };
        for (i, text) in ["The first choice", "The second choice", "The third choice"].iter().enumerate() {
            let c = choices.at(i).ok_or("no choice")?;
            if let Some(f) = c.f("localizedName") {
                // SAFETY: a String field.
                unsafe { red4ext_abi::cstring::CString::assign(f.ptr.cast(), text) };
            }
            if let Some(f) = c.f("inputActionName") {
                // SAFETY: a CName field.
                unsafe { *(f.ptr as *mut u64) = CName::hashed(&format!("Choice{}", i + 1)).0 };
            }
        }
    }
    let var = super::menu_rt::variant_of(&hubs).ok_or("no variant")?;
    rt::call(&board, "SetVariant", &[rt::A::P(hubs_id.cast_const()), rt::A::P(var.as_ptr().cast()), rt::A::B(true)]).ok_or("SetVariant failed")?;
    rt::call(&board, "SetInt", &[rt::A::P(active_id.cast_const()), rt::A::N(if show { 4242.0 } else { -1.0 }), rt::A::B(true)]);
    Ok(())
}
