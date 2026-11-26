//! Research probe: the shape of each render graph the engine runs (formerly src/Stereo/RenderGraphShape.cpp; the file
//! keeps the exported generation and slots and the detour's registration, wanted with CVR_GRAPH_SHAPE set).
//! sub_1403E8F58(owner, graph, _, args, done) runs a view's compiled render graph: one RunRenderJob per node, each job's
//! prerequisites wired from the graph's edges. A node record is 48 bytes at graph+24 (listed through the DWORD indices
//! at graph+40, count graph+16): +0 its job index, +16 the node. An edge record is 24 bytes at graph+72 (indices
//! graph+88, count graph+64): +4 nonzero = not a dependency, +8 / +12 the records of the node before and the node after.
//! Every distinct graph run while CyberpunkVR_GraphShapeGen is nonzero is recorded once per generation into
//! CyberpunkVR_GraphShapes: its counts, the longest chain, the widest level, each node's work function and the edges
//! (tools/hitch_tests/graph_shape.py reads them). The engine's memory is read under SEH, a fault recorded as 0xFFFFFFFF
//! nodes.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering::*};
use std::sync::Mutex;

pub(crate) const SLOTS: usize = 8;
const MAX_NODES: usize = 512;
const MAX_EDGES: usize = 2048;

/// CvrGraphShape (the exported slots' layout).
#[repr(C)]
pub struct GraphShape {
    graph: u64,
    runs: u64,
    gen: u32,
    nodes: u32,
    edges: u32,
    depth: u32,
    width: u32,
    reserved: u32,
    /// node work function RVA, by job index
    work: [u32; MAX_NODES],
    /// job index before, job index after
    edge: [[u16; 2]; MAX_EDGES],
}
const _: () = assert!(core::mem::size_of::<GraphShape>() == 40 + 4 * MAX_NODES + 4 * MAX_EDGES);

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_GraphShapeGen: AtomicU32;
    static mut CyberpunkVR_GraphShapes: [GraphShape; SLOTS];
}
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

/// The trampoline MinHook writes (src/Stereo/RenderGraphShape.cpp's CVR_DETOUR_IF line names it).
#[no_mangle]
pub static mut cvr_stereo_graph_run_original: usize = 0;
static LOCK: Mutex<()> = Mutex::new(());

fn exe_base() -> usize {
    static BASE: AtomicUsize = AtomicUsize::new(0);
    let mut base = BASE.load(Relaxed);
    if base == 0 {
        // SAFETY: the process image's own handle.
        base = unsafe { GetModuleHandleW(core::ptr::null()) } as usize;
        BASE.store(base, Relaxed);
    }
    base
}

unsafe fn rd<T: Copy>(at: *const u8) -> T {
    core::ptr::read_unaligned(at as *const T)
}
/// One graph's shape into `s` (under SEH: called through cvr_engine::code::guarded_call3).
unsafe extern "C" fn record(s: *mut GraphShape, graph: *const u8, gen: usize) -> bool {
    let s = &mut *s;
    let n: u32 = rd(graph.add(16));
    let records: *const u8 = rd(graph.add(24));
    let list: *const u32 = rd(graph.add(40));
    let e: u32 = rd(graph.add(64));
    let edges: *const u8 = rd(graph.add(72));
    let elist: *const u32 = rd(graph.add(88));
    s.graph = graph as u64;
    s.gen = gen as u32;
    s.nodes = n;
    let nn = (n as usize).min(MAX_NODES);
    let base = exe_base();
    for i in 0..nn {
        let rec = records.add(48 * (*list.add(i) & 0xDFFF_FFFF) as usize);
        let job: u32 = rd(rec);
        let node: usize = rd(rec.add(16));
        let mut work = 0usize;
        if node != 0 {
            let vtable: usize = rd(node as *const u8);
            if vtable != 0 {
                work = rd((vtable + 8) as *const u8);
            }
        }
        if (job as usize) < MAX_NODES {
            s.work[job as usize] = if work > base { (work - base) as u32 } else { 0 };
        }
    }
    let mut kept = 0usize;
    let (mut indeg, mut level, mut queue, mut count) = ([0u16; MAX_NODES], [0u16; MAX_NODES], [0u16; MAX_NODES], [0u16; MAX_NODES]);
    for i in 0..e as usize {
        if kept >= MAX_EDGES {
            break;
        }
        let rec = edges.add(24 * (*elist.add(i) & 0xBFFF_FFFF) as usize);
        if rd::<u32>(rec.add(4)) != 0 {
            continue;
        }
        let a: u32 = rd(records.add(48 * (rd::<u32>(rec.add(8)) & 0xDFFF_FFFF) as usize));
        let b: u32 = rd(records.add(48 * (rd::<u32>(rec.add(12)) & 0xDFFF_FFFF) as usize));
        if a as usize >= nn || b as usize >= nn {
            continue;
        }
        s.edge[kept] = [a as u16, b as u16];
        indeg[b as usize] = indeg[b as usize].wrapping_add(1);
        kept += 1;
    }
    s.edges = kept as u32;
    // Kahn's order by levels: the longest chain and the widest level.
    let (mut head, mut tail, mut depth) = (0usize, 0usize, 0u32);
    for i in 0..nn {
        if indeg[i] == 0 {
            queue[tail] = i as u16;
            tail += 1;
            level[i] = 1;
        }
    }
    while head < tail {
        let v = queue[head] as usize;
        head += 1;
        if level[v] as u32 > depth {
            depth = level[v] as u32;
        }
        count[level[v] as usize - 1] = count[level[v] as usize - 1].wrapping_add(1);
        for k in 0..kept {
            if s.edge[k][0] as usize != v {
                continue;
            }
            let w = s.edge[k][1] as usize;
            if (level[w] as u32) < level[v] as u32 + 1 {
                level[w] = (level[v] as u32 + 1) as u16;
            }
            indeg[w] = indeg[w].wrapping_sub(1);
            if indeg[w] == 0 {
                queue[tail] = w as u16;
                tail += 1;
            }
        }
    }
    let mut width = 0u32;
    for c in count.iter().take((depth as usize).min(MAX_NODES)) {
        if *c as u32 > width {
            width = *c as u32;
        }
    }
    s.depth = depth;
    s.width = width;
    true
}

/// The render graph run sub_1403E8F58's detour.
///
/// # Safety
/// Called by the engine in place of its graph run, with its arguments.
#[no_mangle]
pub unsafe extern "C" fn cvr_stereo_graph_run(owner: *mut i64, graph: *mut u8, a3: i64, args: i64, done: i64) -> i64 {
    let gen = CyberpunkVR_GraphShapeGen.load(Relaxed);
    if gen != 0 && !graph.is_null() {
        let _held = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let shapes = &mut *(&raw mut CyberpunkVR_GraphShapes);
        let mut slot = shapes.iter().position(|s| s.graph == graph as u64);
        if slot.is_none() {
            slot = shapes.iter().position(|s| s.graph == 0 || s.gen != gen);
            if let Some(i) = slot {
                shapes[i].graph = 0;
                shapes[i].runs = 0;
            }
        }
        if let Some(i) = slot {
            let s = &mut shapes[i];
            s.runs += 1;
            if s.gen != gen || s.graph == 0 {
                let ok = cvr_engine::code::guarded_call3(
                    record as *const () as usize,
                    s as *mut GraphShape as *mut c_void,
                    graph as *mut c_void,
                    gen as usize as *mut c_void,
                );
                if ok.is_none() {
                    s.nodes = 0xFFFF_FFFF;
                    s.gen = gen;
                    s.graph = graph as u64;
                }
            }
        }
    }
    let original: unsafe extern "C" fn(*mut i64, *mut u8, i64, i64, i64) -> i64 = core::mem::transmute(cvr_stereo_graph_run_original);
    original(owner, graph, a3, args, done)
}
