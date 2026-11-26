//! Script functions (`Scripting/Functions.hpp`, `CProperty.hpp`): what a native is to the engine, and how a plugin
//! makes one -- allocated from PoolRTTIFunction, constructed by the engine's own constructor, its parameters made as
//! RTTI properties from PoolRTTIProperty, then handed to the registry, which owns it from then on.
use crate::assert_layout;
use crate::cname::CName;
use crate::dynarray::DynArray;
use crate::hashes;
use crate::memory::{POOL_RTTI_FUNCTION, POOL_RTTI_PROPERTY};
use crate::reloc::Reloc;
use crate::rtti::{IType, RttiSystem};
use crate::stack::StackFrame;
use core::ffi::{c_void, CStr};

/// The name a property is registered under for the name it is written with: `CProperty::Create` drops an "m_" prefix,
/// so a script's `m_rootWidget` is the RTTI's `rootWidget` (the script dumps and redscript keep the written name).
pub fn registered_name(name: &str) -> &str {
    name.strip_prefix("m_").unwrap_or(name)
}

/// `CProperty::Flags` bits used here.
pub mod property_flags {
    pub const IS_RETURN: u64 = 1 << 6;
    pub const IS_OUT: u64 = 1 << 9;
    pub const IS_OPTIONAL: u64 = 1 << 10;
}

/// `CBaseFunction::Flags` bits.
pub mod function_flags {
    pub const IS_NATIVE: u32 = 1 << 0;
    pub const IS_STATIC: u32 = 1 << 1;
    pub const IS_FINAL: u32 = 1 << 2;
    pub const IS_EVENT: u32 = 1 << 3;
    pub const IS_EXEC: u32 = 1 << 4;
    pub const IS_PRIVATE: u32 = 1 << 7;
    pub const IS_PROTECTED: u32 = 1 << 8;
    pub const IS_PUBLIC: u32 = 1 << 9;
    pub const IS_CONST: u32 = 1 << 13;
    pub const IS_QUEST: u32 = 1 << 14;
    pub const IS_THREADSAFE: u32 = 1 << 15;
}

#[repr(C)]
pub struct Property {
    pub ty: *mut IType,
    pub name: CName,
    pub group: CName,
    pub parent: *mut c_void,
    pub value_offset: u32,
    pub flags: u64,
}
assert_layout!(Property, size = 0x30, ty = 0, name = 8, group = 0x10, parent = 0x18, value_offset = 0x20, flags = 0x28);

/// CProperty::Flags::inValueHolder (bit 0x15): the value lives in the scriptable's value holder.
const PROPERTY_IN_VALUE_HOLDER: u64 = 1 << 0x15;
/// IScriptable: its native type (+0x30) and value holder (+0x38).
const SCRIPTABLE_NATIVE_TYPE: usize = 0x30;
const SCRIPTABLE_VALUE_HOLDER: usize = 0x38;

impl Property {
    /// `CProperty::GetValuePtr<T>(instance)`: where the value is -- in the instance, or in its value holder (made on
    /// first use, as `IScriptable::GetValueHolder` does).
    ///
    /// # Safety
    /// `instance` is a live instance of the property's class (an IScriptable when the value is in a holder) and `T`
    /// the property's type.
    pub unsafe fn value_ptr<T>(&self, instance: *mut c_void) -> *mut T {
        let mut holder = instance;
        if self.flags & PROPERTY_IN_VALUE_HOLDER != 0 {
            // SAFETY: IScriptable's fields; the class's own initialisation when there is no holder yet.
            unsafe {
                let slot = (instance as *mut u8).add(SCRIPTABLE_VALUE_HOLDER) as *mut *mut c_void;
                if (*slot).is_null() {
                    let mut ty = *((instance as *const u8).add(SCRIPTABLE_NATIVE_TYPE) as *const *const crate::rtti::Class);
                    if ty.is_null() {
                        type GetNativeType = unsafe extern "C" fn(*mut c_void) -> *const crate::rtti::Class;
                        ty = crate::rtti::vfunc::<GetNativeType>(instance, 0x00)(instance);
                    }
                    if let Some(ty) = ty.as_ref() {
                        ty.initialize_properties(instance);
                    }
                }
                holder = *slot;
            }
        }
        // SAFETY: the value's offset in its holder.
        unsafe { (holder as *mut u8).add(self.value_offset as usize) as *mut T }
    }
    /// `CProperty::SetValue(instance, value)`: the property's type assigns `value` onto the current one.
    ///
    /// # Safety
    /// As `value_ptr`.
    pub unsafe fn set_value<T>(&self, instance: *mut c_void, value: &T) {
        // SAFETY: as the caller vouches; the type pointer is the property's own.
        unsafe {
            let at = self.value_ptr::<T>(instance);
            if let Some(ty) = self.ty.as_ref() {
                ty.assign(at as *mut c_void, value as *const T as *const c_void);
            }
        }
    }
    /// `CProperty::Create`: allocated from PoolRTTIProperty and filled as its constructor does ("m_" stripped from
    /// the name). A null group (the SDK passes nullptr for parameters and returns) is the empty name.
    pub fn create(ty: *mut IType, name: &CStr, value_offset: u32, flags: u64) -> Option<*mut Property> {
        let memory = POOL_RTTI_PROPERTY.alloc(core::mem::size_of::<Property>())?.memory as *mut Property;
        let bytes = name.to_bytes_with_nul();
        // the name as `registered_name` has it
        let name = if bytes.starts_with(b"m_") {
            // SAFETY: still NUL-terminated after dropping the two-byte prefix.
            unsafe { CStr::from_bytes_with_nul_unchecked(&bytes[2..]) }
        } else {
            name
        };
        let value = Property {
            ty,
            name: CName::add(name),
            group: CName(0),
            parent: core::ptr::null_mut(),
            value_offset,
            flags,
        };
        // SAFETY: fresh memory of the right size from the engine's property pool.
        unsafe { memory.write(value) };
        Some(memory)
    }
}

/// `CBaseFunction`. Fields the port reads are public; the rest are kept opaque at their offsets.
#[repr(C)]
pub struct BaseFunction {
    vtable: *const usize,
    pub full_name: CName,
    pub short_name: CName,
    pub return_type: *mut Property,
    _unk20: i64,
    pub params: DynArray<*mut Property>,
    pub local_vars: DynArray<*mut Property>,
    _unk48: [u8; 0x30],
    _bytecode: [u8; 0x30],
    pub flags: u32,
    pub unk_ac: i32,
    _unk_b0: [u8; 0x18],
    _unk_c8: [u8; 0x18],
    _unk_e0: i64,
    _unk_e8: *mut BaseFunction,
}
assert_layout!(BaseFunction, size = 0xF0, full_name = 0x08, short_name = 0x10, return_type = 0x18, params = 0x28,
    local_vars = 0x38, flags = 0xA8, unk_ac = 0xAC);

#[repr(C)]
pub struct GlobalFunction {
    pub base: BaseFunction,
    pub reg_index: u32,
}
assert_layout!(GlobalFunction, size = 0xF8, reg_index = 0xF0);

/// What the script VM calls: `void (IScriptable* context, CStackFrame* frame, T* out, int64_t)`.
pub type NativeFn = unsafe extern "C" fn(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);

static GLOBAL_CTOR: Reloc = Reloc::new(hashes::CGlobalFunction_ctor);
static EXECUTE_NATIVE: Reloc = Reloc::new(hashes::CBaseFunction_ExecuteNative);
static EXECUTE_SCRIPTED: Reloc = Reloc::new(hashes::CBaseFunction_ExecuteScripted);

impl BaseFunction {
    /// `SetReturnType`: false when the type is not (yet) registered.
    pub fn set_return_type(&mut self, rtti: &RttiSystem, type_name: &CStr) -> bool {
        match rtti.get_type(CName::add(type_name)) {
            Some(ty) => self.set_return_type_of(ty),
            None => false,
        }
    }

    /// `SetReturnType` once the type is found: a return property of that type.
    pub fn set_return_type_of(&mut self, ty: &IType) -> bool {
        match Property::create(ty as *const IType as *mut IType, c"__return", 0, property_flags::IS_RETURN) {
            Some(p) => {
                self.return_type = p;
                true
            }
            None => false,
        }
    }

    /// `AddParam`: false when the type is not (yet) registered or the engine refuses memory.
    pub fn add_param(&mut self, rtti: &RttiSystem, type_name: &CStr, name: &CStr, out: bool, optional: bool) -> bool {
        match rtti.get_type(CName::add(type_name)) {
            Some(ty) => self.add_param_of(ty, name, out, optional),
            None => false,
        }
    }

    /// `AddParam` once the type is found: a parameter property appended.
    pub fn add_param_of(&mut self, ty: &IType, name: &CStr, out: bool, optional: bool) -> bool {
        let mut flags = 0;
        if out {
            flags |= property_flags::IS_OUT;
        }
        if optional {
            flags |= property_flags::IS_OPTIONAL;
        }
        let offset = self.unk_ac as u32;
        match Property::create(ty as *const IType as *mut IType, name, offset, flags) {
            Some(p) => self.params.push(p),
            None => false,
        }
    }

    /// What `CBaseFunction::Execute(CStack*)` calls: `bool (CBaseFunction*, CStack*)` for a native, `bool
    /// (CBaseFunction*, CStack*, void*)` (null third argument) for a scripted one. The address, for a caller that
    /// makes the call itself (under SEH: cvr_engine::code::guarded_call3).
    pub fn execute_address(&self) -> Option<usize> {
        if self.flags & function_flags::IS_NATIVE != 0 {
            EXECUTE_NATIVE.get()
        } else {
            EXECUTE_SCRIPTED.get()
        }
    }

    /// `GetRegIndex` (vtable +0x18).
    pub fn reg_index(&self) -> u32 {
        type F = unsafe extern "C" fn(*const BaseFunction) -> u32;
        // SAFETY: every IFunction has GetRegIndex at +0x18.
        unsafe { crate::rtti::vfunc::<F>(self as *const _ as *const c_void, 0x18)(self) }
    }

    pub fn param_types(&self) -> impl Iterator<Item = Option<&'static IType>> + '_ {
        self.params.as_slice().iter().map(|&p| unsafe { p.as_ref().and_then(|p| p.ty.as_ref()) })
    }

    pub fn return_type(&self) -> Option<&'static IType> {
        unsafe { self.return_type.as_ref().and_then(|p| p.ty.as_ref()) }
    }
}

/// `CClassFunction`: a class's member function.
#[repr(C)]
pub struct ClassFunction {
    pub base: BaseFunction,
    pub parent: *const crate::rtti::Class,
    pub reg_index: u32,
}
assert_layout!(ClassFunction, size = 0x100, parent = 0xF0, reg_index = 0xF8);

static CLASS_CTOR: Reloc = Reloc::new(hashes::CClassFunction_ctor);

impl ClassFunction {
    /// `CClassFunction::Create(parent, fullName, shortName, native, flags)`: allocated from PoolRTTIFunction and
    /// constructed by the engine. `None` when the constructor or the pool cannot be resolved.
    pub fn create(
        parent: *const crate::rtti::Class,
        full_name: &CStr,
        short_name: &CStr,
        native: NativeFn,
        flags: u32,
    ) -> Option<&'static mut ClassFunction> {
        type Ctor = unsafe extern "C" fn(*mut ClassFunction, *const crate::rtti::Class, CName, CName, NativeFn, u32) -> *mut ClassFunction;
        // SAFETY: CClassFunction_ctor's signature per Functions.hpp (Flags is one 32-bit word, passed by value).
        let ctor = unsafe { CLASS_CTOR.func::<Ctor>()? };
        let memory = POOL_RTTI_FUNCTION.alloc(core::mem::size_of::<ClassFunction>())?.memory as *mut ClassFunction;
        let full = CName::add(full_name);
        let short = CName::add(short_name);
        // SAFETY: fresh memory from PoolRTTIFunction; the engine constructs the object in place.
        unsafe {
            ctor(memory, parent, full, short, native, flags);
            memory.as_mut()
        }
    }
}

impl BaseFunction {
    /// What a function made to stand in for `real` takes from it, as src/Natives/ActionReads.cpp's swap did: the
    /// names, the return, the parameters and locals (the same property objects), unk20, unkAC and the compiled code's
    /// bytes. unk48 is a map the SDK copies entry by entry; an empty one copies as nothing, so only that is taken
    /// (false, with nothing changed, when `real`'s holds entries).
    ///
    /// # Safety
    /// Both are live functions; `self` was just constructed (its arrays empty).
    pub unsafe fn take_signature(&mut self, real: &BaseFunction) -> bool {
        // HashMap<uint64_t, void*>::size at +0x08.
        let map_size = u32::from_le_bytes([real._unk48[8], real._unk48[9], real._unk48[10], real._unk48[11]]);
        if map_size != 0 {
            return false;
        }
        self.full_name = real.full_name;
        self.short_name = real.short_name;
        self.return_type = real.return_type;
        self.params.clear();
        for &p in real.params.as_slice() {
            if !self.params.push(p) {
                return false;
            }
        }
        self.local_vars.clear();
        for &p in real.local_vars.as_slice() {
            if !self.local_vars.push(p) {
                return false;
            }
        }
        self._unk20 = real._unk20;
        self.unk_ac = real.unk_ac;
        self._bytecode = real._bytecode;
        true
    }
}

impl GlobalFunction {
    /// `CGlobalFunction::Create`: a global script function calling `native`, flags still zero (set them before
    /// registering). `None` when the engine's constructor or pool cannot be resolved.
    pub fn create(full_name: &CStr, short_name: &CStr, native: NativeFn) -> Option<&'static mut GlobalFunction> {
        type Ctor = unsafe extern "C" fn(*mut GlobalFunction, CName, CName, NativeFn) -> *mut GlobalFunction;
        // SAFETY: CGlobalFunction_ctor's signature per Functions.hpp.
        let ctor = unsafe { GLOBAL_CTOR.func::<Ctor>()? };
        let memory = POOL_RTTI_FUNCTION.alloc(core::mem::size_of::<GlobalFunction>())?.memory as *mut GlobalFunction;
        let full = CName::add(full_name);
        let short = CName::add(short_name);
        // SAFETY: fresh memory from PoolRTTIFunction; the engine constructs the object in place.
        unsafe {
            ctor(memory, full, short, native);
            memory.as_mut()
        }
    }
}
