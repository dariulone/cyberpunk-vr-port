//! THE VR MENU'S RECORDER (formerly src/Overlay/UiRec.cpp; include/Overlay/UiRec.hpp says what it is): the settings
//! panels read out as data for the in-world VR menu. During a RECORD PASS every checkbox, slider, combo, button,
//! header, text and tooltip the panels draw is written down as an item instead, and an edit the VR menu sent for an
//! item is applied the way a click or a drag would have been; the page is published as one string.
//!
//! The pass runs on the render thread at the top of the overlay's frame, where the panels themselves run; the CET
//! natives (any thread) ask for a page, send edits and read the latest page under one lock.
//!
//! THE PAGE STRING: items separated by \x1e, fields by \x1f: kind, id, label, value, min, max, text, flags, tooltip,
//! indent. The first item is the page header: kind "#", value = the page, min = the sequence number.
use core::ffi::{c_char, CStr};
use core::sync::atomic::{AtomicBool, Ordering::*};
use std::collections::HashMap;
use std::sync::Mutex;

use cvr_crt::{format, Arg};
use cvr_settings::live_controls_ui_state::LiveControlsUiState;

extern "C" {
    fn GetLiveControlsUiState(out: *mut LiveControlsUiState);
}

/// The pages, in the panel's own tab order.
pub const PAGE_COUNT: i32 = 7;
const PAGE_NAMES: [&CStr; 7] = [
    c"GENERAL",
    c"FRAMEGEN",
    c"HUD",
    c"CONTROLS",
    c"STEREO",
    c"AVATAR",
    c"OVERLAY",
];

#[derive(Clone, Default)]
struct Edit {
    page: i32,
    id: Vec<u8>,
    value: f64,
}
struct Item {
    kind: u8,
    id: Vec<u8>,
    label: Vec<u8>,
    text: Vec<u8>,
    tooltip: Vec<u8>,
    value: f64,
    minimum: f64,
    maximum: f64,
    flags: i32,
    indent: i32,
}
struct Recorder {
    edits: Vec<Edit>,
    items: Vec<Item>,
    ids: Vec<Vec<u8>>,
    disabled: Vec<bool>,
    seen: HashMap<Vec<u8>, i32>,
    indent: i32,
}
impl Recorder {
    fn disabled(&self) -> bool {
        self.disabled.contains(&true)
    }
    /// The id is the ImGui one -- the PushID scope and the whole label, "##" part included -- so two "Center width"
    /// sliders under different scopes stay apart; a repeat in one scope gets "#n".
    fn make_id(&mut self, label: &[u8]) -> Vec<u8> {
        let mut id = Vec::new();
        for s in &self.ids {
            id.extend_from_slice(s);
            id.push(b'/');
        }
        id.extend_from_slice(label);
        let seen = self.seen.entry(id.clone()).or_insert(0);
        let n = *seen;
        *seen += 1;
        if n != 0 {
            id.push(b'#');
            id.extend_from_slice(n.to_string().as_bytes());
        }
        id
    }
    fn add(&mut self, kind: u8, label: Option<&CStr>) -> usize {
        let bytes = label.map(|l| l.to_bytes()).unwrap_or(b"");
        let id = self.make_id(bytes);
        let shown = match label {
            Some(l) => {
                let b = l.to_bytes();
                match b.windows(2).position(|w| w == b"##") {
                    Some(at) => b[..at].to_vec(),
                    None => b.to_vec(),
                }
            }
            None => Vec::new(),
        };
        let flags = if self.disabled() { 16 } else { 0 };
        self.items.push(Item {
            kind,
            id,
            label: shown,
            text: Vec::new(),
            tooltip: Vec::new(),
            value: 0.0,
            minimum: 0.0,
            maximum: 0.0,
            flags,
            indent: self.indent,
        });
        self.items.len() - 1
    }
    /// The edit the VR menu sent for this item, taken (once); none, and never for a disabled item -- the panel would
    /// not have let it change either.
    fn take(&mut self, item: usize) -> Option<Edit> {
        let it = &self.items[item];
        if it.flags & 16 != 0 {
            return None;
        }
        let at = self.edits.iter().position(|e| e.id == it.id)?;
        Some(self.edits.remove(at))
    }
}

static RECORDING: AtomicBool = AtomicBool::new(false);
static REC: Mutex<Option<Recorder>> = Mutex::new(None);

struct Shared {
    requested: i32,
    edits: Vec<Edit>,
    latest: [Vec<u8>; 7],
    sequence: u32,
}
static SHARED: Mutex<Shared> = Mutex::new(Shared {
    requested: -1,
    edits: Vec::new(),
    latest: [const { Vec::new() }; 7],
    sequence: 0,
});
fn shared() -> std::sync::MutexGuard<'static, Shared> {
    SHARED.lock().unwrap_or_else(|e| e.into_inner())
}
/// The recorder of the pass under way.
fn with<R>(f: impl FnOnce(&mut Recorder) -> R) -> R {
    let mut guard = REC.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.as_mut().expect("a record pass under way"))
}

/// snprintf(buffer, size, format, args...) -- what it leaves in the buffer.
fn snprintf(size: usize, fmt: &CStr, args: &[Arg]) -> Vec<u8> {
    let mut text = format(fmt, args).into_bytes();
    text.truncate(size - 1);
    text
}
/// A slider's value as the panel formats it.
fn format_value(fmt: &CStr, value: f64, integer: bool) -> Vec<u8> {
    let fmt = if fmt.is_empty() {
        if integer {
            c"%d"
        } else {
            c"%.3f"
        }
    } else {
        fmt
    };
    if integer {
        snprintf(128, fmt, &[Arg::I(value as i32)])
    } else {
        snprintf(128, fmt, &[Arg::F(value)])
    }
}
/// The separators out of a field.
fn clean(s: &mut [u8]) {
    for c in s.iter_mut() {
        if *c == 0x1e || *c == 0x1f || *c == 0x1d {
            *c = b' ';
        }
    }
}
fn serialize(page: i32, sequence: u32, items: &mut [Item]) -> Vec<u8> {
    let mut out = snprintf(
        64,
        c"#\x1f\x1f%s\x1f%d\x1f%u\x1f\x1f\x1f\x1f\x1f",
        &[
            Arg::S(PAGE_NAMES[page as usize].as_ptr()),
            Arg::I(page),
            Arg::U(sequence as u64),
        ],
    );
    for it in items.iter_mut() {
        clean(&mut it.label);
        clean(&mut it.tooltip);
        if it.kind != b'O' && it.kind != b'P' {
            clean(&mut it.text);
        }
        out.push(0x1e);
        out.push(it.kind);
        out.push(0x1f);
        out.extend_from_slice(&it.id);
        out.push(0x1f);
        out.extend_from_slice(&it.label);
        out.extend(snprintf(
            96,
            c"\x1f%.9g\x1f%.9g\x1f%.9g\x1f",
            &[Arg::F(it.value), Arg::F(it.minimum), Arg::F(it.maximum)],
        ));
        out.extend_from_slice(&it.text);
        out.extend(snprintf(96, c"\x1f%d\x1f", &[Arg::I(it.flags)]));
        out.extend_from_slice(&it.tooltip);
        out.extend(snprintf(96, c"\x1f%d", &[Arg::I(it.indent)]));
    }
    out
}

/// One run of the page's panel code, recording; true when an edit changed the live controls.
fn record(page: i32, edits: Vec<Edit>, state: &mut LiveControlsUiState) -> (bool, Vec<Item>) {
    *REC.lock().unwrap_or_else(|e| e.into_inner()) = Some(Recorder {
        edits,
        items: Vec::new(),
        ids: vec![page.to_string().into_bytes()],
        disabled: Vec::new(),
        seen: HashMap::new(),
        indent: 0,
    });
    RECORDING.store(true, Release);
    let changed = if (0..=5).contains(&page) {
        // SAFETY: the snapshot, for the pass.
        unsafe { crate::overlay_panels::cvr_draw_live_controls(state, page, core::ptr::null_mut()) }
    } else if page == 6 {
        // SAFETY: as above.
        unsafe { crate::vr_style::cvr_draw_overlay_page(state) }
    } else {
        false
    };
    RECORDING.store(false, Release);
    let rec = REC.lock().unwrap_or_else(|e| e.into_inner()).take();
    (changed, rec.map(|r| r.items).unwrap_or_default())
}

/// A record pass is under way: the widgets write items instead of drawing.
pub fn recording() -> bool {
    RECORDING.load(Acquire)
}

/// Render thread: record the requested page, if one was asked for or an edit is waiting.
#[no_mangle]
pub extern "C" fn cvr_uirec_run_pending_pass() {
    let (page, edits) = {
        let mut s = shared();
        if s.requested < 0 && s.edits.is_empty() {
            return;
        }
        let page = if s.edits.is_empty() {
            s.requested
        } else {
            s.edits[0].page
        };
        let (mine, rest): (Vec<Edit>, Vec<Edit>) = core::mem::take(&mut s.edits)
            .into_iter()
            .partition(|e| e.page == page);
        s.edits = rest;
        if s.requested == page {
            s.requested = -1;
        }
        (page, mine)
    };
    // SAFETY: plain numbers and flags; the C++ half assigns the whole snapshot.
    let mut state: LiveControlsUiState = unsafe { core::mem::zeroed() };
    // SAFETY: a snapshot to fill.
    unsafe { GetLiveControlsUiState(&mut state) };
    let had_edits = !edits.is_empty();
    let (changed, mut items) = record(page, edits, &mut state);
    if changed {
        // SAFETY: the snapshot just edited.
        unsafe { crate::live_controls_ui::SetLiveControlsUiState(&state, 1) };
    }
    // After an edit the page is read once more, so what it publishes is what the edit made of the controls -- items
    // above the edited one that depend on it included.
    if had_edits {
        // SAFETY: as above.
        let mut fresh: LiveControlsUiState = unsafe { core::mem::zeroed() };
        // SAFETY: as above.
        unsafe { GetLiveControlsUiState(&mut fresh) };
        items = record(page, Vec::new(), &mut fresh).1;
    }
    let mut s = shared();
    s.sequence = s.sequence.wrapping_add(1);
    let sequence = s.sequence;
    s.latest[page as usize] = serialize(page, sequence, &mut items);
}
/// Any thread: ask for a page.
#[no_mangle]
pub extern "C" fn cvr_uirec_request(page: i32) {
    if !(0..PAGE_COUNT).contains(&page) {
        return;
    }
    shared().requested = page;
}
/// Any thread: an edit for an item of a page.
///
/// # Safety
/// `id` is null or a C string.
#[no_mangle]
pub unsafe extern "C" fn cvr_uirec_queue_edit(page: i32, id: *const c_char, value: f64) {
    if !(0..PAGE_COUNT).contains(&page) || id.is_null() {
        return;
    }
    // SAFETY: the caller's C string.
    let id = unsafe { CStr::from_ptr(id) }.to_bytes().to_vec();
    let mut s = shared();
    // A slider dragged from the VR menu sends a stream of values; only the newest one matters.
    if let Some(e) = s.edits.iter_mut().find(|e| e.page == page && e.id == id) {
        e.value = value;
        return;
    }
    s.edits.push(Edit { page, id, value });
}
/// Any thread: the page as last published, into `out` (at most `capacity` bytes); its length.
///
/// # Safety
/// `out` is writable for `capacity` bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_uirec_latest(page: i32, out: *mut u8, capacity: usize) -> usize {
    if !(0..PAGE_COUNT).contains(&page) {
        return 0;
    }
    let s = shared();
    let text = &s.latest[page as usize];
    // SAFETY: the caller's buffer.
    unsafe { core::ptr::copy_nonoverlapping(text.as_ptr(), out, text.len().min(capacity)) };
    text.len()
}

/// The page's latest record for the VR menu's natives (cvr-rig's vr_menu.rs; formerly Natives/VrMenu.cpp's, over
/// cvr::uirec::Latest): as cvr_uirec_latest, which it is (tools/vrik_tests keep a stub of this name).
///
/// # Safety
/// `out` holds `capacity` bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_uirec_latest(page: i32, out: *mut u8, capacity: usize) -> usize {
    // SAFETY: as documented.
    unsafe { cvr_uirec_latest(page, out, capacity) }
}

// ---- the recorder's side of the widgets; each returns what the widget would have (changed / clicked) -----------

pub fn checkbox(label: &CStr, value: &mut bool) -> bool {
    with(|r| {
        let it = r.add(b'C', Some(label));
        let mut changed = false;
        if let Some(e) = r.take(it) {
            let next = e.value != 0.0;
            changed = next != *value;
            *value = next;
        }
        r.items[it].value = if *value { 1.0 } else { 0.0 };
        changed
    })
}
/// std::min / std::max, as they compare.
fn min<T: PartialOrd>(a: T, b: T) -> T {
    if b < a {
        b
    } else {
        a
    }
}
fn max<T: PartialOrd>(a: T, b: T) -> T {
    if a < b {
        b
    } else {
        a
    }
}
fn clamp<T: PartialOrd>(v: T, lo: T, hi: T) -> T {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}
/// std::lround then static_cast<int>.
fn lround(v: f64) -> i32 {
    v.round() as i64 as i32
}
pub fn slider_float(label: &CStr, value: &mut f32, minimum: f32, maximum: f32, fmt: &CStr) -> bool {
    with(|r| {
        let it = r.add(b'S', Some(label));
        let mut changed = false;
        if let Some(e) = r.take(it) {
            let next = clamp(e.value as f32, min(minimum, maximum), max(minimum, maximum));
            changed = next != *value;
            *value = next;
        }
        let item = &mut r.items[it];
        item.value = *value as f64;
        item.minimum = minimum as f64;
        item.maximum = maximum as f64;
        item.text = format_value(fmt, *value as f64, false);
        changed
    })
}
pub fn slider_int(label: &CStr, value: &mut i32, minimum: i32, maximum: i32, fmt: &CStr) -> bool {
    with(|r| {
        let it = r.add(b'I', Some(label));
        let mut changed = false;
        if let Some(e) = r.take(it) {
            let next = clamp(
                lround(e.value),
                min(minimum, maximum),
                max(minimum, maximum),
            );
            changed = next != *value;
            *value = next;
        }
        let item = &mut r.items[it];
        item.value = *value as f64;
        item.minimum = minimum as f64;
        item.maximum = maximum as f64;
        item.text = format_value(fmt, *value as f64, true);
        changed
    })
}
pub fn combo(label: &CStr, selected: &mut i32, items: &[*const c_char]) -> bool {
    let count = items.len() as i32;
    with(|r| {
        let it = r.add(b'O', Some(label));
        let mut changed = false;
        if let Some(e) = r.take(it) {
            let next = clamp(lround(e.value), 0, max(0, count - 1));
            changed = next != *selected;
            *selected = next;
        }
        let item = &mut r.items[it];
        item.value = *selected as f64;
        item.minimum = 0.0;
        item.maximum = (count - 1) as f64;
        for (i, option) in items.iter().enumerate() {
            if i != 0 {
                item.text.push(0x1d);
            }
            let mut option = if option.is_null() {
                Vec::new()
            } else {
                // SAFETY: the caller's C string.
                unsafe { CStr::from_ptr(*option) }.to_bytes().to_vec()
            };
            clean(&mut option);
            item.text.extend(option);
        }
        changed
    })
}
pub fn tabs(id: &CStr, selected: &mut i32, labels: &[*const c_char]) -> bool {
    let count = labels.len() as i32;
    with(|r| {
        let it = r.add(b'P', Some(id));
        let mut changed = false;
        if let Some(e) = r.take(it) {
            let next = clamp(lround(e.value), 0, max(0, count - 1));
            changed = next != *selected;
            *selected = next;
        }
        let item = &mut r.items[it];
        item.value = *selected as f64;
        item.maximum = (count - 1) as f64;
        for (i, label) in labels.iter().enumerate() {
            if i != 0 {
                item.text.push(0x1d);
            }
            if !label.is_null() {
                // SAFETY: the caller's C string.
                item.text
                    .extend_from_slice(unsafe { CStr::from_ptr(*label) }.to_bytes());
            }
        }
        changed
    })
}
pub fn button(label: &CStr) -> bool {
    with(|r| {
        let it = r.add(b'B', Some(label));
        r.take(it).is_some()
    })
}
pub fn input_int(label: &CStr, value: &mut i32, step: i32) -> bool {
    with(|r| {
        let it = r.add(b'N', Some(label));
        let mut changed = false;
        if let Some(e) = r.take(it) {
            let next = lround(e.value);
            changed = next != *value;
            *value = next;
        }
        let item = &mut r.items[it];
        item.value = *value as f64;
        item.minimum = step as f64;
        item.text = value.to_string().into_bytes();
        changed
    })
}
pub fn header(label: &CStr, default_open: bool) -> bool {
    with(|r| {
        let it = r.add(b'H', Some(label));
        if default_open {
            r.items[it].flags |= 8;
        }
    });
    true
}
/// A text: `flags` 1 disabled-style, 2 wrapped, 4 coloured (`color` 0xRRGGBB).
pub fn text(flags: i32, color: u32, text: &[u8]) {
    with(|r| {
        let it = r.add(b'T', None);
        let item = &mut r.items[it];
        item.flags |= flags;
        item.value = color as f64;
        item.text = text.to_vec();
    })
}
pub fn binding(input: &CStr, action: &CStr) {
    with(|r| {
        let it = r.add(b'K', Some(input));
        let item = &mut r.items[it];
        item.label = input.to_bytes().to_vec();
        item.text = action.to_bytes().to_vec();
    })
}
pub fn progress(fraction: f32) {
    with(|r| {
        let it = r.add(b'G', None);
        r.items[it].value = fraction as f64;
    })
}
pub fn separator() {
    with(|r| {
        r.add(b'X', None);
    })
}
pub fn tooltip(text: &[u8]) {
    with(|r| {
        if let Some(last) = r.items.last_mut() {
            last.tooltip = text.to_vec();
        }
    })
}
pub fn begin_disabled(disabled: bool) {
    with(|r| r.disabled.push(disabled))
}
pub fn end_disabled() {
    with(|r| {
        r.disabled.pop();
    })
}
pub fn push_id_str(id: &CStr) {
    with(|r| r.ids.push(id.to_bytes().to_vec()))
}
pub fn push_id_int(id: i32) {
    with(|r| r.ids.push(id.to_string().into_bytes()))
}
pub fn pop_id() {
    with(|r| {
        if r.ids.len() > 1 {
            r.ids.pop();
        }
    })
}
pub fn indent(delta: i32) {
    with(|r| r.indent = max(0, r.indent + delta))
}
