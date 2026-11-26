//! CyberpunkVRPort_Menu/modules/data.lua: what the VR menu shows of the game and does in it -- the messenger's threads
//! and their messages and replies (the journal), the callable contacts and the call, the vehicles and the summon, the
//! radio stations, the tuner and its volume, the effects volume, the pause menu's events, the map, the saves' count,
//! the main menu and the exit. Read through the game's own scripts and systems as the Lua read them.
use super::lobby_ink as ink;
use super::lobby_rtti::{self as rt, Val, A};
use super::lobby_spawn::cn;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;

fn player() -> Handle {
    super::belt::player()
}
/// loc: GetLocalizedText of the text, else the text itself (as the Lua menu's loc). Not only "LocKey#n": the game's
/// text keys are names as well ("Gameplay-RPG-Items-Types-Wea_Rifle", the weapon kinds), and those stood raw.
pub(super) fn loc(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    super::lobby_spawn::global_text("GetLocalizedText", s).filter(|t| !t.is_empty()).unwrap_or_else(|| s.to_string())
}
/// locKey: a name key's text ("" when none).
pub(super) fn loc_key_text(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    super::lobby_spawn::global_text("GetLocalizedTextByKey", name).unwrap_or_default()
}
/// locKey of a name by its hash, as CET hands the CName over: a TweakDB display name ("LocKey#n" made a CName) has no
/// text in the name pool, so by its text it was "" (the vehicles stood without names).
pub(super) fn loc_name(hash: u64) -> String {
    if hash == 0 {
        return String::new();
    }
    rt::global_call("GetLocalizedTextByKey", &[A::Name(hash)]).map(|o| Val::of(&o).text()).unwrap_or_default()
}
pub(super) fn loc_key(name: &str, fallback: &str) -> String {
    let t = loc_key_text(name);
    if t.is_empty() {
        fallback.to_string()
    } else {
        t
    }
}
/// An icon record's atlas and part.
#[derive(Clone, Debug)]
pub(super) struct Icon {
    pub atlas: u64,
    pub part: u64,
    pub name: String,
}
pub(super) fn icon_of(rec: &Handle) -> Option<Icon> {
    if rec.is_null() {
        return None;
    }
    let atlas = rt::call(rec, "AtlasResourcePath", &[]).map(|o| unsafe { *(o.as_ptr() as *const u64) })?;
    let part = rt::call(rec, "AtlasPartName", &[]).map(|o| unsafe { *(o.as_ptr() as *const u64) })?;
    Some(Icon { atlas, part, name: CName(part).text().to_string() })
}
pub(super) fn ui_icon(id: u64) -> Option<Icon> {
    icon_of(&rt::static_obj("TweakDBInterface", "GetUIIconRecord", &[A::Name(id)]))
}
pub(super) fn mounted() -> Handle {
    let pl = player();
    if pl.is_null() {
        return Handle::null();
    }
    rt::global_obj("GetMountedVehicle", &[A::H(&pl)])
}
fn qsm() -> Handle {
    let pl = player();
    rt::obj(&pl, "GetQuickSlotsManager", &[])
}
fn phone_system() -> Handle {
    let c = super::game_system("GetScriptableSystemsContainer");
    ink::child(&c, "Get", &[cn("PhoneSystem")])
}
pub(super) fn language() -> String {
    ink::language()
}
fn journal() -> Handle {
    super::game_system("GetJournalManager")
}
fn enum_of(v: Option<Val>) -> i64 {
    v.and_then(|v| v.num()).map_or(-1, |n| n as i64)
}
fn game_time_fn(name: &str, t: &Val) -> Option<f64> {
    rt::static_num("GameTime", name, &[A::P(t.ptr.cast_const())])
}
fn clock(t: Option<Val>) -> String {
    let Some(t) = t else { return String::new() };
    match (game_time_fn("Hours", &t), game_time_fn("Minutes", &t)) {
        (Some(h), Some(m)) => format!("{:02}:{:02}", (h as i64).rem_euclid(24), (m as i64).rem_euclid(60)),
        _ => String::new(),
    }
}
fn stamp(t: Option<Val>) -> f64 {
    t.and_then(|t| game_time_fn("GetSeconds", &t)).unwrap_or(0.0)
}
#[derive(Clone, Debug, Default)]
pub(super) struct Thread {
    pub name: String,
    pub preview: String,
    pub unread: usize,
    pub can_reply: bool,
    pub quest: bool,
    pub time: String,
    pub at: f64,
    pub multi: bool,
    pub hash: i64,
    pub conversation_hash: i64,
    pub callable: bool,
    pub contact_id: String,
    pub avatar: Option<Icon>,
}
fn text_of(v: &Val, f: &str) -> String {
    v.f(f).map(|x| x.text()).unwrap_or_default()
}
fn bool_of(v: &Val, f: &str) -> bool {
    v.f(f).is_some_and(|x| x.bool())
}
fn num_of(v: &Val, f: &str) -> f64 {
    v.f(f).and_then(|x| x.num()).unwrap_or(0.0)
}
fn tdbid_of(v: &Val, f: &str) -> u64 {
    // SAFETY: a TweakDBID field (8 bytes).
    v.f(f).map_or(0, |x| unsafe { *(x.ptr as *const u64) })
}
/// D.threads: the messenger's threads (of a contact when its hash is given), quest ones first, then unread, newest.
pub(super) fn threads(show_all: bool, contact_hash: Option<i64>) -> Vec<Thread> {
    let j = journal();
    if j.is_null() {
        return Vec::new();
    }
    let list = match contact_hash {
        Some(h) => rt::static_call("MessengerUtils", "GetMessageDataArrayForContact", &[A::H(&j), A::N(h as f64), A::B(true), A::B(true)]),
        None => rt::static_call("MessengerUtils", "GetSimpleContactDataArray", &[A::H(&j), A::B(true), A::B(true), A::B(show_all)]),
    };
    let Some(list) = list else { return Vec::new() };
    let arr = Val::of(&list);
    let you = loc_key("UI-Phone-LabelYou", "You");
    let multi = rt::enum_value("MessengerContactType", "MultiThread") as i64;
    let mut out = Vec::new();
    for i in 0..arr.len() {
        let Some(c) = arr.at(i) else { continue };
        let preview = if bool_of(&c, "hasValidTitle") {
            loc(&text_of(&c, "localizedPreview"))
        } else {
            let p = loc(&text_of(&c, "lastMesssagePreview"));
            if bool_of(&c, "playerIsLastSender") {
                format!("{you}: {p}")
            } else {
                p
            }
        };
        out.push(Thread {
            name: loc(&text_of(&c, "localizedName")),
            preview,
            unread: c.f("unreadMessages").map_or(0, |u| u.len()),
            can_reply: bool_of(&c, "playerCanReply"),
            quest: bool_of(&c, "questRelated") || bool_of(&c, "hasQuestImportantReply"),
            time: clock(c.f("timeStamp")),
            at: stamp(c.f("timeStamp")),
            multi: enum_of(c.f("type")) == multi,
            hash: num_of(&c, "hash") as i64,
            conversation_hash: num_of(&c, "conversationHash") as i64,
            callable: bool_of(&c, "isCallable"),
            contact_id: text_of(&c, "contactId"),
            avatar: ui_icon(tdbid_of(&c, "avatarID")),
        });
    }
    out.sort_by(|a, b| {
        if a.quest != b.quest {
            return b.quest.cmp(&a.quest);
        }
        if (a.unread > 0) != (b.unread > 0) {
            return (b.unread > 0).cmp(&(a.unread > 0));
        }
        b.at.partial_cmp(&a.at).unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}
#[derive(Clone)]
pub(super) struct Message {
    pub entry: Handle,
    pub hash: i64,
    pub text: String,
    pub quest: bool,
    pub player: bool,
    pub image: Option<Icon>,
}
#[derive(Default)]
pub(super) struct ThreadView {
    pub messages: Vec<Message>,
    pub replies: Vec<Message>,
}
fn u32h(h: i64) -> f64 {
    (h as i32 as u32) as f64
}
/// D.thread: a thread's messages and the replies on offer.
pub(super) fn thread(t: &Thread) -> ThreadView {
    let mut out = ThreadView::default();
    let j = journal();
    if j.is_null() {
        return out;
    }
    let entry = rt::obj(&j, "GetEntry", &[A::N(u32h(if t.multi { t.conversation_hash } else { t.hash }))]);
    let fname = if t.multi { "GetMessagesAndChoices" } else { "GetFlattenedMessagesAndChoices" };
    let Some((_, outs)) = rt::call_outs(&j, fname, &[A::H(&entry)], 2) else { return out };
    let player_sender = rt::enum_value("gameMessageSender", "Player") as i64;
    let read = |arr: &Val| -> Vec<Handle> { (0..arr.len()).filter_map(|i| arr.at(i)).map(|e| e.handle()).filter(|h| !h.is_null()).collect() };
    let msgs = read(&Val::of(&outs[0]));
    let choices = read(&Val::of(&outs[1]));
    for m in msgs {
        let hash = rt::num(&j, "GetEntryHash", &[A::H(&m)]).unwrap_or(0.0) as i64;
        let text = loc(&rt::call(&m, "GetText", &[]).map(|o| Val::of(&o).text()).unwrap_or_default());
        let quest = rt::num(&m, "IsQuestImportant", &[]).is_some_and(|v| v != 0.0);
        let is_phone = super::belt::class_name(&m) == "gameJournalPhoneMessage";
        let (player, image) = if is_phone {
            let p = rt::num(&m, "GetSender", &[]).map(|v| v as i64) == Some(player_sender);
            let img = rt::call(&m, "GetImageID", &[]).map(|o| unsafe { *(o.as_ptr() as *const u64) }).unwrap_or(0);
            let valid = img != 0 && rt::static_num("TDBID", "IsValid", &[A::Name(img)]).is_some_and(|v| v != 0.0);
            (p, if valid { ui_icon(img) } else { None })
        } else {
            (true, None)
        };
        out.messages.push(Message { entry: m, hash, text, quest, player, image });
    }
    for c in choices {
        let hash = rt::num(&j, "GetEntryHash", &[A::H(&c)]).unwrap_or(0.0) as i64;
        let text = loc(&rt::call(&c, "GetText", &[]).map(|o| Val::of(&o).text()).unwrap_or_default());
        let quest = rt::num(&c, "IsQuestImportant", &[]).is_some_and(|v| v != 0.0);
        out.replies.push(Message { entry: c, hash, text, quest, player: true, image: None });
    }
    out
}
pub(super) fn mark_read(messages: &[Message]) {
    let j = journal();
    for m in messages {
        if rt::num(&j, "IsEntryVisited", &[A::H(&m.entry)]).is_some_and(|v| v == 0.0) {
            rt::call(&j, "SetEntryVisited", &[A::H(&m.entry), A::B(true)]);
        }
    }
}
/// D.reply: the chosen reply succeeded, the others made inactive.
pub(super) fn reply(replies: &[Message], pick: &Message) -> bool {
    let j = journal();
    if j.is_null() {
        return false;
    }
    let inactive = rt::enum_value("gameJournalEntryState", "Inactive");
    let succeeded = rt::enum_value("gameJournalEntryState", "Succeeded");
    let notify = rt::enum_value("JournalNotifyOption", "Notify");
    for r in replies {
        if r.hash != pick.hash {
            rt::call(&j, "SetEntryVisited", &[A::H(&r.entry), A::B(true)]);
            rt::call(&j, "ChangeEntryStateByHash", &[A::N(u32h(r.hash)), A::N(inactive), A::N(notify)]);
        }
    }
    rt::call(&j, "SetEntryVisited", &[A::H(&pick.entry), A::B(true)]);
    rt::call(&j, "ChangeEntryStateByHash", &[A::N(u32h(pick.hash)), A::N(succeeded), A::N(notify)]).is_some()
}
/// D.journalWatch: the player's journal watch (PlayerPuppet.VRMenuJournalWatch).
pub(super) fn journal_watch() -> Option<super::journal_watch::Watch> {
    super::journal_watch::watch()
}
#[derive(Clone, Debug)]
pub(super) struct Contact {
    pub name: String,
    pub contact_id: String,
    pub callable: bool,
    pub messages: bool,
    pub unread: usize,
    pub hash: i64,
    pub conversation_hash: i64,
    pub avatar: Option<Icon>,
}
pub(super) fn contacts() -> Vec<Contact> {
    let j = journal();
    let Some(list) = rt::static_call("MessengerUtils", "GetCallableAndNonEmptyContacts", &[A::H(&j)]) else { return Vec::new() };
    let arr = Val::of(&list);
    let mut out = Vec::new();
    for i in 0..arr.len() {
        let Some(c) = arr.at(i) else { continue };
        let name = loc(&text_of(&c, "localizedName"));
        if name.is_empty() {
            continue;
        }
        out.push(Contact {
            name,
            contact_id: text_of(&c, "contactId"),
            callable: bool_of(&c, "isCallable"),
            messages: num_of(&c, "messagesCount") > 0.0 || num_of(&c, "repliesCount") > 0.0 || bool_of(&c, "playerCanReply") || bool_of(&c, "hasMessages"),
            unread: num_of(&c, "unreadMessegeCount") as usize,
            hash: num_of(&c, "hash") as i64,
            conversation_hash: num_of(&c, "conversationHash") as i64,
            avatar: ui_icon(tdbid_of(&c, "avatarID")),
        });
    }
    out.sort_by_key(|c| c.name.to_lowercase());
    out
}
pub(super) fn calling_enabled() -> bool {
    rt::num(&phone_system(), "IsCallingEnabled", &[]).is_some_and(|v| v != 0.0)
}
fn set_prop_u64(o: &Handle, name: &str, v: u64) {
    // SAFETY: an 8-byte property (a CName).
    if let Some(p) = unsafe { crate::script_call::property(o.instance, name) } {
        unsafe { *(p as *mut u64) = v };
    }
}
fn set_prop_i32(o: &Handle, name: &str, v: i32) {
    // SAFETY: a 4-byte property (an enum, an Int32).
    if let Some(p) = unsafe { crate::script_call::property(o.instance, name) } {
        unsafe { *(p as *mut i32) = v };
    }
}
/// D.call: a video call to the contact (questTriggerCallRequest to the PhoneSystem).
pub(super) fn call(c: &Contact) -> (bool, &'static str) {
    if !c.callable {
        return (false, "unavailable");
    }
    let ps = phone_system();
    if ps.is_null() || !calling_enabled() {
        return (false, "blocked");
    }
    let r = ink::new("questTriggerCallRequest");
    if r.is_null() {
        return (false, "failed");
    }
    set_prop_u64(&r, "addressee", CName::hashed(&c.contact_id).0);
    set_prop_u64(&r, "caller", CName::hashed("Player").0);
    set_prop_i32(&r, "callPhase", rt::enum_value("questPhoneCallPhase", "IncomingCall") as i32);
    set_prop_i32(&r, "callMode", rt::enum_value("questPhoneCallMode", "Video") as i32);
    let ok = rt::call(&ps, "QueueRequest", &[A::H(&r)]).is_some();
    (ok, if ok { "calling" } else { "failed" })
}
/// D.openMessages: the phone hub menu on the contact's thread.
pub(super) fn open_messages(hash: i64, menu_controller: &Handle) -> bool {
    let data = ink::new("MessageMenuAttachmentData");
    let evt = ink::new("StartHubMenuEvent");
    if data.is_null() || evt.is_null() {
        return false;
    }
    set_prop_i32(&data, "entryHash", hash as i32);
    rt::call(&evt, "SetStartMenu", &[A::Name(CName::hashed("phone").0), A::Name(CName::hashed("").0), A::H(&data)]);
    if !menu_controller.is_null() {
        rt::call(menu_controller, "OnRequestHubMenu", &[A::H(&evt)]).is_some()
    } else {
        rt::call(&super::game_system("GetUISystem"), "QueueEvent", &[A::H(&evt)]).is_some()
    }
}
#[derive(Clone, Debug)]
pub(super) struct Vehicle {
    pub name: String,
    pub picture: Option<Icon>,
    /// the PlayerVehicle struct's bytes (recordID, vehicleType, spawnOnlyOnValidRoad...)
    pub data: Vec<u8>,
    pub record_id: u64,
    pub vehicle_type: f64,
    pub spawn_valid_road: bool,
    pub can_be_active: bool,
    pub repair: f64,
    pub kind: &'static str,
}
pub(super) fn vehicles() -> Vec<Vehicle> {
    let pl = player();
    if pl.is_null() {
        return Vec::new();
    }
    let Some(list) = rt::static_call("VehiclesManagerDataHelper", "GetVehicles", &[A::H(&pl)]) else { return Vec::new() };
    let arr = Val::of(&list);
    let bike = rt::enum_value("gamedataVehicleType", "Bike");
    let mounted_weapons = rt::enum_value("gamedataDriverCombatType", "MountedWeapons");
    let mut out = Vec::new();
    for i in 0..arr.len() {
        let Some(d) = arr.at(i) else { continue };
        let dn = d.f("displayName");
        let mut name = loc_name(dn.as_ref().map_or(0, Val::name_hash));
        if name.is_empty() {
            name = dn.map(|v| v.text()).unwrap_or_default();
        }
        let Some(data) = d.f("data") else { continue };
        let vtype = data.f("vehicleType").and_then(|v| v.num()).unwrap_or(0.0);
        let record = tdbid_of(&data, "recordID");
        let mut kind = "car";
        if vtype == bike {
            kind = "motorcycle";
        } else {
            let rec = rt::static_obj("TweakDBInterface", "GetVehicleRecord", &[A::Name(record)]);
            let pkg = rt::obj(&rec, "VehDataPackageHandle", &[]);
            let dc = rt::obj(&pkg, "DriverCombat", &[]);
            if rt::num(&dc, "Type", &[]) == Some(mounted_weapons) {
                kind = "weaponized";
            }
        }
        let size = data.ty.size() as usize;
        // SAFETY: the struct value's bytes, copied.
        let bytes = unsafe { std::slice::from_raw_parts(data.ptr as *const u8, size) }.to_vec();
        out.push(Vehicle {
            name,
            picture: d.f("icon").map(|v| v.handle()).and_then(|h| icon_of(&h)),
            data: bytes,
            record_id: record,
            vehicle_type: vtype,
            spawn_valid_road: bool_of(&data, "spawnOnlyOnValidRoad"),
            can_be_active: bool_of(&d, "canBeActive"),
            repair: num_of(&d, "repairTimeRemaining"),
            kind,
        });
    }
    out.sort_by_key(|v| v.name.to_lowercase());
    out
}
pub(super) fn summon_restricted() -> bool {
    let g = rt::game();
    rt::static_num("VehicleSystem", "IsSummoningVehiclesRestricted", &[A::P((&g as *const crate::script_call::GameInstance).cast())]).is_some_and(|v| v != 0.0)
}
/// D.summon: the vehicle called (made active and summoned, or summoned by its record).
pub(super) fn summon(v: &Vehicle) -> (bool, &'static str) {
    if !mounted().is_null() {
        return (false, "mounted");
    }
    if summon_restricted() {
        return (false, "blocked");
    }
    if v.repair > 0.0 {
        return (false, "repair");
    }
    let m = qsm();
    if m.is_null() {
        return (false, "failed");
    }
    let ok = if v.can_be_active {
        rt::call(&m, "SetActiveVehicle", &[A::P(v.data.as_ptr().cast())]);
        rt::call(&m, "SummonActiveVehicle", &[A::B(true)]).is_some()
    } else {
        rt::call(&m, "SummonVehicle", &[A::B(true), A::N(v.vehicle_type), A::Name(v.record_id), A::B(v.spawn_valid_road)]).is_some()
    };
    (ok, if ok { "summoned" } else { "failed" })
}
const STATIONS: [&str; 14] = ["Downtempo", "AggroIndie", "GrowlFM", "Jazz", "ElectroIndie", "MinimTech", "Metal", "Pop", "Impulse", "HipHop", "AggroTechno", "Latino", "AttRock", "DarkStar"];
#[derive(Clone, Debug)]
pub(super) struct Station {
    pub name: String,
    pub index: i64,
    pub logo: Option<Icon>,
}
static STATION_CACHE: std::sync::Mutex<Option<Vec<Station>>> = std::sync::Mutex::new(None);
pub(super) fn stations() -> Vec<Station> {
    if let Some(c) = STATION_CACHE.lock().unwrap_or_else(|p| p.into_inner()).clone() {
        return c;
    }
    let mut out = Vec::new();
    for key in STATIONS {
        let rec = rt::static_obj("TweakDBInterface", "GetRadioStationRecord", &[A::Name(red4ext_abi::tweakdbid::from_name(&format!("RadioStation.{key}")))]);
        let Some(index) = rt::num(&rec, "Index", &[]) else { continue };
        if index < 0.0 {
            continue;
        }
        let name = loc(&rt::call(&rec, "DisplayName", &[]).map(|o| Val::of(&o).text()).unwrap_or_default());
        out.push(Station { name, index: index as i64, logo: icon_of(&rt::obj(&rec, "Icon", &[])) });
    }
    if !out.is_empty() {
        *STATION_CACHE.lock().unwrap_or_else(|p| p.into_inner()) = Some(out.clone());
    }
    out
}
#[derive(Clone, Debug, Default)]
pub(super) struct Radio {
    pub active: bool,
    pub station: String,
    pub track: String,
    pub index: i64,
}
/// A CName result's hash (locKey takes the name itself, as the Lua's did).
fn name_result(o: &Handle, f: &str) -> u64 {
    rt::call(o, f, &[]).map(|r| Val::of(&r).name_hash()).unwrap_or(0)
}
pub(super) fn radio() -> Radio {
    let mut st = Radio { index: -1, ..Default::default() };
    let pl = player();
    if pl.is_null() {
        return st;
    }
    let v = mounted();
    let (station, track) = if !v.is_null() {
        st.active = rt::num(&v, "IsRadioReceiverActive", &[]).is_some_and(|x| x != 0.0);
        (name_result(&v, "GetRadioReceiverStationName"), name_result(&v, "GetRadioReceiverTrackName"))
    } else {
        let r = rt::obj(&pl, "GetPocketRadio", &[]);
        st.active = rt::num(&r, "IsActive", &[]).is_some_and(|x| x != 0.0);
        (name_result(&r, "GetStationName"), name_result(&r, "GetTrackName"))
    };
    if st.active {
        st.station = loc_name(station);
        st.track = loc_name(track);
    }
    if st.active && !st.station.is_empty() {
        for s in stations() {
            if s.name == st.station {
                st.index = s.index;
            }
        }
    }
    st
}
pub(super) fn tune(index: i64) -> bool {
    rt::call(&qsm(), "SendRadioEvent", &[A::B(true), A::B(true), A::N(index as f64)]).is_some()
}
pub(super) fn radio_off() -> bool {
    rt::call(&qsm(), "SendRadioEvent", &[A::B(false), A::B(false), A::N(-1.0)]).is_some()
}
fn volume_var() -> Handle {
    ink::setting_var("/audio/volume", if mounted().is_null() { "RadioportVolume" } else { "CarRadioVolume" })
}
fn var_num(v: &Handle, f: &str) -> Option<f64> {
    rt::num(v, f, &[])
}
/// D.volume: the radio's volume and its range.
pub(super) fn volume() -> Option<(f64, f64, f64)> {
    let v = volume_var();
    if v.is_null() {
        return None;
    }
    Some((var_num(&v, "GetValue")?, var_num(&v, "GetMinValue").unwrap_or(0.0), var_num(&v, "GetMaxValue").unwrap_or(100.0)))
}
pub(super) fn effects_volume() -> f64 {
    let get = |n: &str| {
        let v = ink::setting_var("/audio/volume", n);
        var_num(&v, "GetValue").map_or(1.0, |x| x.clamp(0.0, 100.0) / 100.0)
    };
    get("MasterVolume") * get("SfxVolume")
}
pub(super) fn set_volume(n: f64) -> Option<f64> {
    let v = volume_var();
    if v.is_null() {
        return None;
    }
    let (lo, hi) = (var_num(&v, "GetMinValue")?, var_num(&v, "GetMaxValue")?);
    let n = (n + 0.5).floor().clamp(lo, hi);
    if Some(n) != var_num(&v, "GetValue") {
        rt::call(&v, "SetValue", &[A::N(n)]);
    }
    Some(n)
}
/// D.label: the game's text for a key, else the fallback.
pub(super) fn label(key: &str, fallback: &str) -> String {
    super::lobby_menu::label(key, fallback)
}
pub(super) fn saves_count() -> i64 {
    super::requests_handler().ok().and_then(|rh| rt::num(&rh, "RequestSavesCountSync", &[])).unwrap_or(0.0) as i64
}
pub(super) fn menu_event(ctrl: &Handle, name: &str) -> bool {
    if ctrl.is_null() {
        return false;
    }
    rt::call(ctrl, "SpawnMenuInstanceEvent", &[A::Name(CName::hashed(name).0)]).is_some()
}
pub(super) fn open_map(ctrl: &Handle) -> bool {
    let pl = player();
    if ctrl.is_null() || pl.is_null() {
        return false;
    }
    if rt::static_num("HubMenuUtility", "IsPlayerHardwareDisabled", &[A::H(&pl)]).is_some_and(|v| v != 0.0) {
        return false;
    }
    rt::call(ctrl, "OpenShortcutMenu", &[A::Name(CName::hashed("OpenMapMenu").0)]).is_some()
}
pub(super) fn exit_to_main_menu() -> bool {
    super::requests_handler().ok().is_some_and(|rh| rt::call(&rh, "GotoMainMenu", &[]).is_some())
}
pub(super) fn exit_game() -> bool {
    super::requests_handler().ok().is_some_and(|rh| rt::call(&rh, "ExitGame", &[]).is_some())
}
