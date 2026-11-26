//! Calling the game's script functions from Rust as a CET mod's Lua called them (`obj:Method(...)`): the member function
//! found by its short name on the object's class (the parents' included), its parameters filled from typed values in
//! order, the call made as CET makes it (RTTIHelper::ExecuteFunction: bytecode, InternalExecute) under SEH, the result
//! -- when the function has one -- read as the type asked for.
use core::ffi::c_void;
use red4ext_abi::cname::CName;
use red4ext_abi::function::{property_flags, BaseFunction};
use red4ext_abi::handle::Handle;
use red4ext_abi::rtti::{object_class, RttiSystem};
use red4ext_abi::stack::StackType;

/// One argument, as the script function's parameter takes it.
#[derive(Clone, Copy, Debug)]
pub enum Value {
    Bool(bool),
    Int32(i32),
    Float(f32),
    /// An 8-byte value: a CName, a TweakDBID, an EntityID.
    U64(u64),
    /// A handle (`ref<T>`), read by the parameter's type from where it lives.
    Handle(*const Handle),
    /// Any other value in place (a struct such as a BlackboardID_Bool, the game instance).
    Ptr(*const c_void),
    /// An `out` parameter's storage (a TraceResult, a Transform): the function writes its value there, as CET hands an
    /// out parameter a placeholder and reads it back after the call.
    Out(*mut c_void),
}
impl Value {
    /// Where the parameter reads the value: in place for a pointer, else from `slot`, which takes a scalar's bytes (the
    /// parameter's type reads its own width from the start).
    fn place(self, slot: &mut u64) -> *mut c_void {
        *slot = match self {
            Value::Bool(b) => b as u64,
            Value::Int32(i) => i as u32 as u64,
            Value::Float(f) => f.to_bits() as u64,
            Value::U64(v) => v,
            Value::Handle(h) => return h as *mut c_void,
            Value::Ptr(p) => return p as *mut c_void,
            Value::Out(p) => return p,
        };
        slot as *mut u64 as *mut c_void
    }
}

/// ScriptGameInstance: the game instance as scripts pass it (`unk8` 1, `unk10` 0, as its constructor sets them).
#[repr(C)]
pub struct GameInstance {
    instance: *mut c_void,
    unk8: i8,
    unk10: i64,
}
/// The running game's instance, for the functions that take a GameInstance.
pub fn game_instance() -> GameInstance {
    GameInstance { instance: red4ext_abi::engine::game_instance(), unk8: 1, unk10: 0 }
}

/// How a call's values fill its function's parameters.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mapping {
    /// Every parameter in order, the values first, an optional tail left out (a Rust call that passes its own out
    /// storage and game instance).
    Explicit,
    /// As CET maps a Lua call's arguments (RTTIHelper::ExecuteFunction): out parameters and the game instance are not
    /// given -- an out parameter gets a default value of its type, read back after the call (CET's extra returns),
    /// the game instance is filled in -- and the values fill the other parameters in order, an optional tail left out.
    Cet,
}
type Params = [*mut red4ext_abi::function::Property];
fn param(p: *mut red4ext_abi::function::Property) -> Option<&'static red4ext_abi::function::Property> {
    // SAFETY: a function's parameter properties are registered properties or null.
    unsafe { p.as_ref() }
}
fn is_optional(p: &red4ext_abi::function::Property) -> bool {
    p.flags & property_flags::IS_OPTIONAL != 0
}
fn is_out(p: &red4ext_abi::function::Property) -> bool {
    p.flags & property_flags::IS_OUT != 0
}
fn is_game_instance(p: &red4ext_abi::function::Property) -> bool {
    // SAFETY: the property's own type.
    unsafe { p.ty.as_ref() }.is_some_and(|t| t.name() == CName::hashed("ScriptGameInstance"))
}
/// The parameters a CET call gives values to (neither out nor the game instance).
fn given(params: &Params) -> Vec<&'static red4ext_abi::function::Property> {
    params.iter().filter_map(|&p| param(p)).filter(|p| !is_out(p) && !is_game_instance(p)).collect()
}
fn mapping_of(params: &Params, count: usize) -> Option<Mapping> {
    if params.len() >= count && params[count..].iter().all(|&p| param(p).is_some_and(is_optional)) {
        return Some(Mapping::Explicit);
    }
    let given = given(params);
    (given.len() < params.len() && given.len() >= count && given[count..].iter().all(|p| is_optional(p))).then_some(Mapping::Cet)
}
/// Whether the parameters the values fill under `mapping` lead with the named types.
fn leading_types(params: &Params, mapping: Mapping, types: &[&str]) -> bool {
    let filled: Vec<&red4ext_abi::function::Property> = match mapping {
        Mapping::Explicit => params.iter().filter_map(|&p| param(p)).collect(),
        Mapping::Cet => given(params),
    };
    types.len() <= filled.len()
        // SAFETY: the parameters' own types.
        && types.iter().zip(filled).all(|(t, p)| unsafe { p.ty.as_ref() }.is_some_and(|ty| ty.name() == CName::hashed(t)))
}

/// `object`'s class's function `name` (its parents' included, the class's own first) that takes `count` values -- its
/// parameters past them optional, or (when no function takes them so) as CET maps them, its out parameters and game
/// instance filled in -- and, with `types`, whose leading parameters' types are named so: the overload a script call
/// with those arguments resolves to (SetSize(Vector2) or SetSize(Float, Float); SetTintColor(HDRColor) or
/// SetTintColor(Color)).
///
/// # Safety
/// `object` is a live scriptable object.
unsafe fn find_overload(object: *mut c_void, name: &str, count: usize, types: Option<&[&str]>) -> Option<&'static BaseFunction> {
    // WHAT A CALL RESOLVES TO IS KEPT once the registry is complete: (the object's class, the name's hash, the value
    // count, the leading types' hash or 0) -> the function or 0 for none. The walk below compared every function of
    // the class chain, and their parameters, for every call: the most frequent own-code leaf of the game thread.
    type Resolved = std::collections::HashMap<(usize, u64, usize, u64), usize>;
    static RESOLVED: std::sync::LazyLock<std::sync::RwLock<Resolved>> = std::sync::LazyLock::new(Default::default);
    // SAFETY: the caller's live object.
    let first = unsafe { object_class(object) }?;
    let types_key = types.map_or(0, |t| t.iter().fold(FNV_OFFSET, |h, ty| fnv_extend(fnv_extend(h, ty), ";")) | 1);
    let key = (first as *const red4ext_abi::rtti::Class as usize, CName::hashed(name).0, count, types_key);
    if let Some(&f) = RESOLVED.read().unwrap_or_else(|p| p.into_inner()).get(&key) {
        // SAFETY: a registered function, alive for the process (0: none).
        return unsafe { (f as *const BaseFunction).as_ref() };
    }
    // SAFETY: the caller's.
    let found = unsafe { walk_overloads(object, name, count, types) };
    if rtti_complete() {
        RESOLVED.write().unwrap_or_else(|p| p.into_inner()).insert(key, found.map_or(0, |f| f as *const BaseFunction as usize));
    }
    found
}
/// find_overload's walk of the class chain: the Explicit mapping first over the whole chain, then CET's.
///
/// # Safety
/// `object` is a live scriptable object.
unsafe fn walk_overloads(object: *mut c_void, name: &str, count: usize, types: Option<&[&str]>) -> Option<&'static BaseFunction> {
    let short = CName::hashed(name);
    for wanted in [Mapping::Explicit, Mapping::Cet] {
        // SAFETY: the caller's live object.
        let mut class = unsafe { object_class(object) };
        while let Some(c) = class {
            for &f in c.static_funcs().iter().chain(c.funcs().iter()) {
                // SAFETY: the class's function pointers are registered functions or null.
                let Some(f) = (unsafe { f.as_ref() }) else { continue };
                if f.short_name != short {
                    continue;
                }
                let params = f.params.as_slice();
                if mapping_of(params, count) == Some(wanted) && types.is_none_or(|types| leading_types(params, wanted, types)) {
                    return Some(f);
                }
            }
            class = c.parent();
        }
    }
    None
}
/// `object`'s function `name` for `count` values whose leading parameters have the named types (an overload).
///
/// # Safety
/// `object` is a live scriptable.
pub unsafe fn find_function_typed(object: *mut c_void, name: &str, count: usize, types: &[&str]) -> Option<&'static BaseFunction> {
    // SAFETY: the caller's.
    unsafe { find_overload(object, name, count, Some(types)) }
}
/// `object`'s function `name` for a call with `count` values.
///
/// # Safety
/// As [`find_overload`].
unsafe fn find(object: *mut c_void, name: &str, count: usize) -> Option<&'static BaseFunction> {
    // SAFETY: the caller's.
    unsafe { find_overload(object, name, count, None) }
}

/// CET's call of a static from an instance (src/Lua/cet/reverse/RTTIHelper.cpp: with no member of the name, the other
/// scope): `object:name(values...)` as `Class.name(object, values...)` -- a static of the object's class chain, native
/// or scripted (a global function "Class::name;..."; by the class's script name, then its native one), taking the
/// object first.
pub(crate) unsafe fn static_for(object: *mut c_void, name: &str, count: usize) -> Option<&'static BaseFunction> {
    // (class, name, count) found to have none: not looked for again (a scripted static's lookup walks every global
    // function, and a mechanic may ask for a missing method every frame)
    static MISSES: std::sync::Mutex<Option<std::collections::HashSet<(usize, u64, usize)>>> = std::sync::Mutex::new(None);
    let rtti = RttiSystem::get()?;
    // SAFETY: the caller's live object.
    let first = unsafe { object_class(object) }?;
    let key = (first as *const red4ext_abi::rtti::Class as usize, CName::hashed(name).0, count);
    if MISSES.lock().unwrap_or_else(|p| p.into_inner()).as_ref().is_some_and(|m| m.contains(&key)) {
        return None;
    }
    let found = (|| {
    let mut class = Some(first);
    while let Some(c) = class {
        let native = c.class_name();
        let script = rtti.native_to_script_name(native);
        for n in [script, native] {
            if n.0 == 0 {
                continue;
            }
            let Some(f) = static_function(&n.text(), name) else { continue };
            if f.flags & red4ext_abi::function::function_flags::IS_STATIC == 0 && !f.full_name.text().contains("::") {
                continue;
            }
            let params = f.params.as_slice();
            // SAFETY: the function's own parameters and their types.
            let first_is_object = params.first().and_then(|p| unsafe { p.as_ref() }).and_then(|p| unsafe { p.ty.as_ref() }).is_some_and(|ty| {
                let k = ty.kind();
                k == red4ext_abi::rtti::RttiKind::Handle as u8 || k == red4ext_abi::rtti::RttiKind::WeakHandle as u8
            });
            let fits = first_is_object && mapping_of(params, count + 1).is_some();
            if fits {
                return Some(f);
            }
        }
        class = c.parent();
    }
    None
    })();
    if found.is_none() {
        MISSES.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(Default::default).insert(key);
    }
    found
}
/// What `object:name(...)` runs: its member, else the static taking it first (static_for).
enum Target {
    Member(&'static BaseFunction),
    Static(&'static BaseFunction),
}
unsafe fn resolve(object: *mut c_void, name: &str, count: usize) -> Option<Target> {
    // SAFETY: the caller's live object.
    let found = unsafe { find(object, name, count).map(Target::Member).or_else(|| static_for(object, name, count).map(Target::Static)) };
    if found.is_none() {
        // SAFETY: as above.
        unsafe { audit_missing(object, name, count) };
    }
    found
}
/// A call that found no function to run, said once per class, name and count (CET says so in its log; the call did
/// nothing either way).
///
/// # Safety
/// `object` is a live scriptable object or null.
unsafe fn audit_missing(object: *mut c_void, name: &str, count: usize) {
    // SAFETY: the caller's.
    let class = if object.is_null() { None } else { unsafe { object_class(object) } };
    let class_name = class.map_or_else(|| "?".to_string(), |c| c.class_name().text().to_string());
    crate::audit::once(crate::audit::key(&[0x5343, CName::hashed(&class_name).0, CName::hashed(name).0, count as u64]), || {
        format!("script call {class_name}.{name} with {count} value(s): no such function ({})", crate::audit::callers())
    });
}
/// A call whose function did not run, said once per function and reason.
fn audit_failed(f: &BaseFunction, why: &'static str) {
    crate::audit::once(crate::audit::key(&[0x5346, f.full_name.0, CName::hashed(why).0]), || {
        format!("script call {}: {why} ({})", f.full_name.text(), crate::audit::callers())
    });
}
/// A static's values: a handle on the object first (held while the call runs), then the caller's.
fn with_object(object: *mut c_void, values: &[Value]) -> (Handle, Vec<Value>) {
    // SAFETY: the caller's live object, held by a new handle.
    let h = unsafe { Handle::from_instance(object) };
    let mut v = Vec::with_capacity(values.len() + 1);
    v.push(Value::Ptr(core::ptr::null()));
    v.extend_from_slice(values);
    (h, v)
}
/// Runs the target: a member in the object, a static with the object first in no context.
unsafe fn run_target(object: *mut c_void, t: Target, values: &[Value], result: Option<*mut c_void>) -> bool {
    // SAFETY: the caller's.
    unsafe { run_target_outs(object, t, values, result, None) }
}
/// As `run_target`, the out parameters CET's mapping filled in handed back (CET's extra returns).
unsafe fn run_target_outs(object: *mut c_void, t: Target, values: &[Value], result: Option<*mut c_void>, outs: Option<&mut Vec<Owned>>) -> bool {
    match t {
        // SAFETY: the caller's.
        Target::Member(f) => unsafe { execute_outs(object, f, values, result, outs) },
        Target::Static(f) => {
            let (h, mut v) = with_object(object, values);
            v[0] = Value::Handle(&h);
            // SAFETY: a static, its values its own.
            unsafe { execute_outs(core::ptr::null_mut(), f, &v, result, outs) }
        }
    }
}

/// Executes `f` on `object` with `values` (as many as its parameters) and, when `result` is given, the function's
/// result written there (the function must have one then, and must not when it is not given). True when it ran.
///
/// # Safety
/// `object` is a live instance of the function's class; `result` has room for the function's result type.
unsafe fn execute(object: *mut c_void, f: &BaseFunction, values: &[Value], result: Option<*mut c_void>) -> bool {
    // SAFETY: the caller's.
    unsafe { execute_in(object, f, values, result) }
}
/// As `execute`, with any context (a global function's: the instance of the system it runs in, or null).
unsafe fn execute_in(context: *mut c_void, f: &BaseFunction, values: &[Value], result: Option<*mut c_void>) -> bool {
    // SAFETY: the caller's.
    unsafe { execute_outs(context, f, values, result, None) }
}
/// As `execute_in`, the out parameters CET's mapping filled in handed back in `outs`.
unsafe fn execute_outs(context: *mut c_void, f: &BaseFunction, values: &[Value], result: Option<*mut c_void>, outs: Option<&mut Vec<Owned>>) -> bool {
    let Some(rtti) = RttiSystem::get() else { return false };
    // No function runs without a context: a static or global one gets a dummy entity, as CET's ExecuteFunction gives
    // it. A native reads its own (inkMenuScenario's statics); a scripted one hands it on to every global native it
    // calls (IsValid, GetGameInstance: the engine's executor takes the calling frame's context for them, and a null one
    // there is a read through the function's missing class -- CodexUtils.GetCodexDataArray crashed so).
    let object = if context.is_null() { dummy_context(rtti) } else { context };
    if object.is_null() {
        return false;
    }
    // SAFETY: the function's own return property.
    let return_type = unsafe { f.return_type.as_ref() }.map_or(core::ptr::null_mut(), |r| r.ty);
    if return_type.is_null() && result.is_some() {
        audit_failed(f, "a result asked of a function without one");
        return false;
    }
    let params = f.params.as_slice();
    // The parameters past the values given must be optional; they are left out of the call (CET: a Nop), so the
    // function keeps its own default for them -- inkWidget.Reparent's `opt index` is "after the last child", where an
    // explicit 0 put every widget before its siblings and drew the holo rows' glass over their labels.
    let Some(mapping) = mapping_of(params, values.len()) else {
        audit_failed(f, "more values than parameters, or a required parameter left out");
        return false;
    };
    let game = game_instance();
    let mut filled: Vec<Owned> = Vec::new();
    let mut slots: Vec<u64> = vec![0; values.len()];
    let mut args: Vec<StackType> = Vec::with_capacity(params.len());
    let mut next = 0usize;
    for (i, &p) in params.iter().enumerate() {
        let prop = param(p);
        let ty = prop.map_or(core::ptr::null_mut(), |p| p.ty);
        let value = match (mapping, prop) {
            (Mapping::Explicit, _) => match values.get(i) {
                Some(value) => value.place(&mut slots[i]),
                None => core::ptr::null_mut(),
            },
            (Mapping::Cet, Some(p)) if is_game_instance(p) => {
                if game.instance.is_null() {
                    audit_failed(f, "the game instance is not ready");
                    return false;
                }
                (&game as *const GameInstance).cast_mut().cast()
            }
            (Mapping::Cet, Some(p)) if is_out(p) => {
                // SAFETY: the parameter's own type (types live as long as the game).
                let Some(mut o) = (unsafe { ty.as_ref() }).and_then(Owned::of_type) else {
                    audit_failed(f, "no storage for an out parameter");
                    return false;
                };
                let at = o.as_mut_ptr().cast();
                filled.push(o);
                at
            }
            (Mapping::Cet, _) => {
                let v = values.get(next).map(|v| v.place(&mut slots[next]));
                next += 1;
                v.unwrap_or(core::ptr::null_mut())
            }
        };
        args.push(StackType { ty, value });
    }
    // A result nobody reads still has its storage, constructed by its type before the call and destroyed after it, as
    // CET gives every call one (a function with a result was not called at all without it).
    let mut unread = None;
    if result.is_none() && !return_type.is_null() {
        // SAFETY: the function's own return type (types live as long as the game).
        match Owned::of_type(unsafe { &*return_type }) {
            Some(o) => unread = Some(o),
            None => {
                audit_failed(f, "no storage for its result");
                return false;
            }
        }
    }
    let out = result.or_else(|| unread.as_mut().map(|o| o.as_mut_ptr().cast())).unwrap_or(core::ptr::null_mut());
    // SAFETY: the caller's context and values; the result storage made above or the caller's.
    let ran = unsafe { execute_function(f, object, &args, out, return_type) };
    if !ran {
        audit_failed(f, "did not run (InternalExecute false or a fault)");
    }
    if let Some(outs) = outs {
        outs.extend(filled);
    }
    ran
}

/// The caller frame's function of every call made here (CET's "$Lua"): some functions expect a non-empty call stack.
fn caller_function() -> *mut c_void {
    /// Skips the ParamEnd of its own (empty) parameter list, as CET's LuaDummyFunction does.
    unsafe extern "C" fn skip_param_end(_: *mut c_void, frame: *mut red4ext_abi::stack::StackFrame, _: *mut c_void, _: i64) {
        // SAFETY: the VM's frame of this function.
        unsafe { (*frame).code = (*frame).code.add(1) };
    }
    static CALLER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let made = CALLER.load(std::sync::atomic::Ordering::Acquire);
    if made != 0 {
        return made as *mut c_void;
    }
    let Some(f) = red4ext_abi::function::GlobalFunction::create(c"$CyberpunkVRPort", c"$CyberpunkVRPort", skip_param_end) else {
        return core::ptr::null_mut();
    };
    let f = f as *mut red4ext_abi::function::GlobalFunction as usize;
    match CALLER.compare_exchange(0, f, std::sync::atomic::Ordering::AcqRel, std::sync::atomic::Ordering::Acquire) {
        Ok(_) => f as *mut c_void,
        Err(first) => first as *mut c_void,
    }
}

/// CET's RTTIHelper::ExecuteFunction (src/Lua/cet/reverse/RTTIHelper.cpp), the way every call of a CET mod ran: the
/// arguments written as the call's bytecode -- Param, type and value for each; a Nop for an optional one without a
/// value, which the function then reads as its own default; ParamEnd -- in a root frame whose function is the caller
/// above, run by CBaseFunction::InternalExecute (a fault caught).
///
/// # Safety
/// `context` is the instance the function runs in; every argument with a value is a live value of its type and only an
/// optional parameter's has none; `result` is constructed storage of `result_type`, or null with no result type.
unsafe fn execute_function(f: &BaseFunction, context: *mut c_void, args: &[StackType], result: *mut c_void, result_type: *mut red4ext_abi::rtti::IType) -> bool {
    const OP_NOP: u8 = 0;
    const OP_PARAM: u8 = 27;
    const OP_PARAM_END: u8 = 38;
    const CODE_SIZE: usize = 264;
    static INTERNAL_EXECUTE: red4ext_abi::reloc::Reloc = red4ext_abi::reloc::Reloc::new(red4ext_abi::hashes::CBaseFunction_InternalExecute);
    let Some(execute) = INTERNAL_EXECUTE.get() else { return false };
    let caller = caller_function();
    if caller.is_null() {
        return false;
    }
    let mut code = [0u8; CODE_SIZE];
    let mut at = 0usize;
    for a in args {
        let size = if a.value.is_null() { 1 } else { 17 };
        if at + size >= CODE_SIZE {
            return false;
        }
        if a.value.is_null() {
            code[at] = OP_NOP;
        } else {
            code[at] = OP_PARAM;
            code[at + 1..at + 9].copy_from_slice(&(a.ty as usize).to_le_bytes());
            code[at + 9..at + 17].copy_from_slice(&(a.value as usize).to_le_bytes());
        }
        at += size;
    }
    code[at] = OP_PARAM_END;
    let mut frame = core::mem::MaybeUninit::<red4ext_abi::stack::StackFrame>::uninit();
    let fp = frame.as_mut_ptr();
    // SAFETY: the frame built in place here and not moved until it is released below; the code outlives it.
    unsafe {
        if !red4ext_abi::stack::StackFrame::init_root(fp, core::ptr::null_mut(), code.as_mut_ptr()) {
            return false;
        }
        (*fp).func = caller;
    }
    // SAFETY: CBaseFunction::InternalExecute(function, context, frame, result, result type), as CET calls it.
    let ran = unsafe {
        cvr_engine::code::guarded_call6(
            execute,
            [f as *const BaseFunction as *mut c_void, context, fp.cast(), result, result_type.cast(), core::ptr::null_mut()],
        )
    }
    .is_some_and(|r| r as usize as u8 != 0);
    // SAFETY: the frame made above, still in place.
    unsafe { red4ext_abi::stack::StackFrame::release_root(fp) };
    ran
}

/// Executes `f` for its handle result, read by the result type's RTTI kind: a `ref<>` as it is; a `wref<>` (a weak
/// reference -- dropping it as a strong one broke the counts and took the game down) locked into a strong handle,
/// the weak one released. None when the result is no handle or the call did not run.
///
/// # Safety
/// As `execute_in`.
unsafe fn execute_ref(context: *mut c_void, f: &BaseFunction, values: &[Value]) -> Option<Handle> {
    // SAFETY: the function's own return property and type.
    let kind = unsafe { f.return_type.as_ref().and_then(|r| r.ty.as_ref()) }?.kind();
    if kind == red4ext_abi::rtti::RttiKind::WeakHandle as u8 {
        let mut weak = red4ext_abi::handle::WeakHandle::null();
        // SAFETY: a null weak handle for the result, assigned by the engine.
        if !unsafe { execute_in(context, f, values, Some((&mut weak as *mut red4ext_abi::handle::WeakHandle).cast())) } {
            return None;
        }
        return Some(weak.lock());
    }
    if kind != red4ext_abi::rtti::RttiKind::Handle as u8 {
        return None;
    }
    let mut out = Handle::null();
    // SAFETY: a null handle for the result, assigned by the engine.
    unsafe { execute_in(context, f, values, Some((&mut out as *mut Handle).cast())) }.then_some(out)
}

/// The dummy context for calls without one: an entEntity instance made once and kept (CET's s_dummyContext).
fn dummy_context(rtti: &RttiSystem) -> *mut c_void {
    static DUMMY: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *DUMMY.get_or_init(|| rtti.get_class(CName::hashed("entEntity")).map_or(0, |class| class.create_instance() as usize)) as *mut c_void
}

/// `object:name(values...)` for a function without a result; false when there is no such function, it takes other
/// arguments or has a result, or the call faulted.
///
/// # Safety
/// `object` is a live scriptable object.
pub unsafe fn call(object: *mut c_void, name: &str, values: &[Value]) -> bool {
    // SAFETY: the caller's live object.
    unsafe { resolve(object, name, values.len()).is_some_and(|t| run_target(object, t, values, None)) }
}

/// `object:name(values...)` for a function returning a ref<> (a strong handle): the handle, which may be null; None
/// when the call did not run.
///
/// # Safety
/// As [`call`]; the function's result is a handle.
pub unsafe fn call_handle(object: *mut c_void, name: &str, values: &[Value]) -> Option<Handle> {
    // SAFETY: the caller's live object.
    match unsafe { resolve(object, name, values.len()) }? {
        // SAFETY: the caller's context; the result read by its own kind.
        Target::Member(f) => unsafe { execute_ref(object, f, values) },
        Target::Static(f) => {
            let (h, mut v) = with_object(object, values);
            v[0] = Value::Handle(&h);
            // SAFETY: a static in no context.
            unsafe { execute_ref(core::ptr::null_mut(), f, &v) }
        }
    }
}

/// The global function `full_name` (e.g. "GetAllBlackboardDefs", "GetPlayer;GameInstance") called in `context`, its
/// handle result; None when it is not there or did not run.
///
/// # Safety
/// `context` is null or the instance the function expects; the function's result is a handle.
pub unsafe fn call_global_handle(context: *mut c_void, full_name: &str, values: &[Value]) -> Option<Handle> {
    let f = &RttiSystem::get()?.get_function(CName::hashed(full_name))?.base;
    // SAFETY: the caller's context; the result read by its own kind.
    unsafe { execute_ref(context, f, values) }
}

/// The static function `name` of the script class `class` (e.g. StatusEffectSystem.ObjectHasStatusEffectWithTag).
pub fn static_function(class: &str, name: &str) -> Option<&'static BaseFunction> {
    let rtti = RttiSystem::get()?;
    // Only a static, as CET's `Class.name(...)` takes: a member of the same name comes first in the class's lookup
    // (AnimationControllerComponent's private native SetInputFloat(name, value) before its static
    // SetInputFloat(obj, name, value)) and called without its object it never ran.
    let found = rtti
        .get_class_by_script_name(CName::hashed(class))
        .or_else(|| rtti.get_class(CName::hashed(class)));
    let own = found.and_then(|c| class_static(c, CName::hashed(name)));
    // A scripted class's static is a global function named "Class::name;Params" (ScriptedPuppet::GetActiveWeapon,
    // WidgetUtils::LocalToGlobal, StatusEffectHelper::ApplyStatusEffect): looked up once by that name and kept.
    // Last, a native function the engine registers in the class's plain list without the static flag (TDBID's,
    // TweakDBInterface's, the natives of static script classes): called with the dummy context, as before.
    let f = own.or_else(|| scripted_static(class, name)).or_else(|| found.and_then(|c| class_native(c, CName::hashed(name))));
    if f.is_none() {
        // a static that is not there is a feature that silently does nothing: said once
        crate::audit::once(crate::audit::key(&[0x5354, CName::hashed(class).0, CName::hashed(name).0]), || {
            format!("static {class}.{name}: no such static ({})", if found.is_some() { "the class is there" } else { "no such class" })
        });
    }
    f
}
/// A class's static by name, its parents' too: whatever is in its list of statics (a native static need not carry the
/// flag: TDBID.ToStringDEBUG), else a member flagged static -- never a plain member, which comes first in the class's
/// lookup by name (AnimationControllerComponent's private native SetInputFloat(name, value) before its static
/// SetInputFloat(obj, name, value)).
fn class_static(class: &'static red4ext_abi::rtti::Class, name: CName) -> Option<&'static BaseFunction> {
    use red4ext_abi::function::function_flags::IS_STATIC;
    // SAFETY: the class's function pointers are registered functions or null.
    let of = |list: &[*mut BaseFunction], flagged: bool| {
        list.iter().filter_map(|&f| unsafe { f.as_ref() }).find(|f| f.short_name == name && (!flagged || f.flags & IS_STATIC != 0))
    };
    let mut at = Some(class);
    while let Some(c) = at {
        if let Some(f) = of(c.static_funcs(), false).or_else(|| of(c.funcs(), true)) {
            return Some(f);
        }
        at = c.parent();
    }
    None
}
/// A native function of a class (its parents' too) by name, whatever its flags.
fn class_native(class: &'static red4ext_abi::rtti::Class, name: CName) -> Option<&'static BaseFunction> {
    let mut at = Some(class);
    while let Some(c) = at {
        // SAFETY: the class's function pointers are registered functions or null.
        let f = c.funcs().iter().filter_map(|&f| unsafe { f.as_ref() }).find(|f| f.short_name == name && f.flags & red4ext_abi::function::function_flags::IS_NATIVE != 0);
        if f.is_some() {
            return f;
        }
        at = c.parent();
    }
    None
}
// ---- the RTTI's functions, indexed -----------------------------------------------------------------------------------

/// Set as the game enters its Running state: the scripts are compiled and loaded and the port's natives registered by
/// then, so the RTTI's function registry is complete -- a lookup's answer, a miss included, stays the answer, and the
/// global functions' index is made once.
static RTTI_COMPLETE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
pub(crate) fn mark_rtti_complete() {
    if !RTTI_COMPLETE.load(core::sync::atomic::Ordering::Relaxed) {
        RTTI_COMPLETE.store(true, core::sync::atomic::Ordering::Release);
    }
}
fn rtti_complete() -> bool {
    RTTI_COMPLETE.load(core::sync::atomic::Ordering::Acquire)
}
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a continued over `s`.
fn fnv_extend(mut h: u64, s: &str) -> u64 {
    for &b in s.as_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}
/// FNV-1a over `parts` as one string ("Class", "::", "name" hash as "Class::name"), without making it.
fn fnv_parts(parts: &[&str]) -> u64 {
    parts.iter().fold(FNV_OFFSET, |h, p| fnv_extend(h, p))
}
/// A registry entry: its place in the registry's order and the function.
type Entry = (u32, usize);
fn entry_function(e: Entry) -> &'static BaseFunction {
    // SAFETY: a registered function, alive for the process (a script hook swaps its contents, not its address).
    unsafe { &*(e.1 as *const BaseFunction) }
}
/// Every global function by its short name, by its full name's head ("Class::name" of "Class::name;Params") and by that
/// head's member part, each list in the registry's order (the order the walks it replaces took). GetGlobalFunctions
/// copies the whole registry and every full name is a name-pool lookup: one walk was milliseconds of the game thread.
#[derive(Default)]
struct GlobalIndex {
    by_short: std::collections::HashMap<u64, Vec<Entry>>,
    by_head: std::collections::HashMap<u64, Vec<Entry>>,
    by_member: std::collections::HashMap<u64, Vec<Entry>>,
}
impl GlobalIndex {
    fn build() -> Option<GlobalIndex> {
        let all = RttiSystem::get()?.global_functions();
        let mut ix = GlobalIndex::default();
        for (i, f) in all.iter().enumerate() {
            let e = (i as u32, *f as *const BaseFunction as usize);
            ix.by_short.entry(f.short_name.0).or_default().push(e);
            let full = f.full_name.text();
            let head = full.split(';').next().unwrap_or("");
            ix.by_head.entry(fnv_parts(&[head])).or_default().push(e);
            if let Some((_, member)) = head.rsplit_once("::") {
                ix.by_member.entry(fnv_parts(&[member])).or_default().push(e);
            }
        }
        Some(ix)
    }
}
/// `f` with the index: the kept one once the registry is complete, else one made for this lookup alone (the first
/// frames, while the game still registers its script functions).
fn with_index<R>(f: impl FnOnce(&GlobalIndex) -> R) -> Option<R> {
    static INDEX: std::sync::OnceLock<GlobalIndex> = std::sync::OnceLock::new();
    if !rtti_complete() {
        return GlobalIndex::build().map(|ix| f(&ix));
    }
    if let Some(ix) = INDEX.get() {
        return Some(f(ix));
    }
    let ix = GlobalIndex::build()?;
    Some(f(INDEX.get_or_init(|| ix)))
}
/// The first global function, in the registry's order, whose full name's head is `parts` joined ("Class::name") and
/// which `keep` accepts.
pub(crate) fn global_by_head(parts: &[&str], keep: impl Fn(&'static BaseFunction) -> bool) -> Option<&'static BaseFunction> {
    let key = fnv_parts(parts);
    with_index(|ix| ix.by_head.get(&key).and_then(|v| v.iter().map(|&e| entry_function(e)).find(|&f| keep(f)))).flatten()
}
/// The first global function, in the registry's order, whose short name is `name`.
pub(crate) fn global_by_short(name: &str) -> Option<&'static BaseFunction> {
    let key = CName::hashed(name).0;
    with_index(|ix| ix.by_short.get(&key).and_then(|v| v.first()).map(|&e| entry_function(e))).flatten()
}

/// The global function "class::name;..." (a scripted class's static): by the class as the scripts name it, else by its
/// native name (a script static added to a native class: entAnimationControllerComponent::ApplyFeatureToReplicate).
/// A miss costs a lookup in the index too: a walk of the registry for each (TDBID.ToStringDEBUG, which is not a
/// scripted static, asked for by the holsters' scan for every weapon and by the belt every frame) was 13-21 ms of the
/// game thread four times a second, the render workers idle behind it -- the world's microfreezes.
pub(crate) fn scripted_static(class: &str, name: &str) -> Option<&'static BaseFunction> {
    global_by_head(&[class, "::", name], |_| true)
        .or_else(|| class_any(class).and_then(|c| global_by_head(&[c.class_name().text(), "::", name], |_| true)))
}

/// The function `name` of the native class `class` (e.g. ScriptGameInstance.GetSystemRequestsHandler: CET's
/// `Game.X()` looks up the game instance's class this way when no global function has the short name); a name the
/// scripts use for a native class (inkWidgetRef for inkWidgetReference, TS_TargetPartInfo ...) is looked up too.
pub fn native_class_function(class: &str, name: &str) -> Option<&'static BaseFunction> {
    class_any(class)?.function(CName::hashed(name))
}
/// A class by its native name, else by the name the scripts know it by (CET resolves both).
pub fn class_any(name: &str) -> Option<&'static red4ext_abi::rtti::Class> {
    let rtti = RttiSystem::get()?;
    rtti.get_class(CName::hashed(name)).or_else(|| rtti.get_class_by_script_name(CName::hashed(name)))
}

/// The global function whose short name is `name` (CET's `Game.X()` first looks here: a GameInstance static such as
/// GetSystemRequestsHandler is a global function), else the first whose full name "Class::name;Params" has `name` for
/// its head or its member (a class static registered globally, as CET reads them); kept in `cache` once found.
pub fn global_by_short_name(name: &'static str, cache: &'static std::sync::OnceLock<usize>) -> Option<&'static BaseFunction> {
    if let Some(&found) = cache.get() {
        // SAFETY: a registered function, alive for the process.
        return unsafe { (found as *const BaseFunction).as_ref() };
    }
    let wanted = CName::hashed(name).0;
    let key = fnv_parts(&[name]);
    let found = with_index(|ix| {
        if let Some(&e) = ix.by_short.get(&wanted).and_then(|v| v.first()) {
            return Some(e);
        }
        let head = ix.by_head.get(&key).and_then(|v| v.first()).copied();
        let member = ix.by_member.get(&key).and_then(|v| v.first()).copied();
        // the earlier in the registry's order, as one walk testing both found it
        match (head, member) {
            (Some(h), Some(m)) => Some(if h.0 <= m.0 { h } else { m }),
            (h, m) => h.or(m),
        }
    })
    .flatten()?;
    let _ = cache.set(found.1);
    Some(entry_function(found))
}

/// The value of `member` in the enum `name` (CRTTISystem::GetEnum, CEnum's hashList/valueList), as EnumInt reads it.
/// The CEnum named `name` (its native name, else the name the scripts know it by: TargetingSet for gameTargetingSet).
fn enum_ptr(name: &str) -> Option<*const u8> {
    let rtti = RttiSystem::get()?;
    type GetEnum = unsafe extern "C" fn(*const c_void, CName) -> *const u8;
    // SAFETY: CRTTISystem::GetEnum (vtable +0x18) and GetEnumByScriptName (+0x110).
    unsafe {
        let this = (rtti as *const RttiSystem).cast::<c_void>();
        let mut e = red4ext_abi::rtti::vfunc::<GetEnum>(this, 0x18)(this, CName::hashed(name));
        if e.is_null() {
            e = red4ext_abi::rtti::vfunc::<GetEnum>(this, 0x110)(this, CName::hashed(name));
        }
        (!e.is_null()).then_some(e)
    }
}
pub fn enum_value(name: &str, member: &str) -> Option<i64> {
    let e = enum_ptr(name)?;
    // SAFETY: CEnum's arrays (hashList +0x28, valueList +0x38).
    unsafe {
        let names = &*(e.add(0x28) as *const red4ext_abi::dynarray::DynArray<CName>);
        let values = &*(e.add(0x38) as *const red4ext_abi::dynarray::DynArray<i64>);
        let wanted = CName::hashed(member);
        let i = names.as_slice().iter().position(|n| *n == wanted)?;
        values.as_slice().get(i).copied()
    }
}

/// The name of `value` in the enum `name` (CET's Enum value name); None when the enum or the value is unknown.
pub fn enum_name(name: &str, value: i64) -> Option<String> {
    let e = enum_ptr(name)?;
    // SAFETY: CEnum's arrays (hashList +0x28, valueList +0x38).
    unsafe {
        let names = &*(e.add(0x28) as *const red4ext_abi::dynarray::DynArray<CName>);
        let values = &*(e.add(0x38) as *const red4ext_abi::dynarray::DynArray<i64>);
        let i = values.as_slice().iter().position(|v| *v == value)?;
        names.as_slice().get(i).map(|n| n.text().to_string())
    }
}

/// A result read back from a call: its storage, and what it is as a value.
pub trait Ret: Sized {
    /// Whether the result is read at all (`()` does not: the function is called whatever it returns, as CET calls it).
    const READS: bool = true;
    /// The script type a plain number of this kind is ("Float", "Int32", "Bool"): a function returning another number
    /// type has its result read by its own type and converted (CET hands Lua the number, whatever its type).
    const NUMBER: Option<&'static str> = None;
    /// Storage for the result, of at least the type's size (64 bytes: up to a Matrix).
    fn storage() -> [u64; 8] {
        [0; 8]
    }
    fn read(storage: &[u64; 8]) -> Self;
    /// This kind from a number of another script type (only the NUMBER kinds).
    fn from_number(_: Number) -> Option<Self> {
        None
    }
}
impl Ret for () {
    const READS: bool = false;
    fn read(_: &[u64; 8]) {}
}
impl Ret for bool {
    const NUMBER: Option<&'static str> = Some("Bool");
    fn read(s: &[u64; 8]) -> bool {
        s[0] as u8 != 0
    }
    fn from_number(v: Number) -> Option<bool> {
        Some(match v {
            Number::Int(i) => i != 0,
            Number::Real(r) => r != 0.0,
        })
    }
}
impl Ret for i32 {
    const NUMBER: Option<&'static str> = Some("Int32");
    fn read(s: &[u64; 8]) -> i32 {
        s[0] as u32 as i32
    }
    fn from_number(v: Number) -> Option<i32> {
        // an integer as the low 32 bits (a Uint32 hash keeps its bits), a real truncated
        Some(match v {
            Number::Int(i) => i as i32,
            Number::Real(r) => r as i32,
        })
    }
}
impl Ret for u32 {
    const NUMBER: Option<&'static str> = Some("Uint32");
    fn read(s: &[u64; 8]) -> u32 {
        s[0] as u32
    }
    fn from_number(v: Number) -> Option<u32> {
        Some(match v {
            Number::Int(i) => i as u32,
            Number::Real(r) => r as u32,
        })
    }
}
impl Ret for f32 {
    const NUMBER: Option<&'static str> = Some("Float");
    fn read(s: &[u64; 8]) -> f32 {
        f32::from_bits(s[0] as u32)
    }
    fn from_number(v: Number) -> Option<f32> {
        Some(match v {
            Number::Int(i) => i as f32,
            Number::Real(r) => r as f32,
        })
    }
}
/// A plain number result as its own script type made it.
#[derive(Clone, Copy)]
pub enum Number {
    Int(i64),
    Real(f64),
}
/// A plain number result read by its own script type; None for a type that is no number.
fn number_of(ty: &str, s: &[u64; 8]) -> Option<Number> {
    let w = s[0];
    Some(match ty {
        "Bool" => Number::Int(i64::from(w as u8 != 0)),
        "Int8" => Number::Int(i64::from(w as u8 as i8)),
        "Uint8" => Number::Int(i64::from(w as u8)),
        "Int16" => Number::Int(i64::from(w as u16 as i16)),
        "Uint16" => Number::Int(i64::from(w as u16)),
        "Int32" => Number::Int(i64::from(w as u32 as i32)),
        "Uint32" => Number::Int(i64::from(w as u32)),
        "Int64" | "Uint64" => Number::Int(w as i64),
        "Float" => Number::Real(f64::from(f32::from_bits(w as u32))),
        "Double" => Number::Real(f64::from_bits(w)),
        _ => return None,
    })
}
impl Ret for u64 {
    fn read(s: &[u64; 8]) -> u64 {
        s[0]
    }
}
/// A 16-byte plain value (an ItemID).
impl Ret for [u64; 2] {
    fn read(s: &[u64; 8]) -> [u64; 2] {
        [s[0], s[1]]
    }
}
/// A Vector2 (x, y).
impl Ret for [f32; 2] {
    fn read(s: &[u64; 8]) -> [f32; 2] {
        [f32::from_bits(s[0] as u32), f32::from_bits((s[0] >> 32) as u32)]
    }
}
/// A Vector4 (x, y, z, w).
impl Ret for [f32; 4] {
    fn read(s: &[u64; 8]) -> [f32; 4] {
        let mut v = [0.0; 4];
        for (i, out) in v.iter_mut().enumerate() {
            *out = f32::from_bits((s[i / 2] >> ((i % 2) * 32)) as u32);
        }
        v
    }
}
/// Three Int32 (a WorldPosition's fixed-point coordinates).
impl Ret for [i32; 3] {
    fn read(s: &[u64; 8]) -> [i32; 3] {
        [s[0] as u32 as i32, (s[0] >> 32) as u32 as i32, s[1] as u32 as i32]
    }
}
/// A Matrix: its rows X, Y, Z, W, each a Vector4.
impl Ret for [[f32; 4]; 4] {
    fn read(s: &[u64; 8]) -> [[f32; 4]; 4] {
        let mut m = [[0.0; 4]; 4];
        for (i, out) in m.iter_mut().flatten().enumerate() {
            *out = f32::from_bits((s[i / 2] >> ((i % 2) * 32)) as u32);
        }
        m
    }
}

/// `f` called in `context` (an object for a member function, null for a static one) with `values` -- the optional
/// parameters after them left at their defaults -- and its result, whatever it is, read as `R` (`()` drops it). Only
/// plain results: a function returning a handle, a string or an array is refused (it would leave a value to destroy).
///
/// # Safety
/// `context` is null or a live instance of the function's class.
pub unsafe fn invoke<R: Ret>(context: *mut c_void, f: &BaseFunction, values: &[Value]) -> Option<R> {
    let mut storage = R::storage();
    // SAFETY: the function's own return property.
    let result = match unsafe { f.return_type.as_ref() } {
        None => None,
        // a result not read: execute_in gives it its own storage, constructed and destroyed by its type
        Some(_) if !R::READS => None,
        Some(r) => {
            // SAFETY: the return type's own descriptor.
            let kind = unsafe { r.ty.as_ref() }?.kind();
            // Name 0, Fundamental 1, Class 2 (a struct value), Simple 4, Enum 5, BitField 13: no destructor to run.
            if !matches!(kind, 0 | 1 | 2 | 4 | 5 | 13) {
                audit_failed(f, "its result read as a plain value");
                return None;
            }
            // SAFETY: as above.
            if unsafe { r.ty.as_ref() }?.size() as usize > core::mem::size_of_val(&storage) {
                audit_failed(f, "its result larger than the plain storage");
                return None;
            }
            Some(storage.as_mut_ptr().cast::<c_void>())
        }
    };
    // SAFETY: the caller's context; the storage takes the result.
    if !unsafe { execute_in(context, f, values, result) } {
        return None;
    }
    // a number asked for as another number type (a ConfigVarInt's GetValue read as a Float was 100 as 1.4e-45: the
    // lobby's sounds played silent): read by its own type and converted
    if let (Some(want), Some(r)) = (R::NUMBER, unsafe { f.return_type.as_ref() }) {
        // SAFETY: the return type's own descriptor.
        if let Some(ty) = unsafe { r.ty.as_ref() } {
            let name = ty.name().text();
            if name != want {
                if let Some(v) = number_of(name, &storage) {
                    audit_failed(f, "its number result read as another number type (converted)");
                    return R::from_number(v);
                }
            }
        }
    }
    Some(R::read(&storage))
}

/// `f` called in `context` with `values` for its handle result (a ref<>), which may be null; None when it did not run.
///
/// # Safety
/// As [`invoke`]; the function's result is a handle.
pub unsafe fn invoke_handle(context: *mut c_void, f: &BaseFunction, values: &[Value]) -> Option<Handle> {
    // SAFETY: the caller's context; the result read by its own kind.
    unsafe { execute_ref(context, f, values) }
}

/// A function's result of any type -- a Variant, a String, an array: what [`invoke`] refuses -- constructed before the
/// call (as CET constructs its result) and destroyed by its type when dropped.
pub struct Owned {
    ty: &'static red4ext_abi::rtti::IType,
    storage: Vec<u128>,
}
impl Owned {
    /// The value's type.
    pub fn ty(&self) -> &'static red4ext_abi::rtti::IType {
        self.ty
    }
    /// The value, as the result type lays it out.
    pub fn as_ptr(&self) -> *const u8 {
        self.storage.as_ptr().cast()
    }
    /// The value, writable (an out parameter's storage).
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.storage.as_mut_ptr().cast()
    }
    /// A default value of `ty` (its own construction), destroyed by it when dropped: a struct to fill, an array an
    /// out parameter writes.
    pub fn of_type(ty: &'static red4ext_abi::rtti::IType) -> Option<Owned> {
        if ty.alignment() > 16 {
            return None;
        }
        let mut owned = Owned { ty, storage: vec![0; (ty.size() as usize).div_ceil(16).max(1)] };
        // SAFETY: zeroed storage of the type's size and alignment.
        unsafe { ty.construct(owned.storage.as_mut_ptr().cast()) };
        Some(owned)
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the value constructed in this storage by its own type.
        unsafe { self.ty.destruct(self.storage.as_mut_ptr().cast()) }
    }
}

/// The function `name` of `object`'s class chain taking `count` values (its optional tail aside).
///
/// # Safety
/// `object` is a live scriptable.
pub unsafe fn find_function(object: *mut c_void, name: &str, count: usize) -> Option<&'static BaseFunction> {
    // SAFETY: the caller's.
    unsafe { find(object, name, count) }
}
/// `f` called in `context` with its result -- a plain value of any size, such as a WorldTransform -- written to `result`.
///
/// # Safety
/// As [`invoke`]; `result` has room for the result type, aligned as it is.
pub unsafe fn invoke_into(context: *mut c_void, f: &BaseFunction, values: &[Value], result: *mut c_void) -> bool {
    // SAFETY: the caller's.
    unsafe { execute_in(context, f, values, Some(result)) }
}
/// `f` called in `context` (null for a static) for its result of any type, kept as [`Owned`] while it is read.
///
/// # Safety
/// As [`invoke`].
pub unsafe fn invoke_owned(context: *mut c_void, f: &'static BaseFunction, values: &[Value]) -> Option<Owned> {
    // SAFETY: the function's own return property and type (types live as long as the game).
    let ty: &'static red4ext_abi::rtti::IType = unsafe { &*f.return_type.as_ref()?.ty };
    if ty.alignment() > 16 {
        return None;
    }
    let mut owned = Owned { ty, storage: vec![0; (ty.size() as usize).div_ceil(16).max(1)] };
    // SAFETY: zeroed storage of the type's size and alignment.
    unsafe { ty.construct(owned.storage.as_mut_ptr().cast()) };
    // SAFETY: the caller's context; the constructed value takes the result.
    unsafe { execute_in(context, f, values, Some(owned.storage.as_mut_ptr().cast())) }.then_some(owned)
}

/// `f(values...)` for a function without a result (its result type null): true when it ran.
///
/// # Safety
/// As [`invoke`].
pub unsafe fn invoke_void(context: *mut c_void, f: &'static BaseFunction, values: &[Value]) -> bool {
    // SAFETY: the caller's context; no result.
    f.return_type.is_null() && unsafe { execute_in(context, f, values, None) }
}

/// `object:name(values...)` for its result of any type, kept as [`Owned`] while it is read.
///
/// # Safety
/// As [`call`].
pub unsafe fn method_owned(object: *mut c_void, name: &str, values: &[Value]) -> Option<Owned> {
    // SAFETY: the caller's live object.
    let t = unsafe { resolve(object, name, values.len()) }?;
    let f = match t {
        Target::Member(f) | Target::Static(f) => f,
    };
    // A function without a result whose out parameter CET's mapping filled in answers that value (CET returns the
    // out parameters after the result: StatusEffectSystem.GetAppliedEffects(id, out effects) read as
    // `system:GetAppliedEffects(id)`).
    if f.return_type.is_null() {
        let mut outs = Vec::new();
        // SAFETY: the caller's object.
        let ran = unsafe { run_target_outs(object, t, values, None, Some(&mut outs)) };
        return if ran && !outs.is_empty() { Some(outs.remove(0)) } else { None };
    }
    // SAFETY: the function's own return property and type (types live as long as the game).
    let ty: &'static red4ext_abi::rtti::IType = unsafe { &*f.return_type.as_ref()?.ty };
    let mut owned = Owned::of_type(ty)?;
    // SAFETY: the caller's object; the constructed value takes the result.
    let ran = unsafe { run_target(object, t, values, Some(owned.as_mut_ptr().cast())) };
    if ran {
        Some(owned)
    } else {
        None
    }
}

/// `object:name(values...)` with any plain result (see [`invoke`]).
///
/// # Safety
/// As [`call`].
pub unsafe fn method<R: Ret>(object: *mut c_void, name: &str, values: &[Value]) -> Option<R> {
    // SAFETY: the caller's live object.
    match unsafe { resolve(object, name, values.len()) }? {
        // SAFETY: as above.
        Target::Member(f) => unsafe { invoke(object, f, values) },
        Target::Static(f) => {
            let (h, mut v) = with_object(object, values);
            v[0] = Value::Handle(&h);
            // SAFETY: a static in no context.
            unsafe { invoke(core::ptr::null_mut(), f, &v) }
        }
    }
}

/// A global function by its full name -- the plugin's own natives, whose full name is their short name -- called with
/// `values` as the script VM calls it (CET's `Name(...)`), its plain result read as `R`.
///
/// # Safety
/// On the game's main thread.
pub unsafe fn call_native<R: Ret>(name: &str, values: &[Value]) -> Option<R> {
    let f = RttiSystem::get()?.get_function(CName::hashed(name))?;
    // SAFETY: a registered global function, no context.
    unsafe { invoke(core::ptr::null_mut(), &f.base, values) }
}

/// `object:name(values...)` for the overload whose leading parameter types are `types` (SetTintColor(HDRColor) rather
/// than SetTintColor(Color)), with any plain result.
///
/// # Safety
/// As [`call`].
pub unsafe fn method_typed<R: Ret>(object: *mut c_void, name: &str, types: &[&str], values: &[Value]) -> Option<R> {
    // SAFETY: the caller's live object.
    let Some(f) = (unsafe { find_overload(object, name, values.len(), Some(types)) }) else {
        // SAFETY: as above.
        unsafe { audit_missing(object, name, values.len()) };
        return None;
    };
    // SAFETY: as above.
    unsafe { invoke(object, f, values) }
}

/// `object:name(values...)` for a function returning a Bool.
///
/// # Safety
/// As [`call`].
pub unsafe fn call_bool_with(object: *mut c_void, name: &str, values: &[Value]) -> Option<bool> {
    let mut value = 0u64;
    // SAFETY: an 8-byte slot for the Bool.
    unsafe { resolve(object, name, values.len()) }
        .is_some_and(|t| unsafe { run_target(object, t, values, Some((&mut value as *mut u64).cast())) })
        .then_some(value as u8 != 0)
}

/// Where `object`'s property `name` lives (its class's or a parent's; a scripted class's in the value holder), or None.
///
/// # Safety
/// `object` is a live scriptable object.
pub unsafe fn property(object: *mut c_void, name: &str) -> Option<*mut c_void> {
    // SAFETY: the caller's live object.
    let p = class_property(unsafe { object_class(object) }?, name)?;
    // SAFETY: in the instance, or in its value holder for a scripted class's property (CProperty::GetValuePtr).
    Some(unsafe { p.value_ptr::<c_void>(object) })
}

/// A class's property by the name it is written with (Class::property_named: the m_ prefix the RTTI does not keep), a
/// miss said once per class and name -- looked up by the written name, breachCanvasWRef was missed without a word.
fn class_property(class: &'static red4ext_abi::rtti::Class, name: &str) -> Option<&'static red4ext_abi::function::Property> {
    let p = class.property_named(name);
    if p.is_none() {
        crate::audit::once(crate::audit::key(&[0x5350, class.class_name().0, CName::hashed(name).0]), || {
            format!("property {}.{name}: no such property (from {})", class.class_name().text(), crate::audit::callers())
        });
    }
    p
}

/// As [`property`], only when the property's type is of RTTI kind `kind` (red4ext_abi::rtti::RttiKind as u8: a handle
/// 9, a struct or class value 2): what is read there is then what it is.
///
/// # Safety
/// As [`property`].
pub unsafe fn property_of_kind(object: *mut c_void, name: &str, kind: u8) -> Option<*mut c_void> {
    // SAFETY: the caller's live object.
    let p = class_property(unsafe { object_class(object) }?, name)?;
    // SAFETY: the property's own type.
    let ty = unsafe { p.ty.as_ref() }?;
    // SAFETY: as in `property`.
    (ty.kind() == kind).then(|| unsafe { p.value_ptr::<c_void>(object) })
}

/// `object:name()` for a function returning a Bool.
///
/// # Safety
/// As [`call`].
pub unsafe fn call_bool(object: *mut c_void, name: &str) -> Option<bool> {
    let mut value = 0u64;
    // SAFETY: an 8-byte slot for the Bool.
    unsafe { resolve(object, name, 0) }
        .is_some_and(|t| unsafe { run_target(object, t, &[], Some((&mut value as *mut u64).cast())) })
        .then_some(value as u8 != 0)
}

/// `object:name()` for a function returning a Float.
///
/// # Safety
/// As [`call`].
pub unsafe fn call_float(object: *mut c_void, name: &str) -> Option<f32> {
    let mut value = 0u64;
    // SAFETY: an 8-byte slot for the Float.
    unsafe { resolve(object, name, 0) }
        .is_some_and(|t| unsafe { run_target(object, t, &[], Some((&mut value as *mut u64).cast())) })
        .then_some(f32::from_bits(value as u32))
}
