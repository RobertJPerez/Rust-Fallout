//! Stream VMAD attachments through the shared plugin reader.
use crate::{Result, census, vmad};
use fallout_data::plugin::{self, Event};
use serde::Serialize;
use serde_json::{Value, json};
use std::{fs::File, io::BufReader, path::Path};

#[derive(Debug, Serialize)]
pub struct Origin {
    pub plugin: String,
    pub record_offset: u64,
    pub record_kind: String,
    pub form_id: u32,
    pub record_version: u16,
    pub record_flags: u32,
    pub payload_offset: usize,
}

/// Callback receives original decompressed VMAD bytes even if decoding fails.
pub fn visit_plugin(
    path: &Path,
    mut visit: impl FnMut(Origin, &[u8], Result<vmad::Adapter<'_>>) -> Result<()>,
) -> Result<()> {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let file = File::open(path)?;
    let length = file.metadata()?.len();
    let mut callback_error = None;
    let result = plugin::visit(
        &mut BufReader::new(file),
        length,
        &name,
        plugin::Limits::default(),
        |event| {
            if let Event::Record(record) = event {
                plugin::visit_subrecords(record, &name, |sub| {
                    if sub.kind == *b"VMAD" {
                        let origin = Origin {
                            plugin: name.clone(),
                            record_offset: record.header.offset,
                            record_kind: plugin::signature(record.header.kind),
                            form_id: record.header.form_id,
                            record_version: record.header.version,
                            record_flags: record.header.flags,
                            payload_offset: sub.payload_offset,
                        };
                        let label = format!(
                            "{name}:{:08X}/VMAD+{}",
                            record.header.form_id, sub.payload_offset
                        );
                        let parsed =
                            vmad::parse(sub.data, record.header.kind, &label, Default::default());
                        if let Err(error) = visit(origin, sub.data, parsed) {
                            callback_error = Some(error);
                            return Err(fallout_data::Error::Unsupported(
                                "VMAD consumer failed".into(),
                            ));
                        }
                    }
                    Ok(())
                })?;
            }
            Ok(())
        },
    );
    if let Some(error) = callback_error {
        return Err(error);
    }
    result?;
    Ok(())
}

/// Canonical semantic token stream for comparison with an independent reader.
/// Hex strings preserve original bytes; lengths delimit every recursive list.
pub fn tokens(adapter: &vmad::Adapter<'_>) -> Vec<Value> {
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
    fn object(o: &vmad::Object, out: &mut Vec<Value>) {
        out.extend([json!(o.form_id), json!(o.alias), json!(o.unused)]);
    }
    fn properties(props: &[vmad::Property<'_>], out: &mut Vec<Value>) {
        out.push(json!(props.len()));
        for p in props {
            out.extend([
                json!(hex(p.name)),
                json!(p.flags),
                json!(p.value.type_code()),
            ]);
            use vmad::Value as V;
            match &p.value {
                V::None => (),
                V::Object(o) => object(o, out),
                V::String(s) => out.push(json!(hex(s))),
                V::Int(v) => out.push(json!(v)),
                V::FloatBits(v) => out.push(json!(v)),
                V::BoolByte(v) => out.push(json!(v)),
                V::Struct(p) => properties(p, out),
                V::Objects(v) => {
                    out.push(json!(v.len()));
                    for o in v {
                        object(o, out);
                    }
                }
                V::Strings(v) => {
                    out.push(json!(v.len()));
                    for s in v {
                        out.push(json!(hex(s)));
                    }
                }
                V::Ints(v) => {
                    out.push(json!(v.len()));
                    out.extend(v.iter().map(|v| json!(v)));
                }
                V::Floats(v) => {
                    out.push(json!(v.len()));
                    out.extend(v.iter().map(|v| json!(v)));
                }
                V::Bools(v) => {
                    out.push(json!(v.len()));
                    out.extend(v.iter().map(|v| json!(v)));
                }
                V::Structs(v) => {
                    out.push(json!(v.len()));
                    for p in v {
                        properties(p, out);
                    }
                }
            }
        }
    }
    fn script(s: &vmad::Script<'_>, out: &mut Vec<Value>) {
        out.extend([json!(hex(s.name)), json!(s.flags)]);
        properties(&s.properties, out);
    }
    fn scripts(s: &[vmad::Script<'_>], out: &mut Vec<Value>) {
        out.push(json!(s.len()));
        for s in s {
            script(s, out);
        }
    }
    let mut out = vec![json!(adapter.version), json!(adapter.object_format)];
    scripts(&adapter.scripts, &mut out);
    out.push(json!(adapter.fragments.is_some()));
    if let Some(f) = &adapter.fragments {
        out.extend([json!(f.extra_bind_version), json!(f.flags)]);
        script(&f.script, &mut out);
        out.push(json!(f.fragments.len()));
        for f in &f.fragments {
            out.extend([
                json!(f.index_bits),
                json!(f.stage_index_bits),
                json!(f.unknown),
                json!(hex(f.script)),
                json!(hex(f.function)),
            ]);
        }
        out.push(json!(f.phases.len()));
        for p in &f.phases {
            out.extend([
                json!(p.flags),
                json!(p.index_bits),
                json!(p.unknown),
                json!(hex(p.script)),
                json!(hex(p.function)),
            ]);
        }
        out.push(json!(f.aliases.len()));
        for a in &f.aliases {
            object(&a.object, &mut out);
            out.extend([json!(a.version), json!(a.object_format)]);
            scripts(&a.scripts, &mut out);
        }
    }
    out
}

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub attachments: u64,
    pub scripts: u64,
    pub empty_scripts: u64,
    pub properties_and_members: u64,
    pub property_types: std::collections::BTreeMap<u8, u64>,
    pub fragment_sections: u64,
    pub fragments: u64,
    pub phases: u64,
    pub aliases: u64,
    pub script_names: std::collections::BTreeMap<String, u64>,
}
impl Counts {
    fn properties(&mut self, props: &[vmad::Property<'_>]) {
        for p in props {
            self.properties_and_members += 1;
            *self.property_types.entry(p.value.type_code()).or_default() += 1;
            match &p.value {
                vmad::Value::Struct(v) => self.properties(v),
                vmad::Value::Structs(v) => {
                    for v in v {
                        self.properties(v);
                    }
                }
                _ => (),
            }
        }
    }
    fn script(&mut self, s: &vmad::Script<'_>) {
        if s.name.is_empty() {
            self.empty_scripts += 1;
        } else {
            self.scripts += 1;
            *self.script_names.entry(census::text(s.name)).or_default() += 1;
        }
        self.properties(&s.properties);
    }
    pub fn add(&mut self, a: &vmad::Adapter<'_>) {
        self.attachments += 1;
        for s in &a.scripts {
            self.script(s);
        }
        if let Some(f) = &a.fragments {
            self.fragment_sections += 1;
            self.script(&f.script);
            self.fragments += f.fragments.len() as u64;
            self.phases += f.phases.len() as u64;
            self.aliases += f.aliases.len() as u64;
            for a in &f.aliases {
                for s in &a.scripts {
                    self.script(s);
                }
            }
        }
    }
}
