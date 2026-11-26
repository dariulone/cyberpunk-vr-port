//! The VR menu's ear on the phone journal (formerly CyberpunkVRPort_Engine's vrport_menu_journal.reds). The game's
//! messenger knows two things a script hears only as journal callbacks: State (a message or a reply option became
//! active -- the thread changed) and StateDelay (a message was scheduled `delay` seconds out -- someone is typing; the
//! game shows the dots from half that delay until it lands). The journal calls a script object back by a function's
//! name, so the listener is an object of a class Rust defines (VRMenuJournalWatch, script_classes.rs) whose callbacks
//! keep the counters and the last scheduled message here, read by the menu (menu_messages.rs). Made on the first ask,
//! it stops listening when the player detaches.
use super::lobby_rtti::{self as rt, A};
use super::melee::n;
use super::menu_rt as r;
use crate::script_classes::{ClassDef, Method, CB};
use crate::script_hooks::{Call, Sig};
use red4ext_abi::handle::Handle;
use std::sync::Mutex;

/// The counters and the last scheduled message.
#[derive(Clone, Copy, Default)]
pub(super) struct Watch {
    pub updates: i64,
    pub scheduled: i64,
    pub last_delay: f64,
    pub last_contact: i64,
    pub last_conversation: i64,
    pub last_from_player: bool,
    pub last_typing: bool,
}
struct Listening {
    object: Handle,
    journal: Handle,
}
// SAFETY: the handles are only used on the main thread (the menu's tick and the journal's callbacks).
unsafe impl Send for Listening {}
static WATCH: Mutex<Watch> = Mutex::new(Watch {
    updates: 0,
    scheduled: 0,
    last_delay: 0.0,
    last_contact: 0,
    last_conversation: 0,
    last_from_player: false,
    last_typing: false,
});
static LISTENING: Mutex<Option<Listening>> = Mutex::new(None);

/// PlayerPuppet.VRMenuJournalWatch: the counters, the listener made and attached on the first call (None without a
/// journal).
pub(super) fn watch() -> Option<Watch> {
    let mut l = LISTENING.lock().unwrap_or_else(|p| p.into_inner());
    if l.is_none() {
        let journal = r::jm();
        let class = crate::script_call::class_any("VRMenuJournalWatch")?;
        if journal.is_null() {
            return None;
        }
        // SAFETY: a new instance owned by the handle made for it.
        let object = unsafe { Handle::from_instance(class.create_instance()) };
        if object.is_null() {
            return None;
        }
        *WATCH.lock().unwrap_or_else(|p| p.into_inner()) = Watch::default();
        rt::call(&journal, "RegisterScriptCallback", &[A::H(&object), n("OnJournalState"), A::N(rt::enum_value("gameJournalListenerType", "State"))]);
        rt::call(&journal, "RegisterScriptCallback", &[A::H(&object), n("OnJournalDelay"), A::N(rt::enum_value("gameJournalListenerType", "StateDelay"))]);
        *l = Some(Listening { object, journal });
    }
    Some(*WATCH.lock().unwrap_or_else(|p| p.into_inner()))
}
/// The player detached: the listener stops (VRMenuJournalWatch.Detach) and goes.
pub(super) fn detach() {
    let Some(l) = LISTENING.lock().unwrap_or_else(|p| p.into_inner()).take() else { return };
    rt::call(&l.journal, "UnregisterScriptCallback", &[A::H(&l.object), n("OnJournalState")]);
    rt::call(&l.journal, "UnregisterScriptCallback", &[A::H(&l.object), n("OnJournalDelay")]);
}

fn class_name(call: &Call) -> u64 {
    call.arg_u64(1)
}
fn is(name: u64, of: &str) -> bool {
    name == red4ext_abi::cname::CName::hashed(of).0
}
/// OnJournalState: a phone entry's change counted.
fn on_state(call: &mut Call) {
    let c = class_name(call);
    if is(c, "gameJournalPhoneMessage") || is(c, "gameJournalPhoneChoiceGroup") || is(c, "gameJournalPhoneChoiceEntry") {
        WATCH.lock().unwrap_or_else(|p| p.into_inner()).updates += 1;
    }
    call.set_result_bool(true);
}
/// OnJournalDelay: a phone message scheduled -- its delay, contact, thread, sender and whether it has text.
fn on_delay(call: &mut Call) {
    call.set_result_bool(false);
    let journal = LISTENING.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map(|l| l.journal.clone());
    let Some(journal) = journal.filter(|j| !j.is_null()) else { return };
    if !is(class_name(call), "gameJournalPhoneMessage") {
        return;
    }
    let hash = call.arg_number(0).unwrap_or(0.0);
    let delay = call.arg_number(4).unwrap_or(0.0);
    let entry = rt::obj(&journal, "GetEntry", &[A::N(hash)]);
    let message = if r::is_a(&entry, "gameJournalPhoneMessage") { entry.clone() } else { Handle::null() };
    let contact = rt::static_num("MessengerUtils", "FindContactHash", &[A::H(&journal), A::H(&entry)]).unwrap_or(0.0) as i64;
    let conversation = rt::static_num("MessengerUtils", "FindConversationHash", &[A::H(&journal), A::H(&entry)]).unwrap_or(0.0) as i64;
    let from_player = !message.is_null() && rt::num(&message, "GetSender", &[]) == Some(rt::enum_value("gameMessageSender", "Player"));
    let typing = !message.is_null() && !r::s(&message, "GetText", &[]).is_empty();
    let mut w = WATCH.lock().unwrap_or_else(|p| p.into_inner());
    w.last_delay = delay;
    w.last_contact = contact;
    w.last_conversation = conversation;
    w.last_from_player = from_player;
    w.last_typing = typing;
    w.scheduled += 1;
}

const STATE: Sig = Sig::Of(
    &[("entryHash", "Uint32"), ("className", "CName"), ("notifyOption", "gameJournalNotifyOption"), ("changeType", "gameJournalChangeType")],
    "Bool",
);
const DELAY: Sig = Sig::Of(
    &[
        ("entryHash", "Uint32"),
        ("className", "CName"),
        ("notifyOption", "gameJournalNotifyOption"),
        ("changeType", "gameJournalChangeType"),
        ("delay", "Float"),
    ],
    "Bool",
);
pub(crate) const CLASSES: [ClassDef; 1] = [ClassDef {
    name: "VRMenuJournalWatch",
    parent: "IScriptable",
    methods: &[
        Method { name: "OnJournalState", sig: STATE, flags: CB, hook: on_state },
        Method { name: "OnJournalDelay", sig: DELAY, flags: CB, hook: on_delay },
    ],
}];
