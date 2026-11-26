//! The resource replay receipt (`include/Render/NativeResourceReplay.hpp`, `resource_replay::Receipt`): the copied
//! declaration grammar of one frozen graph for the guarded 1F0ED0 / 1F0F80 emitters -- two coherent pre-compiler
//! observations (identity, node bindings, per-slot counts and every 48-byte row, flattened in slot/index order), each
//! slot's cursor, one in-flight command per slot whose kind, slot, key and active payload bytes match the expected row
//! (kind 2's DWORD key and QWORD callback around its padding, a buffer or texture descriptor's audited bytes, kind 13's
//! request recorded or required equal), a nonce token for each call, the native result checked only for kind 13, and
//! completion when every slot's cursor reached its count. Failure is sticky.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the C++ Receipt owns one of these and forwards every method to `cvr_rr_*`;
//! `Declarations()` lends the copied rows. It does not fail on allocation (the C++ one reported Allocation).
//! `tools/single_pass_tests/resource_replay_test` runs its cases against it in its Rust variant.

pub const SLOTS: usize = 220;
pub const ROWS_PER_SLOT: u32 = 600;
pub const MAX_ROWS: usize = SLOTS * ROWS_PER_SLOT as usize;
pub type RawDeclaration = [u8; 48];

/// resource_replay::Failure.
pub mod failure {
    pub const NONE: u32 = 0;
    pub const ARGUMENTS: u32 = 1;
    pub const ALREADY_ATTEMPTED: u32 = 2;
    pub const IDENTITY: u32 = 3;
    pub const CHANGED: u32 = 4;
    pub const NODES: u32 = 5;
    pub const COUNTS: u32 = 6;
    pub const ROWS: u32 = 7;
    pub const KIND: u32 = 8;
    pub const SLOT: u32 = 9;
    pub const NOT_CAPTURED: u32 = 10;
    pub const INVOCATION: u32 = 11;
    pub const MODE: u32 = 12;
    pub const CURSOR: u32 = 13;
    pub const BUSY: u32 = 14;
    pub const COMMAND: u32 = 15;
    pub const KIND_MISMATCH: u32 = 16;
    pub const KEY_MISMATCH: u32 = 17;
    pub const PAYLOAD_MISMATCH: u32 = 18;
    pub const TOKEN: u32 = 19;
    pub const RESULT: u32 = 21;
    pub const CAPACITY: u32 = 22;
}
/// Kind13Policy.
pub const RECORD_REQUEST_DIFFERENCE: u32 = 0;
pub const REQUIRE_SAME_REQUEST: u32 = 1;

/// resource_replay::Identity.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    pub consumer_id: u64,
    pub epoch: u64,
    pub generation: u64,
    pub pool: u64,
    pub source_graph: u64,
    pub execution_graph: u64,
}
/// resource_replay::NodeBinding.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeBinding {
    pub handle: u32,
    pub ordinal: u32,
    pub slot: u32,
    pub view_index: u32,
}
/// resource_replay::Token.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Token {
    pub identity: Identity,
    pub node: NodeBinding,
    pub nonce: u64,
    pub index: u32,
    pub kind: u32,
    pub compiled_decision: u8,
    pub requested_decision: u8,
    pub request_difference: bool,
}
const _: () = assert!(
    core::mem::size_of::<Identity>() == 48 && core::mem::size_of::<NodeBinding>() == 16 && core::mem::size_of::<Token>() == 88
);

/// PayloadBytes: a command kind's payload after its 8-byte header (u32::MAX: unknown kind).
pub fn payload_bytes(kind: u32) -> u32 {
    match kind {
        0 | 3 | 5 | 6 | 10 => 0,
        1 => 40,
        2 => 16,
        4 | 7 | 8 | 11 | 12 => 4,
        9 | 13 => 1,
        _ => u32::MAX,
    }
}
/// CommandBytes.
pub fn command_bytes(kind: u32) -> u32 {
    let bytes = payload_bytes(kind);
    if bytes == u32::MAX { 0 } else { 8 + bytes }
}
fn pointer(value: u64, bytes: u64) -> bool {
    value != 0 && value & 7 == 0 && value <= u64::MAX - bytes
}
fn valid_identity(identity: &Identity) -> bool {
    identity.consumer_id != 0
        && identity.epoch != 0
        && identity.epoch <= u32::MAX as u64
        && identity.generation != 0
        && pointer(identity.pool, 3)
        && pointer(identity.source_graph, 0x70)
        && pointer(identity.execution_graph, 0x70)
        && identity.source_graph != identity.execution_graph
}

/// The receipt.
pub struct Receipt {
    identity: Identity,
    policy: u32,
    rows: Vec<RawDeclaration>,
    nodes: Vec<NodeBinding>,
    counts: [u32; SLOTS],
    starts: [u32; SLOTS],
    cursors: [u32; SLOTS],
    pending: Box<[Token; SLOTS]>,
    in_flight: [bool; SLOTS],
    next_nonce: u64,
    completed: u64,
    request_differences: u64,
    attempted: bool,
    captured: bool,
    failed: bool,
    failure: u32,
}
/// The C++ Receipt's queries (cvr_rr_query / cvr_rr_address), for the Rust callers.
#[cfg_attr(not(feature = "rust-build"), allow(dead_code))]
impl Receipt {
    pub(crate) fn is_captured(&self) -> bool {
        self.captured
    }
    pub(crate) fn is_failed(&self) -> bool {
        self.failed
    }
    pub(crate) fn rejection(&self) -> u32 {
        self.failure
    }
    pub(crate) fn binding(&self) -> &Identity {
        &self.identity
    }
    pub(crate) fn completed_calls(&self) -> u64 {
        self.completed
    }
    pub(crate) fn request_difference_count(&self) -> u64 {
        self.request_differences
    }
    pub(crate) fn cursor(&self, slot: u32) -> u32 {
        self.cursors.get(slot as usize).copied().unwrap_or(0)
    }
    pub(crate) fn declared_count(&self, slot: u32) -> u32 {
        self.counts.get(slot as usize).copied().unwrap_or(0)
    }
    pub(crate) fn declarations(&self) -> &[RawDeclaration] {
        &self.rows
    }
}
impl Default for Receipt {
    fn default() -> Self {
        Receipt {
            identity: Identity::default(),
            policy: RECORD_REQUEST_DIFFERENCE,
            rows: Vec::new(),
            nodes: Vec::new(),
            counts: [0; SLOTS],
            starts: [0; SLOTS],
            cursors: [0; SLOTS],
            pending: Box::new([Token::default(); SLOTS]),
            in_flight: [false; SLOTS],
            next_nonce: 0,
            completed: 0,
            request_differences: 0,
            attempted: false,
            captured: false,
            failed: false,
            failure: failure::NONE,
        }
    }
}
impl Receipt {
    fn reject(&mut self, reason: u32) -> bool {
        if !self.failed {
            self.failure = reason;
        }
        self.failed = true;
        false
    }
    /// Capture: the copied grammar of two coherent observations.
    #[allow(clippy::too_many_arguments)]
    pub fn capture(
        &mut self,
        before: &Identity,
        after: &Identity,
        nodes: &[NodeBinding],
        counts_before: &[u32],
        counts_after: &[u32],
        rows_before: &[RawDeclaration],
        rows_after: &[RawDeclaration],
        policy: u32,
    ) -> bool {
        if self.failed {
            return false;
        }
        if self.attempted {
            return self.reject(failure::ALREADY_ATTEMPTED);
        }
        self.attempted = true;
        if !valid_identity(before) {
            return self.reject(failure::IDENTITY);
        }
        if before != after {
            return self.reject(failure::CHANGED);
        }
        if policy != RECORD_REQUEST_DIFFERENCE && policy != REQUIRE_SAME_REQUEST {
            return self.reject(failure::ARGUMENTS);
        }
        if nodes.is_empty() || nodes.len() > SLOTS || counts_before.len() != SLOTS || counts_after.len() != SLOTS {
            return self.reject(failure::COUNTS);
        }
        if counts_before != counts_after {
            return self.reject(failure::CHANGED);
        }
        let (mut ordinals, mut used) = ([false; SLOTS], [false; SLOTS]);
        for node in nodes {
            if node.handle >= 65536
                || node.ordinal as usize >= nodes.len()
                || node.slot as usize >= SLOTS
                || node.view_index > 1
                || ordinals[node.ordinal as usize]
                || used[node.slot as usize]
            {
                return self.reject(failure::NODES);
            }
            ordinals[node.ordinal as usize] = true;
            used[node.slot as usize] = true;
        }
        // Input order need not equal ordinal order; handles must be globally unique.
        for (i, node) in nodes.iter().enumerate() {
            if nodes[i + 1..].iter().any(|other| other.handle == node.handle) {
                return self.reject(failure::NODES);
            }
        }
        let mut total = 0u32;
        for slot in 0..SLOTS {
            if counts_before[slot] > ROWS_PER_SLOT || (counts_before[slot] != 0 && !used[slot]) {
                return self.reject(failure::COUNTS);
            }
            self.starts[slot] = total;
            total += counts_before[slot];
        }
        let total = total as usize;
        if total > MAX_ROWS || rows_before.len() != total || rows_after.len() != total {
            return self.reject(failure::ROWS);
        }
        if rows_before != rows_after {
            return self.reject(failure::CHANGED);
        }
        for slot in 0..SLOTS {
            for index in 0..counts_before[slot] {
                let row = &rows_before[(self.starts[slot] + index) as usize];
                if payload_bytes(row[0] as u32) == u32::MAX {
                    return self.reject(failure::KIND);
                }
                if row[1] as usize != slot {
                    return self.reject(failure::SLOT);
                }
            }
        }
        self.rows = rows_before.to_vec();
        self.nodes = nodes.to_vec();
        self.counts.copy_from_slice(counts_before);
        self.identity = *before;
        self.policy = policy;
        self.captured = true;
        self.failure = failure::NONE;
        true
    }
    /// Before: called before the original increments native slot+40; only 8 + PayloadBytes(kind) bytes are read.
    pub fn before(&mut self, identity: &Identity, node: &NodeBinding, mode: u32, old_cursor: u32, command: &[u8]) -> Option<Token> {
        if self.failed {
            return None;
        }
        let refuse = |receipt: &mut Self, reason: u32| {
            receipt.reject(reason);
            None
        };
        if !self.captured {
            return refuse(self, failure::NOT_CAPTURED);
        }
        if *identity != self.identity {
            return refuse(self, failure::INVOCATION);
        }
        if !self.matches_node(node) {
            return refuse(self, failure::NODES);
        }
        if mode != 2 {
            return refuse(self, failure::MODE);
        }
        let slot = node.slot as usize;
        if self.in_flight[slot] {
            return refuse(self, failure::BUSY);
        }
        if old_cursor != self.cursors[slot] || old_cursor >= self.counts[slot] {
            return refuse(self, failure::CURSOR);
        }
        if command.len() < 8 {
            return refuse(self, failure::COMMAND);
        }
        let kind = command[0] as u32;
        let bytes = payload_bytes(kind);
        if bytes == u32::MAX {
            return refuse(self, failure::KIND);
        }
        if command.len() < 8 + bytes as usize {
            return refuse(self, failure::COMMAND);
        }
        let expected = self.rows[(self.starts[slot] + old_cursor) as usize];
        if kind != expected[0] as u32 {
            return refuse(self, failure::KIND_MISMATCH);
        }
        if command[1] as usize != slot {
            return refuse(self, failure::SLOT);
        }
        if command[4..8] != expected[4..8] {
            return refuse(self, failure::KEY_MISMATCH);
        }
        let request_difference = kind == 13 && command[8] != expected[8];
        if kind != 13 || self.policy == REQUIRE_SAME_REQUEST {
            // Descriptor readers consume only bits 0..2 at TEXTURE+32; BUFFER ignores +32..35; both omit +38..39.
            let buffer = kind == 1 && command[44] == 1 && expected[44] == 1;
            let texture = kind == 1 && command[44] == 0 && expected[44] == 0;
            let equal = if kind == 2 {
                // A DWORD source key, then a QWORD callback: +12..15 is alignment padding.
                command[8..12] == expected[8..12] && command[16..24] == expected[16..24]
            } else if buffer || texture {
                command[8..40] == expected[8..40]
                    && (!texture || (command[40] & 7) == (expected[40] & 7))
                    && command[44..46] == expected[44..46]
            } else {
                command[8..8 + bytes as usize] == expected[8..8 + bytes as usize]
            };
            if !equal {
                return refuse(self, failure::PAYLOAD_MISMATCH);
            }
        }
        if self.next_nonce == u64::MAX {
            return refuse(self, failure::CAPACITY);
        }
        self.next_nonce += 1;
        let token = Token {
            identity: self.identity,
            node: *node,
            nonce: self.next_nonce,
            index: old_cursor,
            kind,
            compiled_decision: if kind == 13 { expected[8] } else { 0 },
            requested_decision: if kind == 13 { command[8] } else { 0 },
            request_difference,
        };
        self.pending[slot] = token;
        self.in_flight[slot] = true;
        Some(token)
    }
    /// After: the native result (AL) is checked only for kind 13.
    pub fn after(&mut self, token: &Token, identity: &Identity, node: &NodeBinding, cursor_after: u32, native_result: u8) -> bool {
        if !self.captured {
            return self.reject(failure::NOT_CAPTURED);
        }
        let slot = token.node.slot as usize;
        if slot >= SLOTS || !self.in_flight[slot] || *token != self.pending[slot] {
            return self.reject(failure::TOKEN);
        }
        self.in_flight[slot] = false;
        self.pending[slot] = Token::default();
        if self.failed {
            return false;
        }
        if *identity != self.identity || token.identity != self.identity || *node != token.node {
            return self.reject(failure::INVOCATION);
        }
        if cursor_after != token.index.wrapping_add(1) {
            return self.reject(failure::CURSOR);
        }
        if token.kind == 13 && native_result != token.compiled_decision {
            return self.reject(failure::RESULT);
        }
        self.cursors[slot] = cursor_after;
        self.completed += 1;
        self.request_differences += token.request_difference as u64;
        true
    }
    pub fn complete(&self) -> bool {
        self.captured
            && !self.failed
            && self.completed == self.rows.len() as u64
            && (0..SLOTS).all(|slot| !self.in_flight[slot] && self.cursors[slot] == self.counts[slot])
    }
    pub fn matches_identity(&self, identity: &Identity) -> bool {
        self.captured && !self.failed && *identity == self.identity
    }
    pub fn matches_node(&self, node: &NodeBinding) -> bool {
        self.captured && self.nodes.contains(node)
    }
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

unsafe fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if data.is_null() || count == 0 { &[] } else { unsafe { core::slice::from_raw_parts(data, count) } }
}
/// PayloadBytes (`which` 0) or CommandBytes (1).
#[no_mangle]
pub extern "C-unwind" fn cvr_rr_bytes(which: u32, kind: u32) -> u32 {
    if which == 0 { payload_bytes(kind) } else { command_bytes(kind) }
}
/// A new receipt, owned by the C++ face.
#[no_mangle]
pub extern "C-unwind" fn cvr_rr_new() -> *mut Receipt {
    Box::into_raw(Box::default())
}
/// Frees a receipt made by `cvr_rr_new`.
///
/// # Safety
/// `receipt` came from `cvr_rr_new` and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rr_free(receipt: *mut Receipt) {
    if !receipt.is_null() {
        drop(unsafe { Box::from_raw(receipt) });
    }
}
/// The observations crossing to Capture: pointers with their counts.
#[repr(C)]
pub struct Observation {
    pub nodes: *const NodeBinding,
    pub node_count: usize,
    pub counts_before: *const u32,
    pub counts_before_len: usize,
    pub counts_after: *const u32,
    pub counts_after_len: usize,
    pub rows_before: *const RawDeclaration,
    pub rows_before_len: usize,
    pub rows_after: *const RawDeclaration,
    pub rows_after_len: usize,
}
/// Capture.
///
/// # Safety
/// `receipt` is live; the identities and `observation` (with its arrays) are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rr_capture(receipt: *mut Receipt, before: *const Identity, after: *const Identity, observation: *const Observation, policy: u32) -> bool {
    unsafe {
        let o = &*observation;
        (*receipt).capture(
            &*before,
            &*after,
            slice(o.nodes, o.node_count),
            slice(o.counts_before, o.counts_before_len),
            slice(o.counts_after, o.counts_after_len),
            slice(o.rows_before, o.rows_before_len),
            slice(o.rows_after, o.rows_after_len),
            policy,
        )
    }
}
/// Before (`output` zeroed, written on success).
///
/// # Safety
/// `receipt` is live; the records and `command` (`command_len` bytes) are valid; `output` is writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_rr_before(
    receipt: *mut Receipt,
    identity: *const Identity,
    node: *const NodeBinding,
    mode: u32,
    old_cursor: u32,
    command: *const u8,
    command_len: usize,
    output: *mut Token,
) -> bool {
    unsafe { *output = Token::default() };
    let token = unsafe { (*receipt).before(&*identity, &*node, mode, old_cursor, slice(command, command_len)) };
    unsafe { *output = token.unwrap_or_default() };
    token.is_some()
}
/// After.
///
/// # Safety
/// `receipt` is live; the records are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rr_after(receipt: *mut Receipt, token: *const Token, identity: *const Identity, node: *const NodeBinding, cursor_after: u32, native_result: u8) -> bool {
    unsafe { (*receipt).after(&*token, &*identity, &*node, cursor_after, native_result) }
}
/// Queries by `which`: Complete (0), Captured (1), Failed (2), Rejection (3), MatchesIdentity (4: `identity`),
/// MatchesNode (5: `node`), CompletedCalls (6), RequestDifferences (7), Cursor (8: `slot`), DeclaredCount (9: `slot`),
/// Pending (10: `slot`), the declarations' count (11).
///
/// # Safety
/// `receipt` is live; `identity` / `node` are valid where used.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rr_query(receipt: *const Receipt, which: u32, identity: *const Identity, node: *const NodeBinding, slot: u32) -> u64 {
    let receipt = unsafe { &*receipt };
    let slot = slot as usize;
    match which {
        0 => receipt.complete() as u64,
        1 => receipt.captured as u64,
        2 => receipt.failed as u64,
        3 => receipt.failure as u64,
        4 => receipt.matches_identity(unsafe { &*identity }) as u64,
        5 => receipt.matches_node(unsafe { &*node }) as u64,
        6 => receipt.completed,
        7 => receipt.request_differences,
        8 => receipt.cursors.get(slot).copied().unwrap_or(0) as u64,
        9 => receipt.counts.get(slot).copied().unwrap_or(0) as u64,
        10 => (slot < SLOTS && receipt.in_flight[slot]) as u64,
        _ => receipt.rows.len() as u64,
    }
}
/// The bound identity (`which` 0) or the declarations' first row (1): stable addresses inside the receipt.
///
/// # Safety
/// `receipt` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rr_address(receipt: *const Receipt, which: u32) -> *const core::ffi::c_void {
    let receipt = unsafe { &*receipt };
    if which == 0 {
        (&receipt.identity as *const Identity).cast()
    } else {
        receipt.rows.as_ptr().cast()
    }
}
