//! The native upload arena's fields and the guarded ready/reset/destroy invocation stack (the logic of
//! include/Render/NativeUploadArena.hpp).
//!
//! A ready arena is a freshly reset ring (head and tail 0, the native 0xC000000 capacity, a bounded cache) with its
//! upload resource and mapping; later reads must keep that identity with a monotonic, bounded live interval. The
//! runtime pairs the guarded native before/after callbacks with this exact per-thread stack: a reset outside the
//! matching ready invocation cannot certify it, and any mismatch poisons every open invocation.

pub const READY_RVA: u64 = 0xB82990;
pub const READY_BYTES: usize = 249;
pub const RESET_RVA: u64 = 0xB82A8C;
pub const RESET_BYTES: usize = 143;
pub const DESTROY_RVA: u64 = 0x5B30DC;
pub const DESTROY_BYTES: usize = 88;
pub const READY_FNV: u64 = 0xFF20_A4D4_F912_F92B;
pub const RESET_FNV: u64 = 0x81DB_72CA_CEBD_5681;
pub const DESTROY_FNV: u64 = 0x17A1_6E7B_EE7A_FB4C;
pub const NATIVE_CAPACITY: u64 = 0xC000000;
pub const MAX_ARENAS: usize = 64;
pub const MAX_ROWS: usize = 4096;
pub const MAX_SCOPES: usize = 8;

/// A read of the engine's memory: (address, out) -> readable.
pub type Read<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> bool;

#[repr(u64)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ready = 1,
    Reset,
    Destroy,
    Cancel,
    Query,
}
#[repr(u64)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Disabled,
    Observed,
    Captured,
    GuardMismatch,
    InvalidInvocation,
    InvalidArena,
    MissingReset,
    ResourceMismatch,
    Changed,
    Capacity,
    Failure,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Invocation {
    pub nonce: u64,
    pub arena: u64,
    pub thread: u64,
    pub kind: u64,
    pub parent_ready: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArenaFields {
    pub arena: u64,
    pub ring: u64,
    pub cache: u64,
    pub head: u64,
    pub tail: u64,
    pub capacity: u64,
    pub resource: u64,
    pub mapped: u64,
    pub cache_count: u32,
}

const _: () =
    assert!(core::mem::size_of::<Invocation>() == 40 && core::mem::size_of::<ArenaFields>() == 72);

fn le64(b: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(b[offset..offset + 8].try_into().expect("eight bytes"))
}
/// The arena's 0x68 bytes: cache +0, cache count +0x0C, ring +0x18 (head, tail +0x20, capacity +0x28), resource
/// +0x50, mapping +0x60.
pub fn read_arena(read: Option<Read<'_>>, arena: u64) -> Option<ArenaFields> {
    let read = read?;
    if arena == 0 || arena & 7 != 0 || arena > u64::MAX - 0x68 {
        return None;
    }
    let mut bytes = [0u8; 0x68];
    if !read(arena, &mut bytes) {
        return None;
    }
    Some(ArenaFields {
        arena,
        ring: arena + 0x18,
        cache: le64(&bytes, 0),
        head: le64(&bytes, 0x18),
        tail: le64(&bytes, 0x20),
        capacity: le64(&bytes, 0x28),
        resource: le64(&bytes, 0x50),
        mapped: le64(&bytes, 0x60),
        cache_count: u32::from_le_bytes(bytes[0x0C..0x10].try_into().expect("four bytes")),
    })
}
pub fn reset_fields(value: &ArenaFields) -> bool {
    value.arena != 0
        && value.arena <= u64::MAX - 0x68
        && value.ring == value.arena + 0x18
        && value.head == 0
        && value.tail == 0
        && value.capacity == NATIVE_CAPACITY
        && value.cache != 0
        && value.cache_count != 0
        && value.cache_count <= 4096
        && value.cache <= u64::MAX - value.cache_count as u64 * 64
}
pub fn ready_fields(value: &ArenaFields) -> bool {
    reset_fields(value)
        && value.resource != 0
        && value.mapped != 0
        && value.mapped <= u64::MAX - value.capacity
}
/// The ready identity kept, with a monotonic live interval within the capacity.
pub fn current_fields(before: &ArenaFields, value: &ArenaFields) -> bool {
    value.arena == before.arena
        && value.ring == before.ring
        && value.cache == before.cache
        && value.cache_count == before.cache_count
        && value.capacity == before.capacity
        && value.resource == before.resource
        && value.mapped == before.mapped
        && value.tail <= value.head
        && value.head - value.tail <= value.capacity
        && value.head >= before.head
        && value.tail >= before.tail
}

#[derive(Clone, Copy, Default)]
struct Entry {
    call: Invocation,
    reset: u64,
    reset_complete: bool,
    poisoned: bool,
}
/// One thread's open guarded native invocations.
#[derive(Default)]
pub struct Scopes {
    entries: [Entry; MAX_SCOPES],
    count: usize,
}
impl Scopes {
    pub fn enter(&mut self, nonce: u64, arena: u64, thread: u64, kind: Kind) -> Invocation {
        if nonce == 0 || arena == 0 || thread == 0 || self.count == MAX_SCOPES {
            self.poison();
            return Invocation::default();
        }
        let mut call = Invocation {
            nonce,
            arena,
            thread,
            kind: kind as u64,
            parent_ready: 0,
        };
        if kind == Kind::Reset && self.count != 0 {
            let top = self.entries[self.count - 1].call;
            if top.kind == Kind::Ready as u64 && top.arena == arena && top.thread == thread {
                call.parent_ready = top.nonce;
            }
        }
        self.entries[self.count] = Entry {
            call,
            ..Entry::default()
        };
        self.count += 1;
        call
    }
    pub fn top(&self, call: &Invocation, thread: u64) -> bool {
        call.nonce != 0
            && call.thread == thread
            && self.count != 0
            && self.entries[self.count - 1].call == *call
    }
    pub fn finish_reset(&mut self, call: &Invocation, thread: u64, valid: bool) -> bool {
        if call.kind != Kind::Reset as u64 || !self.top(call, thread) {
            self.poison();
            return false;
        }
        self.pop();
        if call.parent_ready != 0 {
            if self.count == 0 || self.entries[self.count - 1].call.nonce != call.parent_ready {
                self.poison();
                return false;
            }
            let parent = &mut self.entries[self.count - 1];
            if parent.reset != 0 {
                parent.poisoned = true;
            }
            parent.reset = call.nonce;
            parent.reset_complete = valid;
        }
        valid
    }
    /// The nested reset's nonce, when the ready invocation completed one exact reset.
    pub fn finish_ready(&mut self, call: &Invocation, thread: u64) -> (bool, u64) {
        if call.kind != Kind::Ready as u64 || !self.top(call, thread) {
            self.poison();
            return (false, 0);
        }
        let complete = self.entries[self.count - 1];
        self.pop();
        (
            complete.reset != 0 && complete.reset_complete && !complete.poisoned,
            complete.reset,
        )
    }
    pub fn finish_destroy(&mut self, call: &Invocation, thread: u64) -> bool {
        if call.kind != Kind::Destroy as u64 || !self.top(call, thread) {
            self.poison();
            return false;
        }
        self.pop();
        true
    }
    pub fn cancel(&mut self, call: &Invocation, thread: u64) {
        if call.nonce != 0 && call.thread == thread {
            if let Some(i) = self.entries[..self.count]
                .iter()
                .rposition(|e| e.call == *call)
            {
                while self.count > i {
                    self.pop();
                }
            }
        }
        self.poison();
    }
    pub fn poison(&mut self) {
        for entry in &mut self.entries[..self.count] {
            entry.poisoned = true;
        }
    }
    pub fn clear(&mut self) {
        *self = Scopes::default();
    }
    fn pop(&mut self) {
        self.count -= 1;
        self.entries[self.count] = Entry::default();
    }
}

#[cfg(test)]
mod tests {
    //! tools/single_pass_tests/upload_arena_test.cpp's cases.
    use super::*;

    const BASE: u64 = 0x100000;
    struct Memory {
        bytes: [u8; 0x68],
        failed: bool,
    }
    impl Memory {
        fn ready() -> Self {
            let mut m = Memory {
                bytes: [0; 0x68],
                failed: false,
            };
            m.bytes[0..8].copy_from_slice(&0x200000u64.to_le_bytes());
            m.bytes[0x0C..0x10].copy_from_slice(&8u32.to_le_bytes());
            m.bytes[0x28..0x30].copy_from_slice(&NATIVE_CAPACITY.to_le_bytes());
            m.bytes[0x50..0x58].copy_from_slice(&0x300000u64.to_le_bytes());
            m.bytes[0x60..0x68].copy_from_slice(&0x1_0000_0000u64.to_le_bytes());
            m
        }
        fn read(&self, arena: u64) -> Option<ArenaFields> {
            let mut reader = |address: u64, out: &mut [u8]| {
                if self.failed || address != BASE || out.len() != self.bytes.len() {
                    return false;
                }
                out.copy_from_slice(&self.bytes);
                true
            };
            read_arena(Some(&mut reader), arena)
        }
    }

    #[test]
    fn arena_fields() {
        assert!(READY_RVA == 0xB82990 && READY_BYTES == 249 && READY_FNV == 0xFF20_A4D4_F912_F92B);
        assert!(RESET_RVA == 0xB82A8C && RESET_BYTES == 143 && RESET_FNV == 0x81DB_72CA_CEBD_5681);
        assert!(
            DESTROY_RVA == 0x5B30DC && DESTROY_BYTES == 88 && DESTROY_FNV == 0x17A1_6E7B_EE7A_FB4C
        );
        let mut memory = Memory::ready();
        let fields = memory.read(BASE).expect("the exact original arena fields");
        assert!(ready_fields(&fields));
        assert!(
            fields.ring == BASE + 0x18
                && fields.resource == 0x300000
                && fields.mapped == 0x1_0000_0000
        );
        assert_eq!(fields.cache_count, 8);
        let mut changed = ArenaFields {
            head: 256,
            ..fields
        };
        assert!(
            current_fields(&fields, &changed),
            "a live absolute head advance rejected"
        );
        changed.tail = 128;
        assert!(
            current_fields(&fields, &changed),
            "a live retired tail advance rejected"
        );
        changed.tail = 512;
        assert!(
            !current_fields(&fields, &changed),
            "a retired tail beyond the head admitted"
        );
        let changed = ArenaFields {
            head: NATIVE_CAPACITY + 1,
            ..fields
        };
        assert!(
            !current_fields(&fields, &changed),
            "an over-capacity live interval admitted"
        );
        let zeroed = [
            ArenaFields { arena: 0, ..fields },
            ArenaFields { ring: 0, ..fields },
            ArenaFields { cache: 0, ..fields },
            ArenaFields {
                capacity: 0,
                ..fields
            },
            ArenaFields {
                resource: 0,
                ..fields
            },
            ArenaFields {
                mapped: 0,
                ..fields
            },
        ];
        for changed in zeroed {
            assert!(!ready_fields(&changed) && !current_fields(&fields, &changed));
        }
        assert!(
            !ready_fields(&ArenaFields { head: 1, ..fields }),
            "an initialized arena adopted as fresh"
        );
        assert!(!ready_fields(&ArenaFields {
            cache_count: 4097,
            ..fields
        }));
        assert!(!ready_fields(&ArenaFields {
            mapped: u64::MAX - NATIVE_CAPACITY + 1,
            ..fields
        }));
        memory.failed = true;
        assert!(memory.read(BASE).is_none());
        memory.failed = false;
        assert!(memory.read(BASE + 1).is_none() && memory.read(u64::MAX - 0x60).is_none());
        assert!(read_arena(None, BASE).is_none());
    }
    #[test]
    fn invocation_scopes() {
        let mut scopes = Scopes::default();
        let ready = scopes.enter(1, BASE, 7, Kind::Ready);
        let resetting = scopes.enter(2, BASE, 7, Kind::Reset);
        assert!(resetting.parent_ready == ready.nonce && scopes.finish_reset(&resetting, 7, true));
        assert_eq!(scopes.finish_ready(&ready, 7), (true, 2));
        let ready = scopes.enter(3, BASE, 7, Kind::Ready);
        assert_eq!(
            scopes.finish_ready(&ready, 7),
            (false, 0),
            "a ready without a nested reset certified"
        );
        let resetting = scopes.enter(4, BASE, 7, Kind::Reset);
        assert!(resetting.parent_ready == 0 && scopes.finish_reset(&resetting, 7, true));
        let ready = scopes.enter(5, BASE, 7, Kind::Ready);
        assert!(
            !scopes.finish_ready(&ready, 7).0,
            "a historical standalone reset certified a later ready"
        );
        let ready = scopes.enter(6, BASE, 7, Kind::Ready);
        let resetting = scopes.enter(7, BASE + 0x100, 7, Kind::Reset);
        assert!(resetting.parent_ready == 0 && scopes.finish_reset(&resetting, 7, true));
        assert!(
            !scopes.finish_ready(&ready, 7).0,
            "a foreign arena reset certified ready"
        );
        let ready = scopes.enter(8, BASE, 7, Kind::Ready);
        let resetting = scopes.enter(9, BASE, 7, Kind::Reset);
        assert!(
            !scopes.finish_reset(&resetting, 8, true),
            "a foreign thread completed a native reset"
        );
        scopes.cancel(&ready, 7);
        assert!(
            !scopes.finish_ready(&ready, 7).0,
            "a cancelled ready certified a generation"
        );
        scopes.clear();
        let ready = scopes.enter(10, BASE, 7, Kind::Ready);
        let resetting = scopes.enter(11, BASE, 7, Kind::Reset);
        assert!(!scopes.finish_reset(&resetting, 7, false) && !scopes.finish_ready(&ready, 7).0);
        let ready = scopes.enter(12, BASE, 7, Kind::Ready);
        let resetting = scopes.enter(13, BASE, 7, Kind::Reset);
        assert!(scopes.finish_reset(&resetting, 7, true));
        let resetting = scopes.enter(14, BASE, 7, Kind::Reset);
        assert!(
            scopes.finish_reset(&resetting, 7, true) && !scopes.finish_ready(&ready, 7).0,
            "a repeated reset admitted"
        );
        let destroying = scopes.enter(15, BASE, 7, Kind::Destroy);
        assert!(scopes.finish_destroy(&destroying, 7));
        for i in 0..MAX_SCOPES as u64 {
            assert!(scopes.enter(20 + i, BASE + i * 0x100, 7, Kind::Ready).nonce != 0);
        }
        assert_eq!(
            scopes.enter(99, BASE, 7, Kind::Ready).nonce,
            0,
            "an unbounded invocation depth admitted"
        );
        scopes.clear();
        assert!(
            scopes.enter(0, BASE, 7, Kind::Ready).nonce == 0
                && scopes.enter(1, 0, 7, Kind::Ready).nonce == 0
        );
    }
}
