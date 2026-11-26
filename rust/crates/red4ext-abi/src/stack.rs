//! A native's view of the script VM (`Scripting/Stack.hpp`, `Utils.hpp`): its arguments are bytecode, each one
//! evaluated by running the opcode at `code` through the engine's opcode table, into the native's own variable.
use crate::assert_layout;
use crate::hashes;
use crate::reloc::Reloc;
use crate::cname::CName;
use crate::rtti::{Class, IType, RttiSystem};
use core::ffi::c_void;

#[repr(C)]
pub struct StackFrame {
    pub code: *mut u8,
    pub func: *mut c_void,
    pub local_vars: *mut c_void,
    pub params: *mut c_void,
    _unk20: i64,
    _unk28: i64,
    pub data: *mut c_void,
    pub data_type: *mut IType,
    pub context: *mut c_void,
    pub parent: *mut StackFrame,
    _unk50: i16,
    _unk58: i64,
    pub param_flags: u16,
    pub current_param: u8,
    pub use_direct_data: bool,
    pub is_root: bool,
    _unk65: [u8; 3],
    _unk68: [u8; 0x30],
    _unk98: *mut c_void,
    _unk_a0: u8,
}
assert_layout!(StackFrame, size = 0xA8, data = 0x30, data_type = 0x38, context = 0x40, parent = 0x48,
    param_flags = 0x60, current_param = 0x62, use_direct_data = 0x63, is_root = 0x64, _unk98 = 0x98);

type OpcodeHandler = unsafe extern "C" fn(*mut c_void, *mut StackFrame, *mut c_void, *mut c_void);
static OPCODES: Reloc = Reloc::new(hashes::OpcodeHandlers);

fn handler(opcode: u8) -> Option<OpcodeHandler> {
    let table = OPCODES.get()? as *const Option<OpcodeHandler>;
    // SAFETY: OpcodeHandlers is the engine's 256-entry handler table.
    unsafe { *table.add(opcode as usize) }
}

impl StackFrame {
    /// `GetParameter` for a value parameter: the next argument evaluated into `*out`, which must be a valid,
    /// initialised value of the parameter's script type (the opcode assigns into it).
    ///
    /// # Safety
    /// The frame must be the one the VM passed to the running native, positioned at a parameter whose script type
    /// matches `T`.
    pub unsafe fn get_parameter<T>(&mut self, out: *mut T) {
        self.data = core::ptr::null_mut();
        self.data_type = core::ptr::null_mut();
        self.current_param = self.current_param.wrapping_add(1);
        let opcode = unsafe { *self.code };
        self.code = unsafe { self.code.add(1) };
        if let Some(h) = handler(opcode) {
            unsafe { h(self.context, self, out as *mut c_void, core::ptr::null_mut()) };
        }
    }

    /// `GetParameter` with the opcode's fourth argument: a script reference's own storage (CET's
    /// `OpcodeHandlers::Run(opcode, context, frame, instance, isScriptRef ? instance : nullptr)`).
    ///
    /// # Safety
    /// As `get_parameter`; `out` is a constructed value of the parameter's type.
    pub unsafe fn get_parameter_with(&mut self, out: *mut c_void, a4: *mut c_void) {
        self.data = core::ptr::null_mut();
        self.data_type = core::ptr::null_mut();
        self.current_param = self.current_param.wrapping_add(1);
        let opcode = unsafe { *self.code };
        self.code = unsafe { self.code.add(1) };
        if let Some(h) = handler(opcode) {
            unsafe { h(self.context, self, out, a4) };
        }
    }

    /// `GetParameter` for a pointer parameter: the VM's own instance instead of a copy, when it has one.
    ///
    /// # Safety
    /// As `get_parameter`, for a parameter passed by pointer.
    pub unsafe fn get_parameter_direct<T>(&mut self, out: *mut *mut T) {
        self.use_direct_data = true;
        unsafe { self.get_parameter(out) };
        self.use_direct_data = false;
        if !self.data.is_null() {
            unsafe { *out = self.data as *mut T };
        }
    }

    /// Past the ParamEnd opcode: every native does this after reading its arguments.
    pub fn finish_params(&mut self) {
        self.code = unsafe { self.code.add(1) };
    }
}

/// `CStackType`: a value and its type, for a call's arguments and result.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct StackType {
    pub ty: *const IType,
    pub value: *mut c_void,
}
assert_layout!(StackType, size = 0x10);

/// `CStack`: a call made from native code, its arguments already values (`CBaseFunction::Execute` takes it).
#[repr(C)]
pub struct CStack {
    vtable: usize,
    _unk08: i64,
    _unk10: *mut c_void,
    context18: *mut c_void,
    context20: *mut c_void,
    _unk28: *const Class,
    pub args: *mut StackType,
    pub args_count: u32,
    pub result: *mut StackType,
    _pad48: [u8; 0x10],
}
assert_layout!(CStack, size = 0x58, context18 = 0x18, _unk28 = 0x28, args = 0x30, args_count = 0x38, result = 0x40);

static CSTACK_VTBL: Reloc = Reloc::new(hashes::CStack_vtbl);

static POOL_SCRIPT_VTBL: Reloc = Reloc::new(hashes::Memory_PoolScript_vtbl);
static FRAME_DTOR: Reloc = Reloc::new(hashes::CStackFrame_dtor);

impl StackFrame {
    /// `CStackFrame(context, code)` (Stack-inl.hpp) built in place: a root frame over `code`, its call map initialised
    /// as the game's constructor does and pointing at itself -- so the frame must not move until `release_root`.
    /// False when the call map's vtable cannot be resolved.
    ///
    /// # Safety
    /// `this` is writable memory for a StackFrame that stays where it is; `code` outlives the frame.
    pub unsafe fn init_root(this: *mut StackFrame, context: *mut c_void, code: *mut u8) -> bool {
        let Some(pool) = POOL_SCRIPT_VTBL.get() else { return false };
        // SAFETY: the caller's memory, written whole before anything reads it.
        unsafe {
            core::ptr::write_bytes(this, 0, 1);
            (*this).code = code;
            (*this).context = context;
            (*this).is_root = true;
            let map = core::ptr::addr_of_mut!((*this)._unk68) as *mut u8;
            core::ptr::write_unaligned(map.add(0x20) as *mut u32, u32::MAX);
            core::ptr::write_unaligned(map.add(0x28) as *mut usize, pool);
            (*this)._unk98 = map.cast();
        }
        true
    }
    /// `~CStackFrame()` of a frame made by `init_root`.
    ///
    /// # Safety
    /// `this` was made by `init_root` and has not moved.
    pub unsafe fn release_root(this: *mut StackFrame) {
        type Dtor = unsafe extern "C" fn(*mut StackFrame);
        // SAFETY: CStackFrame_dtor's signature per Stack-inl.hpp.
        if let Some(dtor) = unsafe { FRAME_DTOR.func::<Dtor>() } {
            unsafe { dtor(this) };
        }
    }
}

impl CStack {
    /// `CStack(context, args, count, result)`: CBaseStack's fields as its constructor sets them (the IScriptable
    /// class looked up by name), then CStack's vtable. `None` when the vtable cannot be resolved.
    pub fn new(rtti: &RttiSystem, context: *mut c_void, args: &mut [StackType], result: *mut StackType) -> Option<CStack> {
        Some(CStack {
            vtable: CSTACK_VTBL.get()?,
            _unk08: 0,
            _unk10: core::ptr::null_mut(),
            context18: context,
            context20: context,
            _unk28: rtti.get_class(CName::hashed("IScriptable")).map_or(core::ptr::null(), |c| c as *const Class),
            args: if args.is_empty() { core::ptr::null_mut() } else { args.as_mut_ptr() },
            args_count: args.len() as u32,
            result,
            _pad48: [0; 0x10],
        })
    }
}
