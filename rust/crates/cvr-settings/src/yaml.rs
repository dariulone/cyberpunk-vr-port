//! YAML as yaml-cpp gives it: a tree of nulls, scalars (their text), sequences and maps, each with its tag, read
//! from yaml-rust2's events. A plain `~`, `null` or empty scalar is a null, as yaml-cpp reads it; every other scalar
//! keeps its text, numbers and booleans included -- the reader converts as `as<T>()` would.
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::{Marker, TScalarStyle};

#[derive(Clone, Debug)]
pub enum Node {
    Null,
    Scalar {
        text: String,
        tag: String,
    },
    Seq {
        items: Vec<Node>,
        tag: String,
    },
    Map {
        entries: Vec<(Node, Node)>,
        tag: String,
    },
}
impl Node {
    pub fn tag(&self) -> &str {
        match self {
            Node::Null => "",
            Node::Scalar { tag, .. } | Node::Seq { tag, .. } | Node::Map { tag, .. } => tag,
        }
    }
    /// `as<std::string>()` of a scalar (a null reads "null", as yaml-cpp's does).
    pub fn text(&self) -> Option<String> {
        match self {
            Node::Scalar { text, .. } => Some(text.clone()),
            Node::Null => Some("null".into()),
            _ => None,
        }
    }
    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Map { entries, .. } => entries
                .iter()
                .find(|(k, _)| matches!(k, Node::Scalar { text, .. } if text == key))
                .map(|(_, v)| v),
            _ => None,
        }
    }
}
#[derive(Default)]
struct Builder {
    stack: Vec<(Node, Option<Node>)>,
    done: Option<Node>,
}
fn tag_of(tag: Option<yaml_rust2::parser::Tag>) -> String {
    tag.map(|t| format!("{}{}", t.handle, t.suffix))
        .unwrap_or_default()
}
impl Builder {
    fn push_value(&mut self, node: Node) {
        match self.stack.last_mut() {
            None => {
                if self.done.is_none() {
                    self.done = Some(node);
                }
            }
            Some((Node::Seq { items, .. }, _)) => items.push(node),
            Some((Node::Map { entries, .. }, key)) => match key.take() {
                None => *key = Some(node),
                Some(k) => entries.push((k, node)),
            },
            Some(_) => {}
        }
    }
}
impl MarkedEventReceiver for Builder {
    fn on_event(&mut self, ev: Event, _mark: Marker) {
        match ev {
            Event::Scalar(text, style, _, tag) => {
                let tag = tag_of(tag);
                let null = style == TScalarStyle::Plain
                    && tag.is_empty()
                    && matches!(text.as_str(), "" | "~" | "null" | "Null" | "NULL");
                self.push_value(if null {
                    Node::Null
                } else {
                    Node::Scalar { text, tag }
                });
            }
            Event::SequenceStart(_, tag) => self.stack.push((
                Node::Seq {
                    items: Vec::new(),
                    tag: tag_of(tag),
                },
                None,
            )),
            Event::MappingStart(_, tag) => self.stack.push((
                Node::Map {
                    entries: Vec::new(),
                    tag: tag_of(tag),
                },
                None,
            )),
            Event::SequenceEnd | Event::MappingEnd => {
                if let Some((node, _)) = self.stack.pop() {
                    self.push_value(node);
                }
            }
            _ => {}
        }
    }
}
pub fn load(text: &str) -> Result<Node, String> {
    let mut b = Builder::default();
    Parser::new_from_str(text)
        .load(&mut b, false)
        .map_err(|e| e.to_string())?;
    Ok(b.done.unwrap_or(Node::Null))
}

/// yaml-cpp's `as<int>()`: the whole scalar an int.
pub fn as_int(node: &Node) -> Option<i32> {
    node.text().and_then(|t| t.trim().parse::<i32>().ok())
}
/// yaml-cpp's `as<float>()`.
pub fn as_float(node: &Node) -> Option<f32> {
    node.text().and_then(|t| t.trim().parse::<f32>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_and_nulls_are_kept() {
        let root = load(
            "a: !append [1, ~]
b: null
c: 'x'
",
        )
        .unwrap();
        let Some(Node::Seq { items, tag }) = root.get("a") else {
            panic!()
        };
        assert_eq!(tag, "!append");
        assert!(matches!(items[1], Node::Null));
        assert!(matches!(root.get("b"), Some(Node::Null)));
        assert_eq!(root.get("c").and_then(Node::text).as_deref(), Some("x"));
        assert_eq!(as_int(&load("42").unwrap()), Some(42));
    }

    #[test]
    fn a_broken_document_is_an_error() {
        assert!(load("a: [1, 2").is_err());
    }
}
