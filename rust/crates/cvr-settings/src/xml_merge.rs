//! The port's input bindings merged into the game's input configuration (formerly the pugixml half of
//! src/Core/InputConfig.cpp): a small XML tree read as pugixml's parse_default reads it (comments, declarations,
//! processing instructions and doctypes dropped, whitespace-only text dropped, entities resolved, attribute whitespace
//! turned to spaces, line ends normalised) and written as its save_file(.., "\t", format_default) writes it.

/// An element or a text node of the tree.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Element(Element),
    Text(String),
    CData(String),
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Element {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<Node>,
}

impl Element {
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
    /// pugixml's find_child_by_attribute(name, attr, value): the first child element named `name` with an attribute
    /// `attr` of that value.
    pub fn find_child_by_attribute(&self, name: &str, attr: &str, value: &str) -> Option<usize> {
        self.children.iter().position(|c| match c {
            Node::Element(e) => {
                e.name == name && e.attributes.iter().any(|(n, v)| n == attr && v == value)
            }
            _ => false,
        })
    }
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find_map(|c| match c {
            Node::Element(e) if e.name == name => Some(e),
            _ => None,
        })
    }
    pub fn child_mut(&mut self, name: &str) -> Option<&mut Element> {
        self.children.iter_mut().find_map(|c| match c {
            Node::Element(e) if e.name == name => Some(e),
            _ => None,
        })
    }
}

/// pugixml's as_bool: true for a value starting with 1, t, T, y or Y.
pub fn as_bool(value: Option<&str>) -> bool {
    value
        .and_then(|v| v.bytes().next())
        .is_some_and(|b| matches!(b, b'1' | b't' | b'T' | b'y' | b'Y'))
}

// ---- reading

struct Reader<'a> {
    s: &'a [u8],
    i: usize,
}
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}
fn is_name(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':') || b >= 0x80
}
impl Reader<'_> {
    fn starts(&self, p: &[u8]) -> bool {
        self.s[self.i..].starts_with(p)
    }
    fn skip_space(&mut self) {
        while self.i < self.s.len() && is_space(self.s[self.i]) {
            self.i += 1;
        }
    }
    fn skip_past(&mut self, end: &[u8]) -> Option<()> {
        let at = self.s[self.i..].windows(end.len()).position(|w| w == end)?;
        self.i += at + end.len();
        Some(())
    }
    fn name(&mut self) -> Option<String> {
        let start = self.i;
        while self.i < self.s.len() && is_name(self.s[self.i]) {
            self.i += 1;
        }
        (self.i > start).then(|| String::from_utf8_lossy(&self.s[start..self.i]).into_owned())
    }
}
/// Entities resolved (&lt; &gt; &amp; &quot; &apos; &#N; &#xN;); an unknown one is left as it is (pugixml's parse_escapes).
fn unescape(raw: &[u8], attribute: bool) -> String {
    let mut out: Vec<u8> = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let b = raw[i];
        if b == b'&' {
            let rest = &raw[i..];
            let named: [(&[u8], u8); 5] = [
                (b"&lt;", b'<'),
                (b"&gt;", b'>'),
                (b"&amp;", b'&'),
                (b"&quot;", b'"'),
                (b"&apos;", b'\''),
            ];
            if let Some((e, c)) = named.iter().find(|(e, _)| rest.starts_with(e)) {
                out.push(*c);
                i += e.len();
                continue;
            }
            if rest.starts_with(b"&#") {
                if let Some(end) = rest.iter().position(|&c| c == b';') {
                    let body = &rest[2..end];
                    let code = if body.first().is_some_and(|&c| c == b'x' || c == b'X') {
                        u32::from_str_radix(core::str::from_utf8(&body[1..]).unwrap_or(""), 16).ok()
                    } else {
                        core::str::from_utf8(body)
                            .ok()
                            .and_then(|t| t.parse::<u32>().ok())
                    };
                    if let Some(ch) = code.and_then(char::from_u32) {
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        i += end + 1;
                        continue;
                    }
                }
            }
            out.push(b);
            i += 1;
            continue;
        }
        // parse_eol: \r\n and a lone \r are \n; parse_wconv_attribute: \t \n \r in a value are spaces.
        if b == b'\r' {
            out.push(if attribute { b' ' } else { b'\n' });
            i += if raw.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
            continue;
        }
        if attribute && (b == b'\n' || b == b'\t') {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The document's top-level elements, or a description of why it is not one (pugixml's wording for the common cases).
pub fn parse(text: &[u8]) -> Result<Vec<Element>, &'static str> {
    // A UTF-8 byte order mark is the encoding's, not the document's.
    let s = text.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(text);
    let mut r = Reader { s, i: 0 };
    let mut stack: Vec<Element> = Vec::new();
    let mut top: Vec<Element> = Vec::new();
    while r.i < r.s.len() {
        if r.starts(b"<?") {
            r.skip_past(b"?>")
                .ok_or("Error parsing document declaration/processing instruction")?;
        } else if r.starts(b"<!--") {
            r.skip_past(b"-->").ok_or("Error parsing comment")?;
        } else if r.starts(b"<![CDATA[") {
            r.i += 9;
            let start = r.i;
            r.skip_past(b"]]>").ok_or("Error parsing CDATA section")?;
            let body = String::from_utf8_lossy(&r.s[start..r.i - 3]).replace("\r\n", "\n");
            match stack.last_mut() {
                Some(parent) => parent.children.push(Node::CData(body)),
                None => return Err("Error parsing CDATA section"),
            }
        } else if r.starts(b"<!") {
            r.skip_past(b">")
                .ok_or("Error parsing document type declaration")?;
        } else if r.starts(b"</") {
            r.i += 2;
            let name = r.name().ok_or("Error parsing end element tag")?;
            r.skip_space();
            if !r.starts(b">") {
                return Err("Error parsing end element tag");
            }
            r.i += 1;
            let element = stack.pop().ok_or("Unmatching end element tag")?;
            if element.name != name {
                return Err("Start-end tags mismatch");
            }
            match stack.last_mut() {
                Some(parent) => parent.children.push(Node::Element(element)),
                None => top.push(element),
            }
        } else if r.starts(b"<") {
            r.i += 1;
            let name = r.name().ok_or("Error parsing start element tag")?;
            let mut element = Element {
                name,
                ..Default::default()
            };
            loop {
                r.skip_space();
                if r.starts(b"/>") {
                    r.i += 2;
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(Node::Element(element)),
                        None => top.push(element),
                    }
                    break;
                }
                if r.starts(b">") {
                    r.i += 1;
                    stack.push(element);
                    break;
                }
                let attr = r.name().ok_or("Error parsing start element tag")?;
                r.skip_space();
                if !r.starts(b"=") {
                    return Err("Error parsing attribute");
                }
                r.i += 1;
                r.skip_space();
                let quote = *r.s.get(r.i).ok_or("Error parsing attribute")?;
                if quote != b'"' && quote != b'\'' {
                    return Err("Error parsing attribute");
                }
                r.i += 1;
                let start = r.i;
                let len = r.s[start..]
                    .iter()
                    .position(|&b| b == quote)
                    .ok_or("Error parsing attribute")?;
                r.i = start + len + 1;
                element
                    .attributes
                    .push((attr, unescape(&r.s[start..start + len], true)));
            }
        } else {
            let start = r.i;
            while r.i < r.s.len() && r.s[r.i] != b'<' {
                r.i += 1;
            }
            let raw = &r.s[start..r.i];
            // Whitespace-only text is dropped (parse_ws_pcdata off); text outside the root element is not the tree's.
            if !raw.iter().all(|&b| is_space(b)) {
                match stack.last_mut() {
                    Some(parent) => parent.children.push(Node::Text(unescape(raw, false))),
                    None => {}
                }
            }
        }
    }
    if !stack.is_empty() {
        return Err("Start-end tags mismatch");
    }
    Ok(top)
}

// ---- writing

fn escape(out: &mut String, text: &str, attribute: bool) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' if !attribute => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            c if (c as u32) < 32 && (attribute || !matches!(c, '\t' | '\r' | '\n')) => {
                let n = c as u32;
                out.push_str(&format!("&#{}{};", n / 10, n % 10));
            }
            c => out.push(c),
        }
    }
}
const NEWLINE: u8 = 1;
const INDENT: u8 = 2;
/// pugixml's node_output: the newline and the indent before a node are written by the node that follows (none after a
/// text node, which is written as it is).
fn write_node(out: &mut String, node: &Node, depth: usize, flags: &mut u8) {
    match node {
        Node::Text(t) => {
            escape(out, t, false);
            *flags = 0;
        }
        Node::CData(t) => {
            out.push_str("<![CDATA[");
            out.push_str(t);
            out.push_str("]]>");
            *flags = 0;
        }
        Node::Element(e) => {
            if *flags & NEWLINE != 0 {
                out.push('\n');
            }
            if *flags & INDENT != 0 {
                for _ in 0..depth {
                    out.push('\t');
                }
            }
            out.push('<');
            out.push_str(&e.name);
            for (n, v) in &e.attributes {
                out.push(' ');
                out.push_str(n);
                out.push_str("=\"");
                escape(out, v, true);
                out.push('"');
            }
            *flags = NEWLINE | INDENT;
            if e.children.is_empty() {
                out.push_str(" />");
                return;
            }
            out.push('>');
            for c in &e.children {
                write_node(out, c, depth + 1, flags);
            }
            if *flags & NEWLINE != 0 {
                out.push('\n');
            }
            if *flags & INDENT != 0 {
                for _ in 0..depth {
                    out.push('\t');
                }
            }
            out.push_str("</");
            out.push_str(&e.name);
            out.push('>');
            *flags = NEWLINE | INDENT;
        }
    }
}
/// save_file(.., "	", format_default, encoding_utf8): the declaration, then the tree indented with tabs.
pub fn write(top: &[Element]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\"?>
",
    );
    let mut flags = INDENT;
    for e in top {
        write_node(&mut out, &Node::Element(e.clone()), 0, &mut flags);
    }
    if flags & NEWLINE != 0 {
        out.push('\n');
    }
    out
}

// ---- merging

/// Which file each kind of <bindings> child belongs in, and the attribute that names it (Input Loader's rules: a blend
/// has none and is always added).
struct Kind {
    node: &'static str,
    mappings: bool,
    key: Option<&'static str>,
}
const KINDS: [Kind; 11] = [
    Kind {
        node: "blend",
        mappings: false,
        key: None,
    },
    Kind {
        node: "context",
        mappings: false,
        key: Some("name"),
    },
    Kind {
        node: "hold",
        mappings: false,
        key: Some("action"),
    },
    Kind {
        node: "multitap",
        mappings: false,
        key: Some("action"),
    },
    Kind {
        node: "repeat",
        mappings: false,
        key: Some("action"),
    },
    Kind {
        node: "toggle",
        mappings: false,
        key: Some("action"),
    },
    Kind {
        node: "acceptedEvents",
        mappings: false,
        key: Some("action"),
    },
    Kind {
        node: "mapping",
        mappings: true,
        key: Some("name"),
    },
    Kind {
        node: "buttonGroup",
        mappings: true,
        key: Some("id"),
    },
    Kind {
        node: "pairedAxes",
        mappings: true,
        key: Some("name"),
    },
    Kind {
        node: "preset",
        mappings: true,
        key: Some("name"),
    },
];

/// One port file's <bindings> into the two documents' <bindings>; `skipped` gets each child that is no input binding.
/// Returns the nodes merged.
pub fn merge(
    port: &Element,
    contexts: &mut Element,
    mappings: &mut Element,
    skipped: &mut Vec<String>,
) -> usize {
    let mut merged = 0;
    for node in &port.children {
        let Node::Element(node) = node else { continue };
        let Some(kind) = KINDS.iter().find(|k| k.node == node.name) else {
            skipped.push(node.name.clone());
            continue;
        };
        let target = if kind.mappings {
            &mut *mappings
        } else {
            &mut *contexts
        };
        let existing = kind.key.and_then(|key| {
            node.attribute(key)
                .and_then(|id| target.find_child_by_attribute(&node.name, key, id))
        });
        match existing {
            Some(at) if as_bool(node.attribute("append")) => {
                if let Node::Element(e) = &mut target.children[at] {
                    e.children.extend(node.children.iter().cloned());
                }
            }
            Some(at) => {
                target.children.remove(at);
                target.children.push(Node::Element(node.clone()));
            }
            None => target.children.push(Node::Element(node.clone())),
        }
        merged += 1;
    }
    merged
}

/// std::filesystem::relative for two absolute paths on one root: `..` for each of `base`'s own components, then the
/// rest of `path`.
pub fn relative(path: &std::path::Path, base: &std::path::Path) -> Option<String> {
    let p: Vec<std::path::Component> = path.components().collect();
    let b: Vec<std::path::Component> = base.components().collect();
    let common = p.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if common == 0 {
        return None;
    }
    let mut out = std::path::PathBuf::new();
    for _ in common..b.len() {
        out.push("..");
    }
    for c in &p[common..] {
        out.push(c.as_os_str());
    }
    let s = out.to_string_lossy().into_owned();
    (!s.is_empty()).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_as_pugixml() {
        let text = b"<?xml version=\"1.0\"?>\r\n<bindings>\r\n\t<!-- a comment -->\r\n\t<context name=\"A\" a=\"1&amp;2\">\r\n\t\t<action name=\"x\"/>\r\n\t</context>\r\n\t<blend from=\"__ANY__\" />\r\n</bindings>\r\n";
        let top = parse(text).unwrap();
        assert_eq!(
            write(&top),
            "<?xml version=\"1.0\"?>\n<bindings>\n\t<context name=\"A\" a=\"1&amp;2\">\n\t\t<action name=\"x\" />\n\t</context>\n\t<blend from=\"__ANY__\" />\n</bindings>\n"
        );
    }

    #[test]
    fn merges_by_key_append_and_replace() {
        let mut contexts = parse(b"<bindings><context name=\"A\"><action name=\"a\"/></context><context name=\"B\"/></bindings>").unwrap().remove(0);
        let mut mappings = parse(b"<bindings><mapping name=\"M\"/></bindings>")
            .unwrap()
            .remove(0);
        let port = parse(
            b"<bindings><context name=\"A\" append=\"true\"><action name=\"b\"/></context><context name=\"B\"><action name=\"c\"/></context><mapping name=\"N\"/><other/></bindings>",
        )
        .unwrap()
        .remove(0);
        let mut skipped = Vec::new();
        assert_eq!(merge(&port, &mut contexts, &mut mappings, &mut skipped), 3);
        assert_eq!(skipped, vec!["other".to_string()]);
        assert_eq!(
            write(&[contexts]),
            "<?xml version=\"1.0\"?>\n<bindings>\n\t<context name=\"A\">\n\t\t<action name=\"a\" />\n\t\t<action name=\"b\" />\n\t</context>\n\t<context name=\"B\">\n\t\t<action name=\"c\" />\n\t</context>\n</bindings>\n"
        );
        assert_eq!(write(&[mappings]), "<?xml version=\"1.0\"?>\n<bindings>\n\t<mapping name=\"M\" />\n\t<mapping name=\"N\" />\n</bindings>\n");
    }

    #[test]
    fn relative_goes_up_out_of_r6() {
        let game = std::path::Path::new(r"C:\Games\Cyberpunk 2077");
        let file = game.join(r"red4ext\plugins\cyberpunkvrport\config\input\merged\inputContexts.xml");
        assert_eq!(
            relative(&file, &game.join("r6")).as_deref(),
            Some(r"..\red4ext\plugins\cyberpunkvrport\config\input\merged\inputContexts.xml")
        );
    }

    /// The game's own files and the port's, merged as the C++ half merged them into its last output (when present).
    #[test]
    #[ignore]
    fn the_game_files_merge_byte_for_byte() {
        let game =
            std::path::Path::new(r"C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077");
        let reference = std::path::PathBuf::from(
            std::env::var("CVR_INPUT_REFERENCE").expect("reference folder"),
        );
        let mut contexts =
            parse(&std::fs::read(game.join(r"r6\config\inputContexts.xml")).unwrap()).unwrap();
        let mut mappings =
            parse(&std::fs::read(game.join(r"r6\config\inputUserMappings.xml")).unwrap()).unwrap();
        let port_dir = game.join(r"red4ext\plugins\cyberpunkvrport\config\input");
        let mut files: Vec<_> = std::fs::read_dir(&port_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "xml"))
            .collect();
        files.sort();
        let mut merged = 0;
        for f in files {
            let port = parse(&std::fs::read(f).unwrap()).unwrap();
            let mut skipped = Vec::new();
            let c = contexts.iter_mut().find(|e| e.name == "bindings").unwrap();
            let m = mappings.iter_mut().find(|e| e.name == "bindings").unwrap();
            merged += merge(
                port.iter().find(|e| e.name == "bindings").unwrap(),
                c,
                m,
                &mut skipped,
            );
        }
        assert_eq!(merged, 16);
        std::fs::write(reference.join("rust_inputContexts.xml"), write(&contexts)).unwrap();
        std::fs::write(
            reference.join("rust_inputUserMappings.xml"),
            write(&mappings),
        )
        .unwrap();
        assert_eq!(
            write(&contexts).as_bytes(),
            std::fs::read(reference.join("inputContexts.xml"))
                .unwrap()
                .as_slice()
        );
        assert_eq!(
            write(&mappings).as_bytes(),
            std::fs::read(reference.join("inputUserMappings.xml"))
                .unwrap()
                .as_slice()
        );
    }
}
