//! The subtitles' self-test (CVR_SUBTITLE_TEST=1): some seconds after the player is in the world, lines of dialogue
//! shown through the game's own board (UIGameData.ShowDialogLine, the array of scnDialogLineData a scene's line
//! sends; the type AlwaysCinematicNoSpeaker, which the subtitles' controller always shows) -- a short one, then one
//! long enough to wrap -- each held some seconds and hidden again (UIGameData.HideDialogLine, by its id); the HUD
//! panel's placement of the subtitles' slot is logged by hud_natives while they show, and the session can take its
//! still of the eyes meanwhile.
use super::lobby_rtti::{self as rt, Val, A};
use crate::script_call::Owned;
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::rtti::RttiSystem;
use std::sync::Mutex;

struct Test {
    frames: u32,
    shown: u32,
}
static STATE: Mutex<Test> = Mutex::new(Test { frames: 0, shown: 0 });
static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
/// The frame the first line shows; each line's frames (shown, then hidden before the next).
const START: u32 = 600;
const EACH: u32 = 900;
const HIDE_AFTER: u32 = 780;
const LINES: [&str; 3] = [
    "Subtitle test: one short line.",
    "Subtitle test: a line long enough that the game's subtitle panel has to wrap it onto a second line, so its height grows.",
    "Subtitle test: the short line again.",
];
const ID_BASE: u64 = 0x7E57_5B71_0000_0000;

pub(in crate::mechanics) fn tick() {
    if !*ON.get_or_init(|| std::env::var("CVR_SUBTITLE_TEST").is_ok_and(|v| v == "1")) {
        return;
    }
    let pl = super::menu_rt::player();
    if pl.is_null() || super::belt::native::<bool>("VRLobbyActive", &[]) == Some(true) || !super::menu_rt::b(&pl, "IsAttached", &[]) {
        return;
    }
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    s.frames += 1;
    if s.frames < START {
        return;
    }
    let k = s.frames - START;
    let line = (k / EACH) as usize;
    if line >= LINES.len() {
        if k == LINES.len() as u32 * EACH {
            log!("[subtitle-test] done\n");
        }
        return;
    }
    let id = ID_BASE + line as u64 + 1;
    if k % EACH == 0 {
        s.shown += 1;
        match show(id, LINES[line]) {
            Ok(()) => log!("[subtitle-test] line {} shown: \"{}\"\n", line + 1, LINES[line]),
            Err(e) => log!("[subtitle-test] line {} not shown: {e}\n", line + 1),
        }
    } else if k % EACH == HIDE_AFTER {
        match hide(id) {
            Ok(()) => log!("[subtitle-test] line {} hidden\n", line + 1),
            Err(e) => log!("[subtitle-test] line {} not hidden: {e}\n", line + 1),
        }
    }
}

/// UIGameData's board and one of its fields' id.
fn board(field: &str) -> Result<(red4ext_abi::handle::Handle, *mut core::ffi::c_void), String> {
    let defs = super::boards::defs()?;
    let def = super::boards::definition(&defs, "UIGameData")?.clone();
    let board = super::boards::system_board(&def)?;
    if board.is_null() {
        return Err("no UIGameData board".into());
    }
    Ok((board, super::boards::field(&def, field)?))
}
/// An array of the named type with `n` default elements.
fn array_of(inner: &str, n: u32) -> Result<Owned, String> {
    let rtti = RttiSystem::get().ok_or("no RTTI")?;
    let ty = rtti.get_type(CName::hashed(&format!("array:{inner}"))).ok_or_else(|| format!("no array:{inner}"))?;
    let mut array = Owned::of_type(ty).ok_or("no array")?;
    // SAFETY: an array value of its own type.
    if !unsafe { ty.array_resize(array.as_mut_ptr().cast(), n) } {
        return Err("the array not resized".into());
    }
    Ok(array)
}
fn set_variant(field: &str, value: &Owned) -> Result<(), String> {
    let (board, id) = board(field)?;
    let var = super::menu_rt::variant_of(value).ok_or("no variant")?;
    rt::call(&board, "SetVariant", &[A::P(id.cast_const()), A::P(var.as_ptr().cast()), A::B(true)]).ok_or("SetVariant failed")?;
    Ok(())
}
fn show(id: u64, text: &str) -> Result<(), String> {
    let lines = array_of("scnDialogLineData", 1)?;
    let line = Val::of(&lines).at(0).ok_or("no line")?;
    let field = |name: &str| line.f(name).ok_or_else(|| format!("no scnDialogLineData.{name}"));
    // SAFETY: the line's own fields, each written by its type (CRUID is one 64-bit word, String a CString).
    unsafe {
        *(field("id")?.ptr as *mut u64) = id;
        red4ext_abi::cstring::CString::assign(field("text")?.ptr.cast(), text);
        red4ext_abi::cstring::CString::assign(field("speakerName")?.ptr.cast(), "V");
    }
    super::menu_rt::set(&line, "type", rt::enum_value("scnDialogLineType", "AlwaysCinematicNoSpeaker"));
    super::menu_rt::set(&line, "duration", 12.0);
    super::menu_rt::set(&line, "isPersistent", 1.0);
    set_variant("ShowDialogLine", &lines)
}
fn hide(id: u64) -> Result<(), String> {
    let ids = array_of("CRUID", 1)?;
    let at = Val::of(&ids).at(0).ok_or("no id")?;
    // SAFETY: a CRUID element: one 64-bit word.
    unsafe { *(at.ptr as *mut u64) = id };
    set_variant("HideDialogLine", &ids)
}
