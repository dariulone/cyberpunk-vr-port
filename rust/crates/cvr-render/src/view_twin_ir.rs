//! The text rewrite that makes an engine-stereo material shader view-instanced for single pass (formerly
//! src/Render/ViewTwinIr.cpp; the contract is described in include/Render/ViewTwinIr.hpp).
//!
//! 2.5 engine stereo draws every RenderElements batch twice; a pair differs only in the CBVs b0..b3 of root
//! parameters 0 (vertex) and 18 (pixel). The twin of a shader reads each of its b0..b3 constant buffers twice -- from
//! the engine's table (space 0) and from a second table at the same registers in space 10 (the left eye's) -- and
//! keeps view 0's value from space 10 and view 1's from space 0:
//!
//! ```text
//!     %v  = cbufferLoadLegacy(b1 space 0, row)            (unchanged)
//!     %v' = cbufferLoadLegacy(b1 space 10, row)           (added)
//!     %e  = extractvalue %v, k                  ->  %e = select i1 (SV_ViewID == 0), extract(%v', k), extract(%v, k)
//! ```
//!
//! Input is DXC's lossless disassembly. Anything outside the recognised shape fails closed with a reason: that
//! pipeline keeps the engine's two draws. The module is handled as bytes; positions past the end of a view fail with
//! the message the C++ string_view gave ("invalid string_view position"), so every input fails the same way.
use cvr_host::fast::FastMap;

/// kTwinSpace: register space of the second eye's b0..b3.
pub const TWIN_SPACE: u32 = 10;
/// kTwinRecipe: bump when the rewrite changes (cached twins are keyed by it).
pub const TWIN_RECIPE: u32 = 2;

/// IrTwin: the result of a rewrite.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct IrTwin {
    pub ok: bool,
    /// No b0..b3 read: the original serves both views as it is.
    pub unchanged: bool,
    /// The pixel shader's inputs were completed from the vertex shader's outputs.
    pub signature: bool,
    pub loads: u32,
    pub handles: u32,
    pub extracts: u32,
    /// The rewritten module (ok && !unchanged).
    pub text: Vec<u8>,
    pub error: Vec<u8>,
}

struct Fail(Vec<u8>);
type R<T> = Result<T, Fail>;
fn fail<T>(why: impl AsRef<[u8]>) -> R<T> {
    Err(Fail(why.as_ref().to_vec()))
}
const STRING_VIEW_POSITION: &str = "invalid string_view position";
const MAP_KEY: &str = "invalid unordered_map<K, T> key";

/// string_view::substr: `count` clamped to the rest, a position past the end fails.
fn sub(s: &[u8], pos: usize, count: usize) -> R<&[u8]> {
    if pos > s.len() {
        return fail(STRING_VIEW_POSITION);
    }
    Ok(&s[pos..pos + count.min(s.len() - pos)])
}
fn from(s: &[u8], pos: usize) -> R<&[u8]> {
    sub(s, pos, usize::MAX)
}
fn find(s: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() {
        return (start <= s.len()).then_some(start);
    }
    if start >= s.len() {
        return None;
    }
    s[start..].windows(needle.len()).position(|w| w == needle).map(|p| p + start)
}
fn find_byte(s: &[u8], c: u8, start: usize) -> Option<usize> {
    s.get(start..)?.iter().position(|&b| b == c).map(|p| p + start)
}
fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}
fn num(n: impl ToString) -> Vec<u8> {
    n.to_string().into_bytes()
}
fn is_space(c: u8) -> bool {
    c == b' ' || c == b'\t' || c == b'\r'
}
fn trim(mut s: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = s {
        if !is_space(*first) {
            break;
        }
        s = rest;
    }
    while let [rest @ .., last] = s {
        if !is_space(*last) {
            break;
        }
        s = rest;
    }
    s
}
fn is_name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'.' || c == b'_' || c == b'-' || c == b'$'
}
/// std::from_chars over the trimmed text: decimal digits only, all of them, no overflow.
fn parse<T: TryFrom<u64>>(s: &[u8]) -> R<T> {
    let s = trim(s);
    let mut value: u64 = 0;
    let mut ok = !s.is_empty();
    for &c in s {
        if !c.is_ascii_digit() {
            ok = false;
            break;
        }
        match value.checked_mul(10).and_then(|v| v.checked_add((c - b'0') as u64)) {
            Some(v) => value = v,
            None => {
                ok = false;
                break;
            }
        }
    }
    match T::try_from(value) {
        Ok(v) if ok => Ok(v),
        _ => fail(cat(&[b"not a number: ", s])),
    }
}
fn number(s: &[u8]) -> R<u32> {
    parse::<u32>(s)
}
fn number64(s: &[u8]) -> R<u64> {
    parse::<u64>(s)
}
/// "a, b, {c, d}, !"x, y"" -> top-level fields.
fn fields(s: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, false, 0usize);
    for (i, &c) in s.iter().enumerate() {
        if quote {
            if c == b'"' {
                quote = false;
            }
            continue;
        }
        match c {
            b'"' => quote = true,
            b'{' | b'[' | b'(' | b'<' => depth += 1,
            b'}' | b']' | b')' | b'>' => depth -= 1,
            b',' if depth == 0 => {
                out.push(trim(&s[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(trim(&s[start..]));
    out
}
/// "!12 = !{...}" -> 12 and the inside of the braces.
fn metadata_node(line: &[u8]) -> R<Option<(u32, &[u8])>> {
    if !line.starts_with(b"!") || line.len() < 2 || !line[1].is_ascii_digit() {
        return Ok(None);
    }
    let Some(eq) = find(line, b" = !{", 0) else { return Ok(None) };
    if line.last() != Some(&b'}') {
        return Ok(None);
    }
    let id = number(sub(line, 1, eq.wrapping_sub(1))?)?;
    let body = sub(line, eq + 5, line.len().wrapping_sub(eq).wrapping_sub(6))?;
    Ok(Some((id, body)))
}
/// "!12" -> 12.
fn node_ref(field: &[u8]) -> R<u32> {
    let field = trim(field);
    if !field.starts_with(b"!") {
        return fail(cat(&[b"expected a metadata reference: ", field]));
    }
    number(from(field, 1)?)
}
/// "  %12 = rest" -> "%12", rest; None when the line defines nothing.
fn definition(line: &[u8]) -> Option<(&[u8], &[u8])> {
    if !line.starts_with(b"  %") {
        return None;
    }
    let mut i = 3;
    while i < line.len() && is_name_char(line[i]) {
        i += 1;
    }
    if !line[i..].starts_with(b" = ") {
        return None;
    }
    Some((&line[2..i], &line[i + 3..]))
}
/// Every "%name" token on the line (types included; callers compare against value names only).
fn tokens(line: &[u8], mut each: impl FnMut(&[u8]) -> R<()>) -> R<()> {
    let mut i = 0;
    while i < line.len() {
        match line[i] {
            b';' => return Ok(()),
            b'"' => match find_byte(line, b'"', i + 1) {
                Some(e) => i = e,
                None => return Ok(()),
            },
            b'%' => {
                let mut j = i + 1;
                while j < line.len() && is_name_char(line[j]) {
                    j += 1;
                }
                if j > i + 1 {
                    each(&line[i..j])?;
                }
                i = j - 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}
/// The instruction without its trailing "  ; comment".
fn code_part(line: &[u8]) -> &[u8] {
    let mut quote = false;
    for (i, &c) in line.iter().enumerate() {
        if c == b'"' {
            quote = !quote;
        } else if c == b';' && !quote {
            return trim(&line[..i]);
        }
    }
    trim(line)
}
fn element_type(ret: &[u8]) -> Option<&'static [u8]> {
    Some(match ret {
        b"%dx.types.CBufRet.f32" => b"float",
        b"%dx.types.CBufRet.i32" => b"i32",
        b"%dx.types.CBufRet.f16" => b"half",
        b"%dx.types.CBufRet.i16" => b"i16",
        b"%dx.types.CBufRet.f64" => b"double",
        b"%dx.types.CBufRet.i64" => b"i64",
        _ => return None,
    })
}
fn split_lines(text: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, &c) in text.iter().enumerate() {
        if c == b'\n' {
            lines.push(&text[start..i]);
            start = i + 1;
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    for l in &mut lines {
        if let [rest @ .., b'\r'] = *l {
            *l = rest;
        }
    }
    lines
}
fn at<'m, K: Eq + std::hash::Hash + ?Sized, V>(map: &'m FastMap<Vec<u8>, V>, key: &K) -> R<&'m V>
where
    Vec<u8>: std::borrow::Borrow<K>,
{
    map.get(key).map_or_else(|| fail(MAP_KEY), Ok)
}

struct Cbv {
    id: u32,
    reg: u32,
    size: u32,
    ty: Vec<u8>,
    ext: Vec<u8>,
}
/// A signature element record: !{i32 id, !"name", i8 comp, i8 kind, !indices, i8 interp, i32 rows, i8 cols, i32 row,
/// i8 col, ext}.
#[derive(Clone, Default)]
struct Element {
    node: u32,
    id: u32,
    kind: u32,
    rows: u32,
    row: u32,
    col: u32,
    name: Vec<u8>,
    indices: Vec<u8>,
    fields: [Vec<u8>; 11],
}
struct Module<'t> {
    lines: Vec<&'t [u8]>,
    node: FastMap<u32, usize>,
    max_node: u32,
}
impl<'t> Module<'t> {
    fn new(text: &'t [u8]) -> R<Self> {
        let lines = split_lines(text);
        let mut node = cvr_host::fast::fast_map();
        let mut max_node = 0;
        for (i, l) in lines.iter().enumerate() {
            if let Some((id, _)) = metadata_node(l)? {
                node.insert(id, i);
                max_node = max_node.max(id);
                continue;
            }
            if l.len() > 1 && l[0] == b'!' && l[1].is_ascii_digit() {
                if let Some(eq) = find(l, b" = ", 0) {
                    max_node = max_node.max(number(sub(l, 1, eq.wrapping_sub(1))?)?);
                }
            }
        }
        Ok(Module { lines, node, max_node })
    }
    fn body(&self, id: u32) -> R<&'t [u8]> {
        let Some(&line) = self.node.get(&id) else {
            return fail(format!("missing metadata !{id}"));
        };
        Ok(metadata_node(self.lines[line])?.map_or(&[][..], |(_, body)| body))
    }
    /// "!dx.x = !{!N}".
    fn named(&self, prefix: &[u8]) -> R<u32> {
        for l in &self.lines {
            if l.starts_with(prefix) {
                let b = find(l, b"!{", 0).unwrap_or(usize::MAX);
                let f = fields(sub(l, b.wrapping_add(2), l.len().wrapping_sub(b).wrapping_sub(3))?);
                if f.len() != 1 {
                    return fail("named metadata with several nodes");
                }
                return node_ref(f[0]);
            }
        }
        fail(cat(&[b"missing ", prefix]))
    }
    /// which: 0 inputs, 1 outputs of the (single) entry point; and the list node (0 when null).
    fn signature(&self, which: usize) -> R<(Vec<Element>, u32)> {
        let entry = fields(self.body(self.named(b"!dx.entryPoints = ")?)?);
        if entry.len() != 5 || entry[2] == b"null" {
            return Ok((Vec::new(), 0));
        }
        let sigs = fields(self.body(node_ref(entry[2])?)?);
        if sigs.len() != 3 || sigs[which] == b"null" {
            return Ok((Vec::new(), 0));
        }
        let list = node_ref(sigs[which])?;
        let mut out = Vec::new();
        for m in fields(self.body(list)?) {
            let mut e = Element {
                node: node_ref(m)?,
                ..Default::default()
            };
            let f = fields(self.body(e.node)?);
            if f.len() != 11 {
                return fail("unexpected signature element");
            }
            for (slot, field) in e.fields.iter_mut().zip(&f) {
                *slot = field.to_vec();
            }
            e.id = number(from(f[0], 4)?)?;
            e.name = f[1].to_vec();
            e.kind = number(from(f[3], 3)?)?;
            e.indices = if f[4] == b"null" { Vec::new() } else { self.body(node_ref(f[4])?)?.to_vec() };
            e.rows = number(from(f[6], 4)?)?;
            e.row = number64(from(f[8], 4)?)? as u32;
            e.col = number(from(f[9], 3)?)?;
            e.name.make_ascii_uppercase();
            out.push(e);
        }
        Ok((out, list))
    }
}
/// PS inputs the system makes (front face, primitive id, sample index): not linked to the vertex shader.
fn system_input(kind: u32) -> bool {
    kind == 10 || kind == 12 || kind == 13
}

const HANDLE: &[u8] = b"call %dx.types.Handle @dx.op.createHandle(i32 57, i8 2, i32 ";

/// RewriteForViews: `vertex` is the disassembly of the vertex shader a pixel shader is linked with (its outputs complete
/// the pixel shader's inputs when that one reads a system input); None for a vertex shader.
pub fn rewrite_for_views(text: &[u8], vertex: Option<&[u8]>) -> IrTwin {
    rewrite(text, vertex).unwrap_or_else(|Fail(error)| IrTwin { error, ..Default::default() })
}

fn rewrite(text: &[u8], vertex: Option<&[u8]>) -> R<IrTwin> {
    let mut out = IrTwin::default();
    let lines = split_lines(text);
    let mut node: FastMap<u32, usize> = cvr_host::fast::fast_map();
    let mut max_node = 0u32;
    let none = usize::MAX;
    let (mut define, mut resources_line, mut view_id_line, mut sm_line, mut version_line, mut entry_line) = (none, none, none, none, none, none);
    for (i, &l) in lines.iter().enumerate() {
        if let Some((id, _)) = metadata_node(l)? {
            node.insert(id, i);
            max_node = max_node.max(id);
            continue;
        }
        if l.len() > 1 && l[0] == b'!' && l[1].is_ascii_digit() {
            // "!52 = distinct !{...}"
            if let Some(eq) = find(l, b" = ", 0) {
                max_node = max_node.max(number(sub(l, 1, eq.wrapping_sub(1))?)?);
                continue;
            }
        }
        if l.starts_with(b"define ") {
            if define != none {
                return fail("more than one function");
            }
            define = i;
        } else if l.starts_with(b"!dx.resources = ") {
            resources_line = i;
        } else if l.starts_with(b"!dx.viewIdState = ") {
            view_id_line = i;
        } else if l.starts_with(b"!dx.shaderModel = ") {
            sm_line = i;
        } else if l.starts_with(b"!dx.version = ") {
            version_line = i;
        } else if l.starts_with(b"!dx.entryPoints = ") {
            entry_line = i;
        }
    }
    if define == none || sm_line == none || version_line == none || entry_line == none {
        return fail("missing module structure");
    }
    if find(text, b"@dx.op.viewID.", 0).is_some() {
        return fail("already reads SV_ViewID");
    }
    let node_body = |id: u32| -> R<&[u8]> {
        let Some(&line) = node.get(&id) else {
            return fail(format!("missing metadata !{id}"));
        };
        Ok(metadata_node(lines[line])?.map_or(&[][..], |(_, body)| body))
    };
    let named_ref = |line: usize| -> R<u32> {
        let l = lines[line];
        let Some(b) = find(l, b"!{", 0) else {
            return fail("unexpected named metadata");
        };
        if l.last() != Some(&b'}') {
            return fail("unexpected named metadata");
        }
        let f = fields(sub(l, b + 2, l.len().wrapping_sub(b).wrapping_sub(3))?);
        if f.len() != 1 {
            return fail("named metadata with several nodes");
        }
        node_ref(f[0])
    };
    // ---- shader kind and model
    let sm = fields(node_body(named_ref(sm_line)?)?);
    if sm.len() != 3 || (sm[0] != b"!\"vs\"" && sm[0] != b"!\"ps\"") {
        return fail("not a vertex or pixel shader");
    }
    let sm_major = number(from(sm[1], 4)?)?;
    let sm_minor = number(from(sm[2], 4)?)?;
    if sm_major != 6 {
        return fail("shader model not 6.x");
    }
    // ---- the CBVs at b0..b3 space 0
    let mut cbvs: Vec<Cbv> = Vec::new();
    let (mut cbv_list, mut max_id) = (0u32, 0u32);
    if resources_line != none {
        let lists = fields(node_body(named_ref(resources_line)?)?);
        if lists.len() != 4 {
            return fail("unexpected resource list");
        }
        if lists[2] != b"null" {
            cbv_list = node_ref(lists[2])?;
            for m in fields(node_body(cbv_list)?) {
                let mid = node_ref(m)?;
                let f = fields(node_body(mid)?);
                if f.len() != 8 {
                    return fail("unexpected CBV record");
                }
                let id = number(from(f[0], 4)?)?;
                let space = number(from(f[3], 4)?)?;
                let reg = number(from(f[4], 4)?)?;
                let count = number(from(f[5], 4)?)?;
                let size = number(from(f[6], 4)?)?;
                max_id = max_id.max(id);
                if space == 0 && reg <= 3 {
                    if count != 1 || reg + count > 4 {
                        return fail("b0..b3 as an array");
                    }
                    cbvs.push(Cbv {
                        id,
                        reg,
                        size,
                        ty: f[1].to_vec(),
                        ext: f[7].to_vec(),
                    });
                } else if space == TWIN_SPACE {
                    return fail("space 10 already used");
                }
            }
        }
    }
    // ---- a pixel shader that reads a system input (front face, ...) must, with view instancing, declare every output
    // of its vertex shader as an input, the elements in row order and the system inputs after them (else the runtime
    // refuses the pipeline; measured with the engine's SV_ClipDistance and SV_IsFrontFace).
    let mut input_list = 0u32;
    let mut new_input_id: FastMap<u32, u32> = cvr_host::fast::fast_map(); // old input element id -> new
    let mut element_line: FastMap<u32, Vec<u8>> = cvr_host::fast::fast_map(); // input element node -> its new line
    let mut new_order: Vec<u32> = Vec::new(); // input list after the rewrite (nodes)
    let mut extra_nodes: Vec<Vec<u8>> = Vec::new(); // added element and index nodes
    let this = Module::new(text)?;
    let mut next_node = this.max_node.wrapping_add(64); // above the nodes the writer below allocates
    if sm[0] == b"!\"ps\"" {
        if let Some(vertex) = vertex {
            let (ps_inputs, list) = this.signature(0)?;
            input_list = list;
            if ps_inputs.iter().any(|e| system_input(e.kind)) {
                let vs = Module::new(vertex)?;
                let (vs_outputs, _) = vs.signature(1)?;
                let added: Vec<Element> = vs_outputs
                    .into_iter()
                    .filter(|o| !ps_inputs.iter().any(|e| e.name == o.name && e.indices == o.indices))
                    .collect();
                // the order: linked elements and the added ones by row/column, then the system inputs on new rows
                let mut order: Vec<(&Element, bool)> = ps_inputs.iter().filter(|e| !system_input(e.kind)).map(|e| (e, false)).collect();
                order.extend(added.iter().map(|e| (e, true)));
                order.sort_by(|a, b| (a.0.row, a.0.col).cmp(&(b.0.row, b.0.col)));
                let mut end = 0u32;
                for (e, _) in &order {
                    if (e.row as i32) >= 0 {
                        end = end.max(e.row.wrapping_add(e.rows));
                    }
                }
                order.extend(ps_inputs.iter().filter(|e| system_input(e.kind)).map(|e| (e, false)));
                let mut moved = !added.is_empty();
                for (i, (e, add)) in order.iter().enumerate() {
                    if !add && e.id as usize != i {
                        moved = true;
                    }
                }
                if ps_inputs.iter().any(|e| system_input(e.kind) && e.row < end) {
                    moved = true;
                }
                if moved {
                    let mut system_row = end;
                    for (i, &(e, add)) in order.iter().enumerate() {
                        let mut f = e.fields.clone();
                        f[0] = cat(&[b"i32 ", &num(i)]);
                        if system_input(e.kind) {
                            f[8] = cat(&[b"i32 ", &num(system_row)]);
                            f[9] = b"i8 0".to_vec();
                            system_row = system_row.wrapping_add(e.rows);
                        }
                        let mut node_id = e.node;
                        if add {
                            node_id = next_node;
                            next_node = next_node.wrapping_add(1);
                            if e.indices.is_empty() {
                                f[4] = b"null".to_vec();
                            } else {
                                let idx = next_node;
                                next_node = next_node.wrapping_add(1);
                                extra_nodes.push(cat(&[b"!", &num(idx), b" = !{", &e.indices, b"}"]));
                                f[4] = cat(&[b"!", &num(idx)]);
                            }
                            f[10] = b"null".to_vec();
                        } else {
                            new_input_id.insert(e.id, i as u32);
                        }
                        let record = cat(&[b"!", &num(node_id), b" = !{", &f.join(&b", "[..]), b"}"]);
                        if add {
                            extra_nodes.push(record);
                        } else {
                            element_line.insert(node_id, record);
                        }
                        new_order.push(node_id);
                    }
                    out.signature = true;
                }
            }
        }
    }
    if cbvs.is_empty() && !out.signature {
        out.ok = true;
        out.unchanged = true;
        return Ok(out);
    }
    let mut twin_id: FastMap<u32, u32> = cvr_host::fast::fast_map(); // original range id -> twin range id
    for (i, c) in cbvs.iter().enumerate() {
        twin_id.insert(c.id, max_id.wrapping_add(1).wrapping_add(i as u32));
    }
    let twin_of = |id: u32| -> R<u32> { twin_id.get(&id).copied().map_or_else(|| fail(MAP_KEY), Ok) };
    // ---- handles, loads, extracts
    let mut handle_twin: FastMap<Vec<u8>, Vec<u8>> = cvr_host::fast::fast_map(); // %h -> %cvr.hN
    let mut load_twin: FastMap<Vec<u8>, (Vec<u8>, &'static [u8])> = cvr_host::fast::fast_map(); // %v -> twin load
    let mut kind = vec![0u8; lines.len()]; // 1 handle def, 2 load def, 3 extract
    for i in define..lines.len() {
        let Some((name, rest)) = definition(lines[i]) else { continue };
        let code = code_part(rest);
        if code.starts_with(HANDLE) {
            let f = fields(sub(code, HANDLE.len() - 4, code.len().wrapping_sub(HANDLE.len() - 4).wrapping_sub(1))?); // "i32 ID, i32 IDX, i1 NU"
            if f.len() != 3 {
                return fail("unexpected createHandle");
            }
            let id = number(from(f[0], 4)?)?;
            if twin_id.contains_key(&id) {
                handle_twin.insert(name.to_vec(), cat(&[b"%cvr.h", &num(out.handles)]));
                out.handles += 1;
                kind[i] = 1;
            }
            continue;
        }
        if code.starts_with(b"call %dx.types.CBufRet.") {
            let Some(at) = find(code, b" @dx.op.cbufferLoadLegacy.", 0) else { continue };
            let ret = sub(code, 5, at - 5)?;
            let open = find_byte(code, b'(', 0).unwrap_or(usize::MAX);
            let f = fields(sub(code, open.wrapping_add(1), code.len().wrapping_sub(open).wrapping_sub(2))?);
            if f.len() != 3 || f[0] != b"i32 59" || !f[1].starts_with(b"%dx.types.Handle ") {
                return fail("unexpected cbufferLoadLegacy");
            }
            let h = from(f[1], 17)?;
            if !handle_twin.contains_key(h) {
                continue;
            }
            let Some(element) = element_type(ret) else {
                return fail("unknown CBufRet type");
            };
            load_twin.insert(name.to_vec(), (cat(&[b"%cvr.l", &num(out.loads)]), element));
            out.loads += 1;
            kind[i] = 2;
            continue;
        }
        if code.starts_with(b"extractvalue %dx.types.CBufRet.") {
            let f = fields(from(code, 13)?);
            if f.len() != 2 {
                return fail("unexpected extractvalue");
            }
            let Some(sp) = f[0].iter().rposition(|&c| c == b' ') else {
                return fail("unexpected extractvalue operand");
            };
            if load_twin.contains_key(from(f[0], sp + 1)?) {
                kind[i] = 3;
                out.extracts += 1;
            }
        }
    }
    // ---- every other use of a twinned handle or load is outside the recognised shape
    for i in define..lines.len() {
        let code = match definition(lines[i]) {
            Some((_, rest)) => code_part(rest),
            None => code_part(lines[i]),
        };
        tokens(code, |tok| {
            if handle_twin.contains_key(tok) && kind[i] != 2 {
                return fail("b0..b3 handle used outside cbufferLoadLegacy");
            }
            if load_twin.contains_key(tok) && kind[i] != 3 {
                return fail("constant-buffer load used outside extractvalue");
            }
            Ok(())
        })?;
    }
    // ---- write the module
    let mut w: Vec<u8> = Vec::with_capacity(text.len() + out.loads as usize * 220 + out.extracts as usize * 160 + 4096);
    let mut line = |s: &[u8]| {
        w.extend_from_slice(s);
        w.push(b'\n');
    };
    let replace_all = |mut s: Vec<u8>, what: &[u8], to: &[u8]| -> Vec<u8> {
        let mut p = 0;
        while let Some(found) = find(&s, what, p) {
            let e = found + what.len();
            if e < s.len() && is_name_char(s[e]) {
                // %12 inside %123
                p = e;
                continue;
            }
            s.splice(found..e, to.iter().copied());
            p = found + to.len();
        }
        s
    };
    let mut new_node = max_node.wrapping_add(1);
    let mut take = || {
        let n = new_node;
        new_node = new_node.wrapping_add(1);
        n
    };
    let (sm_node, version_node, props_node) = (take(), take(), take());
    let twin_nodes: Vec<u32> = cbvs.iter().map(|_| take()).collect();
    let mut extract_serial = 0u32;
    // entry point properties: ShaderFlags (tag 0) gains ViewID (bit 21)
    let entry_node = named_ref(entry_line)?;
    let props = {
        let f = fields(node_body(entry_node)?);
        if f.len() != 5 {
            return fail("unexpected entry point record");
        }
        let mut body: Vec<u8> = Vec::new();
        let mut tagged = false;
        if f[4] != b"null" {
            let p = fields(node_body(node_ref(f[4])?)?);
            if !p.len().is_multiple_of(2) {
                return fail("odd entry properties");
            }
            for pair in p.chunks(2) {
                let mut value = pair[1].to_vec();
                if pair[0] == b"i32 0" {
                    if !pair[1].starts_with(b"i64 ") {
                        return fail("unexpected shader flags");
                    }
                    value = cat(&[b"i64 ", &num(number64(from(pair[1], 4)?)? | (1u64 << 21))]);
                    tagged = true;
                }
                body = cat(&[&body, if body.is_empty() { b"" } else { b", " }, pair[0], b", ", &value]);
            }
        }
        if !tagged {
            body = cat(&[b"i32 0, i64 ", &num(1u64 << 21), if body.is_empty() { b"" } else { b", " }, &body]);
        }
        cat(&[b"!", &num(props_node), b" = !{", &body, b"}"])
    };
    for (i, &l) in lines.iter().enumerate() {
        if i == view_id_line {
            continue; // regenerated by -viewid-state
        }
        if i == sm_line {
            line(&cat(&[b"!dx.shaderModel = !{!", &num(sm_node), b"}"]));
            continue;
        }
        if i == version_line {
            line(&cat(&[b"!dx.version = !{!", &num(version_node), b"}"]));
            continue;
        }
        if let Some((id, body)) = metadata_node(l)? {
            if id == cbv_list {
                let mut b = body.to_vec();
                for n in &twin_nodes {
                    b.extend_from_slice(&cat(&[b", !", &num(n)]));
                }
                line(&cat(&[b"!", &num(id), b" = !{", &b, b"}"]));
                continue;
            }
            if id == entry_node {
                let f = fields(body);
                let mut b = Vec::new();
                for field in f.iter().take(4) {
                    b.extend_from_slice(&cat(&[field, b", "]));
                }
                line(&cat(&[b"!", &num(id), b" = !{", &b, b"!", &num(props_node), b"}"]));
                continue;
            }
            if out.signature && id == input_list {
                let mut b = Vec::new();
                for (k, n) in new_order.iter().enumerate() {
                    b.extend_from_slice(&cat(&[if k != 0 { b", !" } else { b"!" }, &num(n)]));
                }
                line(&cat(&[b"!", &num(id), b" = !{", &b, b"}"]));
                continue;
            }
            if out.signature {
                if let Some(record) = element_line.get(&id) {
                    line(record);
                    continue;
                }
            }
            line(l);
            continue;
        }
        if i == define {
            line(l);
            line(b"  %cvr.view = call i32 @dx.op.viewID.i32(i32 138)  ; ViewID()");
            line(b"  %cvr.left = icmp eq i32 %cvr.view, 0");
            continue;
        }
        if out.signature && i > define && l.starts_with(b"  ") {
            // loadInput (4), evalSnapped (87), evalSampleIndex (88), evalCentroid (89), attributeAtVertex (137)
            const OPS: [&[u8]; 5] = [
                b"@dx.op.loadInput.",
                b"@dx.op.evalSnapped.",
                b"@dx.op.evalSampleIndex.",
                b"@dx.op.evalCentroid.",
                b"@dx.op.attributeAtVertex.",
            ];
            let mut rewritten = false;
            for op in OPS {
                let Some(at) = find(l, op, 0) else { continue };
                let open = find_byte(l, b'(', at);
                let Some(comma1) = open.and_then(|open| find(l, b", i32 ", open)) else {
                    return fail("unexpected input read");
                };
                let id_start = comma1 + 6;
                let mut id_end = id_start;
                while id_end < l.len() && l[id_end].is_ascii_digit() {
                    id_end += 1;
                }
                if id_end == id_start {
                    return fail("dynamic input element id");
                }
                let Some(new_id) = new_input_id.get(&number(&l[id_start..id_end])?) else {
                    return fail("input element without a new id");
                };
                line(&cat(&[&l[..id_start], &num(new_id), &l[id_end..]]));
                rewritten = true;
                break;
            }
            if rewritten {
                continue;
            }
        }
        if kind[i] == 1 {
            if let Some((name, rest)) = definition(l) {
                line(l);
                let code = code_part(rest);
                let f = fields(sub(code, HANDLE.len() - 4, code.len().wrapping_sub(HANDLE.len() - 4).wrapping_sub(1))?);
                let twin = at(&handle_twin, name)?;
                let range = twin_of(number(from(f[0], 4)?)?)?;
                line(&cat(&[
                    b"  ",
                    twin,
                    b" = call %dx.types.Handle @dx.op.createHandle(i32 57, i8 2, i32 ",
                    &num(range),
                    b", ",
                    f[1],
                    b", ",
                    f[2],
                    b")",
                ]));
                continue;
            }
        }
        if kind[i] == 2 {
            if let Some((name, rest)) = definition(l) {
                line(l);
                let code = code_part(rest);
                let open = find_byte(code, b'(', 0).unwrap_or(usize::MAX);
                let f = fields(sub(code, open.wrapping_add(1), code.len().wrapping_sub(open).wrapping_sub(2))?);
                let h = from(f[1], 17)?;
                let (twin, _) = at(&load_twin, name)?;
                line(&cat(&[
                    b"  ",
                    twin,
                    b" = ",
                    sub(code, 0, open.wrapping_add(1))?,
                    b"i32 59, %dx.types.Handle ",
                    at(&handle_twin, h)?,
                    b", ",
                    f[2],
                    b")",
                ]));
                continue;
            }
        }
        if kind[i] == 3 {
            if let Some((name, rest)) = definition(l) {
                let code = code_part(rest);
                let f = fields(from(code, 13)?);
                let sp = f[0].iter().rposition(|&c| c == b' ').map_or(usize::MAX, |p| p);
                let v = from(f[0], sp.wrapping_add(1))?;
                let (twin, element) = at(&load_twin, v)?;
                let serial = num(extract_serial);
                extract_serial += 1;
                let own = cat(&[b"%cvr.r", &serial]);
                let other = cat(&[b"%cvr.e", &serial]);
                line(&cat(&[b"  ", &own, b" = ", code]));
                line(&cat(&[b"  ", &other, b" = ", &replace_all(code.to_vec(), v, twin)]));
                line(&cat(&[
                    b"  ",
                    name,
                    b" = select i1 %cvr.left, ",
                    element,
                    b" ",
                    &other,
                    b", ",
                    element,
                    b" ",
                    &own,
                ]));
                continue;
            }
        }
        line(l);
    }
    line(b"declare i32 @dx.op.viewID.i32(i32) nounwind readnone");
    line(&cat(&[b"!", &num(sm_node), b" = !{", sm[0], b", i32 6, i32 ", &num(sm_minor.max(1)), b"}"]));
    let v = fields(node_body(named_ref(version_line)?)?);
    if v.len() != 2 {
        return fail("unexpected DXIL version");
    }
    line(&cat(&[
        b"!",
        &num(version_node),
        b" = !{i32 1, i32 ",
        &num(number(from(v[1], 4)?)?.max(1)),
        b"}",
    ]));
    line(&props);
    for n in &extra_nodes {
        line(n);
    }
    for (c, n) in cbvs.iter().zip(&twin_nodes) {
        line(&cat(&[
            b"!",
            &num(n),
            b" = !{i32 ",
            &num(twin_of(c.id)?),
            b", ",
            &c.ty,
            b", !\"\", i32 ",
            &num(TWIN_SPACE),
            b", i32 ",
            &num(c.reg),
            b", i32 1, i32 ",
            &num(c.size),
            b", ",
            &c.ext,
            b"}",
        ]));
    }
    out.text = w;
    out.ok = true;
    Ok(out)
}

/// What the header copies out of a rewrite or a build.
#[repr(C)]
pub struct TwinInfo {
    pub ok: bool,
    pub unchanged: bool,
    pub linked: bool,
    pub loads: u32,
    pub handles: u32,
    pub extracts: u32,
    pub data: *const u8,
    pub data_len: usize,
    pub error: *const u8,
    pub error_len: usize,
}

/// # Safety
/// `text` points to `len` bytes; `vertex` (when `has_vertex`) to `vertex_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_view_twin_rewrite(text: *const u8, len: usize, vertex: *const u8, vertex_len: usize, has_vertex: bool) -> *mut IrTwin {
    // SAFETY: the caller's byte ranges.
    let bytes = |p: *const u8, n: usize| {
        if n == 0 || p.is_null() {
            &[][..]
        } else {
            unsafe { core::slice::from_raw_parts(p, n) }
        }
    };
    let twin = rewrite_for_views(bytes(text, len), has_vertex.then(|| bytes(vertex, vertex_len)));
    Box::into_raw(Box::new(twin))
}
/// # Safety
/// `twin` came from cvr_view_twin_rewrite; `info` points to a TwinInfo.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_view_twin_ir_info(twin: *const IrTwin, info: *mut TwinInfo) {
    // SAFETY: the caller's rewrite and output.
    let (Some(t), Some(info)) = (unsafe { twin.as_ref() }, unsafe { info.as_mut() }) else {
        return;
    };
    *info = TwinInfo {
        ok: t.ok,
        unchanged: t.unchanged,
        linked: t.signature,
        loads: t.loads,
        handles: t.handles,
        extracts: t.extracts,
        data: t.text.as_ptr(),
        data_len: t.text.len(),
        error: t.error.as_ptr(),
        error_len: t.error.len(),
    };
}
/// # Safety
/// `twin` came from cvr_view_twin_rewrite (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_view_twin_ir_free(twin: *mut IrTwin) {
    if !twin.is_null() {
        // SAFETY: the rewrite handed out once.
        drop(unsafe { Box::from_raw(twin) });
    }
}
