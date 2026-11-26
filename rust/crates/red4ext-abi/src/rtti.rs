//! The engine's RTTI: `CRTTISystem` (`RTTISystem.hpp`) and the `rtti::IType` interface (`RTTI/IType.hpp`), called
//! through their vtables. Virtual member functions returning a struct (IType::GetName) return it through a hidden
//! pointer after `this`, as MSVC does for every member function returning a class.
use crate::callback::VoidCallback;
use crate::cname::CName;
use crate::dynarray::DynArray;
use crate::function::{BaseFunction, GlobalFunction};
use crate::hashes;
use crate::reloc::Reloc;
use core::ffi::c_void;

/// rtti::IType, opaque: every call goes through its vtable.
#[repr(C)]
pub struct IType {
    vtable: *const usize,
    /// The engine changes the object behind every reference (a class grows its functions, the registry its types):
    /// interior mutability, so a reference never promises the optimizer the memory stays put. No size.
    _engine_owned: core::cell::UnsafeCell<()>,
}

/// RED4ext::CClass, opaque here: its fields are read at their offsets (RTTITypes.hpp).
#[repr(C)]
pub struct Class {
    vtable: *const usize,
    /// The engine changes the object behind every reference (a class grows its functions, the registry its types):
    /// interior mutability, so a reference never promises the optimizer the memory stays put. No size.
    _engine_owned: core::cell::UnsafeCell<()>,
}

static CLASS_GET_PROPERTY: Reloc = Reloc::new(hashes::CClass_GetProperty);
static CLASS_INITIALIZE_PROPERTIES: Reloc = Reloc::new(hashes::CClass_InitializeProperties);
static CLASS_ASSIGN_DEFAULT_VALUES: Reloc = Reloc::new(hashes::CClass_AssignDefaultValuesToProperties);

static CLASS_CREATE_INSTANCE: Reloc = Reloc::new(hashes::CClass_CreateInstance);
static CLASS_GET_PROPERTIES_LIST: Reloc = Reloc::new(hashes::CClass_GetProperties);

impl Class {
    /// `CClass::CreateInstance()`: a new instance of the class (its size, constructed and defaulted), owned by nobody
    /// until a Handle takes it (`Handle::from_instance`). The SDK's default, and CET's `Class.new()`: no second zeroing
    /// (the class allocator's memory is zeroed already).
    pub fn create_instance(&self) -> *mut c_void {
        type F = unsafe extern "C" fn(*const Class, u32, bool) -> *mut c_void;
        // SAFETY: CClass_CreateInstance's signature per RTTITypes-inl.hpp (size, zeroing).
        match unsafe { CLASS_CREATE_INSTANCE.func::<F>() } {
            Some(f) => {
                let size = unsafe { (*(self as *const Class as *const IType)).size() };
                unsafe { f(self, size, false) }
            }
            None => core::ptr::null_mut(),
        }
    }
    /// [`Class::property`] by the name the property is written with (its "m_" prefix, if any, dropped as
    /// `CProperty::Create` dropped it: `crate::function::registered_name`).
    pub fn property_named(&self, name: &str) -> Option<&'static crate::function::Property> {
        self.property(CName::hashed(crate::function::registered_name(name)))
    }
    /// `CClass::GetProperty(name)`: the engine's own lookup (overridden properties and the parents included).
    pub fn property(&self, name: CName) -> Option<&'static crate::function::Property> {
        type F = unsafe extern "C" fn(*const Class, CName) -> *const crate::function::Property;
        // SAFETY: CClass_GetProperty's signature per RTTITypes-inl.hpp.
        let f = unsafe { CLASS_GET_PROPERTY.func::<F>() }?;
        unsafe { f(self, name).as_ref() }
    }
    /// `CClass::InitializeProperties(instance)`: the instance's value holder made and filled with the defaults.
    ///
    /// # Safety
    /// `instance` is a live instance of this class.
    pub unsafe fn initialize_properties(&self, instance: *mut c_void) {
        type F = unsafe extern "C" fn(*const Class, *mut c_void);
        // SAFETY: the two engine functions the SDK calls, in its order.
        unsafe {
            if let Some(f) = CLASS_INITIALIZE_PROPERTIES.func::<F>() {
                f(self, instance);
            }
            if let Some(f) = CLASS_ASSIGN_DEFAULT_VALUES.func::<F>() {
                f(self, instance);
            }
        }
    }
    fn field<T: Copy>(&self, at: usize) -> T {
        // SAFETY: a live CClass is at least 0x80 bytes; `at` is one of its fields' offsets.
        unsafe { core::ptr::read((self as *const Class as *const u8).add(at) as *const T) }
    }
    /// `CClass::GetProperties`: the class's properties as the engine lists them (its own and its parents').
    pub fn properties(&self) -> Vec<&'static crate::function::Property> {
        type F = unsafe extern "C" fn(*const Class, *mut DynArray<*const crate::function::Property>) -> *const c_void;
        let mut list = DynArray::<*const crate::function::Property>::default();
        // SAFETY: CClass_GetProperties's signature per RTTITypes-inl.hpp; the array is the engine's to fill.
        let Some(f) = (unsafe { CLASS_GET_PROPERTIES_LIST.func::<F>() }) else { return Vec::new() };
        unsafe { f(self, &mut list) };
        // SAFETY: registered properties, alive as long as their class.
        let out = list.as_slice().iter().filter_map(|&p| unsafe { p.as_ref() }).collect();
        list.free();
        out
    }
    /// `props` (+0x28): the class's own properties, not its parents'.
    pub fn own_properties(&self) -> Vec<&'static crate::function::Property> {
        // SAFETY: a live class's own property array (DynArray<CProperty*>).
        let list: &DynArray<*const crate::function::Property> = unsafe { &*((self as *const Class as *const u8).add(0x28) as *const DynArray<*const crate::function::Property>) };
        // SAFETY: registered properties, alive as long as their class.
        list.as_slice().iter().filter_map(|&p| unsafe { p.as_ref() }).collect()
    }
    /// `parent` (+0x10).
    pub fn parent(&self) -> Option<&'static Class> {
        // SAFETY: a class's parent is a registered class or null.
        unsafe { self.field::<*const Class>(0x10).as_ref() }
    }
    /// `name` (+0x18).
    pub fn class_name(&self) -> CName {
        self.field(0x18)
    }
    /// Natives/Natives.cpp ClassIsA: this class or one of its ancestors is named `name`.
    pub fn is_a(&self, name: CName) -> bool {
        let mut class = Some(self);
        while let Some(c) = class {
            if c.class_name() == name {
                return true;
            }
            class = c.parent();
        }
        false
    }
    fn functions(&self, at: usize) -> &[*mut BaseFunction] {
        // SAFETY: a DynArray<CClassFunction*> of the class, alive as long as the class.
        unsafe { (*((self as *const Class as *const u8).add(at) as *const DynArray<*mut BaseFunction>)).as_slice() }
    }
    /// `funcs` (+0x48): the member functions.
    pub fn funcs(&self) -> &[*mut BaseFunction] {
        self.functions(0x48)
    }
    /// `staticFuncs` (+0x58).
    pub fn static_funcs(&self) -> &[*mut BaseFunction] {
        self.functions(0x58)
    }
    /// `CClass::GetFunction(CName)`: a static, then a member function of this class by short name, then the parent's.
    pub fn function(&self, short_name: CName) -> Option<&'static BaseFunction> {
        let own = self.static_funcs().iter().chain(self.funcs().iter());
        // SAFETY: the class's function pointers are registered functions or null.
        if let Some(f) = own.filter_map(|&f| unsafe { f.as_ref() }).find(|f| f.short_name == short_name) {
            return Some(f);
        }
        self.parent().and_then(|p| p.function(short_name))
    }
}

/// `ISerializable::GetType` (vtable +0x08): the class of a live scriptable object.
///
/// # Safety
/// `object` must be a live ISerializable.
pub unsafe fn object_class(object: *const c_void) -> Option<&'static Class> {
    type F = unsafe extern "C" fn(*const c_void) -> *const Class;
    unsafe { vfunc::<F>(object, 0x08)(object).as_ref() }
}

/// rtti::ERTTIType.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RttiKind {
    Name = 0,
    Fundamental = 1,
    Class = 2,
    Array = 3,
    Simple = 4,
    Enum = 5,
    StaticArray = 6,
    NativeArray = 7,
    Pointer = 8,
    Handle = 9,
    WeakHandle = 10,
    ResourceReference = 11,
    ResourceAsyncReference = 12,
    BitField = 13,
    LegacySingleChannelCurve = 14,
    ScriptReference = 15,
    FixedArray = 16,
}

/// Slot `offset` (bytes) of `obj`'s vtable as a function pointer of type `F`.
///
/// # Safety
/// `obj` must be a live engine object whose vtable has a function of type `F` at `offset`.
#[inline]
pub unsafe fn vfunc<F: Copy>(obj: *const c_void, offset: usize) -> F {
    const { assert!(core::mem::size_of::<F>() == 8) };
    unsafe {
        let vtable = *(obj as *const *const usize);
        core::mem::transmute_copy::<usize, F>(&*vtable.add(offset / 8))
    }
}

impl IType {
    pub fn name(&self) -> CName {
        type F = unsafe extern "C" fn(*const IType, *mut CName) -> *mut CName;
        let mut out = CName(0);
        // SAFETY: IType::GetName, vtable +0x08, member returning CName through a hidden pointer.
        unsafe { vfunc::<F>(self as *const _ as _, 0x08)(self, &mut out) };
        out
    }
    /// `IType::GetComputedName` (+0x30): the name with its inner types spelled out (`array:handle:entEntity`).
    pub fn computed_name(&self) -> CName {
        type F = unsafe extern "C" fn(*const IType, *mut CName) -> *mut CName;
        let mut out = CName(0);
        // SAFETY: a member returning CName through a hidden pointer, as GetName.
        unsafe { vfunc::<F>(self as *const _ as _, 0x30)(self, &mut out) };
        out
    }
    pub fn size(&self) -> u32 {
        type F = unsafe extern "C" fn(*const IType) -> u32;
        unsafe { vfunc::<F>(self as *const _ as _, 0x10)(self) }
    }
    pub fn alignment(&self) -> u32 {
        type F = unsafe extern "C" fn(*const IType) -> u32;
        unsafe { vfunc::<F>(self as *const _ as _, 0x18)(self) }
    }
    pub fn kind(&self) -> u8 {
        type F = unsafe extern "C" fn(*const IType) -> u8;
        unsafe { vfunc::<F>(self as *const _ as _, 0x20)(self) }
    }
    /// `IType::Assign(lhs, rhs)` (+0x50): the type's own copy assignment of one value onto another.
    ///
    /// # Safety
    /// Both point at live values of this type.
    pub unsafe fn assign(&self, lhs: *mut c_void, rhs: *const c_void) {
        type F = unsafe extern "C" fn(*const IType, *mut c_void, *const c_void);
        unsafe { vfunc::<F>(self as *const _ as _, 0x50)(self, lhs, rhs) }
    }
    /// `IType::Construct(memory)` (+0x38): a default value of this type in `memory`.
    ///
    /// # Safety
    /// `memory` holds `size()` bytes aligned to `alignment()`.
    pub unsafe fn construct(&self, memory: *mut c_void) {
        type F = unsafe extern "C" fn(*const IType, *mut c_void);
        unsafe { vfunc::<F>(self as *const _ as _, 0x38)(self, memory) }
    }
    /// `IType::Destruct(memory)` (+0x40).
    ///
    /// # Safety
    /// `memory` holds a live value of this type.
    pub unsafe fn destruct(&self, memory: *mut c_void) {
        type F = unsafe extern "C" fn(*const IType, *mut c_void);
        unsafe { vfunc::<F>(self as *const _ as _, 0x40)(self, memory) }
    }
    /// `IType::IsEqual(lhs, rhs, 0)` (+0x48).
    ///
    /// # Safety
    /// Both point at live values of this type.
    pub unsafe fn is_equal(&self, lhs: *const c_void, rhs: *const c_void) -> bool {
        type F = unsafe extern "C" fn(*const IType, *const c_void, *const c_void, u32) -> bool;
        unsafe { vfunc::<F>(self as *const _ as _, 0x48)(self, lhs, rhs, 0) }
    }
    /// `IType::FromString(instance, text)` (+0x70): the value parsed from its text form.
    ///
    /// # Safety
    /// `instance` holds a live value of this type.
    pub unsafe fn from_string(&self, instance: *mut c_void, text: &crate::cstring::CString) -> bool {
        type F = unsafe extern "C" fn(*const IType, *mut c_void, *const crate::cstring::CString) -> bool;
        unsafe { vfunc::<F>(self as *const _ as _, 0x70)(self, instance, text) }
    }
    // ---- CRTTIBaseArrayType's members (an array type's vtable continues the IType's)
    /// `GetInnerType` (+0xC0).
    ///
    /// # Safety
    /// This is an array type.
    pub unsafe fn array_inner(&self) -> *mut IType {
        type F = unsafe extern "C" fn(*const IType) -> *mut IType;
        unsafe { vfunc::<F>(self as *const _ as _, 0xC0)(self) }
    }
    /// `GetLength(instance)` (+0xD0).
    ///
    /// # Safety
    /// This is an array type and `instance` one of its values.
    pub unsafe fn array_length(&self, instance: *mut c_void) -> u32 {
        type F = unsafe extern "C" fn(*const IType, *mut c_void) -> u32;
        unsafe { vfunc::<F>(self as *const _ as _, 0xD0)(self, instance) }
    }
    /// `GetElement(instance, index)` (+0xE0).
    ///
    /// # Safety
    /// This is an array type and `instance` one of its values.
    pub unsafe fn array_element(&self, instance: *mut c_void, index: u32) -> *mut c_void {
        type F = unsafe extern "C" fn(*const IType, *mut c_void, u32) -> *mut c_void;
        unsafe { vfunc::<F>(self as *const _ as _, 0xE0)(self, instance, index) }
    }
    /// `RemoveAt(instance, index)` (+0xF8).
    ///
    /// # Safety
    /// This is an array type and `instance` one of its values.
    pub unsafe fn array_remove_at(&self, instance: *mut c_void, index: i32) -> bool {
        type F = unsafe extern "C" fn(*const IType, *mut c_void, i32) -> bool;
        unsafe { vfunc::<F>(self as *const _ as _, 0xF8)(self, instance, index) }
    }
    /// `InsertAt(instance, index)` (+0x100).
    ///
    /// # Safety
    /// This is an array type and `instance` one of its values.
    pub unsafe fn array_insert_at(&self, instance: *mut c_void, index: i32) -> bool {
        type F = unsafe extern "C" fn(*const IType, *mut c_void, i32) -> bool;
        unsafe { vfunc::<F>(self as *const _ as _, 0x100)(self, instance, index) }
    }
    /// `Resize(instance, size)` (+0x108).
    ///
    /// # Safety
    /// This is an array type and `instance` one of its values.
    pub unsafe fn array_resize(&self, instance: *mut c_void, size: u32) -> bool {
        type F = unsafe extern "C" fn(*const IType, *mut c_void, u32) -> bool;
        unsafe { vfunc::<F>(self as *const _ as _, 0x108)(self, instance, size) }
    }
}

/// RED4ext::CRTTISystem: the process's single type registry, alive from before plugins load until exit.
#[repr(C)]
pub struct RttiSystem {
    vtable: *const usize,
    /// The engine changes the object behind every reference (a class grows its functions, the registry its types):
    /// interior mutability, so a reference never promises the optimizer the memory stays put. No size.
    _engine_owned: core::cell::UnsafeCell<()>,
}

static GET: Reloc = Reloc::new(hashes::CRTTISystem_Get);

impl RttiSystem {
    /// `CRTTISystem::Get()`. `None` when RED4ext cannot resolve it (unknown build).
    pub fn get() -> Option<&'static RttiSystem> {
        type F = unsafe extern "C" fn() -> *mut RttiSystem;
        // SAFETY: CRTTISystem_Get takes nothing and returns the registry (never freed).
        let f = unsafe { GET.func::<F>()? };
        let p = unsafe { f() };
        unsafe { p.as_ref() }
    }

    fn this(&self) -> *const c_void {
        self as *const _ as *const c_void
    }

    /// `GetType` (+0x00): any type by name, including composite names ("handle:entEntity"), made on first request.
    pub fn get_type(&self, name: CName) -> Option<&'static IType> {
        type F = unsafe extern "C" fn(*const c_void, CName) -> *mut IType;
        unsafe { vfunc::<F>(self.this(), 0x00)(self.this(), name).as_ref() }
    }

    /// `GetClass` (+0x10).
    pub fn get_class(&self, name: CName) -> Option<&'static Class> {
        type F = unsafe extern "C" fn(*const c_void, CName) -> *mut Class;
        unsafe { vfunc::<F>(self.this(), 0x10)(self.this(), name).as_ref() }
    }

    /// `GetClassByScriptName` (+0x108): a class by its script name.
    pub fn get_class_by_script_name(&self, name: CName) -> Option<&'static Class> {
        type F = unsafe extern "C" fn(*const c_void, CName) -> *mut Class;
        unsafe { vfunc::<F>(self.this(), 0x108)(self.this(), name).as_ref() }
    }

    /// `ConvertNativeToScriptName` (+0x118): the name the scripts know a native class by (the same name for another).
    pub fn native_to_script_name(&self, name: CName) -> CName {
        type F = unsafe extern "C" fn(*const c_void, *mut CName, CName) -> *mut CName;
        let mut out = CName(0);
        // SAFETY: the registry's slot +0x118; a returned CName comes back through the hidden pointer after `this`.
        unsafe { vfunc::<F>(self.this(), 0x118)(self.this(), &mut out, name) };
        out
    }

    /// `GetFunction` (+0x30): a global function by full name.
    pub fn get_function(&self, name: CName) -> Option<&'static GlobalFunction> {
        type F = unsafe extern "C" fn(*const c_void, CName) -> *mut GlobalFunction;
        unsafe { vfunc::<F>(self.this(), 0x30)(self.this(), name).as_ref() }
    }

    /// `GetGlobalFunctions` (+0x48): every global function, natives and scripted.
    pub fn global_functions(&self) -> Vec<&'static BaseFunction> {
        type F = unsafe extern "C" fn(*const c_void, *mut DynArray<*mut BaseFunction>);
        let mut list = DynArray::<*mut BaseFunction>::default();
        unsafe { vfunc::<F>(self.this(), 0x48)(self.this(), &mut list) };
        let out = list.as_slice().iter().filter_map(|&f| unsafe { f.as_ref() }).collect();
        list.free();
        out
    }

    /// `RegisterFunction` (+0xA0). The registry owns `f` from here on.
    ///
    /// # Safety
    /// `f` must be a fully constructed global function allocated from PoolRTTIFunction (`GlobalFunction::create`).
    pub unsafe fn register_function(&self, f: *mut GlobalFunction) {
        type F = unsafe extern "C" fn(*const c_void, *mut GlobalFunction);
        unsafe { vfunc::<F>(self.this(), 0xA0)(self.this(), f) };
    }

    /// `AddRegisterCallback` (+0xC0): `f` runs when the engine registers its native types -- the window for
    /// registering a plugin's types and natives.
    pub fn add_register_callback(&self, f: extern "C" fn()) {
        type F = unsafe extern "C" fn(*const c_void, *mut VoidCallback);
        let mut callback = VoidCallback::new(f);
        // The callee owns and destroys the by-value argument (MSVC ABI); `callback` is plain bytes afterwards.
        unsafe { vfunc::<F>(self.this(), 0xC0)(self.this(), &mut callback) };
    }

    /// `AddPostRegisterCallback` (+0xC8): `f` runs once every native type is registered.
    pub fn add_post_register_callback(&self, f: extern "C" fn()) {
        type F = unsafe extern "C" fn(*const c_void, *mut VoidCallback);
        let mut callback = VoidCallback::new(f);
        unsafe { vfunc::<F>(self.this(), 0xC8)(self.this(), &mut callback) };
    }
}

// The opaque objects keep their one word: the cell adds no size.
const _: () = assert!(core::mem::size_of::<Class>() == 8 && core::mem::size_of::<IType>() == 8 && core::mem::size_of::<RttiSystem>() == 8);
