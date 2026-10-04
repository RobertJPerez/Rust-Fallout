//! Exact text-font dependency binding; font layout and rasterization stay unavailable.
use super::{Document, Kind, Span, includes, traits};
use crate::model;
use fallout_data::{
    assets::ArchiveAssets,
    vfs::{AssetPath, AssetSource, profile},
};
use serde::{Deserialize, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, io::Write, mem::size_of, path::Path, path::PathBuf, sync::Arc};

const POLICY: &str = "explicit-font-slot-ini-entry";
const INI_SOURCES: [&str; 3] = [
    "installation/Fallout_default.ini",
    "documents/My Games/FalloutNV/Fallout.ini",
    "documents/My Games/FalloutNV/FalloutPrefs.ini",
];

#[derive(Clone, Copy)]
pub struct Limits {
    pub literal: traits::Limits,
    pub profile: profile::Limits,
    pub selected_line_bytes: usize,
    pub member_bytes: usize,
    pub retained_bytes: usize,
    pub textures: usize,
    pub metadata_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            literal: traits::Limits::default(),
            profile: profile::Limits::default(),
            selected_line_bytes: 4096,
            member_bytes: 8 * 1024 * 1024,
            retained_bytes: 16 * 1024 * 1024,
            textures: 4,
            metadata_bytes: 1024 * 1024,
        }
    }
}
impl Limits {
    fn validate(self) -> model::Result<()> {
        let ceiling = Self::default();
        if self.selected_line_bytes > ceiling.selected_line_bytes
            || self.member_bytes > ceiling.member_bytes
            || self.retained_bytes > ceiling.retained_bytes
            || self.textures > ceiling.textures
            || self.metadata_bytes > ceiling.metadata_bytes
            || self.literal.request_bytes > ceiling.literal.request_bytes
            || self.literal.conversions > ceiling.literal.conversions
            || self.literal.rows > ceiling.literal.rows
            || self.literal.copy_bytes > ceiling.literal.copy_bytes
            || self.literal.metadata_bytes > ceiling.literal.metadata_bytes
            || self.literal.output_bytes > ceiling.literal.output_bytes
            || self.literal.document.source_bytes > ceiling.literal.document.source_bytes
            || self.literal.document.events > ceiling.literal.document.events
            || self.literal.document.nodes > ceiling.literal.document.nodes
            || self.literal.document.depth > ceiling.literal.document.depth
            || self.literal.document.attributes_per_element
                > ceiling.literal.document.attributes_per_element
            || self.literal.document.metadata_bytes > ceiling.literal.document.metadata_bytes
            || self.literal.document.output_bytes > ceiling.literal.document.output_bytes
            || self.profile.files > ceiling.profile.files
            || self.profile.file_bytes > ceiling.profile.file_bytes
            || self.profile.total_bytes > ceiling.profile.total_bytes
            || self.profile.lines > ceiling.profile.lines
            || self.profile.keys > ceiling.profile.keys
            || self.profile.identifier_bytes > ceiling.profile.identifier_bytes
        {
            return Err("Menu font limits exceed supported ceilings".into());
        }
        Ok(())
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub node: usize,
    pub span: Span,
    pub name: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IniSelection {
    pub documents: PathBuf,
    pub local_appdata: PathBuf,
    pub source: String,
    pub sha256: String,
    pub section: String,
    pub key: String,
    /// Physical profile line spans, including their original line terminators.
    pub section_line: Span,
    pub entry_line: Span,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FontMember {
    pub path: String,
    /// Both may be absent for a missing-member observation; a lease requires both.
    pub archive_sha256: Option<String>,
    pub payload_sha256: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub policy: String,
    pub source: includes::Source,
    pub text: Selection,
    pub slot: u32,
    pub ini: IniSelection,
    pub font: FontMember,
    /// Explicit caller dependencies; their relationship to FNT layout is unverified.
    pub textures: Vec<includes::Source>,
}
pub fn read_request(path: &Path, limits: Limits) -> model::Result<Request> {
    limits.validate()?;
    let request = super::read_json(path, limits.literal.request_bytes, "font")?;
    validate_request(&request, limits)?;
    Ok(request)
}
pub fn validate_request(request: &Request, limits: Limits) -> model::Result<()> {
    limits.validate()?;
    if request.schema_version != 1 || request.policy != POLICY {
        return Err("Menu font schema/explicit binding policy differs".into());
    }
    includes::path(&request.source.path)?;
    includes::hash(&request.source.archive_sha256)?;
    includes::hash(&request.source.payload_sha256)?;
    let ini = &request.ini;
    includes::hash(&ini.sha256)?;
    if !INI_SOURCES.contains(&ini.source.as_str())
        || [&ini.documents, &ini.local_appdata]
            .iter()
            .any(|path| !path.is_absolute() || path.to_str().is_none_or(|s| s.len() > 4096))
        || [&ini.section, &ini.key].iter().any(|s| {
            s.is_empty()
                || s.len() > 256
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_graphic() && !b"[]=;#".contains(&b))
        })
        || [ini.section_line, ini.entry_line, request.text.span]
            .iter()
            .any(|span| span.start >= span.end)
        || request
            .text
            .name
            .as_ref()
            .is_some_and(|name| name.is_empty() || name.len() > 256)
    {
        return Err("Menu font exact INI/text selection or explicit roots invalid".into());
    }
    let font = includes::path(&request.font.path)?;
    if !font.bytes().starts_with(b"textures/fonts/") || !font.bytes().ends_with(b".fnt") {
        return Err("Menu font requires an exact full textures/fonts/... FNT member".into());
    }
    match (&request.font.archive_sha256, &request.font.payload_sha256) {
        (Some(archive), Some(payload)) => {
            includes::hash(archive)?;
            includes::hash(payload)?;
        }
        (None, None) => {}
        _ => return Err("Menu font expectation requires both source hashes or neither".into()),
    }
    if request.textures.len() > limits.textures {
        return Err("Menu font texture dependency count exceeded".into());
    }
    let mut seen = BTreeSet::new();
    for texture in &request.textures {
        let path = includes::path(&texture.path)?;
        includes::hash(&texture.archive_sha256)?;
        includes::hash(&texture.payload_sha256)?;
        if !path.bytes().starts_with(b"textures/")
            || !path.bytes().ends_with(b".dds")
            || !seen.insert(path)
        {
            return Err("Menu font requires unique exact full DDS dependencies".into());
        }
    }
    Ok(())
}

#[derive(Serialize)]
pub struct FontSelection {
    pub projection: traits::Projection,
    pub node: usize,
    pub span: Span,
    pub inner_span: Span,
    pub value: f32,
    pub bits: u32,
    pub slot: u32,
}
pub fn project(
    document: &Document,
    request: &Request,
    limits: Limits,
) -> model::Result<FontSelection> {
    validate_request(request, limits)?;
    if let Some(name) = &request.text.name
        && document.named_element(name)? != request.text.node
    {
        return Err("Menu font selected text name/node differs".into());
    }
    let node = document
        .nodes
        .get(request.text.node)
        .filter(|n| n.kind == Kind::Element && n.span == request.text.span)
        .ok_or("Menu font selected text node/span differs")?;
    if node.name.is_none_or(|span| document.text(span) != "text") {
        return Err("Menu font dependency requires an exact text tile".into());
    }
    // Other original text fields stay source observations. Only font is requested.
    let projection = traits::project_exact(
        document,
        &request.source.payload_sha256,
        request.text.node,
        request.text.span,
        &[traits::Conversion {
            name: "font".into(),
            kind: traits::ConversionKind::FiniteF32,
        }],
        limits.literal,
    )?;
    let row = projection
        .rows
        .iter()
        .find(|row| row.name == "font")
        .ok_or("Menu font requires an explicit literal font trait")?;
    let traits::Outcome::Value {
        value: traits::Literal::FiniteF32 { value, bits },
    } = &row.outcome
    else {
        return Err("Menu font requires one explicit literal font trait".into());
    };
    if f64::from(*value) != f64::from(request.slot) {
        return Err("Menu font literal differs from the exact supplied integral slot".into());
    }
    let selection = FontSelection {
        node: row.node.ok_or("Menu font literal source missing")?,
        span: row.span.ok_or("Menu font literal span missing")?,
        inner_span: row
            .inner_span
            .ok_or("Menu font literal inner span missing")?,
        value: *value,
        bits: *bits,
        slot: request.slot,
        projection,
    };
    Ok(selection)
}

#[derive(Serialize)]
pub struct IniBinding {
    pub source_index: usize,
    pub section_line: Span,
    pub section_name: profile::Span,
    pub entry_line: Span,
    pub key: profile::Span,
    pub value: profile::Span,
    pub path: AssetPath,
}
fn physical(span: profile::Span) -> model::Result<Span> {
    Ok(Span {
        start: span.offset,
        end: span
            .offset
            .checked_add(span.bytes)
            .ok_or("Profile span overflow")?,
    })
}
pub fn select_ini(
    snapshot: &profile::Snapshot,
    selection: &IniSelection,
    limits: Limits,
) -> model::Result<IniBinding> {
    limits.validate()?;
    let (source_index, source) = snapshot
        .sources
        .iter()
        .enumerate()
        .find(|(_, source)| source.name == selection.source)
        .ok_or("Menu font selected INI source unavailable")?;
    if source.state != profile::State::Present
        || source.sha256.as_deref() != Some(selection.sha256.as_str())
    {
        return Err("Menu font selected INI source absent/empty or SHA differs".into());
    }
    let mut section = None;
    let mut setting = None;
    for line in &source.lines {
        match &line.content {
            profile::LineKind::Section { name }
                if name
                    .read(source)
                    .is_some_and(|raw| raw.eq_ignore_ascii_case(selection.section.as_bytes())) =>
            {
                if section.is_some() || name.read(source) != Some(selection.section.as_bytes()) {
                    return Err("Menu font INI section repeated or exact spelling differs".into());
                }
                if physical(line.span)? != selection.section_line
                    || line.span.bytes > limits.selected_line_bytes
                {
                    return Err("Menu font selected INI section line/span budget differs".into());
                }
                section = Some(*name);
            }
            profile::LineKind::Setting {
                section: Some(name),
                key,
                value,
                duplicate_of,
            } if name
                .read(source)
                .is_some_and(|raw| raw.eq_ignore_ascii_case(selection.section.as_bytes()))
                && key
                    .read(source)
                    .is_some_and(|raw| raw.eq_ignore_ascii_case(selection.key.as_bytes())) =>
            {
                if setting.is_some()
                    || duplicate_of.is_some()
                    || key.read(source) != Some(selection.key.as_bytes())
                {
                    return Err("Menu font INI key repeated or exact spelling differs".into());
                }
                if physical(line.span)? != selection.entry_line
                    || line.span.bytes > limits.selected_line_bytes
                    || value.bytes > 4096
                {
                    return Err("Menu font selected INI entry line/span budget differs".into());
                }
                setting = Some((*name, *key, *value));
            }
            _ => {}
        }
    }
    let section_name = section.ok_or("Menu font exact INI section missing")?;
    let (setting_section, key, value) = setting.ok_or("Menu font exact INI key missing")?;
    if setting_section != section_name {
        return Err("Menu font INI key belongs to a different source section".into());
    }
    let raw = value
        .read(source)
        .ok_or("Menu font INI value span invalid")?;
    let path = includes::path(std::str::from_utf8(raw)?)?;
    Ok(IniBinding {
        source_index,
        section_line: selection.section_line,
        section_name,
        entry_line: selection.entry_line,
        key,
        value,
        path,
    })
}

#[derive(Clone, Serialize)]
pub struct MemberReceipt {
    pub path: AssetPath,
    pub source: AssetSource,
    pub archive_sha256: String,
    pub payload_sha256: String,
    pub bytes: usize,
}
pub struct Payload {
    pub receipt: MemberReceipt,
    pub bytes: Arc<[u8]>,
}
#[derive(Serialize)]
pub struct TextReceipt {
    pub path: AssetPath,
    pub archive_sha256: String,
    pub payload_sha256: String,
    pub text_node: usize,
    pub text_span: Span,
    pub font_node: usize,
    pub font_span: Span,
    pub font_bits: u32,
    pub slot: u32,
}
/// Immutable bytes and existing profile source pins for a later font consumer.
pub struct PayloadLease {
    pub profile: Arc<profile::Snapshot>,
    pub text: TextReceipt,
    pub font: Payload,
    pub textures: Vec<Payload>,
}
#[derive(Serialize)]
pub struct MissingMember {
    pub path: AssetPath,
    /// Zero is the font, followed by the caller's texture order.
    pub request_index: usize,
}
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Outcome {
    MissingMembers {
        missing: Vec<MissingMember>,
        present_payload_identities_unverified: bool,
    },
    PayloadsBound {
        font: MemberReceipt,
        textures: Vec<MemberReceipt>,
    },
}
#[derive(Default, Serialize)]
pub struct Usage {
    pub retained_payload_bytes: usize,
    pub metadata_bytes: usize,
    pub requested_members: usize,
}
fn serialize_profile<S: Serializer>(
    snapshot: &Arc<profile::Snapshot>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    snapshot.as_ref().serialize(serializer)
}
#[derive(Serialize)]
pub struct Report<'a> {
    pub schema_version: u32,
    pub request: &'a Request,
    pub source: super::Report,
    pub selection: FontSelection,
    #[serde(serialize_with = "serialize_profile")]
    pub profile: Arc<profile::Snapshot>,
    pub ini: IniBinding,
    pub outcome: Outcome,
    pub usage: Usage,
    pub font_codec_available: bool,
    pub font_layout_available: bool,
    pub font_texture_relationship_verified: bool,
    pub original_display_ready: bool,
    pub interpretation: &'static str,
}
fn charge(total: &mut usize, amount: usize, maximum: usize) -> model::Result<()> {
    *total = total
        .checked_add(amount)
        .filter(|v| *v <= maximum)
        .ok_or("Menu font dependency metadata budget exceeded")?;
    Ok(())
}
pub fn bind<'a>(
    install: &Path,
    request: &'a Request,
    limits: Limits,
) -> model::Result<(Report<'a>, Option<Arc<PayloadLease>>)> {
    validate_request(request, limits)?;
    let source = super::inspect(
        install,
        &includes::path(&request.source.path)?,
        None,
        limits.literal.document,
    )?;
    if source.archive_sha256 != request.source.archive_sha256
        || source.payload_sha256 != request.source.payload_sha256
    {
        return Err("Menu font XML archive/payload SHA differs".into());
    }
    let selection = project(&source.document, request, limits)?;
    let snapshot = Arc::new(profile::observe(
        install,
        &request.ini.documents,
        &request.ini.local_appdata,
        limits.profile,
    )?);
    let ini = select_ini(&snapshot, &request.ini, limits)?;
    if ini.path != includes::path(&request.font.path)? {
        return Err("Menu font exact INI value differs from supplied font member".into());
    }
    let mut usage = Usage {
        requested_members: 1 + request.textures.len(),
        ..Default::default()
    };
    charge(
        &mut usage.metadata_bytes,
        size_of::<Report<'_>>()
            + size_of::<PayloadLease>()
            + usage.requested_members * (size_of::<Payload>() + 2 * size_of::<MemberReceipt>())
            + ini.path.bytes().len(),
        limits.metadata_bytes,
    )?;
    let mut paths = Vec::with_capacity(usage.requested_members);
    paths.push(ini.path.clone());
    for texture in &request.textures {
        paths.push(includes::path(&texture.path)?);
    }
    let mut assets = ArchiveAssets::open_nv(install)?;
    let mut missing = Vec::new();
    // Check every requested candidate before retaining a payload prefix.
    for (request_index, path) in paths.iter().enumerate() {
        match assets.candidates(path)?.len() {
            0 => {
                charge(
                    &mut usage.metadata_bytes,
                    size_of::<MissingMember>() + path.bytes().len(),
                    limits.metadata_bytes,
                )?;
                missing.push(MissingMember {
                    path: path.clone(),
                    request_index,
                });
            }
            1 => {}
            _ => return Err("Menu font dependency has ambiguous archive candidates".into()),
        }
    }
    let (outcome, lease) = if !missing.is_empty() {
        (
            Outcome::MissingMembers {
                missing,
                present_payload_identities_unverified: true,
            },
            None,
        )
    } else {
        let (Some(font_archive), Some(font_payload)) =
            (&request.font.archive_sha256, &request.font.payload_sha256)
        else {
            return Err(
                "Menu font present member requires exact archive/payload expectations".into(),
            );
        };
        let mut payloads = Vec::with_capacity(paths.len());
        for (index, path) in paths.iter().enumerate() {
            let remaining = limits
                .retained_bytes
                .checked_sub(usage.retained_payload_bytes)
                .ok_or("Menu font aggregate payload budget exceeded")?;
            let (asset_source, bytes) =
                assets.read_unique_bounded(path, limits.member_bytes.min(remaining) as u64)?;
            let archive_sha256 = assets.source_digest(&asset_source)?.to_owned();
            let payload_sha256 = format!("{:x}", Sha256::digest(&bytes));
            let (archive, payload) = if index == 0 {
                (font_archive, font_payload)
            } else {
                (
                    &request.textures[index - 1].archive_sha256,
                    &request.textures[index - 1].payload_sha256,
                )
            };
            if &archive_sha256 != archive || &payload_sha256 != payload {
                return Err("Menu font dependency archive/payload SHA differs".into());
            }
            charge(
                &mut usage.retained_payload_bytes,
                bytes.len(),
                limits.retained_bytes,
            )?;
            // Account for the receipt in the report and in the immutable lease.
            charge(
                &mut usage.metadata_bytes,
                2 * (path.bytes().len()
                    + asset_source.container.len()
                    + asset_source.original_path.len()
                    + 128),
                limits.metadata_bytes,
            )?;
            payloads.push(Payload {
                receipt: MemberReceipt {
                    path: path.clone(),
                    source: asset_source,
                    archive_sha256,
                    payload_sha256,
                    bytes: bytes.len(),
                },
                bytes: Arc::from(bytes),
            });
        }
        let mut iter = payloads.into_iter();
        let font = iter.next().expect("font precedes textures");
        let textures: Vec<_> = iter.collect();
        let outcome = Outcome::PayloadsBound {
            font: font.receipt.clone(),
            textures: textures.iter().map(|p| p.receipt.clone()).collect(),
        };
        charge(
            &mut usage.metadata_bytes,
            size_of::<TextReceipt>() + request.source.path.len() + 128,
            limits.metadata_bytes,
        )?;
        let text = TextReceipt {
            path: includes::path(&request.source.path)?,
            archive_sha256: source.archive_sha256.clone(),
            payload_sha256: source.payload_sha256.clone(),
            text_node: request.text.node,
            text_span: request.text.span,
            font_node: selection.node,
            font_span: selection.span,
            font_bits: selection.bits,
            slot: selection.slot,
        };
        (
            outcome,
            Some(Arc::new(PayloadLease {
                profile: snapshot.clone(),
                text,
                font,
                textures,
            })),
        )
    };
    Ok((
        Report {
            schema_version: 1,
            request,
            source,
            selection,
            profile: snapshot,
            ini,
            outcome,
            usage,
            font_codec_available: false,
            font_layout_available: false,
            font_texture_relationship_verified: false,
            original_display_ready: false,
            interpretation: "Exact source text font trait and caller-selected existing INI entry; immutable hash-bound font and caller-declared texture bytes when all members are available. Effective INI/slot association is supplied by caller, original precedence unverified; no FNT decode, glyph layout, inferred atlas relation, font substitution or original menu readiness",
        },
        lease,
    ))
}
pub fn write_report(writer: impl Write, report: &Report<'_>, limit: usize) -> model::Result<()> {
    if limit > Limits::default().literal.output_bytes {
        return Err("Menu font report ceiling exceeded".into());
    }
    super::write_json(writer, report, limit)
}

#[cfg(test)]
mod tests;
