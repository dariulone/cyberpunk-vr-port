//! The prepared pair's query and membership readbacks (src/Render/NativePreparedPair.cpp, Poll and PollMembership):
//! the query readback mapped whole -- the three queries' statistics and timestamps exported, the baseline's readback
//! mark, the predicate word into the depth state's last but one, each mesh pair's equal and fallback words, with input
//! predicates their two words (accepted only as exactly one pass and no failure) -- then the mesh results and the
//! verdict judged as before; unmapped with nothing written. The membership readback mapped over its bytes for its rows.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp lends its exports and raises its errors from the returned bits, in its
//! order: 26 unmapped, 29 the inputs refused, 28 a malformed mesh result, 17 the verdict.

use core::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

use crate::prepared_pair_poll::{membership_rows, mesh_results, query_verdict, Verdict, MAX_ROWS, ROW};

/// The queries' bytes (QueryStride 96, three queries).
pub const PREDICATE_OFFSET: usize = 3 * 96;
pub const MESH_OFFSET: usize = PREDICATE_OFFSET + 8;
pub const INPUT_OFFSET: usize = MESH_OFFSET + 256 * 16;
pub const UNMAPPED: u32 = 1;
pub const INPUTS_REFUSED: u32 = 2;
pub const MESH_MALFORMED: u32 = 4;
pub const VERDICT_FAILED: u32 = 8;

/// The poll's facts.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct QueryRead {
    pub pairs: u32,
    pub predicates: bool,
    pub native_exit: bool,
    pub sequence: bool,
    pub draws: u64,
}
/// The exports it writes.
#[repr(C)]
pub struct QueryExports {
    pub gpu: *mut [u64; 36],
    pub baseline: *mut [u64; 24],
    pub depth_state: *mut [u64; 16],
    pub mesh_results: *mut [u64; 512],
    pub input_stats: *mut [u64; 8],
    pub mesh_stats: *mut [u64; 12],
    pub batch_stats: *mut [u64; 16],
    pub pair_stats: *mut [u64; 16],
    pub exit: *mut [u64; 24],
    pub rate: *mut [u64; 16],
}
const _: () = assert!(core::mem::size_of::<QueryRead>() == 16 && core::mem::size_of::<QueryExports>() == 80);

/// The exports, borrowed.
pub struct Exports<'a> {
    pub gpu: &'a mut [u64; 36],
    pub baseline: &'a mut [u64; 24],
    pub depth_state: &'a mut [u64; 16],
    pub mesh_results: &'a mut [u64; 512],
    pub input_stats: &'a mut [u64; 8],
    pub mesh_stats: &'a mut [u64; 12],
    pub batch_stats: &'a mut [u64; 16],
    pub pair_stats: &'a mut [u64; 16],
    pub exit: &'a mut [u64; 24],
    pub rate: &'a mut [u64; 16],
}

/// `words.len()` words from `bytes` at `at` (as memcpy, within the bytes there are).
fn copy_words(words: &mut [u64], bytes: &[u8], at: usize) {
    for (n, word) in words.iter_mut().enumerate() {
        let start = at + n * 8;
        if let Some(chunk) = bytes.get(start..start + 8) {
            *word = u64::from_le_bytes(chunk.try_into().unwrap());
        }
    }
}
/// The mapped queries read into the exports; the error bits after the map.
pub fn read(bytes: &[u8], q: &QueryRead, e: &mut Exports<'_>) -> u32 {
    let mut errors = 0;
    copy_words(e.gpu, bytes, 0);
    e.baseline[23] = 1;
    copy_words(&mut e.depth_state[14..15], bytes, PREDICATE_OFFSET);
    let words = (q.pairs as usize * 2).min(512);
    copy_words(&mut e.mesh_results[..words], bytes, MESH_OFFSET);
    if q.predicates {
        copy_words(&mut e.input_stats[5..7], bytes, INPUT_OFFSET);
        if e.input_stats[5] != 1 || e.input_stats[6] != 0 {
            errors |= INPUTS_REFUSED;
        }
    }
    if mesh_results(&*e.mesh_results, q.pairs, &mut *e.mesh_stats, &mut *e.batch_stats, &mut *e.pair_stats) {
        errors |= MESH_MALFORMED;
    }
    let verdict = Verdict { input: &*e.input_stats, gpu: &*e.gpu, mesh: &*e.mesh_stats, baseline: &mut *e.baseline, exit: &mut *e.exit, rate: &mut *e.rate, pair_stats: &mut *e.pair_stats };
    if query_verdict(q.predicates, q.native_exit, q.sequence, q.draws, verdict) {
        errors |= VERDICT_FAILED;
    }
    errors
}
/// `f` over the whole readback mapped (None when it cannot be mapped).
fn mapped<R>(readback: &ID3D12Resource, bytes: u64, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
    let mut data = core::ptr::null_mut();
    // SAFETY: a live READBACK buffer of at least `bytes`.
    if unsafe { readback.Map(0, Some(&D3D12_RANGE { Begin: 0, End: bytes as usize }), Some(&mut data)) }.is_err() || data.is_null() {
        return None;
    }
    // SAFETY: the mapping holds `bytes`.
    let result = f(unsafe { core::slice::from_raw_parts(data as *const u8, bytes as usize) });
    // SAFETY: as above; nothing written.
    unsafe { readback.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 })) };
    Some(result)
}
/// Poll's query readback: the error bits.
pub fn query_readback(readback: &ID3D12Resource, q: &QueryRead, e: &mut Exports<'_>) -> u32 {
    // SAFETY: a live buffer.
    let width = unsafe { readback.GetDesc() }.Width;
    mapped(readback, width, |bytes| read(bytes, q, e)).unwrap_or(UNMAPPED)
}

/// Poll's query readback.
///
/// # Safety
/// A live readback; `q` lent; every export pointer live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_query_readback(readback: *mut c_void, q: *const QueryRead, exports: *const QueryExports) -> u32 {
    // SAFETY: as documented.
    let Some(readback) = (unsafe { ID3D12Resource::from_raw_borrowed(&readback) }) else { return UNMAPPED };
    let x = unsafe { &*exports };
    let mut e = unsafe {
        Exports {
            gpu: &mut *x.gpu,
            baseline: &mut *x.baseline,
            depth_state: &mut *x.depth_state,
            mesh_results: &mut *x.mesh_results,
            input_stats: &mut *x.input_stats,
            mesh_stats: &mut *x.mesh_stats,
            batch_stats: &mut *x.batch_stats,
            pair_stats: &mut *x.pair_stats,
            exit: &mut *x.exit,
            rate: &mut *x.rate,
        }
    };
    query_readback(readback, unsafe { &*q }, &mut e)
}
/// PollMembership's readback over its `bytes`: 0 its rows valid, 1 unmapped, 2 invalid.
///
/// # Safety
/// A live readback of at least `bytes`; the arrays hold 32 * 512, 24 and 8 values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_membership_readback(readback: *mut c_void, bytes: u64, count: u32, rows: *mut [u64; ROW * MAX_ROWS], stats: *mut [u64; 24], input: *const [u64; 8]) -> u32 {
    // SAFETY: as documented.
    let Some(readback) = (unsafe { ID3D12Resource::from_raw_borrowed(&readback) }) else { return 1 };
    match mapped(readback, bytes, |data| unsafe { membership_rows(data, count, &mut *rows, &mut *stats, &*input) }) {
        None => 1,
        Some(true) => 0,
        Some(false) => 2,
    }
}

#[cfg(test)]
mod tests {
    //! The words read, the input refusal, a malformed mesh pair, the verdict; mapped on WARP.
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    struct Arrays {
        gpu: [u64; 36],
        baseline: [u64; 24],
        depth: [u64; 16],
        results: [u64; 512],
        input: [u64; 8],
        mesh: [u64; 12],
        batch: [u64; 16],
        pair: [u64; 16],
        exit: [u64; 24],
        rate: [u64; 16],
    }
    impl Arrays {
        fn new() -> Self {
            Arrays { gpu: [0; 36], baseline: [0; 24], depth: [0; 16], results: [0; 512], input: [0; 8], mesh: [0; 12], batch: [0; 16], pair: [0; 16], exit: [0; 24], rate: [0; 16] }
        }
        fn exports(&mut self) -> Exports<'_> {
            Exports {
                gpu: &mut self.gpu,
                baseline: &mut self.baseline,
                depth_state: &mut self.depth,
                mesh_results: &mut self.results,
                input_stats: &mut self.input,
                mesh_stats: &mut self.mesh,
                batch_stats: &mut self.batch,
                pair_stats: &mut self.pair,
                exit: &mut self.exit,
                rate: &mut self.rate,
            }
        }
    }
    fn bytes(pairs: &[(u64, u64)], input: (u64, u64)) -> Vec<u8> {
        let mut out = vec![0u8; INPUT_OFFSET + 16];
        let mut put = |at: usize, value: u64| out[at..at + 8].copy_from_slice(&value.to_le_bytes());
        for query in 0..3 {
            for word in [2, 7, 11] {
                put((query * 12 + word) * 8, 5);
            }
        }
        put(PREDICATE_OFFSET, 0xABCD);
        for (n, &(equal, fallback)) in pairs.iter().enumerate() {
            put(MESH_OFFSET + n * 16, equal);
            put(MESH_OFFSET + n * 16 + 8, fallback);
        }
        put(INPUT_OFFSET, input.0);
        put(INPUT_OFFSET + 8, input.1);
        out
    }

    #[test]
    fn query_words_and_verdict() {
        let q = QueryRead { pairs: 2, predicates: true, native_exit: false, sequence: true, draws: 4 };
        let mut a = Arrays::new();
        assert_eq!(read(&bytes(&[(1, 0), (0, 1)], (1, 0)), &q, &mut a.exports()), 0);
        assert_eq!((a.gpu[2], a.gpu[35], a.baseline[23], a.depth[14]), (5, 5, 1, 0xABCD));
        assert_eq!((&a.results[..4], a.input[5], a.input[6]), (&[1, 0, 0, 1][..], 1, 0));
        assert_eq!((a.mesh[4], a.mesh[5], a.pair[3], a.pair[5], a.baseline[20]), (1, 1, 1, 5, 1));
        let mut a = Arrays::new();
        assert_eq!(read(&bytes(&[(1, 0)], (1, 1)), &QueryRead { pairs: 1, ..q }, &mut a.exports()), INPUTS_REFUSED, "a failed input pass");
        assert_eq!((a.baseline[22], a.pair[5]), (1, 0));
        let mut a = Arrays::new();
        assert_eq!(read(&bytes(&[(1, 1)], (1, 0)), &QueryRead { pairs: 1, ..q }, &mut a.exports()), MESH_MALFORMED, "both results");
        let mut a = Arrays::new();
        let unchecked = QueryRead { predicates: false, ..q };
        assert_eq!(read(&bytes(&[(1, 0), (1, 0)], (7, 7)), &unchecked, &mut a.exports()), 0, "no input predicates read");
        assert_eq!(a.input[5], 0);
        let mut a = Arrays::new();
        assert_eq!(read(&bytes(&[(1, 0), (0, 1)], (1, 0)), &QueryRead { sequence: false, ..q }, &mut a.exports()), VERDICT_FAILED, "a fallback outside a sequence");
    }

    #[test]
    fn mapped_on_warp() {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        let data = bytes(&[(1, 0)], (1, 0));
        let buffer = crate::readback_buffers::buffer(device.as_ref().unwrap(), None, data.len() as u64, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ).unwrap();
        let mut at = core::ptr::null_mut();
        unsafe { buffer.Map(0, None, Some(&mut at)) }.unwrap();
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), at.cast::<u8>(), data.len()) };
        unsafe { buffer.Unmap(0, None) };
        let mut a = Arrays::new();
        let q = QueryRead { pairs: 1, predicates: true, native_exit: true, sequence: true, draws: 1 };
        assert_eq!(query_readback(&buffer, &q, &mut a.exports()), 0);
        assert_eq!((a.depth[14], a.results[0], a.exit[22], a.rate[15]), (0xABCD, 1, 0, 0));
    }
}
