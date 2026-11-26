//! Script functions hooked without the Lua host (the CET host's Observe / Override, src/Lua/cet/scripting/
//! FunctionOverride.cpp, for the port's own hooks): the function object is swapped in place with a native one made for
//! it (the engine's calls reach it, the bytecode references the object), the original moves into the object made here
//! and is called through it. A native's frame gives the arguments: each is evaluated once into storage of its type (an
//! out parameter's own variable is used directly; a temporary script reference gets its value), the hooks see them,
//! and the original runs with them through a CStack -- the arguments' bytecode does not run twice.
//!
//! The order is the host's: the Before hooks, then the Override (it calls the original or answers itself; without one
//! the original runs), then the After hooks. A hook names the class it is for; a function a parent class owns is
//! hooked once, and each hook runs only for objects of its class. A third-party CET mod's Override of the same
//! function chains over this one, as over any native.
//!
//! Only member (non-static) functions are hooked; the native handler of each hooked function is one of SLOTS, the
//! slot telling which function it stands for.
use crate::script_call::Owned;
use core::ffi::c_void;
use red4ext_abi::cname::CName;
use red4ext_abi::function::{function_flags, property_flags, BaseFunction, ClassFunction, NativeFn};
use red4ext_abi::handle::{Handle, WeakHandle};
use red4ext_abi::rtti::{object_class, Class, IType, RttiKind, RttiSystem};
use red4ext_abi::stack::{CStack, StackFrame, StackType};
use std::sync::RwLock;

/// A hook: what it sees of the call, and what it may change (the arguments before the original, the result).
pub(crate) type HookFn = fn(&mut Call);
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Kind {
    Before,
    After,
    /// Answers the call: runs the original through Call::original when it wants it.
    Override,
}
#[derive(Clone, Copy)]
struct Hook {
    class: usize,
    kind: Kind,
    hook: HookFn,
    /// A redscript wrapper's part (reds_hooks.rs): it ran inside the function -- after the host's Before observers,
    /// before its After observers.
    inner: bool,
}
struct Chain {
    /// The function object the engine calls (now ours) and the one holding the original.
    real: usize,
    original: usize,
    hooks: Vec<Hook>,
    name: String,
    /// The native's registration index ours got: while the engine's object carries it, the object is ours (another
    /// host -- the CET host, a third-party CET -- may swap its own over it later).
    reg: u32,
    /// A scripted class's static: the engine's object is a CGlobalFunction ("Class::name;Params"), its registration
    /// index where a global function keeps it.
    global: bool,
    /// A function Rust defined (`define`): no original -- its Override answers every call.
    defined: bool,
}
/// A function object's registration index, by its kind (CGlobalFunction +0xF0, CClassFunction +0xF8).
///
/// # Safety
/// `function` is a live function object of that kind.
unsafe fn reg_index(function: usize, global: bool) -> u32 {
    // SAFETY: the caller's function object.
    unsafe {
        if global {
            (*(function as *const red4ext_abi::function::GlobalFunction)).reg_index
        } else {
            (*(function as *const ClassFunction)).reg_index
        }
    }
}
static CHAINS: RwLock<Vec<Chain>> = RwLock::new(Vec::new());

/// One value of a call: where it is and its type, the storage kept while the call runs.
struct Value {
    ty: *const IType,
    ptr: *mut c_void,
    _own: Option<Owned>,
    _inner: Option<Owned>,
}
/// A hooked call as the hooks see it.
pub(crate) struct Call {
    ctx: *mut c_void,
    args: Vec<Value>,
    out: *mut c_void,
    ret: *const IType,
    ret_own: Option<Owned>,
    original: *mut BaseFunction,
    ran: bool,
    slot: usize,
    /// A call from native code (InternalCall): the engine's own stack and frame, which a scripted original runs with,
    /// as the CET host runs it (its arguments are the stack's values the hooks saw).
    pure: Option<(*mut c_void, *mut c_void)>,
    /// A defined function's call: there is no original to run.
    defined: bool,
}
/// Which slots' originals have run (a line each, the first time).
static RAN: [std::sync::atomic::AtomicU64; 4] = [const { std::sync::atomic::AtomicU64::new(0) }; 4];
/// A slot's bit set for the first time (its word and bit in a 256-bit set).
fn first(set: &[std::sync::atomic::AtomicU64; 4], slot: usize) -> bool {
    let bit = 1u64 << (slot % 64);
    set[slot / 64 % 4].fetch_or(bit, std::sync::atomic::Ordering::Relaxed) & bit == 0
}
impl Call {
    /// A read of argument `i` (the result for usize::MAX) as `want`, a type it is not: said once per hooked function,
    /// reader, argument and kind of read.
    fn audit(&self, i: usize, want: &str, ty: Option<&IType>) {
        let hook = HOOK_NOW.get();
        let key = crate::audit::key(&[self.slot as u64, hook as u64, i as u64, crate::audit::key(&want.bytes().map(u64::from).collect::<Vec<_>>())]);
        crate::audit::once(key, || {
            let name = CHAINS.read().ok().and_then(|c| c.get(self.slot).map(|c| c.name.clone())).unwrap_or_default();
            let which = if i == usize::MAX { "the result".to_string() } else { format!("argument {i}") };
            let what = ty.map_or_else(|| "missing".to_string(), crate::audit::type_text);
            format!("hook {name}: {which} is {what}, read as {want} by the hook at cyberpunkvrport.dll+{:#x}", hook.wrapping_sub(crate::audit::own_base()))
        });
    }
    /// The function's return type is not `name` (a void one included).
    fn audit_result(&self, want: &str, name: &str) {
        // SAFETY: the function's return type (types live as long as the game).
        match unsafe { self.ret.as_ref() } {
            Some(t) if t.name().text() == name => {}
            other => self.audit(usize::MAX, want, other),
        }
    }
    /// The object called (a strong handle on it); null for none.
    pub(crate) fn this(&self) -> Handle {
        // SAFETY: the call's live context, an ISerializable.
        unsafe { Handle::from_instance(self.ctx) }
    }
    /// Argument `i`'s storage and type.
    pub(crate) fn arg(&self, i: usize) -> Option<(*mut c_void, &'static IType)> {
        let a = self.args.get(i)?;
        // SAFETY: a parameter's type (types live as long as the game).
        Some((a.ptr, unsafe { &*a.ty }))
    }
    /// Argument `i` as a handle (a ref<> or a wref<> locked); null for another kind.
    pub(crate) fn arg_handle(&self, i: usize) -> Handle {
        let Some((p, ty)) = self.arg(i) else {
            self.audit(i, "a handle", None);
            return Handle::null();
        };
        // SAFETY: the value read by its own kind.
        unsafe {
            match ty.kind() {
                k if k == RttiKind::Handle as u8 => (*(p as *const Handle)).clone(),
                k if k == RttiKind::WeakHandle as u8 => (*(p as *const WeakHandle)).lock(),
                _ => {
                    self.audit(i, "a handle", Some(ty));
                    Handle::null()
                }
            }
        }
    }
    /// Argument `i` as an 8-byte value (a CName, a TweakDBID, an Int64); 0 for another size.
    pub(crate) fn arg_u64(&self, i: usize) -> u64 {
        let Some((p, ty)) = self.arg(i) else {
            self.audit(i, "an 8-byte value", None);
            return 0;
        };
        if ty.size() == 8 {
            // SAFETY: an 8-byte value.
            unsafe { *(p as *const u64) }
        } else {
            self.audit(i, "an 8-byte value", Some(ty));
            0
        }
    }
    pub(crate) fn set_arg_u64(&mut self, i: usize, v: u64) {
        if let Some((p, ty)) = self.arg(i) {
            if ty.size() == 8 {
                // SAFETY: an 8-byte value of the call.
                unsafe { *(p as *mut u64) = v };
            }
        }
    }
    /// Argument `i` as an array of Strings (empty for another type).
    pub(crate) fn arg_strings(&self, i: usize) -> Vec<String> {
        let Some((p, ty)) = self.arg(i) else {
            self.audit(i, "an array of Strings", None);
            return Vec::new();
        };
        if ty.kind() != RttiKind::Array as u8 {
            self.audit(i, "an array of Strings", Some(ty));
            return Vec::new();
        }
        // SAFETY: an array value read through its own type; its elements Strings.
        unsafe {
            let inner = ty.array_inner();
            if inner.is_null() || (*inner).name().text() != "String" {
                return Vec::new();
            }
            (0..ty.array_length(p)).map(|e| (*(ty.array_element(p, e) as *const red4ext_abi::cstring::CString)).as_str().to_string()).collect()
        }
    }
    /// Argument `i` as a Variant's handle (FromVariant of a ref<> / wref<>); null otherwise.
    pub(crate) fn arg_variant_handle(&self, i: usize) -> Handle {
        let Some((p, ty)) = self.arg(i) else {
            self.audit(i, "a Variant", None);
            return Handle::null();
        };
        if ty.name().text() != "Variant" {
            self.audit(i, "a Variant", Some(ty));
            return Handle::null();
        }
        // SAFETY: a Variant: its type word (the inline flag in the low bit), then the value inline or behind a pointer.
        unsafe {
            let word = *(p as *const usize);
            let Some(inner) = ((word & !1) as *const IType).as_ref() else { return Handle::null() };
            let data = if word & 1 != 0 { (p as *mut u8).add(8) as *mut c_void } else { *((p as *const u8).add(8) as *const *mut c_void) };
            if data.is_null() {
                return Handle::null();
            }
            match inner.kind() {
                k if k == RttiKind::Handle as u8 => (*(data as *const Handle)).clone(),
                k if k == RttiKind::WeakHandle as u8 => (*(data as *const WeakHandle)).lock(),
                _ => Handle::null(),
            }
        }
    }
    /// The original with the arguments as they are now; its result where the caller wants it.
    pub(crate) fn original(&mut self) -> bool {
        self.ran = true;
        if self.defined {
            return false;
        }
        // SAFETY: the object holding the original and the call's own values.
        let ok = unsafe { run_original(self) };
        if first(&RAN, self.slot) {
            let name = CHAINS.read().ok().and_then(|c| c.get(self.slot).map(|c| c.name.clone())).unwrap_or_default();
            let result = if self.out.is_null() || self.ret.is_null() {
                String::from("-")
            } else {
                // SAFETY: the result slot of the call, read by its type's size (a Bool's byte).
                unsafe { (*(self.out as *const u8)).to_string() }
            };
            cvr_host::log!("[script-hooks] {name}: the original ran: {ok}, result byte {result} (from Rust)\n");
        }
        ok
    }
    /// Argument `i` as a Bool (false for another type).
    pub(crate) fn arg_bool(&self, i: usize) -> bool {
        let Some((p, ty)) = self.arg(i) else {
            self.audit(i, "a Bool", None);
            return false;
        };
        if ty.name().text() != "Bool" {
            self.audit(i, "a Bool", Some(ty));
            return false;
        }
        // SAFETY: a Bool's byte.
        unsafe { *(p as *const u8) != 0 }
    }
    pub(crate) fn set_arg_bool(&mut self, i: usize, v: bool) {
        match self.arg(i) {
            Some((p, ty)) if ty.name().text() == "Bool" => {
                // SAFETY: a Bool of the call.
                unsafe { *(p as *mut u8) = u8::from(v) };
            }
            Some((_, ty)) => self.audit(i, "a Bool (written)", Some(ty)),
            None => self.audit(i, "a Bool (written)", None),
        }
    }
    /// Argument `i` as a number (Int32, Float...; None for another type).
    pub(crate) fn arg_number(&self, i: usize) -> Option<f64> {
        let Some((p, ty)) = self.arg(i) else {
            self.audit(i, "a number", None);
            return None;
        };
        // SAFETY: the value read by its type's name.
        unsafe {
            Some(match ty.name().text() {
                "Int32" => f64::from(*(p as *const i32)),
                "Uint32" => f64::from(*(p as *const u32)),
                "Float" => f64::from(*(p as *const f32)),
                "Int64" => *(p as *const i64) as f64,
                _ => {
                    self.audit(i, "a number (Int32, Uint32, Float, Int64)", Some(ty));
                    return None;
                }
            })
        }
    }
    /// The result written as a Bool (an Override answering without the original).
    pub(crate) fn set_result_bool(&mut self, v: bool) {
        self.ran = true;
        self.audit_result("a Bool (written)", "Bool");
        // SAFETY: the result slot of a Bool function.
        if !self.out.is_null() && !self.ret.is_null() && unsafe { (*self.ret).name().text() } == "Bool" {
            unsafe { *(self.out as *mut bool) = v };
        }
    }
    /// The result as a Bool (after the original ran; false for another type).
    pub(crate) fn result_bool(&self) -> bool {
        self.audit_result("a Bool", "Bool");
        // SAFETY: the result slot of a Bool function.
        !self.out.is_null() && !self.ret.is_null() && unsafe { (*self.ret).name().text() } == "Bool" && unsafe { *(self.out as *const u8) } != 0
    }
    /// The result's storage and type (a struct read or changed after the original ran).
    pub(crate) fn result(&self) -> Option<(*mut c_void, &'static IType)> {
        // SAFETY: the function's return type (types live as long as the game).
        (!self.out.is_null()).then_some(()).and(unsafe { self.ret.as_ref() }).map(|t| (self.out, t))
    }
    /// The result assigned from a value of its own type (an ItemID, a struct): the call answered.
    pub(crate) fn set_result_from(&mut self, src: *const c_void) {
        self.ran = true;
        if let Some((p, ty)) = self.result() {
            // SAFETY: a value of the return type into the result slot, by the type's own assignment.
            unsafe { ty.assign(p, src) };
        }
    }
    /// The result written as an enum's value (by the enum's size): the call answered.
    pub(crate) fn set_result_enum(&mut self, v: i64) {
        self.ran = true;
        // SAFETY: the function's return type.
        match unsafe { self.ret.as_ref() } {
            Some(t) if t.kind() == RttiKind::Enum as u8 => {}
            other => self.audit(usize::MAX, "an enum (written)", other),
        }
        if let Some((p, ty)) = self.result() {
            // SAFETY: the result slot of an enum, by its size.
            unsafe {
                match ty.size() {
                    1 => *(p as *mut i8) = v as i8,
                    2 => *(p as *mut i16) = v as i16,
                    8 => *(p as *mut i64) = v,
                    _ => *(p as *mut i32) = v as i32,
                }
            }
        }
    }
    /// Argument `i` assigned from a value of its own type (a Variant replaced).
    pub(crate) fn set_arg_from(&mut self, i: usize, src: *const c_void) {
        if let Some((p, ty)) = self.arg(i) {
            // SAFETY: a value of the parameter's type into the call's argument, by the type's own assignment.
            unsafe { ty.assign(p, src) };
        }
    }
    /// The result written as a Float (an Override answering without the original).
    pub(crate) fn set_result_float(&mut self, v: f32) {
        self.ran = true;
        self.audit_result("a Float (written)", "Float");
        // SAFETY: the result slot of a Float function.
        if !self.out.is_null() && !self.ret.is_null() && unsafe { (*self.ret).name().text() } == "Float" {
            unsafe { *(self.out as *mut f32) = v };
        }
    }
    /// Argument `i` set as a Float (nothing for another type).
    pub(crate) fn set_arg_float(&mut self, i: usize, v: f32) {
        match self.arg(i) {
            Some((p, ty)) if ty.name().text() == "Float" => {
                // SAFETY: a Float of the call.
                unsafe { *(p as *mut f32) = v };
            }
            Some((_, ty)) => self.audit(i, "a Float (written)", Some(ty)),
            None => self.audit(i, "a Float (written)", None),
        }
    }
    /// The call answered without the original (a void Override that skips it).
    pub(crate) fn skip(&mut self) {
        self.ran = true;
    }
}

unsafe fn run_original(call: &mut Call) -> bool {
    let Some(rtti) = RttiSystem::get() else { return false };
    // SAFETY: the object holding the original.
    let Some(f) = (unsafe { call.original.as_ref() }) else { return false };
    if let Some((stack, frame)) = call.pure {
        if f.flags & function_flags::IS_NATIVE == 0 {
            // the engine's InternalCall over the original's bytecode with the engine's own stack and frame
            let original = PURE_ORIGINAL.load(std::sync::atomic::Ordering::Acquire);
            // SAFETY: InternalCall with the call's own arguments.
            return original != 0 && unsafe { core::mem::transmute::<usize, PureFn>(original)(call.original, stack, frame) };
        }
    }
    let mut args: Vec<StackType> = call.args.iter().map(|a| StackType { ty: a.ty as *mut IType, value: a.ptr }).collect();
    if call.out.is_null() && !call.ret.is_null() {
        // the caller drops the result: the original still writes one
        // SAFETY: the function's own return type.
        call.ret_own = Owned::of_type(unsafe { &*call.ret });
        call.out = call.ret_own.as_mut().map_or(core::ptr::null_mut(), |o| o.as_mut_ptr().cast());
    }
    let mut result = StackType { ty: call.ret as *mut IType, value: call.out };
    let rp = if call.ret.is_null() || call.out.is_null() { core::ptr::null_mut() } else { &mut result as *mut StackType };
    let Some(mut stack) = CStack::new(rtti, call.ctx, &mut args, rp) else { return false };
    let Some(address) = f.execute_address() else { return false };
    type Execute = unsafe extern "C" fn(*mut BaseFunction, *mut CStack, *mut c_void) -> u8;
    // SAFETY: CBaseFunction::Execute over the original and the stack built above.
    unsafe { core::mem::transmute::<usize, Execute>(address)(call.original, &mut stack, core::ptr::null_mut()) != 0 }
}

/// A script_ref<T> type's inner type (by its name "script_ref:T").
fn script_ref_inner(ty: &IType) -> Option<&'static IType> {
    let name = ty.name();
    let inner = name.text().strip_prefix("script_ref:")?;
    RttiSystem::get()?.get_type(CName::hashed(inner))
}

/// The arguments evaluated from the caller's frame, once each, as CET's handler pops them.
unsafe fn pop(frame: &mut StackFrame, f: &BaseFunction) -> Vec<Value> {
    let mut out = Vec::with_capacity(f.params.len());
    for &p in f.params.as_slice() {
        // SAFETY: the function's own parameter and its type.
        let Some(p) = (unsafe { p.as_ref() }) else { continue };
        let ty: &'static IType = unsafe { &*p.ty };
        let mut own = Owned::of_type(ty);
        let storage = own.as_mut().map_or(core::ptr::null_mut(), |o| o.as_mut_ptr().cast::<c_void>());
        let is_ref = ty.kind() == RttiKind::ScriptReference as u8;
        let temporary = (frame.param_flags >> frame.current_param) & 1 != 0;
        let mut inner_own = None;
        if is_ref && temporary && !storage.is_null() {
            // made from an rvalue: the reference needs a value of its own (ScriptRef: 0x10, inner type, value, CName)
            if let Some(inner) = script_ref_inner(ty) {
                inner_own = Owned::of_type(inner);
                let words = storage as *mut usize;
                // SAFETY: the ScriptRef's fields in its own storage.
                unsafe {
                    *words.add(2) = inner as *const IType as usize;
                    *words.add(3) = inner_own.as_mut().map_or(0, |o| o.as_mut_ptr() as usize);
                    *words.add(4) = inner.name().0 as usize;
                }
            }
        }
        // SAFETY: the frame positioned at this parameter; the storage a constructed value of its type.
        unsafe { frame.get_parameter_with(storage, if is_ref { storage } else { core::ptr::null_mut() }) };
        let mut ptr = storage;
        if p.flags & property_flags::IS_OUT != 0 && !frame.data.is_null() {
            // an out parameter: the caller's own variable
            ptr = frame.data;
        }
        out.push(Value { ty, ptr, _own: own, _inner: inner_own });
    }
    frame.finish_params();
    out
}

/// Whether the object is of the class (or derives from it).
unsafe fn is_of(object: *mut c_void, class: usize) -> bool {
    if class == 0 {
        return true;
    }
    // SAFETY: the call's live context.
    let mut c = unsafe { object_class(object) };
    while let Some(k) = c {
        if k as *const Class as usize == class {
            return true;
        }
        c = k.parent();
    }
    false
}

fn run(h: &Hook, call: &mut Call) {
    let hook = h.hook;
    let previous = HOOK_NOW.replace(hook as usize);
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hook(call))).is_err() {
        cvr_host::log!("[script-hooks] a hook panicked; the call goes on\n");
    }
    HOOK_NOW.set(previous);
}
thread_local! {
    /// The hook function running on this thread (its address names the reader in an audit line).
    static HOOK_NOW: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

/// Which slots have been entered (a line each, the first time).
static ENTERED: [std::sync::atomic::AtomicU64; 4] = [const { std::sync::atomic::AtomicU64::new(0) }; 4];
fn first_call(slot: usize, how: &str) {
    if first(&ENTERED, slot) {
        let name = CHAINS.read().ok().and_then(|c| c.get(slot).map(|c| c.name.clone())).unwrap_or_default();
        cvr_host::log!("[script-hooks] {name}: first call ({how}, from Rust)\n");
    }
}
fn chain(slot: usize) -> Option<(Vec<Hook>, *mut BaseFunction, bool)> {
    let chains = CHAINS.read().unwrap_or_else(|p| p.into_inner());
    let c = chains.get(slot)?;
    Some((c.hooks.clone(), c.original as *mut BaseFunction, c.defined))
}
/// The chain around the call: the Before hooks, the Override or the original, the After hooks.
fn run_chain(hooks: &[Hook], call: &mut Call) {
    let ctx = call.ctx;
    // SAFETY: the call's live context.
    let mine = |h: &Hook| unsafe { is_of(ctx, h.class) };
    // Before: the host's observers (outer) first, then the wrappers' own code (inner)
    for inner in [false, true] {
        for h in hooks.iter().filter(|h| h.kind == Kind::Before && h.inner == inner && mine(h)) {
            run(h, call);
        }
    }
    // the Overrides in their order, as redscript chains its wrappers: the first that answers (a result, a skip, the
    // original run) ends the chain; one that does not passes the call on, and in the end to the original
    for h in hooks.iter().filter(|h| h.kind == Kind::Override && mine(h)) {
        run(h, call);
        if call.ran {
            break;
        }
    }
    if !call.ran {
        call.original();
    }
    // After: the wrappers' code (inner) first, then the host's observers (outer)
    for inner in [true, false] {
        for h in hooks.iter().filter(|h| h.kind == Kind::After && h.inner == inner && mine(h)) {
            run(h, call);
        }
    }
}
/// A call reaching the native object (the VM's call of a native-marked function): its arguments in the frame.
unsafe fn dispatch(slot: usize, context: *mut c_void, frame: *mut StackFrame, out: *mut c_void) {
    first_call(slot, "native");
    let Some((hooks, original, defined)) = chain(slot) else { return };
    // SAFETY: the engine's frame for this native call; the original holds the same signature.
    let (Some(frame), Some(f)) = (unsafe { frame.as_mut() }, unsafe { original.as_ref() }) else { return };
    let ctx = if context.is_null() { frame.context } else { context };
    let args = unsafe { pop(frame, f) };
    let ret = unsafe { f.return_type.as_ref() }.map_or(core::ptr::null(), |r| r.ty as *const IType);
    let mut call = Call { ctx, args, out, ret, ret_own: None, original, ran: false, slot, pure: None, defined };
    run_chain(&hooks, &mut call);
}

/// InternalCall (CET's CScript_RunPureScript, the hash of CBaseFunction_ExecuteScripted): native code's call of a
/// script function runs its bytecode here -- the engine's callbacks (OnInitialize, OnGameAttached, a RegisterToCallback
/// listener) come this way whatever the function's flags say, so a hooked function is caught here too. The plugin's
/// Lua host hands its own overrides on through cvr_pure_calls_attach (one hook on the address: the same MinHook).
type PureFn = unsafe extern "C" fn(*mut BaseFunction, *mut c_void, *mut c_void) -> bool;
static PURE_ORIGINAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static PURE_HOST: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static PURE_INSTALLED: std::sync::Mutex<Option<bool>> = std::sync::Mutex::new(None);
static PURE_TARGET: red4ext_abi::reloc::Reloc = red4ext_abi::reloc::Reloc::new(red4ext_abi::hashes::CBaseFunction_ExecuteScripted);
extern "system" {
    fn MH_Initialize() -> i32;
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_EnableHook(target: *mut c_void) -> i32;
}

/// The slot of a hooked function object (the engine's), when it is one, and whether its content is still ours.
fn slot_of(function: *mut BaseFunction) -> Option<(usize, bool)> {
    let chains = CHAINS.read().unwrap_or_else(|p| p.into_inner());
    let slot = chains.iter().position(|c| c.real == function as usize)?;
    // SAFETY: a hooked function's object, of the chain's kind.
    let reg = unsafe { reg_index(function as usize, chains[slot].global) };
    Some((slot, reg == chains[slot].reg))
}
fn is_native(function: *mut BaseFunction) -> bool {
    // SAFETY: the engine's function object.
    unsafe { function.as_ref() }.is_some_and(|f| f.flags & function_flags::IS_NATIVE != 0)
}
/// The outer hook: a function whose object is ours runs this chain; any other goes to the host (whose own overrides
/// may sit over ours: it runs its chain and hands the call on to pure_inner), else on.
unsafe extern "C" fn pure_detour(function: *mut BaseFunction, stack: *mut c_void, frame: *mut c_void) -> bool {
    if is_native(function) {
        if let Some((slot, true)) = slot_of(function) {
            // SAFETY: the engine's stack for this call.
            if unsafe { pure_call(slot, stack, frame) } {
                return true;
            }
        }
    }
    let host = PURE_HOST.load(std::sync::atomic::Ordering::Acquire);
    // SAFETY: the host's handler or the next layer, with the engine's arguments.
    unsafe {
        if host != 0 {
            return core::mem::transmute::<usize, PureFn>(host)(function, stack, frame);
        }
        pure_inner(function, stack, frame)
    }
}
/// What the host calls next (its "real" InternalCall): a hooked function here is one whose object the host's own
/// override covers -- this chain runs under it -- else the engine's InternalCall.
unsafe extern "C" fn pure_inner(function: *mut BaseFunction, stack: *mut c_void, frame: *mut c_void) -> bool {
    if is_native(function) {
        if let Some((slot, _)) = slot_of(function) {
            // SAFETY: the engine's stack for this call.
            if unsafe { pure_call(slot, stack, frame) } {
                return true;
            }
        }
    }
    let original = PURE_ORIGINAL.load(std::sync::atomic::Ordering::Acquire);
    // SAFETY: the engine's InternalCall with its own arguments.
    original != 0 && unsafe { core::mem::transmute::<usize, PureFn>(original)(function, stack, frame) }
}
/// A hooked call made from native code: its arguments are values in the stack (CScriptStack: a buffer at the
/// parameters' offsets; CStack: an array of CStackTypes), its result where the stack says.
unsafe fn pure_call(slot: usize, stack: *mut c_void, frame: *mut c_void) -> bool {
    first_call(slot, "from native code");
    let Some((hooks, original, defined)) = chain(slot) else { return false };
    // SAFETY: the original's signature; the stack's IStack vtable (GetResultAddr +0x08) and its CBaseStack /
    // CScriptStack fields (context18 +0x18, context20 +0x20, unk28 +0x28, args +0x30, value +0x38).
    unsafe {
        let Some(f) = original.as_ref() else { return false };
        let s = stack as *mut u8;
        let vt = *(stack as *const *const usize);
        type Get = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
        let out = core::mem::transmute::<usize, Get>(*vt.add(1))(stack);
        let ctx = {
            let c18 = *(s.add(0x18) as *const *mut c_void);
            if !c18.is_null() {
                c18
            } else if !(*(s.add(0x28) as *const *mut c_void)).is_null() {
                *(s.add(0x20) as *const *mut c_void)
            } else {
                core::ptr::null_mut()
            }
        };
        let base = *(s.add(0x30) as *const *mut u8);
        let array = *(s.add(0x38) as *const *mut c_void) != out;
        let mut args = Vec::with_capacity(f.params.len());
        for (i, &p) in f.params.as_slice().iter().enumerate() {
            let Some(p) = p.as_ref() else { continue };
            let ptr = if array { (*(base as *const StackType).add(i)).value } else { base.add(p.value_offset as usize) as *mut c_void };
            args.push(Value { ty: p.ty, ptr, _own: None, _inner: None });
        }
        let ret = f.return_type.as_ref().map_or(core::ptr::null(), |r| r.ty as *const IType);
        let mut call = Call { ctx, args, out, ret, ret_own: None, original, ran: false, slot, pure: Some((stack, frame)), defined };
        run_chain(&hooks, &mut call);
    }
    true
}
/// The one hook on InternalCall, made once (false when the address or MinHook fails).
fn install_pure() -> bool {
    let mut done = PURE_INSTALLED.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(ok) = *done {
        return ok;
    }
    let ok = (|| {
        let target = PURE_TARGET.get()? as *mut c_void;
        let mut original: *mut c_void = core::ptr::null_mut();
        // SAFETY: MinHook on the engine's InternalCall, resolved by its address hash; MinHook's own initialisation
        // first (the script classes are defined during the script loading, maybe before the detours' start).
        unsafe {
            if !matches!(MH_Initialize(), 0 | 1) {
                return None;
            }
            if MH_CreateHook(target, pure_detour as *mut c_void, &mut original) != 0 {
                return None;
            }
            PURE_ORIGINAL.store(original as usize, std::sync::atomic::Ordering::Release);
            if MH_EnableHook(target) != 0 {
                return None;
            }
        }
        Some(())
    })()
    .is_some();
    cvr_host::log!("[script-hooks] InternalCall hook: {} (from Rust)\n", if ok { "ok" } else { "FAILED" });
    *done = Some(ok);
    ok
}
/// The plugin's Lua host's InternalCall handler, put behind this hook (src/Lua/cet/scripting/FunctionOverride.cpp's
/// HookPureCalls in the Rust build): `*real` gets what it calls for functions it does not own.
///
/// # Safety
/// `handler` is the host's InternalCall handler; `real` a writable pointer slot.
#[no_mangle]
pub unsafe extern "C" fn cvr_pure_calls_attach(handler: *mut c_void, real: *mut *mut c_void) -> bool {
    if !install_pure() {
        return false;
    }
    // SAFETY: the caller's slot.
    unsafe {
        if !real.is_null() {
            *real = pure_inner as *mut c_void;
        }
    }
    PURE_HOST.store(handler as usize, std::sync::atomic::Ordering::Release);
    true
}

unsafe extern "C" fn slot<const N: usize>(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _a4: i64) {
    // SAFETY: the engine's native call of the function this slot stands for.
    unsafe { dispatch(N, context, frame, out) }
}
macro_rules! slots {
    ($($n:literal)*) => { [$(slot::<$n> as NativeFn),*] };
}
const SLOTS: [NativeFn; 256] = slots!(
    0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39
    40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63 64 65 66 67 68 69 70 71 72 73 74 75 76
    77 78 79 80 81 82 83 84 85 86 87 88 89 90 91 92 93 94 95 96 97 98 99 100 101 102 103 104 105 106 107 108 109
    110 111 112 113 114 115 116 117 118 119 120 121 122 123 124 125 126 127 128 129 130 131 132 133 134 135 136 137
    138 139 140 141 142 143 144 145 146 147 148 149 150 151 152 153 154 155 156 157 158 159 160 161 162 163 164 165
    166 167 168 169 170 171 172 173 174 175 176 177 178 179 180 181 182 183 184 185 186 187 188 189 190 191 192 193
    194 195 196 197 198 199 200 201 202 203 204 205 206 207 208 209 210 211 212 213 214 215 216 217 218 219 220 221
    222 223 224 225 226 227 228 229 230 231 232 233 234 235 236 237 238 239 240 241 242 243 244 245 246 247 248 249
    250 251 252 253 254 255
);

/// The class's function (its own, else a parent's) named so.
fn find(class: &'static Class, name: &str) -> Option<&'static BaseFunction> {
    let short = CName::hashed(name);
    let mut c = Some(class);
    while let Some(k) = c {
        for &f in k.funcs() {
            // SAFETY: the class's registered functions.
            if let Some(f) = unsafe { f.as_ref() } {
                if f.short_name == short {
                    return Some(f);
                }
            }
        }
        c = k.parent();
    }
    None
}

/// `hook` on `class.name` (a member function, script or native); false when the class or the function is missing, it
/// is static, or no slot is left.
pub(crate) fn install(class: &str, name: &str, kind: Kind, hook: HookFn) -> bool {
    install_as(class, name, kind, hook, false)
}
/// [`install`] for a redscript wrapper's part (see Hook::inner).
pub(crate) fn install_inner(class: &str, name: &str, kind: Kind, hook: HookFn) -> bool {
    install_as(class, name, kind, hook, true)
}
fn install_as(class: &str, name: &str, kind: Kind, hook: HookFn, inner: bool) -> bool {
    if !install_pure() {
        return false;
    }
    let Some(cls) = crate::script_call::class_any(class) else {
        cvr_host::log!("[script-hooks] {class}.{name}: no such class\n");
        return false;
    };
    let Some(real) = find(cls, name) else {
        cvr_host::log!("[script-hooks] {class}.{name}: no such function\n");
        return false;
    };
    // a static is not hooked: a scripted class's static is a global function ("Class::name;Params"), not the class's
    if real.flags & function_flags::IS_STATIC != 0 {
        cvr_host::log!("[script-hooks] {class}.{name}: a static, not hooked\n");
        return false;
    }
    let real_ptr = real as *const BaseFunction as usize;
    let entry = Hook { class: cls as *const Class as usize, kind, hook, inner };
    let mut chains = CHAINS.write().unwrap_or_else(|p| p.into_inner());
    if let Some(c) = chains.iter_mut().find(|c| c.real == real_ptr) {
        c.hooks.push(entry);
        return true;
    }
    let n = chains.len();
    if n >= SLOTS.len() {
        cvr_host::log!("[script-hooks] {class}.{name}: no slot left\n");
        return false;
    }
    let (Ok(full), Ok(short)) = (std::ffi::CString::new(real.full_name.text()), std::ffi::CString::new(name)) else { return false };
    // SAFETY: the real function is a class member (CClassFunction); ours is made by the engine's constructor.
    unsafe {
        let real_class = (*(real_ptr as *const ClassFunction)).parent;
        let Some(ours) = ClassFunction::create(real_class, &full, &short, SLOTS[n], real.flags | function_flags::IS_NATIVE) else {
            cvr_host::log!("[script-hooks] {class}.{name}: the function could not be made\n");
            return false;
        };
        ours.parent = real_class;
        if !ours.base.take_signature(real) {
            cvr_host::log!("[script-hooks] {class}.{name}: its signature could not be taken\n");
            return false;
        }
        // the contents swapped: the engine's object is now ours (native, this slot), ours holds the original
        let a = real_ptr as *mut u8;
        let b = ours as *mut ClassFunction as *mut u8;
        let mut held = [0u8; core::mem::size_of::<ClassFunction>()];
        core::ptr::copy_nonoverlapping(a, held.as_mut_ptr(), held.len());
        core::ptr::copy_nonoverlapping(b, a, held.len());
        core::ptr::copy_nonoverlapping(held.as_ptr(), b, held.len());
        let reg = (*(real_ptr as *const ClassFunction)).reg_index;
        chains.push(Chain { real: real_ptr, original: b as usize, hooks: vec![entry], name: format!("{class}.{name}"), reg, global: false, defined: false });
    }
    true
}

/// `hook` on a scripted class's static `class.name` -- a global function "Class::name;Params" -- for every caller (it
/// has no object to filter by); false when it is missing or no slot is left.
pub(crate) fn install_static(class: &str, name: &str, kind: Kind, hook: HookFn) -> bool {
    install_static_as(class, name, kind, hook, false)
}
/// [`install_static`] for a redscript wrapper's part (see Hook::inner).
pub(crate) fn install_static_inner(class: &str, name: &str, kind: Kind, hook: HookFn) -> bool {
    install_static_as(class, name, kind, hook, true)
}
fn install_static_as(class: &str, name: &str, kind: Kind, hook: HookFn, inner: bool) -> bool {
    if !install_pure() {
        return false;
    }
    let Some(real) = crate::script_call::scripted_static(class, name) else {
        cvr_host::log!("[script-hooks] {class}::{name}: no such scripted static\n");
        return false;
    };
    let real_ptr = real as *const BaseFunction as usize;
    let entry = Hook { class: 0, kind, hook, inner };
    let mut chains = CHAINS.write().unwrap_or_else(|p| p.into_inner());
    if let Some(c) = chains.iter_mut().find(|c| c.real == real_ptr) {
        c.hooks.push(entry);
        return true;
    }
    let n = chains.len();
    if n >= SLOTS.len() {
        cvr_host::log!("[script-hooks] {class}::{name}: no slot left\n");
        return false;
    }
    let (Ok(full), Ok(short)) = (std::ffi::CString::new(real.full_name.text()), std::ffi::CString::new(name)) else { return false };
    // SAFETY: the real function is a global one (CGlobalFunction); ours is made by the engine's constructor.
    unsafe {
        let Some(ours) = red4ext_abi::function::GlobalFunction::create(&full, &short, SLOTS[n]) else {
            cvr_host::log!("[script-hooks] {class}::{name}: the function could not be made\n");
            return false;
        };
        ours.base.flags = real.flags | function_flags::IS_NATIVE;
        if !ours.base.take_signature(real) {
            cvr_host::log!("[script-hooks] {class}::{name}: its signature could not be taken\n");
            return false;
        }
        // the contents swapped: the engine's object is now ours (native, this slot), ours holds the original
        let a = real_ptr as *mut u8;
        let b = ours as *mut red4ext_abi::function::GlobalFunction as *mut u8;
        let mut held = [0u8; core::mem::size_of::<red4ext_abi::function::GlobalFunction>()];
        core::ptr::copy_nonoverlapping(a, held.as_mut_ptr(), held.len());
        core::ptr::copy_nonoverlapping(b, a, held.len());
        core::ptr::copy_nonoverlapping(held.as_ptr(), b, held.len());
        let reg = reg_index(real_ptr, true);
        chains.push(Chain { real: real_ptr, original: b as usize, hooks: vec![entry], name: format!("{class}::{name}"), reg, global: true, defined: false });
    }
    true
}

/// A defined function's signature: another function's (its parameters and return, the same property objects), or
/// (name, type) parameters and a return type ("" for none), the types by their RTTI names.
#[derive(Clone, Copy)]
pub(crate) enum Sig {
    Like(&'static str, &'static str),
    Of(&'static [(&'static str, &'static str)], &'static str),
}

/// Codeware's event classes (the VR lobby's exposure callbacks take them): without Codeware they are not in the engine
/// and a callback taking one is never called -- the lobby's known exposure gap, said as a note, not as a wrong read.
const FRAMEWORK_TYPES: &[&str] = &["ResourceEvent", "GameSessionEvent"];

/// A member function of `class` made by Rust (what redscript's @addMethod or a class of its own declared): a native of
/// the next slot whose calls `hook` answers (an Override; there is no original), pushed into the class's functions --
/// the engine finds a function by walking them (rva 0x2255C0). Made before InitializeScriptRuntime, the engine lays its
/// parameters and frame out with every other (script_classes.rs). False when the class already has a function so
/// named, the signature cannot be made, or no slot is left.
pub(crate) fn define(class: &'static Class, name: &str, sig: Sig, flags: u32, hook: HookFn) -> bool {
    let class_name = class.class_name().text().to_string();
    let short = CName::hashed(name);
    // SAFETY: the class's registered functions.
    if class.funcs().iter().any(|&f| unsafe { f.as_ref() }.is_some_and(|f| f.short_name == short)) {
        cvr_host::log!("[script-hooks] {class_name}.{name}: the class has it already, not defined\n");
        return false;
    }
    if !install_pure() {
        return false;
    }
    let Some(rtti) = RttiSystem::get() else { return false };
    let template = match sig {
        Sig::Like(c, f) => match crate::script_call::class_any(c).and_then(|k| find(k, f)) {
            Some(t) => Some(t),
            None => {
                cvr_host::log!("[script-hooks] {class_name}.{name}: no {c}.{f} to take the signature of\n");
                return false;
            }
        },
        Sig::Of(..) => None,
    };
    // the full name as the script compiler writes it: the short name, then the parameters' types after a ';'
    let full = template
        .and_then(|t| t.full_name.text().split_once(';').map(|(_, rest)| format!("{name};{rest}")))
        .unwrap_or_else(|| name.to_string());
    let (Ok(cfull), Ok(cshort)) = (std::ffi::CString::new(full), std::ffi::CString::new(name)) else { return false };
    let mut chains = CHAINS.write().unwrap_or_else(|p| p.into_inner());
    let n = chains.len();
    if n >= SLOTS.len() {
        cvr_host::log!("[script-hooks] {class_name}.{name}: no slot left\n");
        return false;
    }
    let class_ptr = class as *const Class;
    // SAFETY: ours is made by the engine's constructor, its arrays its own; the class's function array grows through
    // the engine's allocator.
    unsafe {
        let Some(ours) = ClassFunction::create(class_ptr, &cfull, &cshort, SLOTS[n], flags | function_flags::IS_NATIVE) else {
            cvr_host::log!("[script-hooks] {class_name}.{name}: the function could not be made\n");
            return false;
        };
        ours.parent = class_ptr;
        let signed = match (template, sig) {
            (Some(t), _) => {
                let ok = ours.base.take_signature(t);
                ours.base.full_name = CName::add(&cfull);
                ours.base.short_name = CName::add(&cshort);
                ok
            }
            (None, Sig::Of(params, ret)) => {
                let mut ok = true;
                for (pn, pt) in params {
                    // the innermost type of handle:/whandle:/array:/script_ref: must exist, or the engine makes the
                    // wrapper over nothing and no caller can ever pass a value of it
                    let inner = pt.rsplit(':').next().unwrap_or(pt);
                    if rtti.get_type(CName::hashed(inner)).is_none() && FRAMEWORK_TYPES.contains(&inner) {
                        cvr_host::log!("[script-hooks] {class_name}.{name}: {inner} is Codeware's, which is not installed: the callback is not called\n");
                    } else if rtti.get_type(CName::hashed(inner)).is_none() {
                        crate::audit::once(crate::audit::key(&[0x5167, CName::hashed(&format!("{class_name}.{name}:{pt}")).0]), || {
                            format!("{class_name}.{name}: parameter {pn} of type {pt}: no type {inner} in the engine")
                        });
                    }
                    let (Ok(pn), Ok(pt)) = (std::ffi::CString::new(*pn), std::ffi::CString::new(*pt)) else { return false };
                    ok &= ours.base.add_param(rtti, &pt, &pn, false, false);
                }
                if !ret.is_empty() {
                    let Ok(r) = std::ffi::CString::new(ret) else { return false };
                    ok &= ours.base.set_return_type(rtti, &r);
                }
                ok
            }
            _ => false,
        };
        if !signed {
            cvr_host::log!("[script-hooks] {class_name}.{name}: its signature could not be made\n");
            return false;
        }
        let funcs = (class_ptr as *mut u8).add(0x48) as *mut red4ext_abi::dynarray::DynArray<*mut BaseFunction>;
        let real = ours as *mut ClassFunction as *mut BaseFunction;
        if !(*funcs).push(real) {
            cvr_host::log!("[script-hooks] {class_name}.{name}: the class's function list could not grow\n");
            return false;
        }
        let entry = Hook { class: 0, kind: Kind::Override, hook, inner: false };
        let reg = (*(real as *const ClassFunction)).reg_index;
        chains.push(Chain {
            real: real as usize,
            original: real as usize,
            hooks: vec![entry],
            name: format!("{class_name}.{name}"),
            reg,
            global: false,
            defined: true,
        });
    }
    true
}

/// Every hooked or defined function's signature by the RTTI's own types, a line each (name, kind, result, parameters),
/// written to `path` when their number changed since the last write (tools/audit/check_reads.py reads it).
pub(crate) fn dump_signatures(path: &std::path::Path) {
    static WRITTEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let chains = CHAINS.read().unwrap_or_else(|p| p.into_inner());
    if chains.len() == WRITTEN.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let mut out = String::new();
    for c in chains.iter() {
        // SAFETY: the original function object (the engine's signature it was made with).
        let Some(f) = (unsafe { (c.original as *const BaseFunction).as_ref() }) else { continue };
        // SAFETY: the function's own return property and parameters.
        let ret = unsafe { f.return_type.as_ref().and_then(|p| p.ty.as_ref()) }.map_or_else(|| "void".to_string(), crate::audit::type_text);
        let params: Vec<String> = f
            .params
            .as_slice()
            .iter()
            .map(|p| {
                // SAFETY: as above.
                match unsafe { p.as_ref() } {
                    Some(p) => format!("{}:{}", p.name.text(), unsafe { p.ty.as_ref() }.map_or_else(|| "?".to_string(), crate::audit::type_text)),
                    None => "?".to_string(),
                }
            })
            .collect();
        let kind = if c.defined { "defined" } else if c.global { "static" } else { "hooked" };
        out.push_str(&format!("{}\t{kind}\t{ret}\t{}\n", c.name, params.join(", ")));
    }
    if std::fs::write(path, out).is_ok() {
        WRITTEN.store(chains.len(), std::sync::atomic::Ordering::Relaxed);
    }
}
