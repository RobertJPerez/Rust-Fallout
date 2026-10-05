//! Physical Papyrus definition binding. Never selects an archive/load-order winner.
use crate::{census, pex};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

#[derive(Clone, Debug, Serialize)]
pub struct Source {
    pub archive: String,
    pub path: String,
    pub sha256: String,
    pub object: usize,
}
#[derive(Debug)]
pub struct Member {
    pub name: Vec<u8>,
    pub type_name: Vec<u8>,
    pub range: Range<usize>,
    pub flags: u8,
    pub auto_variable: Option<Vec<u8>>,
}
#[derive(Debug)]
pub struct Method {
    pub name: Vec<u8>,
    pub state: Vec<u8>,
    pub range: Range<usize>,
    pub flags: u8,
}
#[derive(Debug)]
pub struct Declaration {
    pub name: Vec<u8>,
    pub state: Option<Vec<u8>>,
    pub kind: &'static str,
    pub range: Range<usize>,
    pub flags: u8,
    pub return_type: Vec<u8>,
    pub parameter_count: usize,
}
impl Declaration {
    pub fn native(&self) -> bool {
        self.flags & 2 != 0
    }
}
#[derive(Debug)]
pub struct Class {
    pub name: Vec<u8>,
    pub parent: Vec<u8>,
    pub source: Source,
    pub properties: Vec<Member>,
    pub variables: Vec<(Vec<u8>, Vec<u8>)>,
    pub methods: Vec<Method>,
    /// Physical PEX function declarations, including named states and accessors.
    /// These are candidates only; they do not define runtime dispatch.
    pub declarations: Vec<Declaration>,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Binding {
    Unique { class: usize },
    Missing { name: String },
    Ambiguous { name: String, classes: Vec<usize> },
    Empty,
    UnsupportedIdentifier { name: String },
}
impl Binding {
    pub fn status(&self) -> &'static str {
        match self {
            Self::Unique { .. } => "unique",
            Self::Missing { .. } => "missing",
            Self::Ambiguous { .. } => "ambiguous",
            Self::Empty => "empty",
            Self::UnsupportedIdentifier { .. } => "unsupported_identifier",
        }
    }
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Lookup {
    Found {
        class: usize,
        member: usize,
        inheritance_depth: usize,
    },
    Missing,
    AmbiguousMember {
        class: usize,
        members: Vec<usize>,
    },
    Parent {
        binding: Binding,
    },
    Cycle {
        classes: Vec<usize>,
    },
    DepthLimit,
    UnsupportedIdentifier,
    InvalidClass {
        class: usize,
    },
}
impl Lookup {
    pub fn status(&self) -> &'static str {
        match self {
            Self::Found { .. } => "found",
            Self::Missing => "missing",
            Self::AmbiguousMember { .. } => "ambiguous_member",
            Self::Parent { .. } => "unresolved_parent",
            Self::Cycle { .. } => "inheritance_cycle",
            Self::DepthLimit => "depth_limit",
            Self::UnsupportedIdentifier => "unsupported_identifier",
            Self::InvalidClass { .. } => "invalid_class",
        }
    }
}
#[derive(Default, Debug)]
pub struct Catalog {
    classes: Vec<Class>,
    by_name: BTreeMap<Vec<u8>, Vec<usize>>,
}
impl Catalog {
    pub fn classes(&self) -> &[Class] {
        &self.classes
    }
    pub fn add(&mut self, file: &pex::File<'_>, archive: &str, path: &str, sha256: &str) {
        let string = |i: u16| file.strings[i as usize].to_vec();
        for (index, o) in file.objects.iter().enumerate() {
            let class = Class {
                name: string(o.name),
                parent: string(o.parent),
                source: Source {
                    archive: archive.into(),
                    path: path.into(),
                    sha256: sha256.into(),
                    object: index,
                },
                properties: o
                    .property_definitions
                    .iter()
                    .map(|p| Member {
                        name: string(p.name),
                        type_name: string(p.type_name),
                        range: p.range.clone(),
                        flags: p.flags,
                        auto_variable: p.auto_variable.map(string),
                    })
                    .collect(),
                variables: o
                    .variable_definitions
                    .iter()
                    .map(|v| (string(v.name), string(v.type_name)))
                    .collect(),
                methods: o
                    .functions
                    .iter()
                    .filter_map(|f| {
                        f.state.map(|s| Method {
                            name: string(f.name),
                            state: string(s),
                            range: f.range.clone(),
                            flags: f.flags,
                        })
                    })
                    .collect(),
                declarations: o
                    .functions
                    .iter()
                    .map(|f| Declaration {
                        name: string(f.name),
                        state: f.state.map(string),
                        kind: f.kind,
                        range: f.range.clone(),
                        flags: f.flags,
                        return_type: string(f.return_type),
                        parameter_count: f.parameters.len(),
                    })
                    .collect(),
            };
            self.by_name
                .entry(class.name.to_ascii_lowercase())
                .or_default()
                .push(self.classes.len());
            self.classes.push(class);
        }
    }
    pub fn bind(&self, name: &[u8]) -> Binding {
        if name.is_empty() {
            return Binding::Empty;
        }
        if !name.is_ascii() || name.contains(&0) {
            return Binding::UnsupportedIdentifier {
                name: census::text(name),
            };
        }
        match self
            .by_name
            .get(&name.to_ascii_lowercase())
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            [] => Binding::Missing {
                name: census::text(name),
            },
            [class] => Binding::Unique { class: *class },
            classes => Binding::Ambiguous {
                name: census::text(name),
                classes: classes.to_vec(),
            },
        }
    }
    /// Validate the whole parent chain even when a local declaration was found.
    pub fn lineage(&self, start: usize) -> std::result::Result<Vec<usize>, Lookup> {
        let mut current = start;
        let mut seen = BTreeSet::new();
        let mut chain = Vec::new();
        for _ in 0..128 {
            if !seen.insert(current) {
                chain.push(current);
                return Err(Lookup::Cycle { classes: chain });
            }
            let Some(class) = self.classes.get(current) else {
                return Err(Lookup::InvalidClass { class: current });
            };
            chain.push(current);
            match self.bind(&class.parent) {
                Binding::Unique { class } => current = class,
                Binding::Empty => return Ok(chain),
                binding => return Err(Lookup::Parent { binding }),
            }
        }
        Err(Lookup::DepthLimit)
    }
    /// Derived-first lookup, following only uniquely identified parents. Methods
    /// here are the default-state declarations; this is not runtime state dispatch.
    pub fn lookup(&self, start: usize, name: &[u8], method: bool) -> Lookup {
        if !name.is_ascii() || name.contains(&0) {
            return Lookup::UnsupportedIdentifier;
        }
        let mut current = start;
        let mut seen = BTreeSet::new();
        let mut chain = Vec::new();
        for depth in 0..128 {
            if !seen.insert(current) {
                chain.push(current);
                return Lookup::Cycle { classes: chain };
            }
            chain.push(current);
            let Some(class) = self.classes.get(current) else {
                return Lookup::InvalidClass { class: current };
            };
            let members: Vec<usize> = if method {
                class
                    .methods
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.state.is_empty() && m.name.eq_ignore_ascii_case(name))
                    .map(|(i, _)| i)
                    .collect()
            } else {
                class
                    .properties
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.name.eq_ignore_ascii_case(name))
                    .map(|(i, _)| i)
                    .collect()
            };
            match members.as_slice() {
                [member] => {
                    return Lookup::Found {
                        class: current,
                        member: *member,
                        inheritance_depth: depth,
                    };
                }
                [] => (),
                _ => {
                    return Lookup::AmbiguousMember {
                        class: current,
                        members,
                    };
                }
            }
            match self.bind(&class.parent) {
                Binding::Unique { class } => current = class,
                Binding::Empty => return Lookup::Missing,
                binding => return Lookup::Parent { binding },
            }
        }
        Lookup::DepthLimit
    }
    pub fn duplicate_names(&self) -> Vec<serde_json::Value> {
        self.by_name
            .iter()
            .filter(|(_, ids)| ids.len() > 1)
            .map(|(n, ids)| serde_json::json!({"name":census::text(n),"classes":ids}))
            .collect()
    }
    pub fn definition_report(&self) -> Vec<serde_json::Value> {
        self.classes.iter().enumerate().map(|(id,c)| {
            let mut backing_findings=Vec::new();
            for (i,p) in c.properties.iter().enumerate() {
                if let Some(backing)=&p.auto_variable {
                    let vars:Vec<_>=c.variables.iter().filter(|(n,_)|n.eq_ignore_ascii_case(backing)).collect();
                    if vars.len()!=1 || !vars[0].1.eq_ignore_ascii_case(&p.type_name) {
                        backing_findings.push(serde_json::json!({"property":i,"name":census::text(&p.name),"backing":census::text(backing),"variables_found":vars.len(),"expected_type":census::text(&p.type_name)}));
                    }
                }
            }
            let ancestry = match self.lineage(id) {
                Ok(classes) => serde_json::json!({"status":"rooted","classes":classes}),
                Err(issue) => serde_json::json!(issue),
            };
            serde_json::json!({"id":id,"name":census::text(&c.name),"parent":census::text(&c.parent),"parent_binding":self.bind(&c.parent),"ancestry":ancestry,"source":c.source,"properties":c.properties.len(),"variables":c.variables.len(),"methods":c.methods.len(),"auto_property_findings":backing_findings})
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn class(name: &[u8], parent: &[u8], props: &[&[u8]]) -> Class {
        Class {
            name: name.to_vec(),
            parent: parent.to_vec(),
            source: Source {
                archive: "fixture".into(),
                path: "fixture.pex".into(),
                sha256: "synthetic".into(),
                object: 0,
            },
            properties: props
                .iter()
                .map(|n| Member {
                    name: n.to_vec(),
                    type_name: b"Int".to_vec(),
                    range: 0..0,
                    flags: 4,
                    auto_variable: None,
                })
                .collect(),
            variables: Vec::new(),
            methods: Vec::new(),
            declarations: Vec::new(),
        }
    }
    fn catalog(classes: Vec<Class>) -> Catalog {
        let mut c = Catalog {
            classes,
            ..Default::default()
        };
        for (i, cl) in c.classes.iter().enumerate() {
            c.by_name
                .entry(cl.name.to_ascii_lowercase())
                .or_default()
                .push(i);
        }
        c
    }
    #[test]
    fn unique_parent_lookup_is_case_insensitive_and_keeps_owner() {
        let c = catalog(vec![
            class(b"Base", b"", &[b"Inherited"]),
            class(b"DLC:Child", b"base", &[b"Local"]),
        ]);
        assert_eq!(c.bind(b"dlc:CHILD"), Binding::Unique { class: 1 });
        assert_eq!(
            c.lookup(1, b"INHERITED", false),
            Lookup::Found {
                class: 0,
                member: 0,
                inheritance_depth: 1
            }
        );
        assert_eq!(
            c.lookup(1, b"local", false),
            Lookup::Found {
                class: 1,
                member: 0,
                inheritance_depth: 0
            }
        );
        assert_eq!(c.lookup(1, b"absent", false), Lookup::Missing);
        assert_eq!(c.lineage(1).unwrap(), vec![1, 0]);
    }
    #[test]
    fn duplicates_missing_parents_cycles_and_unknown_identifiers_stay_explicit() {
        let c = catalog(vec![
            class(b"Base", b"", &[]),
            class(b"BASE", b"", &[]),
            class(b"Child", b"base", &[]),
        ]);
        assert!(matches!(c.bind(b"base"), Binding::Ambiguous { .. }));
        assert!(matches!(
            c.lookup(2, b"x", false),
            Lookup::Parent {
                binding: Binding::Ambiguous { .. }
            }
        ));
        let c = catalog(vec![class(b"A", b"B", &[]), class(b"B", b"A", &[])]);
        assert!(matches!(c.lookup(0, b"x", false), Lookup::Cycle { .. }));
        assert!(matches!(c.lineage(0), Err(Lookup::Cycle { .. })));
        let c = catalog(vec![class(b"A", b"Missing", &[])]);
        assert!(matches!(
            c.lookup(0, b"x", false),
            Lookup::Parent {
                binding: Binding::Missing { .. }
            }
        ));
        assert!(matches!(
            c.bind(b"\xff"),
            Binding::UnsupportedIdentifier { .. }
        ));
        let c = catalog(vec![class(b"A", b"", &[b"One", b"ONE"])]);
        assert!(matches!(
            c.lookup(0, b"one", false),
            Lookup::AmbiguousMember { .. }
        ));
    }
    #[test]
    fn fragment_lookup_uses_default_state_and_derived_declarations() {
        let mut base = class(b"Base", b"", &[]);
        base.methods.push(Method {
            name: b"Run".to_vec(),
            state: Vec::new(),
            range: 10..20,
            flags: 0,
        });
        let mut child = class(b"Child", b"Base", &[]);
        child.methods.push(Method {
            name: b"Run".to_vec(),
            state: b"Busy".to_vec(),
            range: 30..40,
            flags: 0,
        });
        let mut c = catalog(vec![base, child]);
        assert_eq!(
            c.lookup(1, b"run", true),
            Lookup::Found {
                class: 0,
                member: 0,
                inheritance_depth: 1
            }
        );
        c.classes[1].methods.push(Method {
            name: b"RUN".to_vec(),
            state: Vec::new(),
            range: 50..60,
            flags: 0,
        });
        assert_eq!(
            c.lookup(1, b"run", true),
            Lookup::Found {
                class: 1,
                member: 1,
                inheritance_depth: 0
            }
        );
        assert_eq!(c.lookup(2, b"run", true), Lookup::InvalidClass { class: 2 });
    }
}
