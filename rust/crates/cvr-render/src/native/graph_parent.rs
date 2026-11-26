//! A render graph node's parent receipt, resolved from the native context's exact view and order slot
//! (include/Render/NativeGraphParent.hpp).
//!
//! `CodeReceipt` is a one-shot original-image certificate captured before our detours are installed (a patched entry
//! cannot be compared against the original whole-function hash; FNV-1a over each function's bytes). Native 1ECF2C
//! copies the node metadata+5 to context+38 and resolves the metadata's view index through context.manager+48;
//! native 1EC7EC copies both values into asynchronous child contexts. These are order slots, not inferred stage IDs.
//! `resolve` is a source-bound classification only: it authorizes neither changed scheduling nor the reuse of target
//! allocations, and every native phase-1 slot must be unique.
use super::graph_snapshot::{field_u32, field_u64, Node, Read, Snapshot};
use std::collections::BTreeSet;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Code {
    pub rva: u32,
    pub bytes: u32,
    pub hash: u64,
}

const _: () = assert!(
    core::mem::size_of::<Code>() == 16
        && core::mem::size_of::<Context>() == 32
        && core::mem::size_of::<Receipt>() == 40
);

#[derive(Clone, Copy, Debug, Default)]
pub struct CodeReceipt {
    base: u64,
    attempted: bool,
    verified: bool,
}
impl CodeReceipt {
    /// The functions' original bytes certified once (the caller serializes this and keeps the image alive).
    pub fn capture(&mut self, base: u64, read: Option<Read<'_>>, functions: &[Code]) -> bool {
        if self.attempted {
            return false;
        }
        self.attempted = true;
        let Some(read) = read else { return false };
        if base == 0 || functions.is_empty() || functions.len() > 64 {
            return false;
        }
        let mut bytes = [0u8; 512];
        for code in functions {
            if code.bytes == 0
                || code.bytes as usize > bytes.len()
                || base > u64::MAX - code.rva as u64
                || base + code.rva as u64 > u64::MAX - code.bytes as u64
                || !read(base + code.rva as u64, &mut bytes[..code.bytes as usize])
            {
                return false;
            }
            let hash = bytes[..code.bytes as usize]
                .iter()
                .fold(14695981039346656037u64, |h, &b| {
                    (h ^ b as u64).wrapping_mul(1099511628211)
                });
            if hash != code.hash {
                return false;
            }
        }
        self.base = base;
        self.verified = true;
        true
    }
    pub fn attempted(&self) -> bool {
        self.attempted
    }
    pub fn verified_for(&self, base: u64) -> bool {
        self.verified && base == self.base
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Context {
    pub address: u64,
    pub view: u64,
    pub manager: u64,
    pub view_index: u32,
    pub slot: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Receipt {
    pub object: u64,
    pub work: u64,
    pub name: u64,
    pub handle: u32,
    pub ordinal: u32,
    pub view_index: u32,
    pub slot: u32,
}

fn read_u64(read: &mut dyn FnMut(u64, &mut [u8]) -> bool, address: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    read(address, &mut b).then(|| u64::from_le_bytes(b))
}
fn context_once(read: &mut dyn FnMut(u64, &mut [u8]) -> bool, address: u64) -> Option<Context> {
    if address == 0 || address > u64::MAX - 0x40 {
        return None;
    }
    let view = read_u64(read, address + 0x18)?;
    let manager = read_u64(read, address + 0x20)?;
    let mut slot = [0u8; 1];
    if !read(address + 0x38, &mut slot) || view == 0 || manager == 0 || manager > u64::MAX - 0x58 {
        return None;
    }
    let views = read_u64(read, manager + 0x48)?;
    let mut count = [0u8; 4];
    if !read(manager + 0x54, &mut count) {
        return None;
    }
    let count = u32::from_le_bytes(count);
    if views == 0 || count == 0 || count > 64 || views > u64::MAX - count as u64 * 8 {
        return None;
    }
    let (mut view_index, mut matches) = (0u32, 0u32);
    for i in 0..count {
        if read_u64(read, views + i as u64 * 8)? == view {
            view_index = i;
            matches += 1;
        }
    }
    (matches == 1).then_some(Context {
        address,
        view,
        manager,
        view_index,
        slot: slot[0] as u32,
    })
}
/// The context at `address` (its view found exactly once among its manager's views), read twice and compared.
pub fn read_context(read: Option<Read<'_>>, address: u64) -> Option<Context> {
    let read = read?;
    let a = context_once(read, address)?;
    let b = context_once(read, address)?;
    (a == b).then_some(a)
}
/// The source node in the context's order slot (and view), with its object, work function and name.
pub fn resolve(source: &Snapshot, context: &Context) -> Option<Receipt> {
    if source.failure != 0
        || source.nodes.is_empty()
        || source.nodes.len() > 255
        || context.address == 0
        || context.view == 0
        || context.manager == 0
    {
        return None;
    }
    let (mut slots, mut ids) = (BTreeSet::new(), BTreeSet::new());
    let mut found: Option<&Node> = None;
    for node in &source.nodes {
        let id = node.handle & 0xDFFF_FFFF;
        let slot = (field_u32(&node.bytes, 28) >> 8) & 255;
        if node.order as usize >= source.nodes.len()
            || field_u32(&node.bytes, 0) != node.order
            || id >= 65536
            || !ids.insert(id)
            || slot as usize >= source.nodes.len()
            || !slots.insert(slot)
        {
            return None;
        }
        if slot == context.slot {
            if field_u32(&node.bytes, 24) != context.view_index {
                return None;
            }
            found = Some(node);
        }
    }
    let found = found?;
    if found.work == 0
        || found.vtable == 0
        || field_u64(&found.bytes, 16) == 0
        || field_u64(&found.definition, 0) != found.vtable
    {
        return None;
    }
    Some(Receipt {
        object: field_u64(&found.bytes, 16),
        work: found.work,
        name: field_u64(&found.definition, 24),
        handle: found.handle,
        ordinal: found.order,
        view_index: context.view_index,
        slot: context.slot,
    })
}

#[cfg(test)]
mod tests {
    //! tools/single_pass_tests/graph_parent_test.cpp's cases.
    use super::*;
    use std::cell::RefCell;

    const START: u64 = 0x10000;
    const CONTEXT: u64 = START;
    const MANAGER: u64 = START + 128;
    const VIEWS: u64 = START + 256;

    struct World {
        memory: Vec<u8>,
        calls: u64,
        change_at: u64,
    }
    thread_local! {
        static W: RefCell<World> = RefCell::new(World { memory: vec![0; 4096], calls: 0, change_at: 0 });
    }
    fn put(address: u64, b: &[u8]) {
        W.with(|w| {
            let at = (address - START) as usize;
            w.borrow_mut().memory[at..at + b.len()].copy_from_slice(b);
        });
    }
    fn read(address: u64, out: &mut [u8]) -> bool {
        let change = W.with(|w| {
            let mut w = w.borrow_mut();
            w.calls += 1;
            w.calls == w.change_at
        });
        if change {
            put(CONTEXT + 0x38, &[2]);
        }
        W.with(|w| {
            let w = w.borrow();
            let size = w.memory.len() as u64;
            if address < START
                || out.len() as u64 > size
                || address - START > size - out.len() as u64
            {
                return false;
            }
            let at = (address - START) as usize;
            out.copy_from_slice(&w.memory[at..at + out.len()]);
            true
        })
    }
    fn calls() -> u64 {
        W.with(|w| w.borrow().calls)
    }
    fn setup() {
        W.with(|w| {
            *w.borrow_mut() = World {
                memory: vec![0; 4096],
                calls: 0,
                change_at: 0,
            }
        });
        put(CONTEXT + 0x18, &0xabcu64.to_le_bytes());
        put(CONTEXT + 0x20, &MANAGER.to_le_bytes());
        put(CONTEXT + 0x38, &[3]);
        put(MANAGER + 0x48, &VIEWS.to_le_bytes());
        put(MANAGER + 0x54, &2u32.to_le_bytes());
        put(VIEWS, &0xabcu64.to_le_bytes());
        put(VIEWS + 8, &0xdefu64.to_le_bytes());
    }
    fn field(bytes: &mut [u8], offset: usize, value: &[u8]) {
        bytes[offset..offset + value.len()].copy_from_slice(value);
    }
    fn graph() -> Snapshot {
        let mut s = Snapshot::default();
        for i in 0..4u32 {
            let mut n = Node {
                handle: (100 + i * 7) | 0x2000_0000,
                order: i,
                bytes: [0; 48],
                vtable: 0x30000 + i as u64 * 8,
                work: 0x1_401e_c1d0,
                definition: [0; 32],
            };
            field(&mut n.bytes, 0, &i.to_le_bytes());
            field(&mut n.bytes, 16, &(0x20000 + i as u64 * 64).to_le_bytes());
            field(&mut n.bytes, 24, &(i / 2).to_le_bytes());
            field(&mut n.bytes, 28, &(i | ((3 - i) << 8)).to_le_bytes());
            let vtable = n.vtable;
            field(&mut n.definition, 0, &vtable.to_le_bytes());
            field(&mut n.definition, 24, &(0x40000 + i as u64).to_le_bytes());
            s.nodes.push(n);
        }
        s
    }

    #[test]
    fn code_receipt() {
        setup();
        // Four zero bytes.
        let code = Code {
            rva: 0x300,
            bytes: 4,
            hash: 0x4D25_767F_9DCE_13F5,
        };
        let mut original = CodeReceipt::default();
        assert!(original.capture(START, Some(&mut read), &[code]) && original.verified_for(START));
        let certified_reads = calls();
        put(START + 0x300, &0xE9u32.to_le_bytes());
        assert!(
            original.verified_for(START)
                && !original.verified_for(START + 0x10000)
                && calls() == certified_reads
        );
        assert!(!original.capture(START, Some(&mut read), &[code]) && original.verified_for(START));
        let mut patched = CodeReceipt::default();
        assert!(
            !patched.capture(START, Some(&mut read), &[code])
                && patched.attempted()
                && !patched.verified_for(START)
        );
        put(START + 0x300, &0u32.to_le_bytes());
        assert!(!patched.capture(START, Some(&mut read), &[code]) && !patched.verified_for(START));
        for bad_code in [
            Code {
                rva: 0x300,
                bytes: 0,
                hash: code.hash,
            },
            Code {
                rva: 0x300,
                bytes: 513,
                hash: code.hash,
            },
            Code {
                rva: u32::MAX,
                bytes: 4,
                hash: code.hash,
            },
        ] {
            let mut bad = CodeReceipt::default();
            assert!(!bad.capture(START, Some(&mut read), &[bad_code]) && !bad.verified_for(START));
        }
        let mut missing = CodeReceipt::default();
        assert!(!missing.capture(START, None, &[code]));
        let mut overflow = CodeReceipt::default();
        assert!(!overflow.capture(u64::MAX - 1, Some(&mut read), &[code]));
    }
    #[test]
    fn view_and_order_parent() {
        setup();
        let mut context = read_context(Some(&mut read), CONTEXT).expect("the context");
        assert!(context.view_index == 0 && context.slot == 3);
        let reads = calls();
        let source = graph();
        let receipt = resolve(&source, &context).expect("the parent");
        assert!(
            receipt.handle == source.nodes[0].handle
                && receipt.ordinal == 0
                && receipt.name == 0x40000
        );
        context.view_index = 1;
        assert!(resolve(&source, &context).is_none());
        context.view_index = 0;
        let mut bad = source.clone();
        let order = field_u32(&bad.nodes[0].bytes, 28);
        field(&mut bad.nodes[1].bytes, 28, &order.to_le_bytes());
        assert!(resolve(&bad, &context).is_none());
        let mut bad = source.clone();
        bad.nodes[0].definition[0] ^= 1;
        assert!(resolve(&bad, &context).is_none());
        let mut bad = source.clone();
        bad.failure = 1;
        assert!(resolve(&bad, &context).is_none());
        let mut bad = source.clone();
        bad.nodes[1].handle = bad.nodes[0].handle;
        assert!(resolve(&bad, &context).is_none());
        let mut bad = source.clone();
        field(&mut bad.nodes[1].bytes, 0, &9u32.to_le_bytes());
        assert!(resolve(&bad, &context).is_none());
        setup();
        put(VIEWS + 8, &0xabcu64.to_le_bytes());
        assert!(read_context(Some(&mut read), CONTEXT).is_none());
        setup();
        put(MANAGER + 0x54, &65u32.to_le_bytes());
        assert!(read_context(Some(&mut read), CONTEXT).is_none());
        setup();
        put(CONTEXT + 0x20, &(u64::MAX - 4).to_le_bytes());
        assert!(read_context(Some(&mut read), CONTEXT).is_none());
        setup();
        W.with(|w| w.borrow_mut().change_at = reads / 2 + 1);
        assert!(read_context(Some(&mut read), CONTEXT).is_none());
        setup();
        put(CONTEXT + 0x18, &0xdefu64.to_le_bytes());
        put(CONTEXT + 0x38, &[1]);
        let context = read_context(Some(&mut read), CONTEXT).expect("the copied context");
        assert_eq!(resolve(&source, &context).map(|r| r.ordinal), Some(2));
    }
}
