//! What CET's dynamic calls do, for the lobby's work with the game's own systems: a method called with numbers
//! converted to its parameters' own types (Int32, Uint32, Float, an enum...), any result read by its type (a number, a
//! Bool, a CName, a String, a strong or weak handle), and values read through the RTTI -- an object's or a struct's
//! field, an array's length and elements -- as `o.info.definitions[i].localizedName` reads them in Lua.
use crate::script_call::{find_function, invoke_owned, Owned, Value};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::{Handle, WeakHandle};
use red4ext_abi::rtti::{Class, IType};
use std::ffi::c_void;

const KIND_CLASS: u8 = 2;
const KIND_ARRAY: u8 = 3;
const KIND_ENUM: u8 = 5;
const KIND_HANDLE: u8 = 9;
const KIND_WEAK: u8 = 10;

#[derive(Clone, Copy)]
pub(super) enum A<'a> {
    N(f64),
    B(bool),
    H(&'a Handle),
    Name(u64),
    P(*const c_void),
    /// The parameter's own default value (an empty array, a zeroed struct).
    D,
}
fn type_name(ty: &IType) -> String {
    ty.name().text().to_string()
}
// The types told apart by their names, compared as hashes: a String of the name per parameter per call (and the
// game's name pool behind it) cost the game thread more than the calls themselves.
const T_BOOL: CName = CName::hashed("Bool");
const T_FLOAT: CName = CName::hashed("Float");
const T_DOUBLE: CName = CName::hashed("Double");
const T_INT8: CName = CName::hashed("Int8");
const T_UINT8: CName = CName::hashed("Uint8");
const T_INT16: CName = CName::hashed("Int16");
const T_UINT16: CName = CName::hashed("Uint16");
const T_INT32: CName = CName::hashed("Int32");
const T_UINT32: CName = CName::hashed("Uint32");
const T_INT64: CName = CName::hashed("Int64");
const T_UINT64: CName = CName::hashed("Uint64");
const T_CNAME: CName = CName::hashed("CName");
const T_STRING: CName = CName::hashed("String");
const T_TWEAKDBID: CName = CName::hashed("TweakDBID");
/// A number as the parameter's own type wants it: a Float as f32, a Double through a box kept in `doubles`.
fn number_value(v: f64, ty: Option<&IType>, doubles: &mut Vec<Box<f64>>) -> Value {
    match ty.map(IType::name) {
        Some(n) if n == T_FLOAT => Value::Float(v as f32),
        Some(n) if n == T_DOUBLE => {
            doubles.push(Box::new(v));
            Value::Ptr((&**doubles.last().unwrap_or(&Box::new(0.0)) as *const f64).cast())
        }
        _ => Value::U64(v as i64 as u64),
    }
}
/// obj:name(args...) for the overload whose leading parameters have the named types.
pub(super) fn call_typed(obj: &Handle, name: &str, types: &[&str], args: &[A]) -> Option<Owned> {
    if obj.is_null() {
        return None;
    }
    breadcrumb(|| format!(".{name}/{}", args.len()));
    // SAFETY: the object, held; its class chain.
    let f = unsafe { crate::script_call::find_function_typed(obj.instance, name, args.len(), types) }?;
    run(obj.instance, f, args, &mut [])
}
/// obj:name(args...) with each number made the parameter's own type; its result kept.
pub(super) fn call(obj: &Handle, name: &str, args: &[A]) -> Option<Owned> {
    if obj.is_null() {
        return None;
    }
    breadcrumb(|| format!(".{name}/{}", args.len()));
    // SAFETY: the object, held; its class chain.
    if let Some(f) = pick_method(obj, name, args).or_else(|| unsafe { find_function(obj.instance, name, args.len()) }) {
        return run(obj.instance, f, args, &mut []);
    }
    // CET's call of a static from an instance: Class.name(obj, args...) (script_call::static_for)
    // SAFETY: as above.
    let Some(f) = (unsafe { crate::script_call::static_for(obj.instance, name, args.len()) }) else {
        // SAFETY: the object, held.
        let class = unsafe { red4ext_abi::rtti::object_class(obj.instance) }.map_or(CName(0), |c| c.class_name());
        crate::audit::once(crate::audit::key(&[class.0, CName::hashed(name).0, args.len() as u64, 0xC1]), || {
            format!("method {}.{name} with {} arguments: not found (from {})", class.text(), args.len(), crate::audit::callers())
        });
        return None;
    };
    let mut with = Vec::with_capacity(args.len() + 1);
    with.push(A::H(obj));
    with.extend(args.iter().map(|a| match a {
        A::N(x) => A::N(*x),
        A::B(x) => A::B(*x),
        A::H(h) => A::H(h),
        A::Name(n) => A::Name(*n),
        A::P(p) => A::P(*p),
        A::D => A::D,
    }));
    run(core::ptr::null_mut(), f, &with, &mut [])
}
/// Whether an argument can stand for a parameter of this type (the overload a CET call would take: a number for a
/// number or an enum, a handle for a handle, a pointer for a struct, a String, an array or a script_ref).
fn fits(a: &A, ty: &IType) -> bool {
    let (k, n) = (ty.kind(), ty.name());
    match a {
        A::N(_) => (k == 1 && n != T_BOOL) || k == 5 || k == 13,
        A::B(_) => k == 1 && n == T_BOOL,
        A::H(_) => k == KIND_HANDLE || k == KIND_WEAK,
        A::Name(_) => k == 0 || k == 4 || n == T_UINT64 || n == T_CNAME || n == T_TWEAKDBID,
        // a value in place is a struct or an id, never a number: ApplyStatusEffect(.., Float | EntityID | TweakDBID)
        // took an EntityID's bytes as its Float overload's duration when that one came first
        A::P(_) => !(k == KIND_HANDLE || k == KIND_WEAK || k == 5 || n == T_BOOL || n == T_FLOAT || n == T_DOUBLE),
        A::D => true,
    }
}
fn compatible(f: &red4ext_abi::function::BaseFunction, args: &[A]) -> bool {
    let params = f.params.as_slice();
    if params.len() < args.len() {
        return false;
    }
    // SAFETY: the function's own parameters and their types.
    let rest_optional = params[args.len()..].iter().all(|p| unsafe { p.as_ref() }.is_some_and(|p| p.flags & red4ext_abi::function::property_flags::IS_OPTIONAL != 0));
    rest_optional && args.iter().zip(params).all(|(a, p)| unsafe { p.as_ref().and_then(|p| p.ty.as_ref()) }.is_some_and(|ty| fits(a, ty)))
}
/// The method of the object's class chain named so whose parameters the arguments fit (overloads told apart).
fn pick_method(obj: &Handle, name: &str, args: &[A]) -> Option<&'static red4ext_abi::function::BaseFunction> {
    let short = CName::hashed(name);
    // SAFETY: the object, held.
    let mut class = unsafe { red4ext_abi::rtti::object_class(obj.instance) };
    while let Some(c) = class {
        for &f in c.funcs().iter().chain(c.static_funcs().iter()) {
            // SAFETY: the class's function pointers are registered functions or null.
            let Some(f) = (unsafe { f.as_ref() }) else { continue };
            if f.short_name == short && compatible(f, args) {
                return Some(f);
            }
        }
        class = c.parent();
    }
    None
}
/// A static of the class whose parameters the arguments fit: the class's own, else a scripted static "Class::name;...".
fn pick_static(class: &str, name: &str, args: &[A]) -> Option<&'static red4ext_abi::function::BaseFunction> {
    let short = CName::hashed(name);
    if let Some(c) = crate::script_call::class_any(class) {
        for &f in c.static_funcs().iter().chain(c.funcs().iter()) {
            // SAFETY: as above.
            let Some(f) = (unsafe { f.as_ref() }) else { continue };
            let is_static = f.flags & (red4ext_abi::function::function_flags::IS_STATIC | red4ext_abi::function::function_flags::IS_NATIVE) != 0;
            if is_static && f.short_name == short && compatible(f, args) {
                return Some(f);
            }
        }
    }
    // by the class's script name, else its native one (the index of the global functions: no walk of the registry)
    crate::script_call::global_by_head(&[class, "::", name], |f| compatible(f, args)).or_else(|| {
        crate::script_call::class_any(class)
            .and_then(|c| crate::script_call::global_by_head(&[c.class_name().text(), "::", name], |f| compatible(f, args)))
    })
}
/// obj:name(args..., out o1, out o2...): the result and the out parameters' values (CET's extra returns).
pub(super) fn call_outs(obj: &Handle, name: &str, args: &[A], n_out: usize) -> Option<(Option<Owned>, Vec<Owned>)> {
    if obj.is_null() {
        return None;
    }
    breadcrumb(|| format!(".{name}/{}", args.len()));
    // SAFETY: the object, held; its class chain.
    let f = unsafe { find_function(obj.instance, name, args.len() + n_out) }?;
    let mut outs: Vec<Owned> = Vec::new();
    for i in args.len()..args.len() + n_out {
        // SAFETY: the function's own parameter types.
        let ty: &'static IType = unsafe { &*(**f.params.as_slice().get(i)?).ty };
        outs.push(Owned::of_type(ty)?);
    }
    let r = run(obj.instance, f, args, &mut outs);
    Some((r, outs))
}
/// Class.name(args...) for a static (scripted or native) function.
/// While the menu's test tour runs: the first call of each static or global, written out at once (the last line
/// before a crash names the culprit).
/// Every call written out (CVR_MENU_TOUR_CALLS=1), from the tour's opening of the menu on.
pub(super) static ALL_CALLS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The text made only while the tour runs (CVR_MENU_TOUR, read once): every call of every mechanic used to format it
/// and read the environment block, under the process's lock, for nothing.
pub(super) fn breadcrumb(what: impl FnOnce() -> String) {
    static TOUR: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*TOUR.get_or_init(|| std::env::var_os("CVR_MENU_TOUR").is_some()) {
        return;
    }
    let what = what();
    let what = what.as_str();
    if ALL_CALLS.load(std::sync::atomic::Ordering::Relaxed) {
        cvr_host::log!("[rtti] call {what}\n");
        cvr_host::log::flush();
        return;
    }
    static SEEN: std::sync::Mutex<Option<std::collections::HashSet<String>>> = std::sync::Mutex::new(None);
    let mut g = SEEN.lock().unwrap_or_else(|p| p.into_inner());
    if g.get_or_insert_with(Default::default).insert(what.to_string()) {
        drop(g);
        cvr_host::log!("[rtti] first {what}\n");
        cvr_host::log::flush();
    }
}
/// The static a call of Class.name with these argument kinds runs (pick_static, else the class's static by name,
/// else the native class's function), found once: pick_static walks every global function of the game, its name
/// read from the game's pool, when the class has no fitting static of its own -- the mechanics call statics every
/// tick. A miss is looked up again after two seconds (the first ticks run while the scripts still register).
fn resolve_static(class: &str, name: &str, args: &[A]) -> Option<&'static red4ext_abi::function::BaseFunction> {
    use std::collections::HashMap;
    use std::time::{Duration, Instant};
    static CACHE: std::sync::Mutex<Option<HashMap<(u64, u64, u64), (usize, Option<Instant>)>>> = std::sync::Mutex::new(None);
    let mut kinds = args.len() as u64;
    for a in args {
        kinds = kinds.wrapping_mul(8).wrapping_add(match a {
            A::N(_) => 1,
            A::B(_) => 2,
            A::H(_) => 3,
            A::Name(_) => 4,
            A::P(_) => 5,
            A::D => 6,
        });
    }
    let key = (CName::hashed(class).0, CName::hashed(name).0, kinds);
    {
        let mut g = CACHE.lock().unwrap_or_else(|p| p.into_inner());
        match g.get_or_insert_with(HashMap::new).get(&key) {
            // SAFETY: a registered function, alive for the process.
            Some(&(f, None)) => return unsafe { (f as *const red4ext_abi::function::BaseFunction).as_ref() },
            Some(&(_, Some(at))) if at.elapsed() < Duration::from_secs(2) => return None,
            _ => {}
        }
    }
    let found = pick_static(class, name, args)
        .or_else(|| crate::script_call::static_function(class, name))
        .or_else(|| crate::script_call::native_class_function(class, name));
    // A scripted method of an instance run without its object would read its `this` (the dummy entity) as its own
    // class: refused, for good (0 with no retry time). A native one (TweakDBInterface's records, TDBID.IsValid)
    // ignores the context it is given.
    use red4ext_abi::function::function_flags::{IS_NATIVE, IS_STATIC};
    let refused = found.is_some_and(|f| f.flags & (IS_STATIC | IS_NATIVE) == 0);
    if refused {
        cvr_host::log!("[rtti] {class}.{name}/{} is not a static: not called without its object (from Rust)\n", args.len());
    }
    let f = found.filter(|_| !refused);
    let entry = match (f, refused) {
        (Some(f), _) => (f as *const red4ext_abi::function::BaseFunction as usize, None),
        (None, true) => (0, None),
        (None, false) => {
            crate::audit::once(crate::audit::key(&[CName::hashed(class).0, CName::hashed(name).0, args.len() as u64, 0xC2]), || {
                format!("static {class}.{name} with {} arguments: not found (from {})", args.len(), crate::audit::callers())
            });
            (0, Some(Instant::now()))
        }
    };
    CACHE.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashMap::new).insert(key, entry);
    f
}
pub(super) fn static_call(class: &str, name: &str, args: &[A]) -> Option<Owned> {
    breadcrumb(|| format!("{class}.{name}/{}", args.len()));
    let f = resolve_static(class, name, args)?;
    run(core::ptr::null_mut(), f, args, &mut [])
}
/// An input listener's argument (ListenerAction, ListenerActionConsumer): native STRUCTS passed in place, so an
/// OnAction's argument `i` is its storage -- read as a handle it is no object (null from a hooked game function, a
/// reference count taken at one of the struct's words in a function the port defines).
pub(super) fn listener_arg(call: &crate::script_hooks::Call, i: usize) -> *const c_void {
    call.arg(i).map_or(core::ptr::null(), |(p, _)| p as *const c_void)
}
/// A static of ListenerAction (`consumer` false) or ListenerActionConsumer on the struct at `value`, by the native
/// name first, then the script name.
pub(super) fn listener_static(consumer: bool, name: &str, value: *const c_void) -> Option<Owned> {
    if value.is_null() {
        return None;
    }
    let (native, script) = if consumer {
        ("gameinputScriptListenerActionConsumer", "ListenerActionConsumer")
    } else {
        ("gameinputScriptListenerAction", "ListenerAction")
    };
    static_call(native, name, &[A::P(value)]).or_else(|| static_call(script, name, &[A::P(value)]))
}
/// ListenerAction.GetName of the struct at `value` (0 when it cannot be read).
pub(super) fn listener_name(value: *const c_void) -> u64 {
    // SAFETY: GetName's CName result.
    listener_static(false, "GetName", value).map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) })
}
pub(super) fn static_obj(class: &str, name: &str, args: &[A]) -> Handle {
    static_call(class, name, args).map(|o| Val::of(&o).handle()).unwrap_or_else(Handle::null)
}
pub(super) fn static_num(class: &str, name: &str, args: &[A]) -> Option<f64> {
    static_call(class, name, args).and_then(|o| Val::of(&o).num())
}
/// The value type a script_ref<T> parameter refers to.
fn script_ref_inner(ty: &IType) -> Option<*const IType> {
    let name = type_name(ty);
    let inner = name.strip_prefix("script_ref:")?;
    let t = red4ext_abi::rtti::RttiSystem::get()?.get_type(CName::hashed(inner))?;
    Some(t as *const IType)
}
/// A call about to run with arguments its parameters do not take (a handle for a struct, a number for a handle, more
/// arguments than parameters, a missing required one): the function was found by its name and count alone, and the
/// engine reads each value as its parameter's type -- said once per function, argument and kind of argument.
fn audit_args(f: &'static red4ext_abi::function::BaseFunction, args: &[A], n_out: usize) {
    // the out parameters after the arguments are given their own values (call_outs)
    let all = f.params.as_slice();
    let params = &all[..all.len().saturating_sub(n_out).max(args.len().min(all.len()))];
    let fname = |f: &red4ext_abi::function::BaseFunction| f.full_name.text().to_string();
    let site = f as *const red4ext_abi::function::BaseFunction as u64;
    if args.len() > params.len() {
        crate::audit::once(crate::audit::key(&[site, 0xA0, args.len() as u64]), || {
            format!("call {}: {} arguments for {} parameters (from {})", fname(f), args.len(), params.len(), crate::audit::callers())
        });
    }
    for (i, p) in params.iter().enumerate() {
        // SAFETY: the function's own parameters and their types.
        let Some(p) = (unsafe { p.as_ref() }) else { continue };
        let Some(ty) = (unsafe { p.ty.as_ref() }) else { continue };
        match args.get(i) {
            Some(a) if !fits(a, ty) => {
                let kind = match a {
                    A::N(_) => "a number",
                    A::B(_) => "a Bool",
                    A::H(_) => "a handle",
                    A::Name(_) => "a CName/8-byte value",
                    A::P(_) => "a pointer to a value",
                    A::D => "a default",
                };
                crate::audit::once(crate::audit::key(&[site, i as u64, kind.len() as u64, kind.as_bytes()[2] as u64]), || {
                    format!("call {}: parameter {i} {} is {}, given {kind} (from {})", fname(f), p.name.text(), crate::audit::type_text(ty), crate::audit::callers())
                });
            }
            None if p.flags & red4ext_abi::function::property_flags::IS_OPTIONAL == 0 => {
                crate::audit::once(crate::audit::key(&[site, 0xB0, i as u64]), || {
                    format!("call {}: required parameter {i} {} ({}) not given (from {})", fname(f), p.name.text(), crate::audit::type_text(ty), crate::audit::callers())
                });
            }
            _ => {}
        }
    }
}
fn run(ctx: *mut c_void, f: &'static red4ext_abi::function::BaseFunction, args: &[A], outs: &mut [Owned]) -> Option<Owned> {
    let mut doubles = Vec::with_capacity(args.len());
    let mut values = Vec::with_capacity(args.len());
    // A value handed to a script_ref<T> parameter goes as RED4ext's ScriptRef: 0x10 bytes, the inner type, the pointer
    // to the value, a CName (a value in place crashes the engine, as GetLocalizedText's String did).
    let mut refs: Vec<Box<[usize; 5]>> = Vec::new();
    let mut defaults: Vec<Owned> = Vec::new();
    audit_args(f, args, outs.len());
    for (i, a) in args.iter().enumerate() {
        // SAFETY: the function's own parameter types.
        let ty = f.params.as_slice().get(i).and_then(|p| unsafe { (**p).ty.as_ref() });
        values.push(match a {
            A::N(v) => number_value(*v, ty, &mut doubles),
            A::B(b) => Value::Bool(*b),
            A::H(h) => Value::Handle(*h),
            A::Name(n) => Value::U64(*n),
            A::P(p) => match ty.and_then(script_ref_inner) {
                Some(inner) => {
                    refs.push(Box::new([0, 0, inner as usize, *p as usize, 0]));
                    Value::Ptr(refs.last().map_or(core::ptr::null(), |r| r.as_ptr().cast()))
                }
                None => Value::Ptr(*p),
            },
            A::D => match ty.and_then(Owned::of_type) {
                Some(o) => {
                    defaults.push(o);
                    Value::Ptr(defaults.last().map_or(core::ptr::null(), |o| o.as_ptr().cast()))
                }
                None => Value::Ptr(core::ptr::null()),
            },
        });
    }
    for o in outs.iter_mut() {
        values.push(Value::Out(o.as_mut_ptr().cast()));
    }
    // SAFETY: the function with values for its parameters (the context null for a static).
    unsafe { exec(ctx, f, &values) }
}
/// The call run whether or not the function has a result: one without (RegisterToCallback, RequestSavesForLoad,
/// CalculateEncumbrance, a void getter with out parameters) answers a Bool true, that it ran. invoke_owned alone never
/// ran such a function: every void call of the menu and the lobby was skipped.
///
/// # Safety
/// As invoke_owned.
unsafe fn exec(ctx: *mut c_void, f: &'static red4ext_abi::function::BaseFunction, values: &[Value]) -> Option<Owned> {
    if f.return_type.is_null() {
        // SAFETY: as the caller's.
        if !unsafe { crate::script_call::invoke_void(ctx, f, values) } {
            return None;
        }
        let bool_ty = red4ext_abi::rtti::RttiSystem::get()?.get_type(CName::hashed("Bool"))?;
        let mut ran = Owned::of_type(bool_ty)?;
        // SAFETY: a constructed Bool's first byte.
        unsafe { *ran.as_mut_ptr() = 1 };
        return Some(ran);
    }
    // SAFETY: as the caller's.
    unsafe { invoke_owned(ctx, f, values) }
}
/// Class.name(...) or obj:name(...) with its out parameters wherever they are: the arguments fill the other parameters
/// in order, each out parameter gets a default value of its type; the result and the outs (CET's extra returns).
fn run_outs(ctx: *mut c_void, f: &'static red4ext_abi::function::BaseFunction, args: &[A]) -> Option<(Option<Owned>, Vec<Owned>)> {
    let params: Vec<&red4ext_abi::function::Property> = f.params.as_slice().iter().filter_map(|p| unsafe { p.as_ref() }).collect();
    let mut ins: Vec<A> = Vec::new();
    let mut outs: Vec<Owned> = Vec::new();
    let mut order: Vec<bool> = Vec::new();
    let mut next = args.iter();
    for p in &params {
        if p.flags & red4ext_abi::function::property_flags::IS_OUT != 0 {
            // SAFETY: the parameter's own type.
            outs.push(Owned::of_type(unsafe { &*p.ty })?);
            order.push(true);
        } else {
            match next.next() {
                Some(a) => ins.push(match a {
                    A::N(v) => A::N(*v),
                    A::B(b) => A::B(*b),
                    A::H(h) => A::H(h),
                    A::Name(n) => A::Name(*n),
                    A::P(p) => A::P(*p),
                    A::D => A::D,
                }),
                None => ins.push(A::D),
            }
            order.push(false);
        }
    }
    // the in-values converted by their own parameters' types, the outs in their places
    let mut doubles = Vec::new();
    let mut refs: Vec<Box<[usize; 5]>> = Vec::new();
    let mut defaults: Vec<Owned> = Vec::new();
    let mut values = Vec::with_capacity(params.len());
    let (mut ii, mut oi) = (0usize, 0usize);
    for (k, is_out) in order.iter().enumerate() {
        // SAFETY: the parameter's own type.
        let ty: Option<&'static IType> = unsafe { params[k].ty.as_ref() };
        if *is_out {
            values.push(Value::Out(outs[oi].as_mut_ptr().cast()));
            oi += 1;
            continue;
        }
        let a = &ins[ii];
        ii += 1;
        values.push(match a {
            A::N(v) => number_value(*v, ty, &mut doubles),
            A::B(b) => Value::Bool(*b),
            A::H(h) => Value::Handle(*h),
            A::Name(n) => Value::U64(*n),
            A::P(p) => match ty.and_then(script_ref_inner) {
                Some(inner) => {
                    refs.push(Box::new([0, 0, inner as usize, *p as usize, 0]));
                    Value::Ptr(refs.last().map_or(core::ptr::null(), |r| r.as_ptr().cast()))
                }
                None => Value::Ptr(*p),
            },
            A::D => match ty.and_then(Owned::of_type) {
                Some(o) => {
                    defaults.push(o);
                    Value::Ptr(defaults.last().map_or(core::ptr::null(), |o| o.as_ptr().cast()))
                }
                None => Value::Ptr(core::ptr::null()),
            },
        });
    }
    // SAFETY: the function with a value for each of its parameters.
    let r = unsafe { exec(ctx, f, &values) };
    Some((r, outs))
}
/// obj:name(...) with `n_out` out parameters wherever they are (run_outs): the result and the outs.
pub(super) fn method_outs(obj: &Handle, name: &str, args: &[A], n_out: usize) -> Option<(Option<Owned>, Vec<Owned>)> {
    if obj.is_null() {
        return None;
    }
    breadcrumb(|| format!(".{name}/{} (outs)", args.len()));
    // SAFETY: the object, held; its class chain.
    let f = unsafe { find_function(obj.instance, name, args.len() + n_out) }?;
    run_outs(obj.instance, f, args)
}
pub(super) fn static_outs(class: &str, name: &str, args: &[A]) -> Option<(Option<Owned>, Vec<Owned>)> {
    breadcrumb(|| format!("{class}.{name}/{} (outs)", args.len()));
    let f = pick_static(class, name, args)
        .or_else(|| crate::script_call::static_function(class, name))
        .or_else(|| crate::script_call::native_class_function(class, name))?;
    run_outs(core::ptr::null_mut(), f, args)
}
/// The class's property by the name it is written with: the RTTI keeps no "m_" prefix (Class::property_named), the
/// script dump's m_sourceIndex is LoadGameMenuGameController's sourceIndex.
fn property_named(class: &'static Class, name: &str) -> Option<&'static red4ext_abi::function::Property> {
    class.property_named(name)
}
/// A value somewhere with its type.
#[derive(Clone, Copy)]
pub(super) struct Val {
    pub ptr: *mut c_void,
    pub ty: &'static IType,
}
impl Val {
    pub(super) fn of(o: &Owned) -> Val {
        Val { ptr: o.as_ptr() as *mut c_void, ty: o_ty(o) }
    }
    fn kind(&self) -> u8 {
        self.ty.kind()
    }
    /// The object a handle value points at (its own class), or the struct value itself.
    fn target(&self) -> Option<(*mut c_void, &'static Class)> {
        match self.kind() {
            KIND_CLASS => Some((self.ptr, as_class(self.ty))),
            KIND_HANDLE | KIND_WEAK => {
                let inst = if self.kind() == KIND_HANDLE {
                    // SAFETY: a handle value.
                    unsafe { (*(self.ptr as *const Handle)).instance }
                } else {
                    // SAFETY: a weak handle value.
                    unsafe { (*(self.ptr as *const WeakHandle)).alive_instance() }
                };
                if inst.is_null() {
                    return None;
                }
                // SAFETY: a live scriptable object.
                Some((inst, unsafe { red4ext_abi::rtti::object_class(inst) }?))
            }
            _ => None,
        }
    }
    /// v.name: a field of the struct or of the object the handle points at.
    pub(super) fn f(&self, name: &str) -> Option<Val> {
        let (at, class) = self.target()?;
        let p = property_named(class, name)?;
        // SAFETY: the property's own type; the value in the instance (or its value holder).
        Some(Val { ptr: unsafe { p.value_ptr::<c_void>(at) }, ty: unsafe { &*p.ty } })
    }
    pub(super) fn len(&self) -> usize {
        if self.kind() != KIND_ARRAY {
            return 0;
        }
        // SAFETY: an array value of this type.
        unsafe { self.ty.array_length(self.ptr) as usize }
    }
    /// v[i] (0-based).
    pub(super) fn at(&self, i: usize) -> Option<Val> {
        if i >= self.len() {
            return None;
        }
        // SAFETY: an element of the array, its inner type.
        unsafe { Some(Val { ptr: self.ty.array_element(self.ptr, i as u32), ty: &*self.ty.array_inner() }) }
    }
    pub(super) fn num(&self) -> Option<f64> {
        let p = self.ptr;
        // SAFETY: the value read by its own type.
        unsafe {
            Some(match self.ty.name() {
                T_FLOAT => f64::from(*(p as *const f32)),
                T_DOUBLE => *(p as *const f64),
                T_INT8 => f64::from(*(p as *const i8)),
                T_UINT8 => f64::from(*(p as *const u8)),
                T_INT16 => f64::from(*(p as *const i16)),
                T_UINT16 => f64::from(*(p as *const u16)),
                T_INT32 => f64::from(*(p as *const i32)),
                T_UINT32 => f64::from(*(p as *const u32)),
                T_INT64 => *(p as *const i64) as f64,
                T_UINT64 => *(p as *const u64) as f64,
                T_BOOL => f64::from(u8::from(*(p as *const bool))),
                _ if self.kind() == KIND_ENUM => match self.ty.size() {
                    1 => f64::from(*(p as *const i8)),
                    2 => f64::from(*(p as *const i16)),
                    8 => *(p as *const i64) as f64,
                    _ => f64::from(*(p as *const i32)),
                },
                _ => return None,
            })
        }
    }
    pub(super) fn bool(&self) -> bool {
        self.num().is_some_and(|v| v != 0.0)
    }
    /// A CName value's hash (0 for a value of another type).
    pub(super) fn name_hash(&self) -> u64 {
        if self.ty.name() == T_CNAME {
            // SAFETY: a CName value.
            unsafe { *(self.ptr as *const u64) }
        } else {
            0
        }
    }
    /// A CName, a String or a LocalizationString's text.
    pub(super) fn text(&self) -> String {
        // SAFETY: the value read by its own type.
        unsafe {
            match self.ty.name() {
                T_CNAME => CName(*(self.ptr as *const u64)).text().to_string(),
                T_STRING => (*(self.ptr as *const red4ext_abi::cstring::CString)).as_str().to_string(),
                _ => self.f("value").map(|v| v.text()).unwrap_or_default(),
            }
        }
    }
    pub(super) fn handle(&self) -> Handle {
        match self.kind() {
            // SAFETY: a handle value, cloned.
            KIND_HANDLE => unsafe { (*(self.ptr as *const Handle)).clone() },
            // SAFETY: a weak handle value, locked.
            KIND_WEAK => unsafe { (*(self.ptr as *const WeakHandle)).lock() },
            _ => Handle::null(),
        }
    }
    pub(super) fn class_name(&self) -> String {
        self.target().map(|(_, c)| c.class_name().text().to_string()).unwrap_or_default()
    }
}
fn as_class(ty: &'static IType) -> &'static Class {
    // SAFETY: an IType of the class kind is the Class itself (its base).
    unsafe { &*(ty as *const IType as *const Class) }
}
fn o_ty(o: &Owned) -> &'static IType {
    o.ty()
}
/// obj.name as a value (the property of the object's class).
pub(super) fn field(obj: &Handle, name: &str) -> Option<Val> {
    if obj.is_null() {
        return None;
    }
    // SAFETY: the object, held.
    let class = unsafe { red4ext_abi::rtti::object_class(obj.instance) }?;
    let Some(p) = property_named(class, name) else {
        crate::audit::once(crate::audit::key(&[class.class_name().0, CName::hashed(name).0, 0xF1]), || {
            format!("field {}.{name}: no such property (from {})", class.class_name().text(), crate::audit::callers())
        });
        return None;
    };
    // SAFETY: as in Val::f.
    Some(Val { ptr: unsafe { p.value_ptr::<c_void>(obj.instance) }, ty: unsafe { &*p.ty } })
}
/// A call's result as a number (Int32, Float, an enum...).
pub(super) fn num(obj: &Handle, name: &str, args: &[A]) -> Option<f64> {
    call(obj, name, args).and_then(|o| Val::of(&o).num())
}
pub(super) fn obj(obj: &Handle, name: &str, args: &[A]) -> Handle {
    call(obj, name, args).map(|o| Val::of(&o).handle()).unwrap_or_else(Handle::null)
}
/// A call returning an array of handles.
pub(super) fn objs(obj: &Handle, name: &str, args: &[A]) -> Vec<Handle> {
    let Some(o) = call(obj, name, args) else { return Vec::new() };
    let v = Val::of(&o);
    (0..v.len()).filter_map(|i| v.at(i)).map(|e| e.handle()).collect()
}
/// A method that fills an out array of handles instead of returning one (a TweakDB record's StatModifiers(out list),
/// StatModifierGroups(out list)): its elements.
pub(super) fn objs_out(obj: &Handle, name: &str) -> Vec<Handle> {
    let Some((_, outs)) = call_outs(obj, name, &[], 1) else { return Vec::new() };
    let Some(o) = outs.first() else { return Vec::new() };
    let v = Val::of(o);
    (0..v.len()).filter_map(|i| v.at(i)).map(|e| e.handle()).collect()
}
/// TweakDBInterface.GetRecord: the record object of that name.
pub(super) fn record(name: &str) -> Handle {
    let Some(f) = crate::script_call::static_function("TweakDBInterface", "GetRecord") else { return Handle::null() };
    let id = red4ext_abi::tweakdbid::from_name(name);
    // SAFETY: a static returning a handle.
    unsafe { crate::script_call::invoke_handle(core::ptr::null_mut(), f, &[Value::U64(id)]) }.unwrap_or_else(Handle::null)
}
pub(super) fn enum_value(en: &str, member: &str) -> f64 {
    crate::script_call::enum_value(en, member).map_or(0.0, |v| v as f64)
}
/// A global function by its short name (a script global such as GetMountedVehicle, or a native one), else by its full
/// name's head: the index of the global functions.
fn global_fn(name: &str) -> Option<&'static red4ext_abi::function::BaseFunction> {
    crate::script_call::global_by_short(name).or_else(|| crate::script_call::global_by_head(&[name], |_| true))
}
/// name(args...) for a global function.
pub(super) fn global_call(name: &str, args: &[A]) -> Option<Owned> {
    breadcrumb(|| format!("global {name}/{}", args.len()));
    let f = global_fn(name)?;
    run(core::ptr::null_mut(), f, args, &mut [])
}
pub(super) fn global_obj(name: &str, args: &[A]) -> Handle {
    global_call(name, args).map(|o| Val::of(&o).handle()).unwrap_or_else(Handle::null)
}
/// The game instance as an argument (statics that take it first).
pub(super) fn game() -> crate::script_call::GameInstance {
    crate::script_call::game_instance()
}
