//! jsoncpp's value tree as the C++ half lends it (include/Rust/JsonTree.hpp's Flatten): the text stays jsoncpp's to read
//! -- the external library, each caller's reader configured as before -- and the tree is handed over flattened, so the
//! rules that ask things of it live here. The accessors keep jsoncpp's semantics: a member of an object (null when
//! absent) and of null (null), anything else's member a LogicError; isUInt true for integers and integral reals in
//! 0..=u32::MAX; asString "" for null, the text of a string, "true"/"false" for a bool, a LogicError for an array or an
//! object.
//!
//! Used by source_selector_plan and stereo_family_manifest. The unit tests below check the nodes and the accessors.

/// A byte string the C++ half lends.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct JsonStr {
    pub p: *const u8,
    pub n: usize,
}
/// One node: kind 0 null, 1 bool (`value`), 2 an integer that is not a u32, 3 an integer that is (`value`), 4 a real that
/// is not an integral u32, 5 a string (`text`), 6 an array and 7 an object (`count` children from `first`, an object's
/// carrying their `name`), 8 a real that is an integral u32 (`value`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct JsonNode {
    pub kind: u32,
    pub count: u32,
    pub value: u64,
    pub text: JsonStr,
    pub name: JsonStr,
    pub first: u64,
}
const _: () = assert!(core::mem::size_of::<JsonNode>() == 56, "mirrored by include/Rust/JsonTree.hpp");

/// A jsoncpp value.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// intValue or uintValue outside 0..=u32::MAX.
    Integer,
    /// intValue or uintValue in 0..=u32::MAX.
    UInt(u32),
    /// A realValue that is not an integral u32.
    Real,
    /// A realValue that is an integral u32 (jsoncpp's isUInt holds).
    RealUInt(u32),
    Str(Vec<u8>),
    Array(Vec<Json>),
    Object(Vec<(Vec<u8>, Json)>),
}
pub const NULL: Json = Json::Null;
/// jsoncpp's message when a member is asked of what is neither an object nor null.
pub const NOT_AN_OBJECT: &str = "in Json::Value::find(begin, end): requires objectValue or nullValue";

/// # Safety
/// `s` points at `n` readable bytes (or `n` is 0).
pub unsafe fn bytes(s: JsonStr) -> Vec<u8> {
    if s.n == 0 || s.p.is_null() {
        return Vec::new();
    }
    // SAFETY: as the caller vouches.
    unsafe { core::slice::from_raw_parts(s.p, s.n) }.to_vec()
}

impl Json {
    /// The tree under node `at`; None when a child index leaves the nodes or a kind is unknown.
    ///
    /// # Safety
    /// The nodes' strings are readable.
    pub unsafe fn from_nodes(nodes: &[JsonNode], at: usize) -> Option<Json> {
        let node = nodes.get(at)?;
        let children = || {
            let first = usize::try_from(node.first).ok()?;
            let end = first.checked_add(node.count as usize)?;
            (end <= nodes.len()).then_some(first..end)
        };
        // SAFETY: as the caller vouches, for the strings below.
        Some(match node.kind {
            0 => Json::Null,
            1 => Json::Bool(node.value != 0),
            2 => Json::Integer,
            3 => Json::UInt(u32::try_from(node.value).ok()?),
            4 => Json::Real,
            5 => Json::Str(unsafe { bytes(node.text) }),
            6 => Json::Array(children()?.map(|i| unsafe { Json::from_nodes(nodes, i) }).collect::<Option<_>>()?),
            7 => Json::Object(
                children()?
                    .map(|i| Some((unsafe { bytes(nodes[i].name) }, unsafe { Json::from_nodes(nodes, i) }?)))
                    .collect::<Option<_>>()?,
            ),
            8 => Json::RealUInt(u32::try_from(node.value).ok()?),
            _ => return None,
        })
    }
    /// A member, null when absent or when this is not an object (callers that have checked for an object).
    pub fn member(&self, field: &str) -> &Json {
        self.find(field).unwrap_or(&NULL)
    }
    /// jsoncpp's const operator[] (find): an object's member or null, null's null, anything else a LogicError.
    pub fn find(&self, field: &str) -> Result<&Json, &'static str> {
        match self {
            Json::Object(members) => Ok(members.iter().find(|(k, _)| k == field.as_bytes()).map_or(&NULL, |(_, v)| v)),
            Json::Null => Ok(&NULL),
            _ => Err(NOT_AN_OBJECT),
        }
    }
    /// jsoncpp's isMember: as find, false for null.
    pub fn is_member(&self, field: &str) -> Result<bool, &'static str> {
        match self {
            Json::Object(members) => Ok(members.iter().any(|(k, _)| k == field.as_bytes())),
            Json::Null => Ok(false),
            _ => Err(NOT_AN_OBJECT),
        }
    }
    /// jsoncpp's isUInt (an integer or an integral real in 0..=u32::MAX) with asUInt's value.
    pub fn as_uint(&self) -> Option<u32> {
        match *self {
            Json::UInt(v) | Json::RealUInt(v) => Some(v),
            _ => None,
        }
    }
    /// (intValue || uintValue) && isUInt: an integer in 0..=u32::MAX, not a real.
    pub fn integer_uint(&self) -> Option<u32> {
        match *self {
            Json::UInt(v) => Some(v),
            _ => None,
        }
    }
    /// jsoncpp's asString. Integers render as decimals; any other number renders as "<number>" -- not jsoncpp's
    /// formatting, but like it never empty and never a digest, which is all its callers ask of a number's text.
    pub fn as_string(&self) -> Result<Vec<u8>, &'static str> {
        match self {
            Json::Null => Ok(Vec::new()),
            Json::Str(s) => Ok(s.clone()),
            Json::Bool(b) => Ok(if *b { b"true".to_vec() } else { b"false".to_vec() }),
            Json::UInt(v) => Ok(v.to_string().into_bytes()),
            Json::Integer | Json::Real | Json::RealUInt(_) => Ok(b"<number>".to_vec()),
            Json::Array(_) | Json::Object(_) => Err("Type is not convertible to string"),
        }
    }
    /// jsoncpp's isBool and asBool.
    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Json::Bool(b) => Some(b),
            _ => None,
        }
    }
    /// An array's elements (jsoncpp's isArray).
    pub fn elements(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }
    /// value == "text": a string of exactly these bytes.
    pub fn is_text(&self, text: &str) -> bool {
        matches!(self, Json::Str(s) if s == text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    //! The nodes and the accessors.
    use super::*;

    fn str(text: &'static [u8]) -> JsonStr {
        JsonStr { p: text.as_ptr(), n: text.len() }
    }
    fn node(kind: u32, value: u64) -> JsonNode {
        JsonNode { kind, count: 0, value, text: str(b""), name: str(b""), first: 0 }
    }

    #[test]
    fn nodes() {
        // {"a": 3, "b": [true, "x", 2.0]} flattened.
        let mut nodes = vec![node(7, 0), node(3, 3), node(6, 0), node(1, 1), node(5, 0), node(8, 2)];
        (nodes[0].count, nodes[0].first) = (2, 1);
        nodes[1].name = str(b"a");
        nodes[2].name = str(b"b");
        (nodes[2].count, nodes[2].first) = (3, 3);
        nodes[4].text = str(b"x");
        let json = unsafe { Json::from_nodes(&nodes, 0) }.unwrap();
        assert_eq!(json.find("a"), Ok(&Json::UInt(3)));
        let b = json.find("b").unwrap().elements().unwrap();
        assert_eq!(b, [Json::Bool(true), Json::Str(b"x".to_vec()), Json::RealUInt(2)]);
        // A child range past the nodes, or an unknown kind, is refused.
        let mut broken = nodes.clone();
        broken[2].count = 9;
        assert_eq!(unsafe { Json::from_nodes(&broken, 0) }, None);
        let mut broken = nodes.clone();
        broken[5].kind = 9;
        assert_eq!(unsafe { Json::from_nodes(&broken, 0) }, None);
    }
    #[test]
    fn accessors() {
        let object = Json::Object(vec![(b"k".to_vec(), Json::RealUInt(4))]);
        assert_eq!((object.find("k"), object.find("z"), object.is_member("k"), object.is_member("z")),
                   (Ok(&Json::RealUInt(4)), Ok(&NULL), Ok(true), Ok(false)));
        assert_eq!((NULL.find("k"), NULL.is_member("k")), (Ok(&NULL), Ok(false)));
        for other in [Json::Bool(true), Json::UInt(1), Json::Str(b"s".to_vec()), Json::Array(vec![])] {
            assert_eq!((other.find("k"), other.is_member("k")), (Err(NOT_AN_OBJECT), Err(NOT_AN_OBJECT)));
        }
        assert_eq!((Json::RealUInt(4).as_uint(), Json::RealUInt(4).integer_uint(), Json::UInt(4).integer_uint()),
                   (Some(4), None, Some(4)));
        assert_eq!((Json::Integer.as_uint(), Json::Real.as_uint()), (None, None));
        assert_eq!(NULL.as_string(), Ok(vec![]));
        assert_eq!(Json::Bool(false).as_string(), Ok(b"false".to_vec()));
        assert_eq!(Json::UInt(12).as_string(), Ok(b"12".to_vec()));
        assert_eq!(Json::Object(vec![]).as_string(), Err("Type is not convertible to string"));
        assert!(Json::Str(b"abc".to_vec()).is_text("abc") && !Json::Str(b"abc".to_vec()).is_text("ab"));
    }
}
