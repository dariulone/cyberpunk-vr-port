//! The original source selectors' package (src/Render/SourceSelectorPlan.cpp): a strict, bounded JSON catalog of
//! original shader families -- each either supported, with its exact b4 constant-buffer requirements (root, visibility,
//! register b4 space 0, parameter 1 for the VS or 19 for the PS, the reflected bytes, the required extent and the
//! ascending DWORD offsets the selectors read) and its selector sites (the shader, the createHandle and index SSA
//! values, the DWORD and the exact original SRV table it indexes: parameter 13 space 1 for the VS, 31 space 1 or 33
//! space 3 for the PS, a 32768-slot declared extent), or unsupported with its reasons -- and the evaluation of a
//! supported family against the exact b4 bytes of one draw: each site's little-endian selector, inside its table's
//! declared extent, gives a descriptor offset; per table the sorted unique offsets, the slot count up to the last one
//! and the merged runs.
//!
//! The text stays jsoncpp's to read (the external library, configured strictly as before: no comments, no trailing
//! commas, no extra text, an object or array root, duplicate members rejected, 32 levels at most); its value tree is
//! lent here flattened (jsoncpp_tree), and everything the package asks of it -- the exact members, integers only where a number is
//! asked for, bounded strings -- is checked here. Every failure is one of the .cpp's messages and publishes nothing.
//!
//! The .cpp's LoadCatalogJson parses and forwards the tree to `cvr_ssp_load`; its Evaluate forwards to
//! `cvr_ssp_evaluate`; both through flat views (the C++ structures stay for the C++ callers; Evaluate validates the
//! plan it is given, as the .cpp did). The unit tests below are tools/single_pass_tests/source_selector_plan_test.cpp's
//! cases.
use crate::jsoncpp_tree::{Json, JsonNode};
use std::collections::{BTreeMap, BTreeSet};

pub const DECLARED_EXTENT: u32 = 32768;
pub const MAX_FAMILIES: u32 = 64;
pub const MAX_SITES: usize = 256;
pub const MAX_TOTAL_SITES: usize = 4096;
pub const MAX_CATALOG_BYTES: usize = 1024 * 1024;

/// Byte strings, compared and ordered as std::string.
pub type Text = Vec<u8>;

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct FamilyKey {
    pub root: Text,
    pub vertex: Text,
    pub pixel: Text,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct CbvKey {
    pub root: Text,
    pub visibility: u32,
    pub shader_register: u32,
    pub space: u32,
    pub parameter: u32,
    pub table_offset: u32,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct TableKey {
    pub root: Text,
    pub visibility: u32,
    pub parameter: u32,
    pub space: u32,
    pub shader_register_base: u32,
    pub original_table_offset: u32,
    pub declared_extent: u32,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CbvRequirement {
    pub key: CbvKey,
    pub reflected_bytes: u32,
    pub required_byte_extent: u32,
    pub dword_offsets: Vec<u32>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Site {
    pub shader: Text,
    pub create_handle_ssa: Text,
    pub index_ssa: Text,
    pub create_handle_line: u32,
    pub byte_offset: u32,
    pub cbv: CbvKey,
    pub table: TableKey,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FamilyPlan {
    pub key: FamilyKey,
    pub number: u32,
    pub supported: bool,
    pub unsupported_reason: Text,
    pub unresolved_inputs: Vec<Text>,
    pub cbvs: Vec<CbvRequirement>,
    pub sites: Vec<Site>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    pub evidence: Text,
    pub families: Vec<FamilyPlan>,
}
impl Catalog {
    /// The one family with this key (none when absent or ambiguous).
    pub fn find(&self, key: &FamilyKey) -> Option<&FamilyPlan> {
        let mut found = self.families.iter().filter(|plan| plan.key == *key);
        let first = found.next()?;
        found.next().is_none().then_some(first)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SiteSelection {
    pub site_index: u32,
    pub source_dword: u32,
    pub shader_index: u32,
    pub descriptor_offset: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SlotInterval {
    pub offset: u32,
    pub count: u32,
    pub kind: u32,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableSelection {
    pub key: TableKey,
    pub offsets: Vec<u32>,
    pub count: u32,
    pub uses: Vec<SlotInterval>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Evaluation {
    pub family: FamilyKey,
    pub evaluated_sites: u32,
    pub selections: Vec<SiteSelection>,
    pub tables: Vec<TableSelection>,
}

type Outcome<T> = Result<T, &'static str>;
fn require(value: bool, message: &'static str) -> Outcome<()> {
    if value { Ok(()) } else { Err(message) }
}
fn digest(value: &[u8]) -> bool {
    value.len() == 64 && value.iter().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
}
fn ssa(value: &[u8]) -> bool {
    value.len() > 1 && value.len() <= 32 && value[0] == b'%' && value[1..].iter().all(u8::is_ascii_digit)
}

// ---- the strict JSON value -------------------------------------------------------------------------------------------

fn fields(value: &Json, names: &[&str]) -> Outcome<()> {
    let Json::Object(members) = value else { return Err("Unknown/missing selector package fields") };
    require(members.len() == names.len(), "Unknown/missing selector package fields")?;
    for name in names {
        require(members.iter().any(|(k, _)| k == name.as_bytes()), "Missing selector package field")?;
    }
    Ok(())
}
fn number(value: &Json, field: &str, maximum: u32) -> Outcome<u32> {
    match value.member(field).integer_uint() {
        Some(v) if v <= maximum => Ok(v),
        _ => Err("Invalid selector package integer"),
    }
}
fn text(value: &Json, field: &str, maximum: usize) -> Outcome<Text> {
    match value.member(field) {
        Json::Str(s) if s.len() <= maximum => Ok(s.clone()),
        _ => Err("Invalid selector package string"),
    }
}
fn cbv(value: &Json) -> Outcome<CbvKey> {
    fields(value, &["root", "visibility", "register", "space", "parameter", "tableOffset"])?;
    Ok(CbvKey {
        root: text(value, "root", 64)?,
        visibility: number(value, "visibility", 5)?,
        shader_register: number(value, "register", 13)?,
        space: number(value, "space", u32::MAX)?,
        parameter: number(value, "parameter", 63)?,
        table_offset: number(value, "tableOffset", u32::MAX)?,
    })
}
fn table(value: &Json) -> Outcome<TableKey> {
    fields(
        value,
        &["root", "visibility", "parameter", "space", "shaderRegisterBase", "originalTableOffset", "declaredExtent"],
    )?;
    Ok(TableKey {
        root: text(value, "root", 64)?,
        visibility: number(value, "visibility", 5)?,
        parameter: number(value, "parameter", 63)?,
        space: number(value, "space", u32::MAX)?,
        shader_register_base: number(value, "shaderRegisterBase", u32::MAX)?,
        original_table_offset: number(value, "originalTableOffset", u32::MAX)?,
        declared_extent: number(value, "declaredExtent", u32::MAX)?,
    })
}

// ---- the rules ---------------------------------------------------------------------------------------------------------

fn valid_cbv(key: &CbvKey, family: &FamilyKey) -> bool {
    key.root == family.root
        && key.shader_register == 4
        && key.space == 0
        && key.table_offset == 0
        && ((key.visibility == 1 && key.parameter == 1) || (key.visibility == 5 && key.parameter == 19))
}
fn valid_table(key: &TableKey, cbv: &CbvKey) -> bool {
    key.root == cbv.root
        && key.visibility == cbv.visibility
        && key.shader_register_base == 0
        && key.original_table_offset == 0
        && key.declared_extent == DECLARED_EXTENT
        && ((key.visibility == 1 && key.parameter == 13 && key.space == 1)
            || (key.visibility == 5 && ((key.parameter == 31 && key.space == 1) || (key.parameter == 33 && key.space == 3))))
}
/// A family's identity, extent and internal consistency.
pub fn validate(plan: &FamilyPlan) -> Outcome<()> {
    require(
        plan.number != 0
            && plan.number <= MAX_FAMILIES
            && digest(&plan.key.root)
            && digest(&plan.key.vertex)
            && digest(&plan.key.pixel),
        "Invalid original selector family identity",
    )?;
    if !plan.supported {
        require(
            !plan.unsupported_reason.is_empty()
                && plan.unsupported_reason.len() <= 1024
                && !plan.unresolved_inputs.is_empty()
                && plan.unresolved_inputs.len() <= 64
                && plan.cbvs.is_empty()
                && plan.sites.is_empty(),
            "Unsupported selector family fabricated a partial plan",
        )?;
        for input in &plan.unresolved_inputs {
            require(!input.is_empty() && input.len() <= 1024, "Invalid unresolved selector input")?;
        }
        return Ok(());
    }
    require(
        plan.unsupported_reason.is_empty()
            && plan.unresolved_inputs.is_empty()
            && !plan.cbvs.is_empty()
            && plan.cbvs.len() <= 2
            && !plan.sites.is_empty()
            && plan.sites.len() <= MAX_SITES,
        "Invalid supported selector family extent",
    )?;
    let mut offsets: BTreeMap<&CbvKey, BTreeSet<u32>> = BTreeMap::new();
    for requirement in &plan.cbvs {
        require(
            valid_cbv(&requirement.key, &plan.key)
                && requirement.reflected_bytes != 0
                && requirement.reflected_bytes <= 65536
                && requirement.required_byte_extent != 0
                && requirement.required_byte_extent <= requirement.reflected_bytes
                && !requirement.dword_offsets.is_empty()
                && requirement.dword_offsets.len() <= MAX_SITES,
            "Invalid exact original b4 requirement",
        )?;
        require(offsets.insert(&requirement.key, BTreeSet::new()).is_none(), "Duplicate original b4 requirement")?;
        let mut prior = None;
        for &offset in &requirement.dword_offsets {
            require(
                offset & 3 == 0
                    && offset <= requirement.reflected_bytes
                    && 4 <= requirement.reflected_bytes - offset
                    && prior.is_none_or(|p| offset > p),
                "Invalid b4 DWORD offset/ordering/extent",
            )?;
            prior = Some(offset);
        }
    }
    let mut identities = BTreeSet::new();
    for site in &plan.sites {
        let found = offsets.get_mut(&site.cbv);
        require(
            found.is_some() && valid_cbv(&site.cbv, &plan.key) && valid_table(&site.table, &site.cbv),
            "Selector lost its exact original CBV/SRV binding",
        )?;
        let shader = if site.cbv.visibility == 1 { &plan.key.vertex } else { &plan.key.pixel };
        require(
            site.shader == *shader
                && ssa(&site.create_handle_ssa)
                && ssa(&site.index_ssa)
                && site.create_handle_ssa != site.index_ssa
                && site.create_handle_line != 0
                && site.create_handle_line <= 1_000_000
                && site.byte_offset & 3 == 0,
            "Selector original shader/SSA/DWORD identity differs",
        )?;
        require(
            identities.insert((site.shader.clone(), site.create_handle_ssa.clone())),
            "Duplicate original selector site",
        )?;
        if let Some(set) = found {
            set.insert(site.byte_offset);
        }
    }
    for requirement in &plan.cbvs {
        let actual = &offsets[&requirement.key];
        require(
            actual.len() == requirement.dword_offsets.len()
                && actual.iter().eq(requirement.dword_offsets.iter())
                && actual.last().map(|&last| last.wrapping_add(4)) == Some(requirement.required_byte_extent),
            "Selector sites and b4 byte requirements differ",
        )?;
    }
    Ok(())
}
fn family(value: &Json) -> Outcome<FamilyPlan> {
    fields(
        value,
        &["number", "root", "vs", "ps", "supported", "unsupportedReason", "unresolvedInputs", "cbvs", "sites"],
    )?;
    let key = FamilyKey { root: text(value, "root", 64)?, vertex: text(value, "vs", 64)?, pixel: text(value, "ps", 64)? };
    let family_number = number(value, "number", MAX_FAMILIES)?;
    let Json::Bool(supported) = *value.member("supported") else { return Err("Invalid selector support flag") };
    let unsupported_reason = text(value, "unsupportedReason", 1024)?;
    let Json::Array(unresolved) = value.member("unresolvedInputs") else {
        return Err("Invalid unresolved selector inputs");
    };
    require(unresolved.len() <= 64, "Invalid unresolved selector inputs")?;
    let mut unresolved_inputs = Vec::new();
    for input in unresolved {
        match input {
            Json::Str(s) if s.len() <= 1024 => unresolved_inputs.push(s.clone()),
            _ => return Err("Invalid unresolved selector input"),
        }
    }
    let Json::Array(cbvs) = value.member("cbvs") else { return Err("Invalid b4 requirement count") };
    require(cbvs.len() <= 2, "Invalid b4 requirement count")?;
    let mut requirements = Vec::new();
    for item in cbvs {
        fields(
            item,
            &[
                "root",
                "visibility",
                "register",
                "space",
                "parameter",
                "tableOffset",
                "reflectedBytes",
                "requiredByteExtent",
                "dwordOffsets",
            ],
        )?;
        let key = CbvKey {
            root: text(item, "root", 64)?,
            visibility: number(item, "visibility", 5)?,
            shader_register: number(item, "register", 13)?,
            space: number(item, "space", u32::MAX)?,
            parameter: number(item, "parameter", 63)?,
            table_offset: number(item, "tableOffset", u32::MAX)?,
        };
        let reflected_bytes = number(item, "reflectedBytes", 65536)?;
        let required_byte_extent = number(item, "requiredByteExtent", 65536)?;
        let Json::Array(offsets) = item.member("dwordOffsets") else { return Err("Invalid b4 DWORD count") };
        require(offsets.len() <= MAX_SITES, "Invalid b4 DWORD count")?;
        let mut dword_offsets = Vec::new();
        for offset in offsets {
            match offset.integer_uint() {
                Some(v) if v <= 65532 => dword_offsets.push(v),
                _ => return Err("Invalid b4 DWORD offset"),
            }
        }
        requirements.push(CbvRequirement { key, reflected_bytes, required_byte_extent, dword_offsets });
    }
    let Json::Array(sites) = value.member("sites") else { return Err("Invalid selector site count") };
    require(sites.len() <= MAX_SITES, "Invalid selector site count")?;
    let mut out_sites = Vec::new();
    for item in sites {
        fields(item, &["shader", "createHandleSSA", "createHandleLine", "indexSSA", "cbv", "byteOffset", "operations", "table"])?;
        let Json::Array(operations) = item.member("operations") else {
            return Err("Only original add-zero selectors are supported");
        };
        require(operations.len() == 1, "Only original add-zero selectors are supported")?;
        fields(&operations[0], &["opcode", "operand"])?;
        let add = matches!(operations[0].member("opcode"), Json::Str(s) if s == b"add");
        require(add, "Only original add-zero selectors are supported")?;
        require(number(&operations[0], "operand", 0)? == 0, "Only original add-zero selectors are supported")?;
        let shader = text(item, "shader", 64)?;
        let create_handle_ssa = text(item, "createHandleSSA", 32)?;
        let index_ssa = text(item, "indexSSA", 32)?;
        let create_handle_line = number(item, "createHandleLine", 1_000_000)?;
        let byte_offset = number(item, "byteOffset", 65532)?;
        let cbv = cbv(item.member("cbv"))?;
        let table = table(item.member("table"))?;
        out_sites.push(Site { shader, create_handle_ssa, index_ssa, create_handle_line, byte_offset, cbv, table });
    }
    let plan = FamilyPlan {
        key,
        number: family_number,
        supported,
        unsupported_reason,
        unresolved_inputs,
        cbvs: requirements,
        sites: out_sites,
    };
    validate(&plan)?;
    Ok(plan)
}
/// LoadCatalogJson, after jsoncpp has read the text into `manifest`.
fn load_catalog(manifest: &Json) -> Outcome<Catalog> {
    let manifest = manifest;
    fields(manifest, &["schema", "purpose", "evidenceSha256", "families"])?;
    let schema = number(manifest, "schema", u32::MAX)?;
    let purpose = matches!(manifest.member("purpose"), Json::Str(s) if s == b"original-source-selector-evaluation-only");
    require(schema == 1 && purpose, "Unsupported selector catalog schema/purpose")?;
    let evidence = text(manifest, "evidenceSha256", 64)?;
    require(digest(&evidence), "Invalid selector evidence digest")?;
    let Json::Array(families) = manifest.member("families") else { return Err("Invalid selector family count") };
    require(
        !families.is_empty() && families.len() <= MAX_FAMILIES as usize,
        "Invalid selector family count",
    )?;
    let (mut identities, mut numbers, mut sites) = (BTreeSet::new(), BTreeSet::new(), 0usize);
    let mut out = Vec::new();
    for value in families {
        let plan = family(value)?;
        sites += plan.sites.len();
        require(
            sites <= MAX_TOTAL_SITES && identities.insert(plan.key.clone()) && numbers.insert(plan.number),
            "Ambiguous/excessive original selector families",
        )?;
        out.push(plan);
    }
    Ok(Catalog { evidence, families: out })
}
/// Evaluate: the selections of a supported family's sites against the exact b4 bytes of each of its requirements.
pub fn evaluate(plan: &FamilyPlan, inputs: &[(CbvKey, Option<&[u8]>)]) -> Outcome<Evaluation> {
    validate(plan)?;
    require(plan.supported, "Original shader family needs unsupported selector inputs")?;
    require(inputs.len() == plan.cbvs.len(), "Supplied b4 view set differs from original selector family")?;
    let mut views: BTreeMap<&CbvKey, &[u8]> = BTreeMap::new();
    for (key, bytes) in inputs {
        let found = plan.cbvs.iter().find(|requirement| requirement.key == *key);
        let fits = match (found, bytes) {
            (Some(requirement), Some(bytes)) => bytes.len() == requirement.reflected_bytes as usize,
            _ => false,
        };
        require(
            fits && views.insert(key, bytes.unwrap_or_default()).is_none(),
            "Supplied b4 identity/logical extent is wrong or duplicated",
        )?;
    }
    let mut selections = Vec::new();
    let mut tables: BTreeMap<&TableKey, BTreeSet<u32>> = BTreeMap::new();
    for (index, site) in plan.sites.iter().enumerate() {
        // Never absent once validated (each site's view is one of the requirements'); the .cpp's map::at threw.
        let input = *views.get(&site.cbv).ok_or("invalid map<K, T> key")?;
        let at = site.byte_offset as usize;
        require(at <= input.len() && 4 <= input.len() - at, "Selector DWORD is outside exact b4 bytes")?;
        let selector = u32::from_le_bytes(input[at..at + 4].try_into().unwrap_or_default());
        require(
            selector < site.table.declared_extent && site.table.original_table_offset <= u32::MAX - selector,
            "Actual original selector is outside its declared table",
        )?;
        let offset = site.table.original_table_offset + selector;
        selections.push(SiteSelection {
            site_index: index as u32,
            source_dword: selector,
            shader_index: selector,
            descriptor_offset: offset,
        });
        tables.entry(&site.table).or_default().insert(offset);
    }
    let tables = tables
        .into_iter()
        .map(|(key, offsets)| {
            let offsets: Vec<u32> = offsets.into_iter().collect();
            let count = offsets.last().map_or(0, |last| last.wrapping_add(1));
            let mut uses: Vec<SlotInterval> = Vec::new();
            for &offset in &offsets {
                match uses.last_mut() {
                    Some(run) if run.offset.wrapping_add(run.count) == offset => run.count += 1,
                    _ => uses.push(SlotInterval { offset, count: 1, kind: 0 }),
                }
            }
            TableSelection { key: key.clone(), offsets, count, uses }
        })
        .collect();
    Ok(Evaluation { family: plan.key.clone(), evaluated_sites: plan.sites.len() as u32, selections, tables })
}

// ---- the flat views the C++ half reads and lends ----------------------------------------------------------------------

/// A byte string (std::string's data and size).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SsStr {
    pub p: *const u8,
    pub n: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SsCbvKey {
    pub root: SsStr,
    pub visibility: u32,
    pub shader_register: u32,
    pub space: u32,
    pub parameter: u32,
    pub table_offset: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SsTableKey {
    pub root: SsStr,
    pub visibility: u32,
    pub parameter: u32,
    pub space: u32,
    pub shader_register_base: u32,
    pub original_table_offset: u32,
    pub declared_extent: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SsRequirement {
    pub key: SsCbvKey,
    pub reflected_bytes: u32,
    pub required_byte_extent: u32,
    pub offsets: *const u32,
    pub offset_count: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SsSite {
    pub shader: SsStr,
    pub create_handle_ssa: SsStr,
    pub index_ssa: SsStr,
    pub create_handle_line: u32,
    pub byte_offset: u32,
    pub cbv: SsCbvKey,
    pub table: SsTableKey,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SsFamily {
    pub root: SsStr,
    pub vertex: SsStr,
    pub pixel: SsStr,
    pub number: u32,
    pub supported: bool,
    pub unsupported_reason: SsStr,
    pub unresolved: *const SsStr,
    pub unresolved_count: usize,
    pub cbvs: *const SsRequirement,
    pub cbv_count: usize,
    pub sites: *const SsSite,
    pub site_count: usize,
}
#[repr(C)]
pub struct SsCatalog {
    pub evidence: SsStr,
    pub families: *const SsFamily,
    pub family_count: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SsInput {
    pub key: SsCbvKey,
    pub bytes: *const u8,
    pub length: usize,
}
#[repr(C)]
pub struct SsTable {
    pub key: SsTableKey,
    pub offsets: *const u32,
    pub offset_count: usize,
    pub count: u32,
    pub uses: *const SlotInterval,
    pub use_count: usize,
}
#[repr(C)]
pub struct SsEvaluation {
    pub evaluated_sites: u32,
    pub selections: *const SiteSelection,
    pub selection_count: usize,
    pub tables: *const SsTable,
    pub table_count: usize,
}
const _: () = {
    use core::mem::size_of;
    assert!(size_of::<SiteSelection>() == 16 && size_of::<SlotInterval>() == 12, "SiteSelection and SlotInterval are the header's");
    assert!(size_of::<SsFamily>() == 120 && size_of::<SsSite>() == 136);
    assert!(size_of::<SsRequirement>() == 64 && size_of::<SsInput>() == 56 && size_of::<SsTable>() == 80);
    assert!(size_of::<SsEvaluation>() == 40 && size_of::<SsCatalog>() == 32, "mirrored by SourceSelectorPlan.cpp");
};

fn lend(text: &[u8]) -> SsStr {
    SsStr { p: text.as_ptr(), n: text.len() }
}
fn lend_cbv(key: &CbvKey) -> SsCbvKey {
    SsCbvKey {
        root: lend(&key.root),
        visibility: key.visibility,
        shader_register: key.shader_register,
        space: key.space,
        parameter: key.parameter,
        table_offset: key.table_offset,
    }
}
fn lend_table(key: &TableKey) -> SsTableKey {
    SsTableKey {
        root: lend(&key.root),
        visibility: key.visibility,
        parameter: key.parameter,
        space: key.space,
        shader_register_base: key.shader_register_base,
        original_table_offset: key.original_table_offset,
        declared_extent: key.declared_extent,
    }
}
/// # Safety
/// `s` points at `n` readable bytes (or `n` is 0).
unsafe fn own(s: SsStr) -> Text {
    if s.n == 0 || s.p.is_null() {
        return Vec::new();
    }
    // SAFETY: as the caller vouches.
    unsafe { core::slice::from_raw_parts(s.p, s.n) }.to_vec()
}
/// # Safety
/// `ptr` points at `n` readable items (or `n` is 0).
unsafe fn items<'a, T>(ptr: *const T, n: usize) -> &'a [T] {
    if n == 0 || ptr.is_null() {
        return &[];
    }
    // SAFETY: as the caller vouches.
    unsafe { core::slice::from_raw_parts(ptr, n) }
}
/// # Safety
/// The key's string is readable.
unsafe fn own_cbv(key: &SsCbvKey) -> CbvKey {
    CbvKey {
        // SAFETY: as the caller vouches.
        root: unsafe { own(key.root) },
        visibility: key.visibility,
        shader_register: key.shader_register,
        space: key.space,
        parameter: key.parameter,
        table_offset: key.table_offset,
    }
}
/// # Safety
/// The key's string is readable.
unsafe fn own_table(key: &SsTableKey) -> TableKey {
    TableKey {
        // SAFETY: as the caller vouches.
        root: unsafe { own(key.root) },
        visibility: key.visibility,
        parameter: key.parameter,
        space: key.space,
        shader_register_base: key.shader_register_base,
        original_table_offset: key.original_table_offset,
        declared_extent: key.declared_extent,
    }
}
/// The C++ plan a caller lends, owned.
///
/// # Safety
/// Every pointer of the view is readable for its count.
pub unsafe fn own_family(view: &SsFamily) -> FamilyPlan {
    // SAFETY: as the caller vouches, for every string and array below.
    unsafe {
        FamilyPlan {
            key: FamilyKey { root: own(view.root), vertex: own(view.vertex), pixel: own(view.pixel) },
            number: view.number,
            supported: view.supported,
            unsupported_reason: own(view.unsupported_reason),
            unresolved_inputs: items(view.unresolved, view.unresolved_count).iter().map(|s| own(*s)).collect(),
            cbvs: items(view.cbvs, view.cbv_count)
                .iter()
                .map(|r| CbvRequirement {
                    key: own_cbv(&r.key),
                    reflected_bytes: r.reflected_bytes,
                    required_byte_extent: r.required_byte_extent,
                    dword_offsets: items(r.offsets, r.offset_count).to_vec(),
                })
                .collect(),
            sites: items(view.sites, view.site_count)
                .iter()
                .map(|s| Site {
                    shader: own(s.shader),
                    create_handle_ssa: own(s.create_handle_ssa),
                    index_ssa: own(s.index_ssa),
                    create_handle_line: s.create_handle_line,
                    byte_offset: s.byte_offset,
                    cbv: own_cbv(&s.cbv),
                    table: own_table(&s.table),
                })
                .collect(),
        }
    }
}

/// A loaded catalog and the views lent to the C++ half (freed by cvr_ssp_free).
struct CatalogHolder {
    _catalog: Catalog,
    _unresolved: Vec<Vec<SsStr>>,
    _requirements: Vec<Vec<SsRequirement>>,
    _sites: Vec<Vec<SsSite>>,
    _families: Vec<SsFamily>,
    view: SsCatalog,
}
fn hold_catalog(catalog: Catalog) -> Box<CatalogHolder> {
    let unresolved: Vec<Vec<SsStr>> =
        catalog.families.iter().map(|f| f.unresolved_inputs.iter().map(|s| lend(s)).collect()).collect();
    let requirements: Vec<Vec<SsRequirement>> = catalog
        .families
        .iter()
        .map(|f| {
            f.cbvs
                .iter()
                .map(|r| SsRequirement {
                    key: lend_cbv(&r.key),
                    reflected_bytes: r.reflected_bytes,
                    required_byte_extent: r.required_byte_extent,
                    offsets: r.dword_offsets.as_ptr(),
                    offset_count: r.dword_offsets.len(),
                })
                .collect()
        })
        .collect();
    let sites: Vec<Vec<SsSite>> = catalog
        .families
        .iter()
        .map(|f| {
            f.sites
                .iter()
                .map(|s| SsSite {
                    shader: lend(&s.shader),
                    create_handle_ssa: lend(&s.create_handle_ssa),
                    index_ssa: lend(&s.index_ssa),
                    create_handle_line: s.create_handle_line,
                    byte_offset: s.byte_offset,
                    cbv: lend_cbv(&s.cbv),
                    table: lend_table(&s.table),
                })
                .collect()
        })
        .collect();
    let families: Vec<SsFamily> = catalog
        .families
        .iter()
        .enumerate()
        .map(|(i, f)| SsFamily {
            root: lend(&f.key.root),
            vertex: lend(&f.key.vertex),
            pixel: lend(&f.key.pixel),
            number: f.number,
            supported: f.supported,
            unsupported_reason: lend(&f.unsupported_reason),
            unresolved: unresolved[i].as_ptr(),
            unresolved_count: unresolved[i].len(),
            cbvs: requirements[i].as_ptr(),
            cbv_count: requirements[i].len(),
            sites: sites[i].as_ptr(),
            site_count: sites[i].len(),
        })
        .collect();
    // Moving the vectors and strings into the holder keeps their heap buffers, which the views point at.
    let view = SsCatalog { evidence: lend(&catalog.evidence), families: families.as_ptr(), family_count: families.len() };
    Box::new(CatalogHolder {
        _catalog: catalog,
        _unresolved: unresolved,
        _requirements: requirements,
        _sites: sites,
        _families: families,
        view,
    })
}
/// An evaluation and the views lent to the C++ half (freed by cvr_ssp_free).
struct EvaluationHolder {
    _evaluation: Evaluation,
    _tables: Vec<SsTable>,
    view: SsEvaluation,
}
fn hold_evaluation(evaluation: Evaluation) -> Box<EvaluationHolder> {
    let tables: Vec<SsTable> = evaluation
        .tables
        .iter()
        .map(|t| SsTable {
            key: lend_table(&t.key),
            offsets: t.offsets.as_ptr(),
            offset_count: t.offsets.len(),
            count: t.count,
            uses: t.uses.as_ptr(),
            use_count: t.uses.len(),
        })
        .collect();
    let view = SsEvaluation {
        evaluated_sites: evaluation.evaluated_sites,
        selections: evaluation.selections.as_ptr(),
        selection_count: evaluation.selections.len(),
        tables: tables.as_ptr(),
        table_count: tables.len(),
    };
    Box::new(EvaluationHolder { _evaluation: evaluation, _tables: tables, view })
}
/// The catalog a cvr_ssp_load handle holds (the retained sources' own, alive while the handle is).
pub fn catalog_of(handle: *mut core::ffi::c_void) -> Option<&'static Catalog> {
    // SAFETY: null or a handle cvr_ssp_load returned, kept for the process.
    match unsafe { handle.cast::<Held>().as_ref() }? {
        Held::Catalog(holder) => Some(&holder._catalog),
        Held::Evaluation(_) => None,
    }
}
/// The holders, kept only to be dropped by cvr_ssp_free.
#[allow(dead_code)]
enum Held {
    Catalog(Box<CatalogHolder>),
    Evaluation(Box<EvaluationHolder>),
}
fn fail(error: *mut SsStr, message: &'static str) -> *mut core::ffi::c_void {
    if !error.is_null() {
        // SAFETY: the caller's out-parameter.
        unsafe { *error = SsStr { p: message.as_ptr(), n: message.len() } };
    }
    core::ptr::null_mut()
}

/// LoadCatalogJson after the parse: the catalog's view in `out` and a handle for cvr_ssp_free, or null with the
/// message in `error` (a static string). `nodes[0]` is the root.
///
/// # Safety
/// `nodes` is readable for `count`, with its strings; `out` and `error` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ssp_load(
    nodes: *const JsonNode,
    count: usize,
    out: *mut *const SsCatalog,
    error: *mut SsStr,
) -> *mut core::ffi::c_void {
    // SAFETY: as the caller vouches.
    let Some(manifest) = (unsafe { Json::from_nodes(items(nodes, count), 0) }) else {
        return fail(error, "Selector catalog exception");
    };
    match load_catalog(&manifest) {
        Ok(catalog) => {
            let holder = hold_catalog(catalog);
            // SAFETY: as the caller vouches.
            unsafe { *out = &holder.view };
            Box::into_raw(Box::new(Held::Catalog(holder))).cast()
        }
        Err(message) => fail(error, message),
    }
}
/// Evaluate: the evaluation's view in `out` and a handle for cvr_ssp_free, or null with the message in `error`.
///
/// # Safety
/// `family` and `inputs` (for `count`) are readable views; `out` and `error` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ssp_evaluate(
    family: *const SsFamily,
    inputs: *const SsInput,
    count: usize,
    out: *mut *const SsEvaluation,
    error: *mut SsStr,
) -> *mut core::ffi::c_void {
    // SAFETY: as the caller vouches.
    let plan = unsafe { own_family(&*family) };
    let views: Vec<(CbvKey, Option<&[u8]>)> = unsafe { items(inputs, count) }
        .iter()
        .map(|input| {
            // SAFETY: as the caller vouches; a null view is no view.
            let bytes = (!input.bytes.is_null()).then(|| unsafe { items(input.bytes, input.length) });
            (unsafe { own_cbv(&input.key) }, bytes)
        })
        .collect();
    match evaluate(&plan, &views) {
        Ok(evaluation) => {
            let holder = hold_evaluation(evaluation);
            // SAFETY: as the caller vouches.
            unsafe { *out = &holder.view };
            Box::into_raw(Box::new(Held::Evaluation(holder))).cast()
        }
        Err(message) => fail(error, message),
    }
}
/// # Safety
/// `handle` is null or one cvr_ssp_load or cvr_ssp_evaluate returned, freed once.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ssp_free(handle: *mut core::ffi::c_void) {
    if !handle.is_null() {
        // SAFETY: as the caller vouches.
        drop(unsafe { Box::from_raw(handle.cast::<Held>()) });
    }
}

#[cfg(test)]
mod tests {
    //! tools/single_pass_tests/source_selector_plan_test.cpp's cases.
    use super::*;
    use crate::jsoncpp_tree::JsonStr;
    use serde_json::{json, Value};

    fn hash(c: char) -> String {
        c.to_string().repeat(64)
    }
    fn cbv_json(visibility: u32) -> Value {
        json!({"root": hash('a'), "visibility": visibility, "register": 4, "space": 0,
               "parameter": if visibility == 1 { 1 } else { 19 }, "tableOffset": 0})
    }
    fn requirement(visibility: u32) -> Value {
        let mut value = cbv_json(visibility);
        let bytes = if visibility == 1 { 8 } else { 16 };
        value["reflectedBytes"] = json!(bytes);
        value["requiredByteExtent"] = json!(bytes);
        value["dwordOffsets"] = json!((0..bytes).step_by(4).collect::<Vec<u32>>());
        value
    }
    fn site(visibility: u32, offset: u32, identity: u32, space: u32) -> Value {
        json!({"shader": hash(if visibility == 1 { 'b' } else { 'c' }), "createHandleSSA": format!("%{identity}"),
               "createHandleLine": identity, "indexSSA": format!("%{}", identity + 1000), "cbv": cbv_json(visibility),
               "byteOffset": offset, "operations": [{"opcode": "add", "operand": 0}],
               "table": {"root": hash('a'), "visibility": visibility,
                         "parameter": if visibility == 1 { 13 } else if space == 1 { 31 } else { 33 },
                         "space": space, "shaderRegisterBase": 0, "originalTableOffset": 0, "declaredExtent": 32768}})
    }
    fn fixture() -> Value {
        let mut sites = vec![site(1, 0, 100, 1), site(1, 4, 101, 1)];
        sites.extend((0..4).map(|i| site(5, i * 4, 200 + i, 1)));
        sites.push(site(5, 12, 204, 3));
        let supported = json!({"number": 1, "root": hash('a'), "vs": hash('b'), "ps": hash('c'), "supported": true,
            "unsupportedReason": "", "unresolvedInputs": [], "cbvs": [requirement(1), requirement(5)], "sites": sites});
        let mut unsupported = supported.clone();
        unsupported["number"] = json!(13);
        unsupported["vs"] = json!(hash('d'));
        unsupported["ps"] = json!(hash('e'));
        unsupported["supported"] = json!(false);
        unsupported["unsupportedReason"] = json!("Actual t5 selector inputs are required");
        unsupported["unresolvedInputs"] = json!(["ps:t5 space0 field100"]);
        unsupported["cbvs"] = json!([]);
        unsupported["sites"] = json!([]);
        json!({"schema": 1, "purpose": "original-source-selector-evaluation-only", "evidenceSha256": hash('f'),
               "families": [supported, unsupported]})
    }
    /// jsoncpp's tree for a value (an integer that is not a u32 is `Integer`, a fraction a real).
    fn tree(value: &Value) -> Json {
        match value {
            Value::Null => Json::Null,
            Value::Bool(b) => Json::Bool(*b),
            Value::Number(n) => match (n.as_u64(), n.as_f64()) {
                (Some(u), _) if !n.is_f64() => u32::try_from(u).map_or(Json::Integer, Json::UInt),
                (_, Some(f)) if n.is_f64() && f >= 0.0 && f <= f64::from(u32::MAX) && f.fract() == 0.0 => Json::RealUInt(f as u32),
                _ if n.is_f64() => Json::Real,
                _ => Json::Integer,
            },
            Value::String(s) => Json::Str(s.as_bytes().to_vec()),
            Value::Array(items) => Json::Array(items.iter().map(tree).collect()),
            Value::Object(members) => Json::Object(members.iter().map(|(k, v)| (k.as_bytes().to_vec(), tree(v))).collect()),
        }
    }
    fn load(value: &Value) -> Outcome<Catalog> {
        load_catalog(&tree(value))
    }
    /// The tree flattened as the .cpp flattens jsoncpp's.
    fn flatten(json: &Json, nodes: &mut Vec<JsonNode>, at: usize) {
        let empty = JsonStr { p: core::ptr::null(), n: 0 };
        let mut node = JsonNode { kind: 0, count: 0, value: 0, text: empty, name: empty, first: 0 };
        let children: Vec<(Option<&Text>, &Json)> = match json {
            Json::Null => vec![],
            Json::Bool(b) => {
                (node.kind, node.value) = (1, u64::from(*b));
                vec![]
            }
            Json::Integer => {
                node.kind = 2;
                vec![]
            }
            Json::UInt(v) => {
                (node.kind, node.value) = (3, u64::from(*v));
                vec![]
            }
            Json::Real => {
                node.kind = 4;
                vec![]
            }
            Json::RealUInt(v) => {
                (node.kind, node.value) = (8, u64::from(*v));
                vec![]
            }
            Json::Str(s) => {
                (node.kind, node.text) = (5, JsonStr { p: s.as_ptr(), n: s.len() });
                vec![]
            }
            Json::Array(items) => {
                node.kind = 6;
                items.iter().map(|v| (None, v)).collect()
            }
            Json::Object(members) => {
                node.kind = 7;
                members.iter().map(|(k, v)| (Some(k), v)).collect()
            }
        };
        node.count = children.len() as u32;
        node.first = nodes.len() as u64;
        nodes.resize(nodes.len() + children.len(), node);
        nodes[at] = node;
        for (i, (name, child)) in children.into_iter().enumerate() {
            let index = node.first as usize + i;
            flatten(child, nodes, index);
            if let Some(name) = name {
                nodes[index].name = JsonStr { p: name.as_ptr(), n: name.len() };
            }
        }
    }
    fn reject(value: Value) {
        assert!(load(&value).is_err(), "admitted: {value}");
    }
    fn views(family: &FamilyPlan, vs: &[u8], ps: &[u8]) -> Vec<(CbvKey, Option<Vec<u8>>)> {
        vec![(family.cbvs[0].key.clone(), Some(vs.to_vec())), (family.cbvs[1].key.clone(), Some(ps.to_vec()))]
    }
    fn run(family: &FamilyPlan, inputs: &[(CbvKey, Option<Vec<u8>>)]) -> Outcome<Evaluation> {
        let borrowed: Vec<(CbvKey, Option<&[u8]>)> = inputs.iter().map(|(k, b)| (k.clone(), b.as_deref())).collect();
        evaluate(family, &borrowed)
    }
    const VS: [u8; 8] = [0, 0, 0, 0, 255, 127, 0, 0];
    const PS: [u8; 16] = [10, 0, 0, 0, 11, 0, 0, 0, 13, 0, 0, 0, 11, 0, 0, 0];

    #[test]
    fn exact_evaluation() {
        let catalog = load(&fixture()).unwrap();
        let key = FamilyKey { root: hash('a').into(), vertex: hash('b').into(), pixel: hash('c').into() };
        let family = catalog.find(&key).expect("exact family");
        assert_eq!(family.number, 1);
        for wrong in [
            FamilyKey { root: hash('d').into(), ..key.clone() },
            FamilyKey { vertex: hash('d').into(), ..key.clone() },
            FamilyKey { pixel: hash('d').into(), ..key.clone() },
        ] {
            assert!(catalog.find(&wrong).is_none());
        }
        let result = run(family, &views(family, &VS, &PS)).unwrap();
        assert_eq!((result.evaluated_sites, result.selections.len(), result.tables.len()), (7, 7, 3));
        assert_eq!((result.selections[1].source_dword, result.selections[1].shader_index), (32767, 32767));
        let vs = &result.tables[0];
        assert_eq!((vs.key.visibility, vs.key.parameter, vs.offsets.clone(), vs.count), (1, 13, vec![0, 32767], 32768));
        let ps = &result.tables[1];
        assert_eq!((ps.key.visibility, ps.key.parameter, ps.offsets.clone(), ps.count), (5, 31, vec![10, 11, 13], 14));
        assert_eq!(
            ps.uses,
            [SlotInterval { offset: 10, count: 2, kind: 0 }, SlotInterval { offset: 13, count: 1, kind: 0 }]
        );
        assert_eq!((result.tables[2].key.parameter, result.tables[2].key.space, result.tables[2].offsets.clone()), (33, 3, vec![11]));
    }
    #[test]
    fn strict_package() {
        let mut value = fixture();
        value["nativeExecutionPermission"] = json!(true);
        reject(value);
        for schema in [json!(true), json!(1.0)] {
            let mut value = fixture();
            value["schema"] = schema;
            reject(value);
        }
        let mut value = fixture();
        value["purpose"] = json!("execute");
        reject(value);
        let mut value = fixture();
        value["families"] = json!([]);
        reject(value);
        // A root that is no object, and an integer where a string is asked for.
        assert_eq!(load(&json!([])), Err("Unknown/missing selector package fields"));
        let mut value = fixture();
        value["evidenceSha256"] = json!(7);
        assert_eq!(load(&value), Err("Invalid selector package string"));
        let mut value = fixture();
        value["schema"] = json!(-1);
        assert_eq!(load(&value), Err("Invalid selector package integer"));
    }
    #[test]
    fn hashes_and_ambiguity() {
        for field in ["root", "vs", "ps"] {
            let mut value = fixture();
            value["families"][0][field] = json!(hash('A'));
            reject(value);
        }
        let mut value = fixture();
        value["evidenceSha256"] = json!(hash('g'));
        reject(value);
        let mut value = fixture();
        let mut duplicate = value["families"][0].clone();
        duplicate["number"] = json!(2);
        value["families"].as_array_mut().unwrap().push(duplicate);
        reject(value);
        let mut value = fixture();
        value["families"][1]["number"] = json!(1);
        reject(value);
        let mut value = fixture();
        let first = value["families"][0]["sites"][0].clone();
        value["families"][0]["sites"].as_array_mut().unwrap().push(first);
        reject(value);
        let mut value = fixture();
        let first = value["families"][0]["cbvs"][0].clone();
        value["families"][0]["cbvs"].as_array_mut().unwrap().push(first);
        reject(value);
    }
    #[test]
    fn qualified_bindings() {
        for field in ["visibility", "register", "space", "parameter", "tableOffset"] {
            let mut value = fixture();
            let cbv = &mut value["families"][0]["sites"][0]["cbv"];
            cbv[field] = json!(cbv[field].as_u64().unwrap() + 1);
            reject(value);
        }
        for field in ["visibility", "parameter", "space", "shaderRegisterBase", "originalTableOffset", "declaredExtent"] {
            let mut value = fixture();
            let table = &mut value["families"][0]["sites"][0]["table"];
            table[field] = json!(table[field].as_u64().unwrap() + 1);
            reject(value);
        }
        for (path, replacement) in [("shader", 'c'), ("cbv", 'd'), ("table", 'd')] {
            let mut value = fixture();
            if path == "shader" {
                value["families"][0]["sites"][0]["shader"] = json!(hash(replacement));
            } else {
                value["families"][0]["sites"][0][path]["root"] = json!(hash(replacement));
            }
            reject(value);
        }
    }
    #[test]
    fn offsets_operations_and_bounds() {
        for offset in [1, 8, 65532] {
            let mut value = fixture();
            value["families"][0]["sites"][0]["byteOffset"] = json!(offset);
            reject(value);
        }
        let edits: [(&str, Value); 2] = [("requiredByteExtent", json!(7)), ("reflectedBytes", json!(7))];
        for (field, v) in edits {
            let mut value = fixture();
            value["families"][0]["cbvs"][0][field] = v;
            reject(value);
        }
        let mut value = fixture();
        value["families"][0]["cbvs"][0]["dwordOffsets"][1] = json!(0);
        reject(value);
        let mut value = fixture();
        value["families"][0]["sites"][0]["operations"][0]["operand"] = json!(1);
        assert_eq!(load(&value), Err("Invalid selector package integer"));
        let mut value = fixture();
        value["families"][0]["sites"][0]["operations"][0]["opcode"] = json!("and");
        assert_eq!(load(&value), Err("Only original add-zero selectors are supported"));
        let mut value = fixture();
        value["families"][0]["sites"][0]["createHandleSSA"] = json!("%named");
        reject(value);
        let mut value = fixture();
        value["families"][0]["sites"][0]["createHandleLine"] = json!(0);
        reject(value);
        let mut value = fixture();
        let second = value["families"][1].clone();
        for _ in 0..MAX_FAMILIES {
            value["families"].as_array_mut().unwrap().push(second.clone());
        }
        reject(value);
    }
    #[test]
    fn exact_byte_views() {
        let catalog = load(&fixture()).unwrap();
        let family = &catalog.families[0];
        let good = views(family, &VS, &PS);
        for index in 0..2 {
            let mut wrong = good.clone();
            let bytes = wrong[index].1.as_mut().unwrap();
            bytes.pop();
            assert!(run(family, &wrong).is_err());
        }
        let mut wrong = good.clone();
        wrong[0].0.root = hash('f').into();
        assert!(run(family, &wrong).is_err());
        let mut wrong = good.clone();
        wrong[0] = wrong[1].clone();
        assert!(run(family, &wrong).is_err());
        assert_eq!(run(family, &good[..1]), Err("Supplied b4 view set differs from original selector family"));
        let mut wrong = good.clone();
        wrong[1].1 = Some(vec![0; 256]);
        assert!(run(family, &wrong).is_err());
        let mut wrong = good.clone();
        wrong[1].1 = None;
        assert_eq!(run(family, &wrong), Err("Supplied b4 identity/logical extent is wrong or duplicated"));
    }
    #[test]
    fn failure_atomicity_and_unsupported() {
        let catalog = load(&fixture()).unwrap();
        let mut family = catalog.families[0].clone();
        let mut ps = PS;
        assert!(run(&family, &views(&family, &VS, &ps)).is_ok());
        ps[1] = 128;
        assert_eq!(run(&family, &views(&family, &VS, &ps)), Err("Actual original selector is outside its declared table"));
        assert_eq!(run(&catalog.families[1], &[]), Err("Original shader family needs unsupported selector inputs"));
        family.sites[0].byte_offset = 65532;
        assert!(run(&family, &views(&family, &VS, &PS)).is_err());
        let mut value = fixture();
        let first = value["families"][0]["sites"][0].clone();
        value["families"][1]["sites"].as_array_mut().unwrap().push(first);
        assert_eq!(load(&value), Err("Unsupported selector family fabricated a partial plan"));
    }
    #[test]
    fn flat_views() {
        // The catalog's view round-trips through the C ABI into the evaluation.
        let manifest = tree(&fixture());
        let empty = JsonStr { p: core::ptr::null(), n: 0 };
        let mut nodes = vec![JsonNode { kind: 0, count: 0, value: 0, text: empty, name: empty, first: 0 }];
        flatten(&manifest, &mut nodes, 0);
        let (mut view, mut error) = (core::ptr::null(), SsStr { p: core::ptr::null(), n: 0 });
        unsafe {
            // A child index past the nodes is refused, not read.
            let mut broken = nodes.clone();
            broken[0].first = 1 << 40;
            assert!(cvr_ssp_load(broken.as_ptr(), broken.len(), &mut view, &mut error).is_null());
            let handle = cvr_ssp_load(nodes.as_ptr(), nodes.len(), &mut view, &mut error);
            assert!(!handle.is_null());
            let catalog = &*view;
            assert_eq!(catalog.family_count, 2);
            let family = &*catalog.families;
            let owned = own_family(family);
            assert_eq!(owned, load(&fixture()).unwrap().families[0]);
            let requirements = items(family.cbvs, family.cbv_count);
            let inputs = [
                SsInput { key: requirements[0].key, bytes: VS.as_ptr(), length: VS.len() },
                SsInput { key: requirements[1].key, bytes: PS.as_ptr(), length: PS.len() },
            ];
            let mut out = core::ptr::null();
            let evaluated = cvr_ssp_evaluate(family, inputs.as_ptr(), 2, &mut out, &mut error);
            assert!(!evaluated.is_null());
            assert_eq!(((*out).evaluated_sites, (*out).selection_count, (*out).table_count), (7, 7, 3));
            cvr_ssp_free(evaluated);
            // A failure: null, the message, nothing to free.
            let failed = cvr_ssp_evaluate(family, inputs.as_ptr(), 1, &mut out, &mut error);
            assert!(failed.is_null());
            assert_eq!(core::slice::from_raw_parts(error.p, error.n), b"Supplied b4 view set differs from original selector family");
            cvr_ssp_free(handle);
        }
    }
}
