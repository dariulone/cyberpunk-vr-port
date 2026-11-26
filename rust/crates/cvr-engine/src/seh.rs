//! The guarded accesses to engine memory (formerly native/seh.c; before it include/Utils/MemorySafe.hpp): a copy, a
//! compare-exchange, a call or an OS unwind step that may fault, reporting whether it completed. Rust has no
//! structured exception handling -- catch_unwind sees Rust panics, never an access violation -- so each function is
//! written here in assembly with the x64 SEH tables MSVC makes for `__try { ... } __except (EXCEPTION_EXECUTE_HANDLER)`:
//! the language handler `__C_specific_handler` and a scope table of one entry (the guarded range, the filter 1 =
//! EXCEPTION_EXECUTE_HANDLER, the target). A fault in the range -- or in a callee whose return address is in it -- is
//! dispatched to the handler, which unwinds to the target in this same frame; the function then returns 0.
//!
//! The rules the C adapter kept are kept: nothing but the engine's or the OS's own frames runs inside a guarded range,
//! so no Rust frame is ever unwound by an access violation. Each guarded call is followed by a `nop` inside the range,
//! so the call's return address (the caller frame's ControlPc during dispatch) lies in it. The ranges are half-open,
//! [begin, end), as __C_specific_handler compares them.
//!
//! The symbols and their C ABI are seh.c's: code.rs, stack.rs and cvr-render's queue_residency.rs declare them.

core::arch::global_asm!(
    // ---- int cvr_seh_copy_from(void *dst, const void *src, size_t n) / cvr_seh_copy_to: n bytes, 1 when all arrived
    ".globl cvr_seh_copy_from",
    ".def cvr_seh_copy_from",
    ".scl 2",
    ".type 32",
    ".endef",
    "cvr_seh_copy_from:",
    ".seh_proc cvr_seh_copy_from",
    ".seh_handler __C_specific_handler, @unwind, @except",
    "    push rsi",
    ".seh_pushreg rsi",
    "    push rdi",
    ".seh_pushreg rdi",
    "    sub rsp, 40",
    ".seh_stackalloc 40",
    ".seh_endprologue",
    "    mov rdi, rcx",
    "    mov rsi, rdx",
    "    mov rcx, r8",
    ".Lseh_copy_from_begin:",
    "    rep movsb",
    "    nop",
    ".Lseh_copy_from_end:",
    "    mov eax, 1",
    ".Lseh_copy_from_out:",
    "    add rsp, 40",
    "    pop rdi",
    "    pop rsi",
    "    ret",
    ".Lseh_copy_from_except:",
    "    xor eax, eax",
    "    jmp .Lseh_copy_from_out",
    ".seh_handlerdata",
    "    .long 1",
    "    .long .Lseh_copy_from_begin@IMGREL",
    "    .long .Lseh_copy_from_end@IMGREL",
    "    .long 1",
    "    .long .Lseh_copy_from_except@IMGREL",
    ".text",
    ".seh_endproc",
    //
    ".globl cvr_seh_copy_to",
    ".def cvr_seh_copy_to",
    ".scl 2",
    ".type 32",
    ".endef",
    "cvr_seh_copy_to:",
    ".seh_proc cvr_seh_copy_to",
    ".seh_handler __C_specific_handler, @unwind, @except",
    "    push rsi",
    ".seh_pushreg rsi",
    "    push rdi",
    ".seh_pushreg rdi",
    "    sub rsp, 40",
    ".seh_stackalloc 40",
    ".seh_endprologue",
    "    mov rdi, rcx",
    "    mov rsi, rdx",
    "    mov rcx, r8",
    ".Lseh_copy_to_begin:",
    "    rep movsb",
    "    nop",
    ".Lseh_copy_to_end:",
    "    mov eax, 1",
    ".Lseh_copy_to_out:",
    "    add rsp, 40",
    "    pop rdi",
    "    pop rsi",
    "    ret",
    ".Lseh_copy_to_except:",
    "    xor eax, eax",
    "    jmp .Lseh_copy_to_out",
    ".seh_handlerdata",
    "    .long 1",
    "    .long .Lseh_copy_to_begin@IMGREL",
    "    .long .Lseh_copy_to_end@IMGREL",
    "    .long 1",
    "    .long .Lseh_copy_to_except@IMGREL",
    ".text",
    ".seh_endproc",
    // ---- int cvr_seh_call3(void *fn, void *a, void *b, void *c, unsigned char *result): bool fn(a, b, c)
    ".globl cvr_seh_call3",
    ".def cvr_seh_call3",
    ".scl 2",
    ".type 32",
    ".endef",
    "cvr_seh_call3:",
    ".seh_proc cvr_seh_call3",
    ".seh_handler __C_specific_handler, @unwind, @except",
    "    push rbx",
    ".seh_pushreg rbx",
    "    sub rsp, 32",
    ".seh_stackalloc 32",
    ".seh_endprologue",
    // the fifth argument (result) at the entry's rsp + 40, now rsp + 40 + 40
    "    mov rbx, qword ptr [rsp + 80]",
    "    mov rax, rcx",
    "    mov rcx, rdx",
    "    mov rdx, r8",
    "    mov r8, r9",
    ".Lseh_call3_begin:",
    "    call rax",
    "    nop",
    ".Lseh_call3_end:",
    "    mov byte ptr [rbx], al",
    "    mov eax, 1",
    ".Lseh_call3_out:",
    "    add rsp, 32",
    "    pop rbx",
    "    ret",
    ".Lseh_call3_except:",
    "    xor eax, eax",
    "    jmp .Lseh_call3_out",
    ".seh_handlerdata",
    "    .long 1",
    "    .long .Lseh_call3_begin@IMGREL",
    "    .long .Lseh_call3_end@IMGREL",
    "    .long 1",
    "    .long .Lseh_call3_except@IMGREL",
    ".text",
    ".seh_endproc",
    // ---- int cvr_seh_call4p(void *fn, void *a, void *b, void *c, void *d, void **result): void *fn(a, b, c, d)
    ".globl cvr_seh_call4p",
    ".def cvr_seh_call4p",
    ".scl 2",
    ".type 32",
    ".endef",
    "cvr_seh_call4p:",
    ".seh_proc cvr_seh_call4p",
    ".seh_handler __C_specific_handler, @unwind, @except",
    "    push rbx",
    ".seh_pushreg rbx",
    "    sub rsp, 32",
    ".seh_stackalloc 32",
    ".seh_endprologue",
    // d at the entry's rsp + 40 and result at + 48: now rsp + 80 and rsp + 88
    "    mov rbx, qword ptr [rsp + 88]",
    "    mov rax, rcx",
    "    mov rcx, rdx",
    "    mov rdx, r8",
    "    mov r8, r9",
    "    mov r9, qword ptr [rsp + 80]",
    ".Lseh_call4p_begin:",
    "    call rax",
    "    nop",
    ".Lseh_call4p_end:",
    "    mov qword ptr [rbx], rax",
    "    mov eax, 1",
    ".Lseh_call4p_out:",
    "    add rsp, 32",
    "    pop rbx",
    "    ret",
    ".Lseh_call4p_except:",
    "    xor eax, eax",
    "    jmp .Lseh_call4p_out",
    ".seh_handlerdata",
    "    .long 1",
    "    .long .Lseh_call4p_begin@IMGREL",
    "    .long .Lseh_call4p_end@IMGREL",
    "    .long 1",
    "    .long .Lseh_call4p_except@IMGREL",
    ".text",
    ".seh_endproc",
    // ---- int cvr_seh_call6p(void *fn, void *a, void *b, void *c, void *d, void *e, void *f, void **result):
    // void *fn(a, b, c, d, e, f) (PhysX's PxGeometryQuery::computePenetration, all six by address)
    ".globl cvr_seh_call6p",
    ".def cvr_seh_call6p",
    ".scl 2",
    ".type 32",
    ".endef",
    "cvr_seh_call6p:",
    ".seh_proc cvr_seh_call6p",
    ".seh_handler __C_specific_handler, @unwind, @except",
    "    push rbx",
    ".seh_pushreg rbx",
    "    sub rsp, 48",
    ".seh_stackalloc 48",
    ".seh_endprologue",
    // d, e, f and result at the entry's rsp + 40, 48, 56, 64: now rsp + 96, 104, 112, 120; e and f go to the callee's
    // stack arguments at rsp + 32 and + 40
    "    mov rbx, qword ptr [rsp + 120]",
    "    mov rax, qword ptr [rsp + 104]",
    "    mov qword ptr [rsp + 32], rax",
    "    mov rax, qword ptr [rsp + 112]",
    "    mov qword ptr [rsp + 40], rax",
    "    mov rax, rcx",
    "    mov rcx, rdx",
    "    mov rdx, r8",
    "    mov r8, r9",
    "    mov r9, qword ptr [rsp + 96]",
    ".Lseh_call6p_begin:",
    "    call rax",
    "    nop",
    ".Lseh_call6p_end:",
    "    mov qword ptr [rbx], rax",
    "    mov eax, 1",
    ".Lseh_call6p_out:",
    "    add rsp, 48",
    "    pop rbx",
    "    ret",
    ".Lseh_call6p_except:",
    "    xor eax, eax",
    "    jmp .Lseh_call6p_out",
    ".seh_handlerdata",
    "    .long 1",
    "    .long .Lseh_call6p_begin@IMGREL",
    "    .long .Lseh_call6p_end@IMGREL",
    "    .long 1",
    "    .long .Lseh_call6p_except@IMGREL",
    ".text",
    ".seh_endproc",
    // ---- int cvr_seh_call4x(void *fn, u64 a, u64 b, u64 c, u64 d, u64 *rax, u64 *xmm0): fn(a, b, c, d) with each
    // argument in both its integer and its float register (the callee reads the class its prototype has: a float's bits
    // in the low 32 of xmm), and both of its result registers kept (PhysX's setMass(float), getMass() -> float)
    ".globl cvr_seh_call4x",
    ".def cvr_seh_call4x",
    ".scl 2",
    ".type 32",
    ".endef",
    "cvr_seh_call4x:",
    ".seh_proc cvr_seh_call4x",
    ".seh_handler __C_specific_handler, @unwind, @except",
    "    push rbx",
    ".seh_pushreg rbx",
    "    push rsi",
    ".seh_pushreg rsi",
    "    sub rsp, 40",
    ".seh_stackalloc 40",
    ".seh_endprologue",
    // d, rax* and xmm0* at the entry's rsp + 40, 48, 56: now rsp + 96, 104, 112
    "    mov rbx, qword ptr [rsp + 104]",
    "    mov rsi, qword ptr [rsp + 112]",
    "    mov rax, rcx",
    "    mov rcx, rdx",
    "    mov rdx, r8",
    "    mov r8, r9",
    "    mov r9, qword ptr [rsp + 96]",
    "    movq xmm0, rcx",
    "    movq xmm1, rdx",
    "    movq xmm2, r8",
    "    movq xmm3, r9",
    ".Lseh_call4x_begin:",
    "    call rax",
    "    nop",
    ".Lseh_call4x_end:",
    "    mov qword ptr [rbx], rax",
    "    movq qword ptr [rsi], xmm0",
    "    mov eax, 1",
    ".Lseh_call4x_out:",
    "    add rsp, 40",
    "    pop rsi",
    "    pop rbx",
    "    ret",
    ".Lseh_call4x_except:",
    "    xor eax, eax",
    "    jmp .Lseh_call4x_out",
    ".seh_handlerdata",
    "    .long 1",
    "    .long .Lseh_call4x_begin@IMGREL",
    "    .long .Lseh_call4x_end@IMGREL",
    "    .long 1",
    "    .long .Lseh_call4x_except@IMGREL",
    ".text",
    ".seh_endproc",
    // ---- int cvr_seh_call10x(void *fn, const u64 *args, u64 *rax, u64 *xmm0): fn(args[0..10]) with the first four in
    // both their integer and their float registers and the other six on the stack (a float's bits in the low 32), both
    // result registers kept (PhysX's PxGeometryQuery::sweep: a direction, a float distance, four by address, the hit,
    // the flags by address, a float inflation)
    ".globl cvr_seh_call10x",
    ".def cvr_seh_call10x",
    ".scl 2",
    ".type 32",
    ".endef",
    "cvr_seh_call10x:",
    ".seh_proc cvr_seh_call10x",
    ".seh_handler __C_specific_handler, @unwind, @except",
    "    push rbx",
    ".seh_pushreg rbx",
    "    push rsi",
    ".seh_pushreg rsi",
    "    sub rsp, 88",
    ".seh_stackalloc 88",
    ".seh_endprologue",
    "    mov rbx, r8",
    "    mov rsi, r9",
    "    mov rax, rcx",
    "    mov r10, rdx",
    "    mov rcx, qword ptr [r10 + 32]",
    "    mov qword ptr [rsp + 32], rcx",
    "    mov rcx, qword ptr [r10 + 40]",
    "    mov qword ptr [rsp + 40], rcx",
    "    mov rcx, qword ptr [r10 + 48]",
    "    mov qword ptr [rsp + 48], rcx",
    "    mov rcx, qword ptr [r10 + 56]",
    "    mov qword ptr [rsp + 56], rcx",
    "    mov rcx, qword ptr [r10 + 64]",
    "    mov qword ptr [rsp + 64], rcx",
    "    mov rcx, qword ptr [r10 + 72]",
    "    mov qword ptr [rsp + 72], rcx",
    "    mov rcx, qword ptr [r10]",
    "    mov rdx, qword ptr [r10 + 8]",
    "    mov r8, qword ptr [r10 + 16]",
    "    mov r9, qword ptr [r10 + 24]",
    "    movq xmm0, rcx",
    "    movq xmm1, rdx",
    "    movq xmm2, r8",
    "    movq xmm3, r9",
    ".Lseh_call10x_begin:",
    "    call rax",
    "    nop",
    ".Lseh_call10x_end:",
    "    mov qword ptr [rbx], rax",
    "    movq qword ptr [rsi], xmm0",
    "    mov eax, 1",
    ".Lseh_call10x_out:",
    "    add rsp, 88",
    "    pop rsi",
    "    pop rbx",
    "    ret",
    ".Lseh_call10x_except:",
    "    xor eax, eax",
    "    jmp .Lseh_call10x_out",
    ".seh_handlerdata",
    "    .long 1",
    "    .long .Lseh_call10x_begin@IMGREL",
    "    .long .Lseh_call10x_end@IMGREL",
    "    .long 1",
    "    .long .Lseh_call10x_except@IMGREL",
    ".text",
    ".seh_endproc",
    // ---- int cvr_seh_cmpxchg32(volatile long *target, long exchange, long comparand, long *found)
    ".globl cvr_seh_cmpxchg32",
    ".def cvr_seh_cmpxchg32",
    ".scl 2",
    ".type 32",
    ".endef",
    "cvr_seh_cmpxchg32:",
    ".seh_proc cvr_seh_cmpxchg32",
    ".seh_handler __C_specific_handler, @unwind, @except",
    "    sub rsp, 40",
    ".seh_stackalloc 40",
    ".seh_endprologue",
    "    mov eax, r8d",
    ".Lseh_cmpxchg32_begin:",
    "    lock cmpxchg dword ptr [rcx], edx",
    "    nop",
    ".Lseh_cmpxchg32_end:",
    "    mov dword ptr [r9], eax",
    "    mov eax, 1",
    ".Lseh_cmpxchg32_out:",
    "    add rsp, 40",
    "    ret",
    ".Lseh_cmpxchg32_except:",
    "    xor eax, eax",
    "    jmp .Lseh_cmpxchg32_out",
    ".seh_handlerdata",
    "    .long 1",
    "    .long .Lseh_cmpxchg32_begin@IMGREL",
    "    .long .Lseh_cmpxchg32_end@IMGREL",
    "    .long 1",
    "    .long .Lseh_cmpxchg32_except@IMGREL",
    ".text",
    ".seh_endproc",
    // ---- int cvr_seh_add_ref(void *object): object->vtable[1](object), 1 when the call ran
    ".globl cvr_seh_add_ref",
    ".def cvr_seh_add_ref",
    ".scl 2",
    ".type 32",
    ".endef",
    "cvr_seh_add_ref:",
    ".seh_proc cvr_seh_add_ref",
    ".seh_handler __C_specific_handler, @unwind, @except",
    "    sub rsp, 40",
    ".seh_stackalloc 40",
    ".seh_endprologue",
    ".Lseh_add_ref_begin:",
    "    mov rax, qword ptr [rcx]",
    "    call qword ptr [rax + 8]",
    "    nop",
    ".Lseh_add_ref_end:",
    "    mov eax, 1",
    ".Lseh_add_ref_out:",
    "    add rsp, 40",
    "    ret",
    ".Lseh_add_ref_except:",
    "    xor eax, eax",
    "    jmp .Lseh_add_ref_out",
    ".seh_handlerdata",
    "    .long 1",
    "    .long .Lseh_add_ref_begin@IMGREL",
    "    .long .Lseh_add_ref_end@IMGREL",
    "    .long 1",
    "    .long .Lseh_add_ref_except@IMGREL",
    ".text",
    ".seh_endproc",
    // ---- int cvr_seh_unwind_step(CONTEXT *context): 1 stepped, 2 no unwind data (a leaf), 0 faulted
    ".globl cvr_seh_unwind_step",
    ".def cvr_seh_unwind_step",
    ".scl 2",
    ".type 32",
    ".endef",
    "cvr_seh_unwind_step:",
    ".seh_proc cvr_seh_unwind_step",
    ".seh_handler __C_specific_handler, @unwind, @except",
    "    push rbx",
    ".seh_pushreg rbx",
    "    sub rsp, 96",
    ".seh_stackalloc 96",
    ".seh_endprologue",
    // [rsp + 0..32) home, [rsp + 32..64) the fifth to eighth arguments, + 64 image, + 72 handler, + 80 establisher
    "    mov rbx, rcx",
    "    mov qword ptr [rsp + 64], 0",
    ".Lseh_unwind_step_begin:",
    "    mov rcx, qword ptr [rbx + 248]",
    "    lea rdx, [rsp + 64]",
    "    xor r8d, r8d",
    "    call qword ptr [rip + __imp_RtlLookupFunctionEntry]",
    "    test rax, rax",
    "    jz .Lseh_unwind_step_none",
    "    mov qword ptr [rsp + 72], 0",
    "    mov qword ptr [rsp + 80], 0",
    "    mov qword ptr [rsp + 32], rbx",
    "    lea rcx, [rsp + 72]",
    "    mov qword ptr [rsp + 40], rcx",
    "    lea rcx, [rsp + 80]",
    "    mov qword ptr [rsp + 48], rcx",
    "    mov qword ptr [rsp + 56], 0",
    "    mov r9, rax",
    "    mov r8, qword ptr [rbx + 248]",
    "    mov rdx, qword ptr [rsp + 64]",
    "    xor ecx, ecx",
    "    call qword ptr [rip + __imp_RtlVirtualUnwind]",
    "    nop",
    ".Lseh_unwind_step_end:",
    "    mov eax, 1",
    "    jmp .Lseh_unwind_step_out",
    ".Lseh_unwind_step_none:",
    "    mov eax, 2",
    ".Lseh_unwind_step_out:",
    "    add rsp, 96",
    "    pop rbx",
    "    ret",
    ".Lseh_unwind_step_except:",
    "    xor eax, eax",
    "    jmp .Lseh_unwind_step_out",
    ".seh_handlerdata",
    "    .long 1",
    "    .long .Lseh_unwind_step_begin@IMGREL",
    "    .long .Lseh_unwind_step_end@IMGREL",
    "    .long 1",
    "    .long .Lseh_unwind_step_except@IMGREL",
    ".text",
    ".seh_endproc",
    // ---- int cvr_seh_function_entry(DWORD64 rip, DWORD64 *image, DWORD *begin, DWORD *end): 1 found, 2 none, 0 fault
    ".globl cvr_seh_function_entry",
    ".def cvr_seh_function_entry",
    ".scl 2",
    ".type 32",
    ".endef",
    "cvr_seh_function_entry:",
    ".seh_proc cvr_seh_function_entry",
    ".seh_handler __C_specific_handler, @unwind, @except",
    "    push rbx",
    ".seh_pushreg rbx",
    "    push rsi",
    ".seh_pushreg rsi",
    "    sub rsp, 40",
    ".seh_stackalloc 40",
    ".seh_endprologue",
    "    mov rbx, r8",
    "    mov rsi, r9",
    ".Lseh_function_entry_begin:",
    "    xor r8d, r8d",
    "    call qword ptr [rip + __imp_RtlLookupFunctionEntry]",
    "    test rax, rax",
    "    jz .Lseh_function_entry_none",
    "    mov ecx, dword ptr [rax]",
    "    mov dword ptr [rbx], ecx",
    "    mov ecx, dword ptr [rax + 4]",
    "    mov dword ptr [rsi], ecx",
    "    nop",
    ".Lseh_function_entry_end:",
    "    mov eax, 1",
    "    jmp .Lseh_function_entry_out",
    ".Lseh_function_entry_none:",
    "    mov eax, 2",
    ".Lseh_function_entry_out:",
    "    add rsp, 40",
    "    pop rsi",
    "    pop rbx",
    "    ret",
    ".Lseh_function_entry_except:",
    "    xor eax, eax",
    "    jmp .Lseh_function_entry_out",
    ".seh_handlerdata",
    "    .long 1",
    "    .long .Lseh_function_entry_begin@IMGREL",
    "    .long .Lseh_function_entry_end@IMGREL",
    "    .long 1",
    "    .long .Lseh_function_entry_except@IMGREL",
    ".text",
    ".seh_endproc",
    // ---- the ranges whose own instruction faults (no callee between): begin, end, except, three of them
    ".section .rdata,\"dr\"",
    ".globl cvr_seh_direct_ranges",
    ".p2align 3",
    "cvr_seh_direct_ranges:",
    "    .quad .Lseh_copy_from_begin, .Lseh_copy_from_end, .Lseh_copy_from_except",
    "    .quad .Lseh_copy_to_begin, .Lseh_copy_to_end, .Lseh_copy_to_except",
    "    .quad .Lseh_cmpxchg32_begin, .Lseh_cmpxchg32_end, .Lseh_cmpxchg32_except",
    ".text",
);

extern "C" {
    static cvr_seh_direct_ranges: [[usize; 3]; 3];
}

/// THE ENGINE'S VECTORED HANDLER SEES A FAULT FIRST: REDengine reports any access violation as a crash (CrashReporter,
/// the game stops) before a frame's SEH handler is asked, so a guarded read of a stale or wrong address took the
/// game down however well its __except caught it. This handler, put at the head of the vectored list, takes the
/// faults of the guarded copies and the compare-exchange -- whose own instruction faults, with no callee frame
/// between -- before the engine's: their context resumes at their except label, which returns 0 as the SEH path does.
/// A fault anywhere else goes on to the next handler untouched.
unsafe extern "system" fn first_chance(info: *mut windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS) -> i32 {
    const EXCEPTION_ACCESS_VIOLATION: i32 = 0xC000_0005_u32 as i32;
    const CONTINUE_SEARCH: i32 = 0;
    const CONTINUE_EXECUTION: i32 = -1;
    // SAFETY: the OS's exception record and context for this thread's fault.
    unsafe {
        let Some(info) = info.as_ref() else { return CONTINUE_SEARCH };
        let (Some(rec), Some(ctx)) = (info.ExceptionRecord.as_ref(), info.ContextRecord.as_mut()) else { return CONTINUE_SEARCH };
        if rec.ExceptionCode != EXCEPTION_ACCESS_VIOLATION {
            return CONTINUE_SEARCH;
        }
        let rip = ctx.Rip as usize;
        for r in &*(&raw const cvr_seh_direct_ranges) {
            if rip >= r[0] && rip < r[1] {
                ctx.Rip = r[2] as u64;
                GUARDED_FAULTS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                return CONTINUE_EXECUTION;
            }
        }
    }
    CONTINUE_SEARCH
}
/// The guarded copies' faults taken before the engine's handler saw them (each a crash report before).
pub static GUARDED_FAULTS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static FIRST_CHANCE: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
/// The first-chance handler put at the head of the vectored list (again, when it was there already: a handler the
/// engine or another mod added since stands before it otherwise).
pub fn install_first_chance() {
    use windows_sys::Win32::System::Diagnostics::Debug::{AddVectoredExceptionHandler, RemoveVectoredExceptionHandler};
    // SAFETY: the handler is a static function of this module, alive as long as the process.
    unsafe {
        let old = FIRST_CHANCE.swap(0, core::sync::atomic::Ordering::AcqRel);
        if old != 0 {
            RemoveVectoredExceptionHandler(old as *const core::ffi::c_void);
        }
        let h = AddVectoredExceptionHandler(1, Some(first_chance));
        FIRST_CHANCE.store(h as usize, core::sync::atomic::Ordering::Release);
    }
}

// The OS functions the unwind step calls through their import slots (__imp_*): kernel32's, declared with the
// library so that the import library is on the link line whatever else links it.
#[link(name = "kernel32")]
extern "system" {
    #[allow(dead_code)]
    fn RtlLookupFunctionEntry(control_pc: u64, image_base: *mut u64, history_table: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    #[allow(dead_code)]
    fn RtlVirtualUnwind();
}

#[cfg(test)]
mod tests {
    use crate::code::{guarded_add_ref, guarded_call10x, guarded_call3, guarded_call4, guarded_call4x, guarded_call6, guarded_cmpxchg32};
    use core::ffi::c_void;
    use core::ptr::null_mut;
    use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

    extern "C" fn yes(_: *mut c_void, _: *mut c_void, _: *mut c_void) -> bool {
        true
    }
    extern "C" fn fourth(_: *mut c_void, _: *mut c_void, _: *mut c_void, d: *mut c_void) -> *mut c_void {
        d
    }
    extern "C" fn sum6(a: *mut c_void, b: *mut c_void, c: *mut c_void, d: *mut c_void, e: *mut c_void, f: *mut c_void) -> *mut c_void {
        (a as usize + 2 * b as usize + 3 * c as usize + 4 * d as usize + 5 * e as usize + 6 * f as usize) as *mut c_void
    }
    extern "C" fn scaled(this: *mut c_void, by: f32, add: f32) -> f32 {
        this as usize as f32 * by + add
    }
    extern "C" fn second_int(_: f32, b: u32) -> u32 {
        b * 3
    }
    #[test]
    fn floats_in_and_out_and_a_fault_caught() {
        // SAFETY: local functions of mixed arguments, then a call into the null page.
        unsafe {
            let (_, x) = guarded_call4x(scaled as usize, [7, u64::from(2.5f32.to_bits()), u64::from(0.25f32.to_bits()), 0]).expect("no fault");
            assert_eq!(f32::from_bits(x as u32), 17.75);
            let (r, _) = guarded_call4x(second_int as usize, [u64::from(1.0f32.to_bits()), 11, 0, 0]).expect("no fault");
            assert_eq!(r as u32, 33);
            assert_eq!(guarded_call4x(0x20, [0; 4]), None);
        }
    }
    /// A sweep's shape: a pointer, a float, six pointers, a float after them on the stack.
    extern "C" fn sweep_like(d: *const f32, dist: f32, a: usize, b: usize, c: usize, e: usize, f: usize, g: usize, inflation: f32) -> f32 {
        // SAFETY: the test passes a live float.
        unsafe { *d * dist + (a + 2 * b + 3 * c + 4 * e + 5 * f + 6 * g) as f32 + inflation }
    }
    #[test]
    fn ten_arguments_floats_on_the_stack_and_a_fault_caught() {
        let dir = 2.0f32;
        // SAFETY: a local function of nine mixed arguments, then a call into the null page.
        unsafe {
            let args = [&dir as *const f32 as u64, u64::from(3.0f32.to_bits()), 1, 2, 3, 4, 5, 6, u64::from(0.5f32.to_bits()), 0];
            let (_, x) = guarded_call10x(sweep_like as usize, args).expect("no fault");
            assert_eq!(f32::from_bits(x as u32), 2.0 * 3.0 + (1 + 4 + 9 + 16 + 25 + 36) as f32 + 0.5);
            assert_eq!(guarded_call10x(0x20, [0; 10]), None);
            // and again: the stack is as it was
            assert!(guarded_call10x(sweep_like as usize, args).is_some());
        }
    }
    #[test]
    fn six_arguments_in_order_and_a_fault_caught() {
        let p = |v: usize| v as *mut c_void;
        // SAFETY: a local function of six pointer arguments, then a call into the null page.
        unsafe {
            assert_eq!(guarded_call6(sum6 as usize, [p(1), p(10), p(100), p(1000), p(10000), p(100000)]), Some(p(1 + 20 + 300 + 4000 + 50000 + 600000)));
            assert_eq!(guarded_call6(0x20, [null_mut(); 6]), None);
        }
    }
    static ADDED: AtomicU32 = AtomicU32::new(0);
    extern "system" fn add_ref(_: *mut c_void) -> u32 {
        ADDED.fetch_add(1, Relaxed) + 1
    }

    #[test]
    fn guarded_calls_survive_faults() {
        // SAFETY: the calls below either run a function of this test or fault inside the adapter's guarded range.
        unsafe {
            assert_eq!(guarded_call3(0x10, null_mut(), null_mut(), null_mut()), None, "a call into the null page");
            assert_eq!(guarded_call3(yes as *const () as usize, null_mut(), null_mut(), null_mut()), Some(true));
            assert_eq!(guarded_call4(0x20, null_mut(), null_mut(), null_mut(), null_mut()), None);
            assert_eq!(guarded_call4(fourth as *const () as usize, null_mut(), null_mut(), null_mut(), 0x55 as *mut c_void), Some(0x55 as *mut c_void));
            // a fault in the callee's frame, not ours: a call through a vtable read from the null page
            assert!(!guarded_add_ref(0x10));
            let vtable: [usize; 2] = [0, add_ref as *const () as usize];
            let object: [usize; 1] = [vtable.as_ptr() as usize];
            assert!(guarded_add_ref(object.as_ptr() as usize));
            assert_eq!(ADDED.load(Relaxed), 1);
            assert_eq!(guarded_cmpxchg32(0x10, 1, 0), None);
            let mut word = 7u32;
            assert_eq!(guarded_cmpxchg32(&mut word as *mut u32 as usize, 9, 7), Some(7));
            assert_eq!(word, 9);
            assert_eq!(guarded_cmpxchg32(&mut word as *mut u32 as usize, 1, 7), Some(9), "no exchange when it differs");
            assert_eq!(word, 9);
        }
    }

    #[test]
    fn many_faults_in_a_row() {
        // The handler's unwind leaves the frame as it found it: thousands of faults in a row, the stack unchanged.
        for i in 0..10_000usize {
            assert_eq!(crate::code::read::<u64>(8 + (i % 64) * 8), None);
        }
        let value = 42u64;
        assert_eq!(crate::code::read::<u64>(&value as *const u64 as usize), Some(42));
    }
}
