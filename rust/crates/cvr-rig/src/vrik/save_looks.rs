//! VRLobbySaveLooks() -> String: the saves and the looks the player's playthroughs stand at, for the lobby's engrams
//! (formerly src/Lobby/SaveLooks.cpp, where the save layout of the look and of what V wears is written up). One record a
//! line, fields split by tabs -- every save first, newest first, then one look per playthrough (from its newest save)
//! with its app, morph, wear and outfit lines:
//!   save  <folder> <playthrough> <life path> <body gender> <level> <save time> <location> <quest> <play time> <modded>
//!   look  <playthrough> <folder> <life path> <body gender> <brain gender> <level> <save time>
//!   app   <group> <option> <appearance>      morph <group> <region> <morph>
//!   wear  <equipment area> <item TweakDBID hex> <rng seed> <unique counter>
//!   outfit <slot TweakDBID hex> <item TweakDBID hex> <rng seed> <unique counter>
use super::save_file::{self, u32_at, vlq, Package};
use core::ffi::c_void;
use red4ext_abi::cname::CName;
use red4ext_abi::cstring::CString;
use red4ext_abi::rtti::{RttiKind, RttiSystem};
use red4ext_abi::stack::StackFrame;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;

fn log(text: &str) {
    super::env::log(text);
}

#[derive(Default, Clone)]
struct Look {
    playthrough: Vec<u8>,
    folder: Vec<u8>,
    life_path: Vec<u8>,
    body_gender: Vec<u8>,
    brain_gender: Vec<u8>,
    folder_path: std::ffi::OsString,
    location: Vec<u8>,
    quest: Vec<u8>,
    level: f64,
    play_time: f64,
    time: i64,
    modded: bool,
    lines: Vec<Vec<u8>>,
}

/// %USERPROFILE%\Saved Games\CD Projekt Red\Cyberpunk 2077 (the known folder, redirected or not).
fn save_root() -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_SavedGames, SHGetKnownFolderPath};
    let mut games: *mut u16 = core::ptr::null_mut();
    // SAFETY: the shell fills a string the caller frees.
    let hr = unsafe { SHGetKnownFolderPath(&FOLDERID_SavedGames, 0, core::ptr::null_mut(), &mut games) };
    let root = if hr >= 0 && !games.is_null() {
        let mut n = 0;
        // SAFETY: a NUL-terminated wide string.
        while unsafe { *games.add(n) } != 0 {
            n += 1;
        }
        let wide = unsafe { core::slice::from_raw_parts(games, n) };
        use std::os::windows::ffi::OsStringExt;
        Some(PathBuf::from(std::ffi::OsString::from_wide(wide)).join("CD Projekt Red").join("Cyberpunk 2077"))
    } else {
        None
    };
    // SAFETY: frees what the shell allocated (null is allowed).
    unsafe { CoTaskMemFree(games as *const c_void) };
    root
}
fn clean(s: &[u8]) -> Vec<u8> {
    s.iter().map(|&c| if c == b'\t' || c == b'\n' || c == b'\r' { b' ' } else { c }).collect()
}
fn join(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.push(b'\t');
        }
        out.extend_from_slice(p);
    }
    out
}

/// The look out of the node's bytes into `look.lines`; false when they do not read.
fn read_look(v: &[u8], look: &mut Look) -> bool {
    let mut p = 15usize;
    let mut narrow = false;
    let u32v = |p: &mut usize, out: &mut u32| -> bool {
        if *p + 4 > v.len() {
            return false;
        }
        *out = u32_at(v, *p);
        *p += 4;
        true
    };
    let str = |p: &mut usize, narrow: &mut bool, out: &mut Vec<u8>| -> bool {
        let len = vlq(v, p, narrow);
        out.clear();
        if *narrow {
            if *p + len > v.len() {
                return false;
            }
            out.extend_from_slice(&v[*p..*p + len]);
            *p += len;
        } else {
            if *p + 2 * len > v.len() {
                return false;
            }
            for c in 0..len {
                out.push(v[*p + 2 * c]);
            }
            *p += 2 * len;
        }
        true
    };
    let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();
    for group in 0..3 {
        let mut sections = 0u32;
        if !u32v(&mut p, &mut sections) || sections > 256 {
            return false;
        }
        for _ in 0..sections {
            let (mut name, mut a, mut b) = (Vec::new(), Vec::new(), Vec::new());
            let (mut apps, mut morphs) = (0u32, 0u32);
            if !str(&mut p, &mut narrow, &mut name) || !u32v(&mut p, &mut apps) || apps > 4096 {
                return false;
            }
            for _ in 0..apps {
                if p + 8 > v.len() {
                    return false;
                }
                p += 8;
                if !str(&mut p, &mut narrow, &mut a) || !str(&mut p, &mut narrow, &mut b) || p + 8 > v.len() {
                    return false;
                }
                p += 8;
                let key = [b"a".as_slice(), &b].concat();
                if seen.insert(key) {
                    look.lines.push(join(&[b"app", group.to_string().as_bytes(), &clean(&b), &clean(&a)]));
                }
            }
            if !u32v(&mut p, &mut morphs) || morphs > 4096 {
                return false;
            }
            for _ in 0..morphs {
                if !str(&mut p, &mut narrow, &mut a) || !str(&mut p, &mut narrow, &mut b) || p + 8 > v.len() {
                    return false;
                }
                p += 8;
                let key = [b"m".as_slice(), &a].concat();
                if seen.insert(key) {
                    look.lines.push(join(&[b"morph", group.to_string().as_bytes(), &clean(&a), &clean(&b)]));
                }
            }
        }
    }
    !look.lines.is_empty()
}

/// A gameItemID: its TweakDBID, rng seed and unique counter.
#[derive(Default, Clone, Copy)]
struct Item {
    id: u64,
    seed: u32,
    counter: u16,
}
fn item_at(pk: &Package, at: usize) -> Item {
    let props = pk.props(at);
    let mut item = Item::default();
    if let Some(p) = pk.find(&props, "id") {
        item.id = pk.u64(p.at);
    }
    if let Some(p) = pk.find(&props, "rngSeed") {
        item.seed = pk.u32(p.at);
    }
    if let Some(p) = pk.find(&props, "uniqueCounter") {
        item.counter = pk.u16(p.at);
    }
    item
}
fn hex(v: u64) -> String {
    format!("{v:016x}")
}
fn item_line(item: &Item) -> String {
    format!("{}\t{}\t{}", hex(item.id), item.seed, item.counter)
}
/// What V wears into `look.lines`.
fn read_wear(d: &[u8], look: &mut Look) -> bool {
    let mut pk = Package::new();
    pk.is_enum = Box::new(|t: &str| {
        RttiSystem::get()
            .and_then(|r| r.get_type(CName::hashed(t)))
            .is_some_and(|ty| ty.kind() == RttiKind::Enum as u8)
    });
    if !pk.open(d) {
        return false;
    }
    const CLOTHING: [&str; 9] = [
        "Head",
        "Face",
        "OuterChest",
        "InnerChest",
        "Legs",
        "Feet",
        "Outfit",
        "UnderwearTop",
        "UnderwearBottom",
    ];
    let mut found = false;
    let mut k = 0;
    while k < pk.chunks() && !found {
        if pk.chunk_type(k) != "EquipmentSystemPlayerData" {
            k += 1;
            continue;
        }
        found = true;
        let props = pk.props(pk.chunk_at(k));
        // Per area: hidden, or another item shown in its place.
        let mut visuals: HashMap<String, (bool, Item)> = HashMap::new();
        if let Some(info) = pk.find(&props, "clothingVisualsInfo") {
            for at in pk.elements(&info.ty, info.at) {
                let v = pk.props(at);
                let Some(area) = pk.find(&v, "areaType") else { continue };
                let hidden = pk.find(&v, "isHidden");
                let item = pk.find(&v, "visualItem");
                visuals.insert(
                    pk.name(pk.u16(area.at) as usize).to_string(),
                    (hidden.is_some_and(|h| pk.u8(h.at) != 0), item.map_or(Item::default(), |i| item_at(&pk, i.at))),
                );
            }
        }
        let loadout = pk.find(&props, "equipment").map_or(Vec::new(), |e| pk.props(e.at));
        let Some(areas) = pk.find(&loadout, "equipAreas") else {
            k += 1;
            continue;
        };
        for at in pk.elements(&areas.ty, areas.at) {
            let a = pk.props(at);
            let (Some(ty), Some(slots)) = (pk.find(&a, "areaType"), pk.find(&a, "equipSlots")) else {
                continue;
            };
            let area = pk.name(pk.u16(ty.at) as usize).to_string();
            if !CLOTHING.contains(&area.as_str()) {
                continue;
            }
            let mut item = Item::default();
            let elements = pk.elements(&slots.ty, slots.at);
            if let Some(&first) = elements.first() {
                if let Some(id) = pk.find(&pk.props(first), "itemID") {
                    item = item_at(&pk, id.at);
                }
            }
            if let Some((hidden, visual)) = visuals.get(&area) {
                if *hidden {
                    continue;
                }
                if visual.id != 0 {
                    item = *visual;
                }
            }
            if item.id != 0 {
                look.lines.push(format!("wear\t{area}\t{}", item_line(&item)).into_bytes());
            }
        }
        k += 1;
    }
    for k in 0..pk.chunks() {
        if pk.chunk_type(k) != "EquipmentEx.OutfitState" {
            continue;
        }
        let props = pk.props(pk.chunk_at(k));
        let active = pk.find(&props, "active");
        let parts = pk.find(&props, "parts");
        let (Some(active), Some(parts)) = (active, parts) else { break };
        if pk.u8(active.at) == 0 {
            break;
        }
        for at in pk.elements(&parts.ty, parts.at) {
            let chunk = pk.u32(at) as usize;
            if chunk >= pk.chunks() {
                continue;
            }
            let part = pk.props(pk.chunk_at(chunk));
            if let (Some(item), Some(slot)) = (pk.find(&part, "itemID"), pk.find(&part, "slotID")) {
                look.lines
                    .push(format!("outfit\t{}\t{}", hex(pk.u64(slot.at)), item_line(&item_at(&pk, item.at))).into_bytes());
            }
        }
        break;
    }
    found
}

/// jsoncpp's asString for what the metadata holds (strings; null and absent are empty).
fn json_text(v: &serde_json::Value) -> Vec<u8> {
    match v {
        serde_json::Value::String(s) => s.as_bytes().to_vec(),
        serde_json::Value::Bool(b) => {
            if *b {
                b"true".to_vec()
            } else {
                b"false".to_vec()
            }
        }
        serde_json::Value::Number(n) => n.to_string().into_bytes(),
        _ => Vec::new(),
    }
}
fn unix_time(path: &std::path::Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}
fn f0(v: f64) -> String {
    cvr_crt::format(c"%.0f", &[cvr_crt::Arg::F(v)]).to_str().unwrap_or("").to_string()
}

/// sav.dat path -> (its time, its look): a save is read again only when its file changed.
static CACHE: Mutex<BTreeMap<PathBuf, (i64, Look)>> = Mutex::new(BTreeMap::new());

fn scan() -> Vec<u8> {
    let mut cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    let Some(root) = save_root() else { return Vec::new() };
    if !root.is_dir() {
        return Vec::new();
    }
    let mut newest: BTreeMap<Vec<u8>, Look> = BTreeMap::new();
    let mut saves: Vec<Look> = Vec::new();
    let Ok(entries) = std::fs::read_dir(&root) else { return Vec::new() };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let (meta, sav) = (entry.path().join("metadata.9.json"), entry.path().join("sav.dat"));
        if !meta.exists() || !sav.exists() {
            continue;
        }
        let Ok(text) = std::fs::read(&meta) else { continue };
        let Ok(j) = serde_json::from_slice::<serde_json::Value>(&text) else {
            continue;
        };
        let m = &j["Data"]["metadata"];
        if !m.is_object() || !m["playthroughID"].is_string() {
            continue;
        }
        let folder_path = entry.file_name();
        let look = Look {
            playthrough: json_text(&m["playthroughID"]),
            folder: folder_path.to_string_lossy().into_owned().into_bytes(),
            folder_path,
            life_path: json_text(&m["lifePath"]),
            body_gender: json_text(&m["bodyGender"]),
            brain_gender: json_text(&m["brainGender"]),
            level: m["level"].as_f64().unwrap_or(0.0),
            location: json_text(&m["locationName"]),
            quest: json_text(&m["trackedQuest"]),
            play_time: m["playTime"].as_f64().unwrap_or(0.0),
            modded: m["isModded"].as_bool().unwrap_or(false),
            time: unix_time(&sav),
            lines: Vec::new(),
        };
        saves.push(look.clone());
        if newest.get(&look.playthrough).is_none_or(|n| n.time < look.time) {
            newest.insert(look.playthrough.clone(), look);
        }
    }
    let mut order: Vec<Look> = Vec::new();
    for (_, mut look) in newest {
        let sav = root.join(&look.folder_path).join("sav.dat");
        if let Some((time, cached)) = cache.get(&sav) {
            if *time == look.time {
                look.lines = cached.lines.clone();
                order.push(look);
                continue;
            }
        }
        let save = save_file::read(&sav);
        let folder = String::from_utf8_lossy(&look.folder).into_owned();
        let node = save.find("CharacetrCustomization_Appearances");
        let read = node.is_some_and(|n| {
            (n.offset as usize + n.size as usize) <= save.data.len() && read_look(&save.data[n.offset as usize..n.offset as usize + n.size as usize], &mut look)
        });
        if !read {
            log(&format!(
                "[VRLobby] save looks: {folder} does not read ({})\n",
                if save.error.is_empty() { "no look node" } else { save.error.as_str() }
            ));
            continue;
        }
        let systems = save.find("ScriptableSystemsContainer");
        let worn = systems.is_some_and(|s| {
            (s.offset as usize + s.size as usize) <= save.data.len() && read_wear(&save.data[s.offset as usize..s.offset as usize + s.size as usize], &mut look)
        });
        if !worn {
            log(&format!("[VRLobby] save looks: {folder}: what V wears does not read\n"));
        }
        cache.insert(sav, (look.time, look.clone()));
        order.push(look);
    }
    order.sort_by(|a, b| b.time.cmp(&a.time));
    saves.sort_by(|a, b| b.time.cmp(&a.time));
    let mut out = Vec::new();
    for l in &saves {
        let numbers = format!("{}\t{}", f0(l.level), l.time);
        out.extend_from_slice(&join(&[
            b"save",
            &clean(&l.folder),
            &clean(&l.playthrough),
            &clean(&l.life_path),
            &clean(&l.body_gender),
            numbers.as_bytes(),
            &clean(&l.location),
            &clean(&l.quest),
            (l.play_time as i64).to_string().as_bytes(),
            if l.modded { b"1" } else { b"0" },
        ]));
        out.push(b'\n');
    }
    for l in &order {
        out.extend_from_slice(&join(&[
            b"look",
            &clean(&l.playthrough),
            &clean(&l.folder),
            &clean(&l.life_path),
            &clean(&l.body_gender),
            &clean(&l.brain_gender),
            f0(l.level).as_bytes(),
            l.time.to_string().as_bytes(),
        ]));
        out.push(b'\n');
        for line in &l.lines {
            out.extend_from_slice(line);
            out.push(b'\n');
        }
    }
    out
}

/// VRLobbySaveLooks() -> String.
#[no_mangle]
pub unsafe extern "C" fn VRLobbySaveLooks(_this: *mut c_void, frame: *mut StackFrame, out: *mut CString, _unused: i64) {
    if let Some(frame) = frame.as_mut() {
        frame.finish_params();
    }
    let text = std::panic::catch_unwind(scan).unwrap_or_else(|_| {
        log("[VRLobby] save looks: scan failed\n");
        Vec::new()
    });
    if out.is_null() {
        return;
    }
    // The engine's string takes the bytes up to a NUL, as the C++ c_str() did.
    let end = text.iter().position(|&b| b == 0).unwrap_or(text.len());
    let text = std::ffi::CString::new(&text[..end]).unwrap_or_default();
    if let Some(s) = CString::new(&text) {
        core::ptr::write(out, s);
    }
}
