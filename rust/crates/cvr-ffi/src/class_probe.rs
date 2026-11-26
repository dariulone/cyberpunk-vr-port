//! A development probe of script classes' RTTI objects (CVR_CLASS_PROBE): what the script loader makes of a scripted
//! class next to its parent -- the vtable, the sizes and flags, the counts of every array and map of the CClass, which
//! of the flattened property lists a child shares with its parent, and its functions. The ground truth for making a
//! scripted class from Rust (script_classes.rs).
use red4ext_abi::cname::CName;
use red4ext_abi::function::{BaseFunction, Property};
use red4ext_abi::rtti::{Class, RttiSystem};

const CLASSES: &[&str] = &[
    "LeftHandCyberwareTransition",
    "VrLauncherReadyDecisions",
    "LeftHandCyberwareQuickActionDecisions",
    "LeftHandCyberwareEventsTransition",
    "VrLauncherReadyEvents",
    "LeftHandCyberwareQuickActionEvents",
    "IScriptable",
    "VRMenuJournalWatch",
    "ScriptableService",
    "VRPortLobbyExposure",
    "inkGameController",
    "VRPhysicalScopeController",
    "VRLeftHandedWeapon",
    "QuickhacksListGameController",
];

unsafe fn rd<T: Copy>(base: usize, at: usize) -> T {
    // SAFETY: the caller's live object and one of its fields' offsets.
    unsafe { core::ptr::read_unaligned((base + at) as *const T) }
}

/// A DynArray's {entries, capacity, size}.
unsafe fn arr(base: usize, at: usize) -> (usize, u32, u32) {
    // SAFETY: the caller's.
    unsafe { (rd(base, at), rd(base, at + 8), rd(base, at + 12)) }
}

fn rva(p: usize) -> String {
    let b = cvr_engine::code::image_base();
    if p >= b && p < b + 0x8000_0000 {
        format!("rva:0x{:X}", p - b)
    } else {
        format!("0x{p:X}")
    }
}

unsafe fn prop_names(base: usize, at: usize, max: usize) -> String {
    // SAFETY: the caller's array of CProperty*.
    let (e, cap, n) = unsafe { arr(base, at) };
    if cap == 0 || e == 0 {
        return String::new();
    }
    let mut out = Vec::new();
    for i in 0..(n as usize).min(max) {
        // SAFETY: an element of the array.
        let p: usize = unsafe { rd(e, i * 8) };
        // SAFETY: a registered property.
        if let Some(p) = unsafe { (p as *const Property).as_ref() } {
            out.push(format!("{}@{:X}", p.name.text(), p.value_offset));
        }
    }
    out.join(",")
}

unsafe fn dump(class: &'static Class, parent_118: Option<usize>) -> usize {
    let c = class as *const Class as usize;
    // SAFETY: a registered class (0x2D0 bytes, RTTITypes.hpp).
    unsafe {
        let vt: usize = rd(c, 0);
        let parent = class.parent().map(|p| p.class_name().text()).unwrap_or_default();
        let flags: u32 = rd(c, 0x70);
        let lists = [0x28usize, 0x38, 0x48, 0x58, 0x118, 0x128, 0x138, 0x148, 0x1B0]
            .iter()
            .map(|&o| {
                let (e, cap, n) = arr(c, o);
                format!("{o:X}:{n}/{cap}{}", if cap == 0 { format!("(alloc {})", rva(e)) } else { String::new() })
            })
            .collect::<Vec<_>>()
            .join(" ");
        let maps = [0x78usize, 0xA8, 0xE8, 0x180]
            .iter()
            .map(|&o| {
                let n: u32 = rd(c, o + 8);
                let cap: u32 = rd(c, o + 12);
                let alloc: usize = rd(c, o + 0x28);
                format!("{o:X}:{n}/{cap}(alloc {})", rva(alloc))
            })
            .collect::<Vec<_>>()
            .join(" ");
        let defaults: u32 = rd(c, 0x158 + 12);
        let defaults_flags: u32 = rd(c, 0x158 + 0x20);
        let (e118, _, _) = arr(c, 0x118);
        cvr_host::log!(
            "[class-probe] {} @{} vt {} parent {} size 0x{:X} holder 0x{:X} flags 0x{:X} align {} | arrays {} | maps {} | D8 {:X} E0 {:X} defaults {} (flags {:X}) event {} 2C4 {} 2C9 {:X} | 118 shared with parent: {}\n",
            class.class_name().text(),
            rva(c),
            rva(vt),
            parent,
            rd::<u32>(c, 0x68),
            rd::<u32>(c, 0x6C),
            flags,
            rd::<u32>(c, 0x74),
            lists,
            maps,
            rd::<u64>(c, 0xD8),
            rd::<u64>(c, 0xE0),
            defaults,
            defaults_flags,
            rd::<i16>(c, 0x2C0),
            rd::<i32>(c, 0x2C4),
            rd::<u8>(c, 0x2C9),
            parent_118.map_or("-".to_string(), |p| (p == e118).to_string())
        );
        cvr_host::log!(
            "[class-probe]   props {} | overridden {} | 118 {} | 138 {}\n",
            prop_names(c, 0x28, 12),
            prop_names(c, 0x38, 12),
            prop_names(c, 0x118, 40),
            prop_names(c, 0x138, 12)
        );
        let funcs: Vec<String> = class
            .funcs()
            .iter()
            .filter_map(|&f| (f as *const BaseFunction).as_ref())
            .take(12)
            .map(|f| {
                let params: Vec<String> = f
                    .params
                    .as_slice()
                    .iter()
                    .filter_map(|&p| p.as_ref())
                    .map(|p| format!("{}:{}/{:X}@{:X}", p.name.text(), p.ty.as_ref().map(|t| t.name().text()).unwrap_or_default(), p.flags, p.value_offset))
                    .collect();
                let ret = f.return_type().map(|t| t.name().text()).unwrap_or_default();
                format!("{}[{:X} ac {:X}]({}) -> {} full {}", f.short_name.text(), f.flags, f.unk_ac, params.join(", "), ret, f.full_name.text())
            })
            .collect();
        cvr_host::log!("[class-probe]   funcs {}\n", funcs.join(" "));
        let slots: Vec<String> = (0..30).map(|i| rva(rd::<usize>(vt, i * 8))).collect();
        cvr_host::log!("[class-probe]   vtable {}\n", slots.join(" "));
        e118
    }
}

/// Once, when the scripts are bound (the first Running frame).
pub(crate) fn run() {
    if std::env::var_os("CVR_CLASS_PROBE").is_none() {
        return;
    }
    let Some(rtti) = RttiSystem::get() else { return };
    let mut last_118 = None;
    for name in CLASSES {
        let Some(class) = rtti.get_class(CName::hashed(name)) else {
            cvr_host::log!("[class-probe] {name}: no such class\n");
            continue;
        };
        // SAFETY: a registered class.
        let e = unsafe { dump(class, last_118) };
        last_118 = Some(e);
    }
}
