//! Shorthands for the VR menu's reads of the game, CET's chained calls (`jm:GetEntry(h):GetTitle()`) in a few letters:
//! a call's result as an object, a number, a Bool, a text, a list of objects, an 8-byte id; a static's; a struct made
//! by its type's name and its fields set; a String argument.
use super::lobby_rtti::{self as rt, Val, A};
use crate::script_call::Owned;
use red4ext_abi::handle::Handle;
use std::ffi::c_void;

pub(super) fn o(h: &Handle, name: &str, args: &[A]) -> Handle {
    rt::obj(h, name, args)
}
pub(super) fn n(h: &Handle, name: &str, args: &[A]) -> f64 {
    rt::num(h, name, args).unwrap_or(0.0)
}
pub(super) fn b(h: &Handle, name: &str, args: &[A]) -> bool {
    n(h, name, args) != 0.0
}
pub(super) fn s(h: &Handle, name: &str, args: &[A]) -> String {
    rt::call(h, name, args).map(|o| Val::of(&o).text()).unwrap_or_default()
}
pub(super) fn os(h: &Handle, name: &str, args: &[A]) -> Vec<Handle> {
    rt::objs(h, name, args)
}
/// The first eight bytes of a call's result (a TweakDBID, a CName, a ResRef).
pub(super) fn id(h: &Handle, name: &str, args: &[A]) -> u64 {
    // SAFETY: a result of at least eight bytes (an id).
    rt::call(h, name, args).map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) })
}
pub(super) fn so(class: &str, name: &str, args: &[A]) -> Handle {
    rt::static_obj(class, name, args)
}
pub(super) fn sn(class: &str, name: &str, args: &[A]) -> f64 {
    rt::static_num(class, name, args).unwrap_or(0.0)
}
pub(super) fn sb(class: &str, name: &str, args: &[A]) -> bool {
    sn(class, name, args) != 0.0
}
pub(super) fn ss(class: &str, name: &str, args: &[A]) -> String {
    rt::static_call(class, name, args).map(|o| Val::of(&o).text()).unwrap_or_default()
}
pub(super) fn jm() -> Handle {
    super::game_system("GetJournalManager")
}
pub(super) fn player() -> Handle {
    super::belt::player()
}
pub(super) fn ev(en: &str, member: &str) -> f64 {
    rt::enum_value(en, member)
}
pub(super) fn enum_name(en: &str, v: f64) -> String {
    crate::script_call::enum_name(en, v as i64).unwrap_or_default()
}
pub(super) fn tdb(name: &str) -> u64 {
    red4ext_abi::tweakdbid::from_name(name)
}
pub(super) fn cname(name: &str) -> u64 {
    red4ext_abi::cname::CName::hashed(name).0
}
/// A struct of the named type, default-constructed (JournalRequestStateFilter.new()).
pub(super) fn new_struct(name: &str) -> Option<Owned> {
    let c = crate::script_call::class_any(name)?;
    // SAFETY: a Class is an IType (its base).
    let ty: &'static red4ext_abi::rtti::IType = unsafe { &*(c as *const red4ext_abi::rtti::Class as *const red4ext_abi::rtti::IType) };
    Owned::of_type(ty)
}
/// A value's field set from a number (a Bool, an int, a float, an enum) by its own type.
pub(super) fn set(v: &Val, field: &str, x: f64) {
    if let Some(f) = v.f(field) {
        write(&f, x);
    }
}
/// An object's property set from a number by its own type (obj.field = x).
pub(super) fn hset(h: &Handle, field: &str, x: f64) {
    if let Some(f) = rt::field(h, field) {
        write(&f, x);
    }
}
/// A value written from a number by its own type.
pub(super) fn write(f: &Val, x: f64) {
    let tn = f.ty.name().text().to_string();
    // SAFETY: the field written by its own type.
    unsafe {
        match tn.as_str() {
            "Bool" => *(f.ptr as *mut bool) = x != 0.0,
            "Float" => *(f.ptr as *mut f32) = x as f32,
            "Double" => *(f.ptr as *mut f64) = x,
            "Int8" | "Uint8" => *(f.ptr as *mut u8) = x as i64 as u8,
            "Int16" | "Uint16" => *(f.ptr as *mut u16) = x as i64 as u16,
            "Int64" | "Uint64" => *(f.ptr as *mut u64) = x as i64 as u64,
            _ => match f.ty.size() {
                1 => *(f.ptr as *mut u8) = x as i64 as u8,
                2 => *(f.ptr as *mut u16) = x as i64 as u16,
                8 => *(f.ptr as *mut u64) = x as i64 as u64,
                _ => *(f.ptr as *mut u32) = x as i64 as u32,
            },
        }
    }
}
/// A handle field set (a struct's ref<> or wref<> member).
pub(super) fn set_h(v: &Val, field: &str, h: &Handle) {
    let Some(f) = v.f(field) else { return };
    // SAFETY: the property's own type assigns the value of its kind.
    unsafe {
        match f.ty.kind() {
            9 => f.ty.assign(f.ptr, (h as *const Handle).cast()),
            10 => {
                let w = red4ext_abi::handle::WeakHandle::from_handle(h);
                f.ty.assign(f.ptr, (&w as *const red4ext_abi::handle::WeakHandle).cast());
            }
            _ => {}
        }
    }
}
pub(super) fn val(o: &Owned) -> Val {
    Val::of(o)
}
pub(super) fn ptr(o: &Owned) -> *const c_void {
    o.as_ptr().cast()
}
/// A field read as a number / text / handle / list of handles.
pub(super) fn fnum(v: &Val, f: &str) -> f64 {
    v.f(f).and_then(|x| x.num()).unwrap_or(0.0)
}
pub(super) fn fbool(v: &Val, f: &str) -> bool {
    v.f(f).is_some_and(|x| x.bool())
}
pub(super) fn ftext(v: &Val, f: &str) -> String {
    v.f(f).map(|x| x.text()).unwrap_or_default()
}
pub(super) fn fid(v: &Val, f: &str) -> u64 {
    // SAFETY: an 8-byte field (an id).
    v.f(f).map_or(0, |x| unsafe { *(x.ptr as *const u64) })
}
pub(super) fn fh(v: &Val, f: &str) -> Handle {
    v.f(f).map(|x| x.handle()).unwrap_or_else(Handle::null)
}
pub(super) fn items(v: &Val) -> Vec<Val> {
    (0..v.len()).filter_map(|i| v.at(i)).collect()
}
/// An object's property as a value (obj.field).
pub(super) fn pf(h: &Handle, f: &str) -> Option<Val> {
    rt::field(h, f)
}
pub(super) fn pnum(h: &Handle, f: &str) -> f64 {
    pf(h, f).and_then(|v| v.num()).unwrap_or(0.0)
}
pub(super) fn ptext(h: &Handle, f: &str) -> String {
    pf(h, f).map(|v| v.text()).unwrap_or_default()
}
/// GetLocalizedText (a LocKey's text, else the text).
pub(super) fn loc(s: &str) -> String {
    super::menu_data::loc(s)
}
/// A record's class is (or derives from) `class`. IsA compares the engine's own class names, so a script name that is
/// an alias of a native class (WeaponObject = gameweaponObject, GameObject = gameObject) is taken by its native name
/// (asked by the alias, IsA said false for every weapon).
pub(super) fn is_a(h: &Handle, class: &str) -> bool {
    let native = red4ext_abi::rtti::RttiSystem::get()
        .and_then(|r| r.get_class_by_script_name(red4ext_abi::cname::CName::hashed(class)))
        .map_or_else(|| cname(class), |c| c.class_name().0);
    b(h, "IsA", &[A::Name(native)])
}
/// A String argument kept alive for a call: `let a = Arg::new("x"); call(.., &[a.a()])`.
pub(super) struct Arg(super::lobby_spawn::Str);
impl Arg {
    pub(super) fn new(s: &str) -> Arg {
        Arg(super::lobby_spawn::Str::new(s))
    }
    pub(super) fn a(&self) -> A<'_> {
        match self.0.v() {
            crate::script_call::Value::Ptr(p) => A::P(p),
            _ => A::P(core::ptr::null()),
        }
    }
}
/// An 8-byte value passed by pointer (a TweakDBID, a ResRef where the parameter is a struct).
pub(super) fn p64(v: &u64) -> A<'_> {
    A::P((v as *const u64).cast())
}
/// A struct value of 16 bytes kept by copy (an ItemID: its TweakDBID, seed, counter and flags).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) struct Id16(pub [u64; 2]);
impl Id16 {
    pub(super) fn of(v: &Val) -> Id16 {
        let mut out = [0u64; 2];
        let n = (v.ty.size() as usize).min(16);
        // SAFETY: the value's own bytes, at most 16.
        unsafe { core::ptr::copy_nonoverlapping(v.ptr as *const u8, out.as_mut_ptr().cast::<u8>(), n) };
        Id16(out)
    }
    pub(super) fn a(&self) -> A<'_> {
        A::P(self.0.as_ptr().cast())
    }
    pub(super) fn tdbid(&self) -> u64 {
        self.0[0]
    }
}
/// A call's struct result kept by copy.
pub(super) fn id16(h: &Handle, name: &str, args: &[A]) -> Id16 {
    rt::call(h, name, args).map(|o| Id16::of(&Val::of(&o))).unwrap_or_default()
}
/// ToVariant(value) for a small value: a Variant holding it inline (its type with the inline flag, then the value).
pub(super) fn variant_of(v: &Owned) -> Option<Owned> {
    let ty = v.ty();
    if ty.size() > 16 || ty.alignment() > 8 {
        return None;
    }
    let vty = red4ext_abi::rtti::RttiSystem::get()?.get_type(red4ext_abi::cname::CName::hashed("Variant"))?;
    let mut var = Owned::of_type(vty)?;
    let p = var.as_mut_ptr();
    // SAFETY: a constructed (empty) Variant: its type word, then the inline storage the value is built in.
    unsafe {
        *(p as *mut usize) = (ty as *const red4ext_abi::rtti::IType as usize) | 1;
        ty.construct(p.add(8).cast());
        ty.assign(p.add(8).cast(), v.as_ptr().cast());
    }
    Some(var)
}
/// FromVariant: the value a Variant holds, with its type (inline, or behind its pointer).
pub(super) fn variant_inner(v: &Owned) -> Option<Val> {
    let p = v.as_ptr() as *const usize;
    // SAFETY: a Variant: its type word (low bit: held inline), then its data or a pointer to it.
    unsafe {
        let word = *p;
        let ty = (word & !1usize) as *const red4ext_abi::rtti::IType;
        if ty.is_null() {
            return None;
        }
        let data = if word & 1 != 0 { p.add(1) as *mut c_void } else { *(p.add(1)) as *mut c_void };
        if data.is_null() {
            return None;
        }
        Some(Val { ptr: data, ty: &*ty })
    }
}
/// A static called as CET calls it: the GameInstance parameter filled in by the call wherever the function takes it
/// (RPGManager.CalculateStatModifiers(mods, game, root, targetID) takes it second), the arguments filling the others in
/// order. Filled in only when it was first, the stamina cost of every melee swing ran with the weapon's handle read as
/// the game instance, the weapon's entity id as its root object and no target id.
pub(super) fn static_gi(class: &str, name: &str, args: &[A]) -> Option<Owned> {
    let f = crate::script_call::static_function(class, name).or_else(|| crate::script_call::native_class_function(class, name))?;
    let params = f.params.as_slice();
    // SAFETY: the function's own parameters and their types.
    let at = params.iter().take(args.len() + 1).position(|p| {
        unsafe { p.as_ref().and_then(|p| p.ty.as_ref()) }.is_some_and(|t| matches!(t.name().text(), "ScriptGameInstance" | "GameInstance"))
    });
    let Some(at) = at.filter(|_| params.len() > args.len()) else {
        return rt::static_call(class, name, args);
    };
    let g = rt::game();
    let mut all: Vec<A> = Vec::with_capacity(args.len() + 1);
    for a in args.iter().take(at) {
        all.push(*a);
    }
    all.push(A::P((&g as *const crate::script_call::GameInstance).cast()));
    for a in args.iter().skip(at) {
        all.push(*a);
    }
    rt::static_call(class, name, &all)
}
