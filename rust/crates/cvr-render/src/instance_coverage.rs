//! THE PEER'S INSTANCE COVERAGE (include/Render/InstanceCoverage.hpp): the plan matching each peer instance (48 or
//! 64 bytes) to a not yet covered, byte-identical instance of the caller's sources -- a source may concatenate one
//! native main batch and its owned peer-only additions; an instance whose rendered-view mask lacks the peer layer
//! counts as used -- with the matched peer instances' contiguous runs and the unmatched count; and the commit marking
//! a plan's matches covered only if every one is still free and rendered for the peer. No coverage changes until the
//! commit. The header forwards to these in the Rust build (the coverage flags lent as bytes);
//! tools/single_pass_tests/instance_coverage_test runs its cases against them.

/// InstanceSource, lent: its bytes, its coverage flags (one byte each) and its optional rendered-view masks.
#[repr(C)]
pub struct SourceView {
    pub bytes: *const u8,
    pub byte_count: usize,
    pub covered: *mut u8,
    pub covered_count: usize,
    pub has_covered: bool,
    pub view_masks: *const u8,
    pub mask_count: usize,
}
/// InstanceMatch.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct InstanceMatch {
    pub source: u32,
    pub instance: u32,
    pub peer: u32,
}
/// InstanceRun.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct InstanceRun {
    pub first: u32,
    pub count: u32,
}
#[derive(Default, Debug)]
pub struct Plan {
    pub matches: Vec<InstanceMatch>,
    pub runs: Vec<InstanceRun>,
    pub unmatched: u32,
}
const _: () = assert!(
    core::mem::size_of::<SourceView>() == 56
        && core::mem::size_of::<InstanceMatch>() == 12
        && core::mem::size_of::<InstanceRun>() == 8
);

/// A lent source.
pub struct Source<'a> {
    pub bytes: &'a [u8],
    pub covered: Option<Vec<bool>>,
    pub view_masks: &'a [u8],
}

/// PlanInstanceCoverage.
pub fn plan(sources: &[Source], peer: &[u8], stride: u32) -> Option<Plan> {
    let stride = stride as usize;
    if (stride != 48 && stride != 64)
        || peer.is_empty()
        || peer.len() % stride != 0
        || peer.len() > 16384
        || sources.len() > 128
    {
        return None;
    }
    let mut used: Vec<Vec<bool>> = Vec::with_capacity(sources.len());
    for source in sources {
        let covered = source.covered.as_ref()?;
        if source.bytes.is_empty()
            || source.bytes.len() % stride != 0
            || source.bytes.len() > 32768
            || covered.len() != source.bytes.len() / stride
            || (!source.view_masks.is_empty() && source.view_masks.len() != covered.len())
        {
            return None;
        }
        let mut flags = covered.clone();
        for (i, &mask) in source.view_masks.iter().enumerate() {
            if mask == 0 || mask > 3 {
                return None;
            }
            if mask & 1 == 0 {
                flags[i] = true;
            }
        }
        used.push(flags);
    }
    let mut out = Plan::default();
    for (p, instance) in peer.chunks_exact(stride).enumerate() {
        let p = p as u32;
        let mut found = false;
        'sources: for (s, source) in sources.iter().enumerate() {
            for i in 0..used[s].len() {
                if used[s][i] || instance != &source.bytes[i * stride..(i + 1) * stride] {
                    continue;
                }
                used[s][i] = true;
                out.matches.push(InstanceMatch {
                    source: s as u32,
                    instance: i as u32,
                    peer: p,
                });
                found = true;
                match out.runs.last_mut() {
                    Some(run) if run.first + run.count == p => run.count += 1,
                    _ => out.runs.push(InstanceRun { first: p, count: 1 }),
                }
                break 'sources;
            }
        }
        if !found {
            out.unmatched += 1;
        }
    }
    Some(out)
}

/// CommitInstanceCoverage: the sources' new coverage.
pub fn commit(sources: &[Source], matches: &[InstanceMatch]) -> Option<Vec<Vec<bool>>> {
    let mut used = Vec::with_capacity(sources.len());
    for source in sources {
        let covered = source.covered.as_ref()?;
        if !source.view_masks.is_empty()
            && (source.view_masks.len() != covered.len()
                || source.view_masks.iter().any(|&m| m == 0 || m > 3))
        {
            return None;
        }
        used.push(covered.clone());
    }
    for m in matches {
        let (s, i) = (m.source as usize, m.instance as usize);
        if s >= used.len() || i >= used[s].len() || used[s][i] {
            return None;
        }
        let masks = sources[s].view_masks;
        if !masks.is_empty() && (masks.len() != used[s].len() || masks[i] & 1 == 0 || masks[i] > 3)
        {
            return None;
        }
        used[s][i] = true;
    }
    Some(used)
}

unsafe fn sources_of<'a>(views: &'a [SourceView]) -> Vec<Source<'a>> {
    let slice = |p: *const u8, n: usize| {
        if n == 0 || p.is_null() {
            &[][..]
        } else {
            unsafe { core::slice::from_raw_parts(p, n) }
        }
    };
    views
        .iter()
        .map(|v| Source {
            bytes: slice(v.bytes, v.byte_count),
            covered: v.has_covered.then(|| {
                slice(v.covered, v.covered_count)
                    .iter()
                    .map(|&b| b != 0)
                    .collect()
            }),
            view_masks: slice(v.view_masks, v.mask_count),
        })
        .collect()
}
unsafe fn views<'a>(sources: *const SourceView, count: usize) -> &'a [SourceView] {
    if count == 0 || sources.is_null() {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(sources, count) }
    }
}

/// PlanInstanceCoverage: the plan (read it with cvr_ic_plan_view, free it with cvr_ic_free) or null.
///
/// # Safety
/// `sources` holds `count` lent sources; `peer` holds `peer_bytes`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ic_plan(
    sources: *const SourceView,
    count: usize,
    peer: *const u8,
    peer_bytes: usize,
    stride: u32,
) -> *mut Plan {
    unsafe {
        let sources = sources_of(views(sources, count));
        let peer = if peer_bytes == 0 || peer.is_null() {
            &[][..]
        } else {
            core::slice::from_raw_parts(peer, peer_bytes)
        };
        match plan(&sources, peer, stride) {
            Some(p) => Box::into_raw(Box::new(p)),
            None => core::ptr::null_mut(),
        }
    }
}

/// A plan's matches, runs and unmatched count (borrowed until cvr_ic_free).
///
/// # Safety
/// `plan` comes from cvr_ic_plan; the outputs are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ic_plan_view(
    plan: *const Plan,
    matches: *mut *const InstanceMatch,
    match_count: *mut usize,
    runs: *mut *const InstanceRun,
    run_count: *mut usize,
) -> u32 {
    unsafe {
        let p = &*plan;
        *matches = p.matches.as_ptr();
        *match_count = p.matches.len();
        *runs = p.runs.as_ptr();
        *run_count = p.runs.len();
        p.unmatched
    }
}

/// # Safety
/// `plan` comes from cvr_ic_plan (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ic_free(plan: *mut Plan) {
    if !plan.is_null() {
        drop(unsafe { Box::from_raw(plan) });
    }
}

/// CommitInstanceCoverage: on success every source's coverage bytes rewritten.
///
/// # Safety
/// `sources` holds `count` lent sources with writable coverage bytes; `matches` holds `match_count`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ic_commit(
    sources: *const SourceView,
    count: usize,
    matches: *const InstanceMatch,
    match_count: usize,
) -> bool {
    unsafe {
        let lent = views(sources, count);
        let matches = if match_count == 0 || matches.is_null() {
            &[][..]
        } else {
            core::slice::from_raw_parts(matches, match_count)
        };
        let Some(covered) = commit(&sources_of(lent), matches) else {
            return false;
        };
        for (view, flags) in lent.iter().zip(covered) {
            for (i, flag) in flags.into_iter().enumerate().take(view.covered_count) {
                *view.covered.add(i) = flag as u8;
            }
        }
        true
    }
}
