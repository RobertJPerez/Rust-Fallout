//! Bounded structural decoding for Fallout 4 BGSM/BGEM source files.
//! This extracts texture slots only; it does not evaluate rendering behavior.
use crate::{Error, Result, bad};
use serde::Serialize;
use serde_json::Value;

const MAX_STRING_CHARS: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Bgsm,
    Bgem,
}
impl Kind {
    pub fn from_extension(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("bgsm") {
            Some(Self::Bgsm)
        } else if value.eq_ignore_ascii_case("bgem") {
            Some(Self::Bgem)
        } else {
            None
        }
    }
    fn signature(self) -> &'static [u8; 4] {
        match self {
            Self::Bgsm => b"BGSM",
            Self::Bgem => b"BGEM",
        }
    }
    fn texture_fields(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Bgsm => &[
                ("DiffuseTexture", "sDiffuseTexture"),
                ("NormalTexture", "sNormalTexture"),
                ("SmoothSpecTexture", "sSmoothSpecTexture"),
                ("GreyscaleTexture", "sGreyscaleTexture"),
                ("EnvmapTexture", "sEnvmapTexture"),
                ("GlowTexture", "sGlowTexture"),
                ("InnerLayerTexture", "sInnerLayerTexture"),
                ("WrinklesTexture", "sWrinklesTexture"),
                ("DisplacementTexture", "sDisplacementTexture"),
                ("SpecularTexture", "sSpecularTexture"),
                ("LightingTexture", "sLightingTexture"),
                ("FlowTexture", "sFlowTexture"),
                ("DistanceFieldAlphaTexture", "sDistanceFieldAlphaTexture"),
            ],
            Self::Bgem => &[
                ("BaseTexture", "sBaseTexture"),
                ("GrayscaleTexture", "sGrayscaleTexture"),
                ("EnvmapTexture", "sEnvmapTexture"),
                ("NormalTexture", "sNormalTexture"),
                ("EnvmapMaskTexture", "sEnvmapMaskTexture"),
                ("SpecularTexture", "sSpecularTexture"),
                ("LightingTexture", "sLightingTexture"),
                ("GlowTexture", "sGlowTexture"),
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Binary,
    Json,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TextureSlot {
    pub field: &'static str,
    pub value: String,
}

/// A field value with exact float/color source bits retained for offline comparison.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum FieldValue {
    Bool {
        decoded: bool,
        source_byte: u8,
    },
    U8(u8),
    F32Bits(u32),
    String(String),
    Enum {
        decoded: String,
        source_words: [u32; 3],
    },
    ColorRgb {
        packed_rgb: u32,
        source_component_bits: [u32; 3],
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MaterialField {
    pub name: &'static str,
    pub value: FieldValue,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Material {
    pub kind: Kind,
    pub format: Format,
    pub version: Option<u32>,
    pub bytes_consumed: usize,
    pub noncanonical_boolean_bytes: usize,
    pub textures: Vec<TextureSlot>,
    pub fields: Vec<MaterialField>,
}

pub fn parse(bytes: &[u8], kind: Kind, source: &str) -> Result<Material> {
    if bytes
        .first()
        .is_some_and(|byte| matches!(byte, b'{' | b'['))
    {
        return parse_json(bytes, kind, source);
    }
    let mut reader = Reader {
        bytes,
        offset: 0,
        source,
        noncanonical_boolean_bytes: 0,
    };
    let signature = reader.take(4)?;
    if signature != kind.signature() {
        return Err(bad(
            source,
            0,
            "BGSM/BGEM signature does not match file kind",
        ));
    }
    let version = reader.u32()?;
    if version != 2 {
        return Err(Error::Unsupported(format!(
            "{source}: BGSM/BGEM binary version {version} is not implemented"
        )));
    }
    let mut fields = Vec::new();
    reader.base(version, &mut fields)?;
    let mut textures = Vec::new();
    match kind {
        Kind::Bgsm => reader.bgsm_v2(&mut textures, &mut fields)?,
        Kind::Bgem => reader.bgem_v2(&mut textures, &mut fields)?,
    }
    if reader.offset != bytes.len() {
        return Err(bad(
            source,
            reader.offset,
            "trailing bytes after complete BGSM/BGEM v2 body",
        ));
    }
    Ok(Material {
        kind,
        format: Format::Binary,
        version: Some(version),
        bytes_consumed: reader.offset,
        noncanonical_boolean_bytes: reader.noncanonical_boolean_bytes,
        textures,
        fields,
    })
}

fn parse_json(bytes: &[u8], kind: Kind, source: &str) -> Result<Material> {
    let value: Value = serde_json::from_slice(bytes)?;
    let object = value
        .as_object()
        .ok_or_else(|| bad(source, 0, "material JSON root is not an object"))?;
    let mut textures = Vec::new();
    for (field, json_name) in kind.texture_fields() {
        match object.get(*json_name) {
            None | Some(Value::Null) => {}
            Some(Value::String(value)) if !value.is_empty() => textures.push(TextureSlot {
                field,
                value: value.clone(),
            }),
            Some(Value::String(_)) => {}
            Some(_) => {
                return Err(bad(
                    source,
                    0,
                    format!("JSON texture field {json_name} is not a string or null"),
                ));
            }
        }
    }
    Ok(Material {
        kind,
        format: Format::Json,
        version: None,
        bytes_consumed: bytes.len(),
        noncanonical_boolean_bytes: 0,
        textures,
        fields: Vec::new(),
    })
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
    source: &'a str,
    noncanonical_boolean_bytes: usize,
}
impl Reader<'_> {
    fn take(&mut self, length: usize) -> Result<&[u8]> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| bad(self.source, self.offset, "material range overflow"))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| bad(self.source, self.offset, "truncated BGSM/BGEM field"))?;
        self.offset = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self, fields: &mut Vec<MaterialField>, name: &'static str) -> Result<()> {
        let bits = u32::from_le_bytes(self.take(4)?.try_into().unwrap());
        fields.push(MaterialField {
            name,
            value: FieldValue::F32Bits(bits),
        });
        Ok(())
    }
    fn bool(&mut self, fields: &mut Vec<MaterialField>, name: &'static str) -> Result<()> {
        let source_byte = self.u8()?;
        if source_byte > 1 {
            self.noncanonical_boolean_bytes += 1;
        }
        fields.push(MaterialField {
            name,
            value: FieldValue::Bool {
                decoded: source_byte != 0,
                source_byte,
            },
        });
        Ok(())
    }
    fn u8_field(&mut self, fields: &mut Vec<MaterialField>, name: &'static str) -> Result<u8> {
        let value = self.u8()?;
        fields.push(MaterialField {
            name,
            value: FieldValue::U8(value),
        });
        Ok(value)
    }
    fn string_field(&mut self, fields: &mut Vec<MaterialField>, name: &'static str) -> Result<()> {
        let value = self.string()?;
        fields.push(MaterialField {
            name,
            value: FieldValue::String(value),
        });
        Ok(())
    }
    fn color(&mut self, fields: &mut Vec<MaterialField>, name: &'static str) -> Result<()> {
        let bits = [
            u32::from_le_bytes(self.take(4)?.try_into().unwrap()),
            u32::from_le_bytes(self.take(4)?.try_into().unwrap()),
            u32::from_le_bytes(self.take(4)?.try_into().unwrap()),
        ];
        let packed_rgb = bits.iter().fold(0_u32, |packed, raw| {
            (packed << 8) | ((f32::from_bits(*raw) * 255.0) as u8 as u32)
        });
        fields.push(MaterialField {
            name,
            value: FieldValue::ColorRgb {
                packed_rgb,
                source_component_bits: bits,
            },
        });
        Ok(())
    }
    fn string(&mut self) -> Result<String> {
        let length = self.u32()? as usize;
        if length > MAX_STRING_CHARS || length > self.bytes.len().saturating_sub(self.offset) {
            return Err(bad(
                self.source,
                self.offset - 4,
                "material string character count exceeds input or budget",
            ));
        }
        let mut chars = Vec::new();
        chars.try_reserve_exact(length).map_err(|error| {
            Error::Unsupported(format!(
                "{}: material string allocation: {error}",
                self.source
            ))
        })?;
        for _ in 0..length {
            let first = *self
                .bytes
                .get(self.offset)
                .ok_or_else(|| bad(self.source, self.offset, "truncated material string"))?;
            let encoded_length = match first {
                0x00..=0x7f => 1,
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => {
                    return Err(bad(
                        self.source,
                        self.offset,
                        "invalid UTF-8 material string",
                    ));
                }
            };
            let encoded = self.take(encoded_length)?;
            let scalar = std::str::from_utf8(encoded)
                .ok()
                .and_then(|text| text.chars().next())
                .ok_or_else(|| {
                    bad(
                        self.source,
                        self.offset - encoded_length,
                        "invalid UTF-8 material string",
                    )
                })?;
            chars.push(scalar);
        }
        if let Some(nul) = chars.iter().rposition(|character| *character == '\0') {
            chars.remove(nul);
        }
        Ok(chars.into_iter().collect())
    }
    fn texture(&mut self, output: &mut Vec<TextureSlot>, field: &'static str) -> Result<()> {
        let value = self.string()?;
        if !value.is_empty() {
            output.push(TextureSlot { field, value });
        }
        Ok(())
    }
    fn base(&mut self, version: u32, fields: &mut Vec<MaterialField>) -> Result<()> {
        let tile_flags = self.u32()?;
        for (name, mask) in [("TileU", 2), ("TileV", 1)] {
            let decoded = tile_flags & mask != 0;
            fields.push(MaterialField {
                name,
                value: FieldValue::Bool {
                    decoded,
                    source_byte: u8::from(decoded),
                },
            });
        }
        for name in ["UOffset", "VOffset", "UScale", "VScale", "Alpha"] {
            self.f32(fields, name)?;
        }
        let blend_mode = self.u8()?;
        let blend_source = self.u32()?;
        let blend_destination = self.u32()?;
        let decoded_blend = match (blend_mode, blend_source, blend_destination) {
            (0, 6, 7) => "Unknown",
            (0, 0, 0) => "None",
            (1, 6, 7) => "Standard",
            (1, 6, 0) => "Additive",
            (1, 4, 1) => "Multiplicative",
            _ => {
                return Err(bad(
                    self.source,
                    self.offset - 9,
                    "unsupported alpha blend mode tuple",
                ));
            }
        };
        fields.push(MaterialField {
            name: "AlphaBlendMode",
            value: FieldValue::Enum {
                decoded: decoded_blend.to_owned(),
                source_words: [u32::from(blend_mode), blend_source, blend_destination],
            },
        });
        self.u8_field(fields, "AlphaTestRef")?;
        for name in [
            "AlphaTest",
            "ZBufferWrite",
            "ZBufferTest",
            "ScreenSpaceReflections",
            "WetnessControlScreenSpaceReflections",
            "Decal",
            "TwoSided",
            "DecalNoFade",
            "NonOccluder",
        ] {
            self.bool(fields, name)?;
        }
        self.bool(fields, "Refraction")?;
        self.bool(fields, "RefractionFalloff")?;
        self.f32(fields, "RefractionPower")?;
        if version < 10 {
            self.bool(fields, "EnvironmentMapping")?;
            self.f32(fields, "EnvironmentMappingMaskScale")?;
        } else {
            self.bool(fields, "DepthBias")?;
        }
        self.bool(fields, "GrayscaleToPaletteColor")?;
        if version >= 6 {
            self.u8_field(fields, "MaskWrites")?;
        }
        Ok(())
    }
    fn bgsm_v2(
        &mut self,
        textures: &mut Vec<TextureSlot>,
        fields: &mut Vec<MaterialField>,
    ) -> Result<()> {
        for field in [
            "DiffuseTexture",
            "NormalTexture",
            "SmoothSpecTexture",
            "GreyscaleTexture",
            "EnvmapTexture",
            "GlowTexture",
            "InnerLayerTexture",
            "WrinklesTexture",
            "DisplacementTexture",
        ] {
            self.texture(textures, field)?;
        }
        self.bool(fields, "EnableEditorAlphaRef")?;
        self.bool(fields, "RimLighting")?;
        self.f32(fields, "RimPower")?;
        self.f32(fields, "BackLightPower")?;
        self.bool(fields, "SubsurfaceLighting")?;
        self.f32(fields, "SubsurfaceLightingRolloff")?;
        self.bool(fields, "SpecularEnabled")?;
        self.color(fields, "SpecularColor")?;
        for name in [
            "SpecularMult",
            "Smoothness",
            "FresnelPower",
            "WetnessControlSpecScale",
            "WetnessControlSpecPowerScale",
            "WetnessControlSpecMinvar",
            "WetnessControlEnvMapScale",
            "WetnessControlFresnelPower",
            "WetnessControlMetalness",
        ] {
            self.f32(fields, name)?;
        }
        self.string_field(fields, "RootMaterialPath")?;
        self.bool(fields, "AnisoLighting")?;
        let emit_enabled = self.u8()?;
        if emit_enabled > 1 {
            self.noncanonical_boolean_bytes += 1;
        }
        fields.push(MaterialField {
            name: "EmitEnabled",
            value: FieldValue::Bool {
                decoded: emit_enabled != 0,
                source_byte: emit_enabled,
            },
        });
        if emit_enabled != 0 {
            self.color(fields, "EmittanceColor")?;
        }
        self.f32(fields, "EmittanceMult")?;
        self.bool(fields, "ModelSpaceNormals")?;
        self.bool(fields, "ExternalEmittance")?;
        for name in [
            "BackLighting",
            "ReceiveShadows",
            "HideSecret",
            "CastShadows",
            "DissolveFade",
            "AssumeShadowmask",
        ] {
            self.bool(fields, name)?;
        }
        self.bool(fields, "Glowmap")?;
        self.bool(fields, "EnvironmentMappingWindow")?;
        self.bool(fields, "EnvironmentMappingEye")?;
        self.bool(fields, "Hair")?;
        self.color(fields, "HairTintColor")?;
        for name in ["Tree", "Facegen", "SkinTint", "Tessellate"] {
            self.bool(fields, name)?;
        }
        for name in [
            "DisplacementTextureBias",
            "DisplacementTextureScale",
            "TessellationPnScale",
            "TessellationBaseFactor",
            "TessellationFadeDistance",
        ] {
            self.f32(fields, name)?;
        }
        self.f32(fields, "GrayscaleToPaletteScale")?;
        self.bool(fields, "SkewSpecularAlpha")?;
        Ok(())
    }
    fn bgem_v2(
        &mut self,
        textures: &mut Vec<TextureSlot>,
        fields: &mut Vec<MaterialField>,
    ) -> Result<()> {
        for field in [
            "BaseTexture",
            "GrayscaleTexture",
            "EnvmapTexture",
            "NormalTexture",
            "EnvmapMaskTexture",
        ] {
            self.texture(textures, field)?;
        }
        for name in [
            "BloodEnabled",
            "EffectLightingEnabled",
            "FalloffEnabled",
            "FalloffColorEnabled",
            "GrayscaleToPaletteAlpha",
            "SoftEnabled",
        ] {
            self.bool(fields, name)?;
        }
        self.color(fields, "BaseColor")?;
        for name in [
            "BaseColorScale",
            "FalloffStartAngle",
            "FalloffStopAngle",
            "FalloffStartOpacity",
            "FalloffStopOpacity",
        ] {
            self.f32(fields, name)?;
        }
        self.f32(fields, "LightingInfluence")?;
        self.u8_field(fields, "EnvmapMinLOD")?;
        self.f32(fields, "SoftDepth")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_bool(bytes: &mut Vec<u8>, value: u8) {
        bytes.push(value);
    }
    fn put_f32(bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(&0.0f32.to_le_bytes());
    }
    fn put_color(bytes: &mut Vec<u8>) {
        for _ in 0..3 {
            put_f32(bytes);
        }
    }
    fn put_string(bytes: &mut Vec<u8>, value: &str) {
        let value = format!("{value}\0");
        bytes.extend_from_slice(&(value.chars().count() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }
    fn base(bytes: &mut Vec<u8>, signature: &[u8; 4]) {
        bytes.extend_from_slice(signature);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        for _ in 0..5 {
            put_f32(bytes);
        }
        bytes.push(0);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&[0; 9]);
        bytes.extend_from_slice(&[0; 2]);
        put_f32(bytes);
        put_bool(bytes, 0);
        put_f32(bytes);
        put_bool(bytes, 0);
    }
    fn bgsm_v2() -> Vec<u8> {
        let mut bytes = Vec::new();
        base(&mut bytes, b"BGSM");
        put_string(&mut bytes, "Textures/Test/Diffuse.DDS");
        for _ in 0..8 {
            put_string(&mut bytes, "");
        }
        put_bool(&mut bytes, 0);
        put_bool(&mut bytes, 0);
        put_f32(&mut bytes);
        put_f32(&mut bytes);
        put_bool(&mut bytes, 0);
        put_f32(&mut bytes);
        put_bool(&mut bytes, 0);
        put_color(&mut bytes);
        for _ in 0..9 {
            put_f32(&mut bytes);
        }
        put_string(&mut bytes, "");
        put_bool(&mut bytes, 0);
        put_bool(&mut bytes, 0);
        put_f32(&mut bytes);
        put_bool(&mut bytes, 0);
        put_bool(&mut bytes, 0);
        put_bool(&mut bytes, 0);
        for _ in 0..5 {
            put_bool(&mut bytes, 0);
        }
        put_bool(&mut bytes, 0);
        put_bool(&mut bytes, 0);
        put_bool(&mut bytes, 0);
        put_bool(&mut bytes, 0);
        put_color(&mut bytes);
        for _ in 0..4 {
            put_bool(&mut bytes, 0);
        }
        for _ in 0..5 {
            put_f32(&mut bytes);
        }
        put_f32(&mut bytes);
        put_bool(&mut bytes, 0);
        bytes
    }
    fn bgem_v2() -> Vec<u8> {
        let mut bytes = Vec::new();
        base(&mut bytes, b"BGEM");
        put_string(&mut bytes, "Effects/Test_D.dds");
        for _ in 0..4 {
            put_string(&mut bytes, "");
        }
        for _ in 0..6 {
            put_bool(&mut bytes, 0);
        }
        put_color(&mut bytes);
        for _ in 0..6 {
            put_f32(&mut bytes);
        }
        bytes.push(0); // environment-map minimum LOD
        put_f32(&mut bytes);
        bytes
    }

    #[test]
    fn decodes_version_two_texture_fields_and_consumes_full_stream() {
        let bytes = bgsm_v2();
        let material = parse(&bytes, Kind::Bgsm, "synthetic.bgsm").unwrap();
        assert_eq!(material.version, Some(2));
        assert_eq!(material.bytes_consumed, bytes.len());
        assert_eq!(material.textures.len(), 1);
        assert_eq!(material.textures[0].field, "DiffuseTexture");
        assert_eq!(material.textures[0].value, "Textures/Test/Diffuse.DDS");
    }

    #[test]
    fn keeps_named_float_color_and_noncanonical_boolean_source_bits() {
        let mut bytes = bgsm_v2();
        let u_offset_bits = 0x3f00_0001_u32;
        bytes[12..16].copy_from_slice(&u_offset_bits.to_le_bytes());
        bytes[42] = 0x80;

        let mut cursor = 63;
        for _ in 0..9 {
            let characters =
                u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4 + characters;
        }
        cursor += 1 + 1 + 4 + 4 + 1 + 4 + 1;
        let color_bits = [0x3e80_0000_u32, 0x3f00_0000, 0x3f40_0000];
        for (channel, bits) in color_bits.iter().enumerate() {
            let start = cursor + channel * 4;
            bytes[start..start + 4].copy_from_slice(&bits.to_le_bytes());
        }

        let material = parse(&bytes, Kind::Bgsm, "field-bits.bgsm").unwrap();
        assert_eq!(material.noncanonical_boolean_bytes, 1);
        assert!(material.fields.iter().any(|field| {
            field.name == "UOffset" && field.value == FieldValue::F32Bits(u_offset_bits)
        }));
        assert!(material.fields.iter().any(|field| {
            field.name == "AlphaTest"
                && field.value
                    == FieldValue::Bool {
                        decoded: true,
                        source_byte: 0x80,
                    }
        }));
        assert!(material.fields.iter().any(|field| {
            field.name == "SpecularColor"
                && field.value
                    == FieldValue::ColorRgb {
                        packed_rgb: 0x003f_7fbf,
                        source_component_bits: color_bits,
                    }
        }));
    }

    #[test]
    fn unknown_alpha_blend_tuple_is_explicitly_unsupported() {
        let mut bytes = bgsm_v2();
        bytes[32] = 2;
        assert!(parse(&bytes, Kind::Bgsm, "unknown-blend.bgsm").is_err());
    }

    #[test]
    fn every_truncated_prefix_unknown_version_bad_signature_and_trailing_data_fail() {
        let bytes = bgsm_v2();
        for end in 0..bytes.len() {
            assert!(
                parse(&bytes[..end], Kind::Bgsm, "truncated.bgsm").is_err(),
                "prefix {end}"
            );
        }
        let mut unknown = bytes.clone();
        unknown[4..8].copy_from_slice(&21u32.to_le_bytes());
        assert!(parse(&unknown, Kind::Bgsm, "future.bgsm").is_err());
        let mut bad_signature = bytes.clone();
        bad_signature[..4].copy_from_slice(b"NOPE");
        assert!(parse(&bad_signature, Kind::Bgsm, "bad.bgsm").is_err());
        let mut trailing = bytes;
        trailing.push(0);
        assert!(parse(&trailing, Kind::Bgsm, "trailing.bgsm").is_err());

        let mut oversized = bgsm_v2();
        oversized[63..67].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse(&oversized, Kind::Bgsm, "oversized-string.bgsm").is_err());
    }

    #[test]
    fn effect_v2_and_noncanonical_boolean_observation_are_explicit() {
        let effect = bgem_v2();
        let decoded = parse(&effect, Kind::Bgem, "effect.bgem").unwrap();
        assert_eq!(decoded.version, Some(2));
        assert_eq!(decoded.bytes_consumed, effect.len());
        assert_eq!(decoded.textures[0].field, "BaseTexture");
        assert_eq!(decoded.textures[0].value, "Effects/Test_D.dds");

        let mut material = bgsm_v2();
        material[42] = 0x80;
        let decoded = parse(&material, Kind::Bgsm, "noncanonical-bool.bgsm").unwrap();
        assert_eq!(decoded.noncanonical_boolean_bytes, 1);
    }

    #[test]
    fn json_texture_fields_keep_the_distinct_versionless_format() {
        let material = parse(
            br#"{"sBaseTexture":"Effects\\ColorBlack.dds","sGlowTexture":null}"#,
            Kind::Bgem,
            "startup.bgem",
        )
        .unwrap();
        assert_eq!(material.format, Format::Json);
        assert_eq!(material.version, None);
        assert_eq!(material.textures[0].value, r"Effects\ColorBlack.dds");
        assert!(parse(br#"{"sBaseTexture":4}"#, Kind::Bgem, "bad-json.bgem").is_err());
    }
}
