//! JSON as jsoncpp 1.9.5 has it, for the game's UserSettings.json (game_settings.rs): the file the game reads is the
//! one the C++ half wrote, so it is read and written exactly as `Json::CharReaderBuilder::strictMode` and
//! `Json::StreamWriterBuilder` (indentation four spaces, everything else default) did.
//!
//! - Values keep jsoncpp's types: a number without '.', 'e', 'E', '+' or an inner '-' is an integer (signed when it
//!   fits in 64 bits, else unsigned, else a double), and `==` compares the type first (an int is not a real).
//! - Objects are sorted by their keys' bytes (jsoncpp's std::map), strings are bytes (what the file holds, unchecked).
//! - Writing: comment style "All" makes every non-empty array multi-line; " : " between a key and its value; `{` and
//!   `[` of a member's value open on a line of their own; reals `%.17g` (a ".0" added to what reads as an integer);
//!   strings escaped with every non-ASCII code point as `\uxxxx` (lowercase, surrogate pairs past the BMP).
//! - Reading (strict): comments, trailing commas, single quotes, special floats and duplicate keys refused; a UTF-8
//!   BOM skipped; the root an object or an array; nothing but whitespace after it; 1000 levels deep at most.
//! Errors carry jsoncpp's message without its position.
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Int(i64),
    UInt(u64),
    Real(f64),
    Str(Vec<u8>),
    Bool(bool),
    Array(Vec<Value>),
    Object(BTreeMap<Vec<u8>, Value>),
}

/// What a jsoncpp accessor throws for (a value of a type it cannot convert).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeError;

static NULL: Value = Value::Null;

impl PartialEq for Value {
    /// `Json::Value::operator==`: the types first, then the values.
    fn eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::UInt(a), Value::UInt(b)) => a == b,
            (Value::Real(a), Value::Real(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => a == b,
            _ => false,
        }
    }
}

impl Value {
    pub fn str(text: &str) -> Value {
        Value::Str(text.as_bytes().to_vec())
    }
    /// The const `operator[](key)`: the member, or null.
    pub fn get(&self, key: &[u8]) -> &Value {
        match self {
            Value::Object(m) => m.get(key).unwrap_or(&NULL),
            _ => &NULL,
        }
    }
    /// The non-const `operator[](key)`: the member, made null when missing (a null value becomes an object first).
    /// Panics for any other type, as jsoncpp throws.
    pub fn member(&mut self, key: &[u8]) -> &mut Value {
        if matches!(self, Value::Null) {
            *self = Value::Object(BTreeMap::new());
        }
        match self {
            Value::Object(m) => m.entry(key.to_vec()).or_insert(Value::Null),
            _ => panic!("Json::Value::operator[](key): requires objectValue"),
        }
    }
    /// The const `operator[](index)` of an array: the element, or null.
    pub fn at(&self, index: usize) -> &Value {
        match self {
            Value::Array(a) => a.get(index).unwrap_or(&NULL),
            _ => &NULL,
        }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
    pub fn is_string(&self) -> bool {
        matches!(self, Value::Str(_))
    }
    pub fn is_bool(&self) -> bool {
        matches!(self, Value::Bool(_))
    }
    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(_))
    }
    pub fn is_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }
    pub fn is_numeric(&self) -> bool {
        matches!(self, Value::Int(_) | Value::UInt(_) | Value::Real(_))
    }
    /// `isInt`: representable as a 32-bit int (a real only when integral).
    pub fn is_int(&self) -> bool {
        match *self {
            Value::Int(v) => v >= i32::MIN as i64 && v <= i32::MAX as i64,
            Value::UInt(v) => v <= i32::MAX as u64,
            Value::Real(v) => v >= i32::MIN as f64 && v <= i32::MAX as f64 && integral(v),
            _ => false,
        }
    }
    /// `isIntegral`.
    pub fn is_integral(&self) -> bool {
        match *self {
            Value::Int(_) | Value::UInt(_) => true,
            Value::Real(v) => v >= i64::MIN as f64 && v < 18446744073709551615.0 && integral(v),
            _ => false,
        }
    }
    /// `isMember` (a null has none; any other non-object throws).
    pub fn is_member(&self, key: &[u8]) -> bool {
        match self {
            Value::Object(m) => m.contains_key(key),
            Value::Null => false,
            _ => panic!("Json::Value::isMember: requires objectValue or nullValue"),
        }
    }
    pub fn size(&self) -> usize {
        match self {
            Value::Array(a) => a.len(),
            Value::Object(m) => m.len(),
            _ => 0,
        }
    }
    /// `empty`: a null, an array or an object without members; never anything else.
    pub fn empty(&self) -> bool {
        match self {
            Value::Null => true,
            Value::Array(a) => a.is_empty(),
            Value::Object(m) => m.is_empty(),
            _ => false,
        }
    }
    /// `asString`.
    pub fn as_string(&self) -> Result<Vec<u8>, TypeError> {
        Ok(match self {
            Value::Null => Vec::new(),
            Value::Str(s) => s.clone(),
            Value::Bool(b) => (if *b { "true" } else { "false" }).as_bytes().to_vec(),
            Value::Int(v) => v.to_string().into_bytes(),
            Value::UInt(v) => v.to_string().into_bytes(),
            Value::Real(v) => real_to_string(*v).into_bytes(),
            _ => return Err(TypeError),
        })
    }
    /// `asBool`.
    pub fn as_bool(&self) -> Result<bool, TypeError> {
        match *self {
            Value::Bool(b) => Ok(b),
            Value::Null => Ok(false),
            Value::Int(v) => Ok(v != 0),
            Value::UInt(v) => Ok(v != 0),
            Value::Real(v) => Ok(v != 0.0),
            _ => Err(TypeError),
        }
    }
    /// `asInt`: out of the 32-bit range throws.
    pub fn as_int(&self) -> Result<i32, TypeError> {
        match *self {
            Value::Int(v) if v >= i32::MIN as i64 && v <= i32::MAX as i64 => Ok(v as i32),
            Value::UInt(v) if v <= i32::MAX as u64 => Ok(v as i32),
            Value::Real(v) if v >= i32::MIN as f64 && v <= i32::MAX as f64 => Ok(v as i32),
            Value::Null => Ok(0),
            Value::Bool(b) => Ok(b as i32),
            _ => Err(TypeError),
        }
    }
    /// `asDouble`.
    pub fn as_double(&self) -> Result<f64, TypeError> {
        match *self {
            Value::Int(v) => Ok(v as f64),
            Value::UInt(v) => Ok(v as f64),
            Value::Real(v) => Ok(v),
            Value::Null => Ok(0.0),
            Value::Bool(b) => Ok(if b { 1.0 } else { 0.0 }),
            _ => Err(TypeError),
        }
    }
    /// `isConvertibleTo(stringValue)`.
    pub fn is_convertible_to_string(&self) -> bool {
        matches!(
            self,
            Value::Int(_) | Value::UInt(_) | Value::Real(_) | Value::Bool(_) | Value::Str(_) | Value::Null
        )
    }
}

/// IsIntegral: modf(d, &integral) == 0.0 (an infinity passes, but every caller's range keeps it out).
fn integral(v: f64) -> bool {
    v.is_infinite() || v.fract() == 0.0
}

/// `valueToString(double, false, 17, significantDigits)`.
pub fn real_to_string(v: f64) -> String {
    if !v.is_finite() {
        return if v.is_nan() {
            "null".into()
        } else if v < 0.0 {
            "-1e+9999".into()
        } else {
            "1e+9999".into()
        };
    }
    let mut text = cvr_crt::format(c"%.17g", &[cvr_crt::Arg::F(v)]).to_string_lossy().replace(',', ".");
    if !text.contains('.') && !text.contains('e') {
        text.push_str(".0");
    }
    text
}

// ---- writing (BuiltStyledStreamWriter: commentStyle "All", indentation "    ")

struct Writer {
    out: Vec<u8>,
    indent: Vec<u8>,
    indented: bool,
}
const INDENTATION: &[u8] = b"    ";

impl Writer {
    fn write_indent(&mut self) {
        self.out.push(b'\n');
        self.out.extend_from_slice(&self.indent);
    }
    fn write_with_indent(&mut self, text: &[u8]) {
        if !self.indented {
            self.write_indent();
        }
        self.out.extend_from_slice(text);
        self.indented = false;
    }
    fn value(&mut self, v: &Value) {
        match v {
            Value::Null => self.out.extend_from_slice(b"null"),
            Value::Int(i) => self.out.extend_from_slice(i.to_string().as_bytes()),
            Value::UInt(u) => self.out.extend_from_slice(u.to_string().as_bytes()),
            Value::Real(r) => self.out.extend_from_slice(real_to_string(*r).as_bytes()),
            Value::Str(s) => quote(s, &mut self.out),
            Value::Bool(b) => self.out.extend_from_slice(if *b { b"true" } else { b"false" }),
            Value::Array(a) => {
                if a.is_empty() {
                    self.out.extend_from_slice(b"[]");
                    return;
                }
                // "All" comments: every array on lines of its own.
                self.write_with_indent(b"[");
                self.indent.extend_from_slice(INDENTATION);
                for (i, child) in a.iter().enumerate() {
                    if !self.indented {
                        self.write_indent();
                    }
                    self.indented = true;
                    self.value(child);
                    self.indented = false;
                    if i + 1 < a.len() {
                        self.out.push(b',');
                    }
                }
                self.indent.truncate(self.indent.len() - INDENTATION.len());
                self.write_with_indent(b"]");
            }
            Value::Object(m) => {
                if m.is_empty() {
                    self.out.extend_from_slice(b"{}");
                    return;
                }
                self.write_with_indent(b"{");
                self.indent.extend_from_slice(INDENTATION);
                let count = m.len();
                for (i, (name, child)) in m.iter().enumerate() {
                    let mut quoted = Vec::new();
                    quote(name, &mut quoted);
                    self.write_with_indent(&quoted);
                    self.out.extend_from_slice(b" : ");
                    self.value(child);
                    if i + 1 < count {
                        self.out.push(b',');
                    }
                }
                self.indent.truncate(self.indent.len() - INDENTATION.len());
                self.write_with_indent(b"}");
            }
        }
    }
}

/// `Json::writeString` of the builder the C++ half configured.
pub fn write(root: &Value) -> Vec<u8> {
    let mut w = Writer {
        out: Vec::new(),
        indent: Vec::new(),
        indented: true,
    };
    w.value(root);
    w.out
}

fn hex(out: &mut Vec<u8>, ch: u32) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    out.extend_from_slice(b"\\u");
    for shift in [12, 8, 4, 0] {
        out.push(DIGITS[((ch >> shift) & 0xF) as usize]);
    }
}
/// `utf8ToCodepoint`: the code point at `s[*i]`, `*i` left on its last byte.
fn codepoint(s: &[u8], i: &mut usize) -> u32 {
    const REPLACEMENT: u32 = 0xFFFD;
    let left = s.len() - *i;
    let b = |k: usize| s[*i + k] as u32;
    let first = b(0);
    if first < 0x80 {
        return first;
    }
    if first < 0xE0 {
        if left < 2 {
            return REPLACEMENT;
        }
        let c = ((first & 0x1F) << 6) | (b(1) & 0x3F);
        *i += 1;
        return if c < 0x80 { REPLACEMENT } else { c };
    }
    if first < 0xF0 {
        if left < 3 {
            return REPLACEMENT;
        }
        let c = ((first & 0x0F) << 12) | ((b(1) & 0x3F) << 6) | (b(2) & 0x3F);
        *i += 2;
        if (0xD800..=0xDFFF).contains(&c) {
            return REPLACEMENT;
        }
        return if c < 0x800 { REPLACEMENT } else { c };
    }
    if first < 0xF8 {
        if left < 4 {
            return REPLACEMENT;
        }
        let c = ((first & 0x07) << 18) | ((b(1) & 0x3F) << 12) | ((b(2) & 0x3F) << 6) | (b(3) & 0x3F);
        *i += 3;
        return if c < 0x10000 { REPLACEMENT } else { c };
    }
    REPLACEMENT
}
/// `valueToQuotedStringN(s, emitUTF8 = false)`.
fn quote(s: &[u8], out: &mut Vec<u8>) {
    if !s.iter().any(|&c| c == b'\\' || c == b'"' || c < 0x20 || c > 0x7F) {
        // The C string: what comes before a NUL.
        let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
        out.push(b'"');
        out.extend_from_slice(&s[..end]);
        out.push(b'"');
        return;
    }
    out.push(b'"');
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            0x08 => out.extend_from_slice(b"\\b"),
            0x0C => out.extend_from_slice(b"\\f"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            _ => {
                let c = codepoint(s, &mut i);
                if c < 0x20 {
                    hex(out, c);
                } else if c < 0x80 {
                    out.push(c as u8);
                } else if c < 0x10000 {
                    hex(out, c);
                } else {
                    let c = c - 0x10000;
                    hex(out, 0xD800 + ((c >> 10) & 0x3FF));
                    hex(out, 0xDC00 + (c & 0x3FF));
                }
            }
        }
        i += 1;
    }
    out.push(b'"');
}

// ---- reading (OurReader with CharReaderBuilder::strictMode, comments not collected)

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    End,
    ObjectBegin,
    ObjectEnd,
    ArrayBegin,
    ArrayEnd,
    String,
    Number,
    True,
    False,
    Null,
    ArraySeparator,
    MemberSeparator,
    Comment,
    Error,
}
#[derive(Clone, Copy)]
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
}
struct ReadError {
    message: String,
    at: usize,
    extra: Option<usize>,
}

struct Reader<'a> {
    text: &'a [u8],
    at: usize,
    depth: usize,
    errors: Vec<ReadError>,
}

impl<'a> Reader<'a> {
    fn next(&mut self) -> u8 {
        if self.at >= self.text.len() {
            return 0;
        }
        let c = self.text[self.at];
        self.at += 1;
        c
    }
    fn skip_spaces(&mut self) {
        while self.at < self.text.len() && matches!(self.text[self.at], b' ' | b'\t' | b'\r' | b'\n') {
            self.at += 1;
        }
    }
    fn matches(&mut self, rest: &[u8]) -> bool {
        if self.text.len() - self.at < rest.len() || &self.text[self.at..self.at + rest.len()] != rest {
            return false;
        }
        self.at += rest.len();
        true
    }
    fn comment(&mut self) -> bool {
        match self.next() {
            b'*' => {
                while self.at + 1 < self.text.len() {
                    let c = self.next();
                    if c == b'*' && self.text[self.at] == b'/' {
                        break;
                    }
                }
                self.next() == b'/'
            }
            b'/' => {
                while self.at < self.text.len() {
                    let c = self.next();
                    if c == b'\n' {
                        break;
                    }
                    if c == b'\r' {
                        if self.at < self.text.len() && self.text[self.at] == b'\n' {
                            self.next();
                        }
                        break;
                    }
                }
                true
            }
            _ => false,
        }
    }
    /// `readToken`: (false, an error token) when the token is not one.
    fn token(&mut self) -> (bool, Token) {
        self.skip_spaces();
        let start = self.at;
        let c = self.next();
        let (mut kind, ok) = match c {
            b'{' => (Kind::ObjectBegin, true),
            b'}' => (Kind::ObjectEnd, true),
            b'[' => (Kind::ArrayBegin, true),
            b']' => (Kind::ArrayEnd, true),
            b'"' => {
                let mut last = 0;
                while self.at < self.text.len() {
                    last = self.next();
                    if last == b'\\' {
                        self.next();
                    } else if last == b'"' {
                        break;
                    }
                }
                (Kind::String, last == b'"')
            }
            b'/' => (Kind::Comment, self.comment()),
            b'0'..=b'9' | b'-' => {
                self.number();
                (Kind::Number, true)
            }
            b't' => (Kind::True, self.matches(b"rue")),
            b'f' => (Kind::False, self.matches(b"alse")),
            b'n' => (Kind::Null, self.matches(b"ull")),
            b',' => (Kind::ArraySeparator, true),
            b':' => (Kind::MemberSeparator, true),
            0 => (Kind::End, true),
            _ => (Kind::Error, false),
        };
        if !ok {
            kind = Kind::Error;
        }
        (ok, Token { kind, start, end: self.at })
    }
    /// `readNumber`: digits, a fraction, an exponent -- as loose as jsoncpp's.
    fn number(&mut self) {
        let text = self.text;
        let mut p = self.at;
        let mut at = self.at;
        let mut step = || -> u8 {
            at = p;
            if p < text.len() {
                p += 1;
                text[p - 1]
            } else {
                0
            }
        };
        let mut c = b'0';
        while c.is_ascii_digit() {
            c = step();
        }
        if c == b'.' {
            c = step();
            while c.is_ascii_digit() {
                c = step();
            }
        }
        if c == b'e' || c == b'E' {
            c = step();
            if c == b'+' || c == b'-' {
                c = step();
            }
            while c.is_ascii_digit() {
                c = step();
            }
        }
        self.at = at;
    }
    fn add_error(&mut self, message: impl Into<String>, at: usize, extra: Option<usize>) -> bool {
        self.errors.push(ReadError {
            message: message.into(),
            at,
            extra,
        });
        false
    }
    /// `recoverFromError`: tokens skipped up to `until` (or the end); errors on the way dropped.
    fn recover(&mut self, until: Kind) -> bool {
        let count = self.errors.len();
        loop {
            let (_, t) = self.token();
            if t.kind == until || t.kind == Kind::End {
                break;
            }
        }
        self.errors.truncate(count);
        false
    }
    fn error_and_recover(&mut self, message: impl Into<String>, at: usize, until: Kind) -> bool {
        self.add_error(message, at, None);
        self.recover(until)
    }
    /// `readValue` into `out`.
    fn value(&mut self, out: &mut Value) -> bool {
        self.depth += 1;
        if self.depth > 1000 {
            // jsoncpp throws "Exceeded stackLimit in readValue()."; an error here.
            self.depth -= 1;
            let at = self.at;
            return self.add_error("Exceeded stackLimit in readValue().", at, None);
        }
        let (_, t) = self.token();
        let ok = match t.kind {
            Kind::ObjectBegin => self.object(out),
            Kind::ArrayBegin => self.array(out),
            Kind::Number => match decode_number(&self.text[t.start..t.end]) {
                Ok(v) => {
                    *out = v;
                    true
                }
                Err(m) => self.add_error(m, t.start, None),
            },
            Kind::String => match self.decode_string(t) {
                Some(s) => {
                    *out = Value::Str(s);
                    true
                }
                None => false,
            },
            Kind::True => {
                *out = Value::Bool(true);
                true
            }
            Kind::False => {
                *out = Value::Bool(false);
                true
            }
            Kind::Null => {
                *out = Value::Null;
                true
            }
            _ => self.add_error("Syntax error: value, object or array expected.", t.start, None),
        };
        self.depth -= 1;
        ok
    }
    fn object(&mut self, out: &mut Value) -> bool {
        *out = Value::Object(BTreeMap::new());
        let mut name: Vec<u8> = Vec::new();
        let mut last;
        loop {
            let (ok, t) = self.token();
            last = t;
            if !ok {
                break;
            }
            if t.kind == Kind::ObjectEnd && name.is_empty() {
                return true;
            }
            name.clear();
            if t.kind != Kind::String {
                break;
            }
            let Some(decoded) = self.decode_string(t) else {
                return self.recover(Kind::ObjectEnd);
            };
            name = decoded;
            let Value::Object(map) = &*out else { unreachable!() };
            if map.contains_key(&name) {
                let message = format!("Duplicate key: '{}'", String::from_utf8_lossy(&name));
                return self.error_and_recover(message, t.start, Kind::ObjectEnd);
            }
            let (ok, colon) = self.token();
            if !ok || colon.kind != Kind::MemberSeparator {
                return self.error_and_recover("Missing ':' after object member name", colon.start, Kind::ObjectEnd);
            }
            let Value::Object(map) = out else { unreachable!() };
            let slot = map.entry(name.clone()).or_insert(Value::Null);
            if !self.value(slot) {
                return self.recover(Kind::ObjectEnd);
            }
            let (ok, comma) = self.token();
            if !ok || (comma.kind != Kind::ObjectEnd && comma.kind != Kind::ArraySeparator && comma.kind != Kind::Comment) {
                return self.error_and_recover("Missing ',' or '}' in object declaration", comma.start, Kind::ObjectEnd);
            }
            if comma.kind == Kind::ObjectEnd {
                return true;
            }
        }
        self.error_and_recover("Missing '}' or object member name", last.start, Kind::ObjectEnd)
    }
    fn array(&mut self, out: &mut Value) -> bool {
        *out = Value::Array(Vec::new());
        loop {
            self.skip_spaces();
            let Value::Array(items) = out else { unreachable!() };
            if self.at < self.text.len() && self.text[self.at] == b']' && items.is_empty() {
                self.token();
                return true;
            }
            items.push(Value::Null);
            let slot = items.last_mut().unwrap();
            if !self.value(slot) {
                return self.recover(Kind::ArrayEnd);
            }
            let (ok, t) = self.token();
            if !ok || (t.kind != Kind::ArraySeparator && t.kind != Kind::ArrayEnd) {
                return self.error_and_recover("Missing ',' or ']' in array declaration", t.start, Kind::ArrayEnd);
            }
            if t.kind == Kind::ArrayEnd {
                return true;
            }
        }
    }
    /// `decodeString`: the token without its quotes, its escapes undone; None with the error added.
    fn decode_string(&mut self, t: Token) -> Option<Vec<u8>> {
        let end = t.end - 1;
        let mut at = t.start + 1;
        let mut out = Vec::with_capacity(end.saturating_sub(at));
        while at < end {
            let c = self.text[at];
            at += 1;
            if c == b'"' {
                break;
            }
            if c != b'\\' {
                out.push(c);
                continue;
            }
            if at == end {
                self.add_error("Empty escape sequence in string", t.start, Some(at));
                return None;
            }
            let e = self.text[at];
            at += 1;
            match e {
                b'"' => out.push(b'"'),
                b'/' => out.push(b'/'),
                b'\\' => out.push(b'\\'),
                b'b' => out.push(0x08),
                b'f' => out.push(0x0C),
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b't' => out.push(b'\t'),
                b'u' => {
                    let mut code = self.hex4(t, &mut at, end)?;
                    if (0xD800..=0xDBFF).contains(&code) {
                        if end - at < 6 {
                            self.add_error("additional six characters expected to parse unicode surrogate pair.", t.start, Some(at));
                            return None;
                        }
                        let backslash = self.text[at] == b'\\';
                        at += 1;
                        if !(backslash && {
                            let u = self.text[at] == b'u';
                            at += 1;
                            u
                        }) {
                            self.add_error(
                                "expecting another \\u token to begin the second half of a unicode surrogate pair",
                                t.start,
                                Some(at),
                            );
                            return None;
                        }
                        let low = self.hex4(t, &mut at, end)?;
                        code = 0x10000 + ((code & 0x3FF) << 10) + (low & 0x3FF);
                    }
                    utf8(code, &mut out);
                }
                _ => {
                    self.add_error("Bad escape sequence in string", t.start, Some(at));
                    return None;
                }
            }
        }
        Some(out)
    }
    fn hex4(&mut self, t: Token, at: &mut usize, end: usize) -> Option<u32> {
        if end - *at < 4 {
            self.add_error("Bad unicode escape sequence in string: four digits expected.", t.start, Some(*at));
            return None;
        }
        let mut v = 0u32;
        for _ in 0..4 {
            let c = self.text[*at];
            *at += 1;
            v *= 16;
            v += match c {
                b'0'..=b'9' => (c - b'0') as u32,
                b'a'..=b'f' => (c - b'a' + 10) as u32,
                b'A'..=b'F' => (c - b'A' + 10) as u32,
                _ => {
                    self.add_error("Bad unicode escape sequence in string: hexadecimal digit expected.", t.start, Some(*at));
                    return None;
                }
            };
        }
        Some(v)
    }
    /// `getLocationLineAndColumn`: "Line %d, Column %d" of `location`, counted from the document's first byte.
    fn line_and_column(&self, location: usize) -> String {
        let (mut current, mut line_start, mut line) = (0usize, 0usize, 0);
        while current < location && current < self.text.len() {
            let c = self.text[current];
            current += 1;
            if c == b'\r' {
                if current < self.text.len() && self.text[current] == b'\n' {
                    current += 1;
                }
                line_start = current;
                line += 1;
            } else if c == b'\n' {
                line_start = current;
                line += 1;
            }
        }
        format!("Line {}, Column {}", line + 1, location as i64 - line_start as i64 + 1)
    }
    /// `getFormattedErrorMessages`.
    fn formatted(&self) -> String {
        let mut out = String::new();
        for e in &self.errors {
            out.push_str(&format!("* {}\n  {}\n", self.line_and_column(e.at), e.message));
            if let Some(extra) = e.extra {
                out.push_str(&format!("See {} for detail.\n", self.line_and_column(extra)));
            }
        }
        out
    }
}

/// `decodeNumber`: an integer when it is one and fits, else a double.
fn decode_number(token: &[u8]) -> Result<Value, String> {
    let negative = token.first() == Some(&b'-');
    let digits = if negative { &token[1..] } else { token };
    let threshold: u64 = if negative { (i64::MIN / 10).unsigned_abs() } else { u64::MAX / 10 };
    let last: u64 = if negative { (i64::MIN % 10).unsigned_abs() } else { u64::MAX % 10 };
    let mut value: u64 = 0;
    for (i, &c) in digits.iter().enumerate() {
        if !c.is_ascii_digit() {
            return decode_double(token);
        }
        let digit = (c - b'0') as u64;
        if value >= threshold && (value > threshold || i + 1 != digits.len() || digit > last) {
            return decode_double(token);
        }
        value = value * 10 + digit;
    }
    Ok(if negative {
        Value::Int((-((value / 10) as i64)) * 10 - (value % 10) as i64)
    } else if value <= i64::MAX as u64 {
        Value::Int(value as i64)
    } else {
        Value::UInt(value)
    })
}
fn decode_double(token: &[u8]) -> Result<Value, String> {
    std::str::from_utf8(token)
        .ok()
        .and_then(|t| t.parse::<f64>().ok())
        .map(Value::Real)
        .ok_or_else(|| format!("'{}' is not a number.", String::from_utf8_lossy(token)))
}
fn utf8(c: u32, out: &mut Vec<u8>) {
    if c <= 0x7F {
        out.push(c as u8);
    } else if c <= 0x7FF {
        out.push(0xC0 | (c >> 6) as u8);
        out.push(0x80 | (c & 0x3F) as u8);
    } else if c <= 0xFFFF {
        out.push(0xE0 | (c >> 12) as u8);
        out.push(0x80 | ((c >> 6) & 0x3F) as u8);
        out.push(0x80 | (c & 0x3F) as u8);
    } else if c <= 0x10FFFF {
        out.push(0xF0 | (c >> 18) as u8);
        out.push(0x80 | ((c >> 12) & 0x3F) as u8);
        out.push(0x80 | ((c >> 6) & 0x3F) as u8);
        out.push(0x80 | (c & 0x3F) as u8);
    }
}

/// `CharReader::parse` with strictMode: the value, or jsoncpp's formatted messages.
pub fn parse(text: &[u8]) -> Result<Value, String> {
    let mut r = Reader {
        text,
        at: 0,
        depth: 0,
        errors: Vec::new(),
    };
    if text.starts_with(&[0xEF, 0xBB, 0xBF]) {
        r.at = 3;
    }
    let mut root = Value::Null;
    let mut ok = r.value(&mut root);
    let (_, t) = r.token();
    if t.kind != Kind::End {
        r.add_error("Extra non-whitespace after JSON value.", t.start, None);
        ok = false;
    } else if !root.is_array() && !root.is_object() {
        r.add_error("A valid JSON document must be either an array or an object value.", 0, None);
        ok = false;
    }
    if ok {
        Ok(root)
    } else {
        Err(r.formatted())
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_as_jsoncpp() {
        let v = parse(b"{\"b\":[1,2.5,\"x\"],\"a\":{\"k\":[]},\"c\":{},\"d\":1.0,\"e\":\"\xc3\xa9\xf0\x9f\x98\x80\\n\",\"f\":-0}").unwrap();
        let out = String::from_utf8(write(&v)).unwrap();
        let expected = "{\n    \"a\" : \n    {\n        \"k\" : []\n    },\n    \"b\" : \n    [\n        1,\n        2.5,\n        \"x\"\n    ],\n    \"c\" : {},\n    \"d\" : 1.0,\n    \"e\" : \"\\u00e9\\ud83d\\ude00\\n\",\n    \"f\" : 0\n}";
        assert_eq!(out, expected);
        assert_eq!(real_to_string(1.35), "1.3500000000000001");
        assert_eq!(real_to_string(1e20), "1e+20");
    }

    #[test]
    fn strict_reading() {
        for bad in [
            &b"{"[..],
            b"[]x",
            b"{\"a\":1,}",
            b"{\"a\":1,\"a\":2}",
            b"// c\n{}",
            b"1",
            b"{'a':1}",
            b"[1,]",
            b"{\"a\":NaN}",
        ] {
            assert!(parse(bad).is_err(), "{}", String::from_utf8_lossy(bad));
        }
        assert!(parse(b"\xEF\xBB\xBF{}").is_ok());
        assert_eq!(parse(b"[9223372036854775807]").unwrap().at(0), &Value::Int(i64::MAX));
        assert_eq!(parse(b"[18446744073709551615]").unwrap().at(0), &Value::UInt(u64::MAX));
        assert_eq!(parse(b"[18446744073709551616]").unwrap().at(0), &Value::Real(18446744073709551616.0));
        assert_eq!(parse(b"[-9223372036854775808]").unwrap().at(0), &Value::Int(i64::MIN));
        assert_ne!(Value::Int(1), Value::Real(1.0));
    }
}
