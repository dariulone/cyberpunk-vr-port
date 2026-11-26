//! The script natives a C++ registration table names, read statically: the first string literal of every
//! `CGlobalFunction::Create(` / `CClassStaticFunction::Create(` / `CClassFunction::Create(` and of every `add("`
//! helper call. Names only -- the signatures are taken from the running game's RTTI (cvr-probe's dump), which is the
//! authority for what scripts see.
use crate::Result;
use std::collections::BTreeSet;
use std::fs;

fn first_literal(s: &str) -> Option<&str> {
    let start = s.find('"')? + 1;
    let end = start + s[start..].find('"')?;
    // Only a literal that comes before the call's first ')' belongs to the call.
    if s[..start].contains(')') {
        return None;
    }
    Some(&s[start..end])
}

pub fn names(source: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for marker in ["CGlobalFunction::Create(", "CClassStaticFunction::Create(", "CClassFunction::Create(", "add(\""] {
        let mut rest = source;
        while let Some(at) = rest.find(marker) {
            let after = &rest[at + marker.len() - usize::from(marker.ends_with('"'))..];
            let class_call = marker.starts_with("CClass");
            // A class function names its parent first: skip to the second argument.
            let args = if class_call { after.split_once(',').map(|(_, r)| r).unwrap_or(after) } else { after };
            if let Some(name) = first_literal(args) {
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':') {
                    out.insert(name.to_string());
                }
            }
            rest = &rest[at + marker.len()..];
        }
    }
    out
}

pub fn run(paths: &[String]) -> Result {
    let mut all = BTreeSet::new();
    for p in paths {
        let text = fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?;
        all.extend(names(&text));
    }
    for n in &all {
        println!("{n}");
    }
    eprintln!("{} natives", all.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_both_forms() {
        let n = super::names(
            r#"auto f = RED4ext::CGlobalFunction::Create("VRA", "VRA", &VRA);
               add("VRB", &VRB, "Bool", {{"handle:entEntity", "entity"}});
               RED4ext::CClassStaticFunction::Create(cls, "Cls::C", "C", &C);"#,
        );
        assert_eq!(n.into_iter().collect::<Vec<_>>(), vec!["Cls::C".to_string(), "VRA".into(), "VRB".into()]);
    }
}
