//! The port's archive manifest (config\archive.yaml in the plugin's folder; the repository's
//! mods\archive\cyberpunkvrport.archive.xl), in ArchiveXL's format, read as src/Core/ArchiveExtensions.cpp read it with
//! yaml-cpp: the item factories, the animation sets with their entities, and the resource patches.
//!
//! A conversion yaml-cpp would throw on ends the read there, with what was read before it kept -- the C++ read filled
//! its manifest as it went, and the depot then applied the part it had.
use crate::yaml::{self, Node};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Animation {
    pub entities: Vec<String>,
    /// Empty: the set goes to every animated component of the entity.
    pub component: String,
    pub set: String,
    pub vars: Vec<String>,
    pub priority: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Patch {
    pub source: String,
    pub targets: Vec<String>,
    pub props: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    pub factories: Vec<String>,
    pub animations: Vec<Animation>,
    pub patches: Vec<Patch>,
}

/// `as<std::string>()`: a scalar's text ("null" for a null); a missing node or a collection throws.
fn string(node: Option<&Node>, key: &str) -> Result<String, String> {
    match node {
        None => Err(format!("invalid node; first invalid key: \"{key}\"")),
        Some(n) => n.text().ok_or_else(|| "bad conversion".to_owned()),
    }
}
/// The C++ read's List(): one scalar, or a sequence of scalars; anything else none.
fn list(node: Option<&Node>) -> Result<Vec<String>, String> {
    match node {
        Some(Node::Scalar { text, .. }) => Ok(vec![text.clone()]),
        Some(Node::Seq { items, .. }) => items.iter().map(|i| string(Some(i), "")).collect(),
        _ => Ok(Vec::new()),
    }
}
/// The items a range-for over a node visits: a sequence's (a map's entries are not nodes with keys of their own).
fn items(node: Option<&Node>) -> &[Node] {
    match node {
        Some(Node::Seq { items, .. }) => items,
        _ => &[],
    }
}

fn read_into(root: &Node, out: &mut Manifest) -> Result<(), String> {
    out.factories = list(root.get("factories"))?;
    for item in items(root.get("animations")) {
        let entities = list(item.get("entity"))?;
        let component = match item.get("component") {
            Some(c) => string(Some(c), "component")?,
            None => String::new(),
        };
        let set = string(item.get("set"), "set")?;
        let vars = list(item.get("vars"))?;
        let priority = match item.get("priority") {
            Some(p) => yaml::as_int(p).ok_or_else(|| "bad conversion".to_owned())?,
            None => 128,
        };
        out.animations.push(Animation {
            entities,
            component,
            set,
            vars,
            priority,
        });
    }
    match root.get("resource").and_then(|r| r.get("patch")) {
        Some(Node::Map { entries, .. }) => {
            for (key, value) in entries {
                let source = string(Some(key), "")?;
                let (targets, props) = match value {
                    Node::Map { .. } => (list(value.get("targets"))?, list(value.get("props"))?),
                    _ => (list(Some(value))?, Vec::new()),
                };
                out.patches.push(Patch { source, targets, props });
            }
        }
        // A sequence's items have no key: `item.first` is an invalid node.
        Some(Node::Seq { items, .. }) if !items.is_empty() => {
            return Err("invalid node; this may result from using a map iterator as a sequence iterator, or vice-versa".into())
        }
        _ => {}
    }
    Ok(())
}

/// The manifest, and the error that ended its read early (what was read before it is kept).
pub fn parse(text: &str) -> (Manifest, Option<String>) {
    let mut manifest = Manifest::default();
    let error = match yaml::load(text) {
        Ok(root) => read_into(&root, &mut manifest).err(),
        Err(e) => Some(e),
    };
    (manifest, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
factories:
  - base\vrport\vrp_props.csv
  - vrcigarette\vr_wheel.csv
resource:
  patch:
    vrport\tutorial\q000_tutorial.streamingblock:
      - base\worlds\04_main_menu\_compiled\default\04_main_menu.streamingworld
    vrport\construct\a.mesh:
      props: [appearances]
      targets:
        - base\a.mesh
animations:
  - entity:
      - base\player_wa_fpp.ent
      - ep1\player_wa_fpp_ep1.ent
    component: root
    set: vrport\lefthanded\default_weapon_left.anims
    vars: [vr_left_handed]
    priority: 196
  - entity: base\one.ent
    set: vrport\b.anims
"#;

    #[test]
    fn the_manifest_reads_as_the_cpp_read_it() {
        let (m, error) = parse(SAMPLE);
        assert_eq!(error, None);
        assert_eq!(
            m.factories,
            ["base\\vrport\\vrp_props.csv", "vrcigarette\\vr_wheel.csv"]
        );
        assert_eq!(m.animations.len(), 2);
        assert_eq!(m.animations[0].entities.len(), 2);
        assert_eq!(m.animations[0].component, "root");
        assert_eq!(m.animations[0].vars, ["vr_left_handed"]);
        assert_eq!(m.animations[0].priority, 196);
        assert_eq!(m.animations[1].entities, ["base\\one.ent"]);
        assert_eq!(
            (m.animations[1].component.as_str(), m.animations[1].priority),
            ("", 128)
        );
        assert_eq!(m.patches.len(), 2);
        assert_eq!(m.patches[0].targets.len(), 1);
        assert!(m.patches[0].props.is_empty());
        assert_eq!(m.patches[1].props, ["appearances"]);
        assert_eq!(m.patches[1].targets, ["base\\a.mesh"]);
    }

    #[test]
    fn a_bad_entry_ends_the_read_and_keeps_what_came_before() {
        let (m, error) = parse(
            "factories: [a.csv]\nanimations:\n  - entity: x.ent\n  - entity: y.ent\n    set: s\n",
        );
        assert!(error.unwrap().contains("set"));
        assert_eq!(m.factories, ["a.csv"]);
        assert!(m.animations.is_empty());
        let (_, error) = parse("animations:\n  - set: s\n    priority: high\n");
        assert_eq!(error.as_deref(), Some("bad conversion"));
    }

    #[test]
    fn the_repository_manifest_reads() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../mods/archive/cyberpunkvrport.archive.xl"
        );
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let (m, error) = parse(&text);
        assert_eq!(error, None);
        assert_eq!(m.factories.len(), 2);
        assert!(m.patches.len() >= 7 && !m.animations.is_empty());
    }
}
