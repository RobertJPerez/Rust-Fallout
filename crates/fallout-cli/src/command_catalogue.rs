//! Read descriptor metadata from the exact, fingerprinted NV executable. Source
//! addresses identify bytes on disk only; no code in the original image runs.
use super::{
    Result,
    pe_image::{Image, u16_at, u32_at},
};
use fallout_data::baseline;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

const EXECUTABLE_SHA256: &str = "3a87f92f011e5dc9179ddf733cf08be2b39ea6e5b7a8a9e3a9a72dafcc1b104d";
const DESCRIPTOR_BYTES: usize = 40;

#[derive(Debug, Serialize)]
pub(super) struct Parameter {
    pub descriptor_file_offset: usize,
    pub type_name: String,
    pub type_id: u32,
    /// Preserve the word; xNVSE explicitly leaves other possible bits uncertain.
    pub optional_word: u32,
}

#[derive(Debug, Serialize)]
pub(super) struct Descriptor {
    pub descriptor_file_offset: usize,
    pub table_index: usize,
    /// In xNVSE, script IDs are assigned from 0x1000 by table position.
    pub id: u32,
    pub stored_opcode: u32,
    pub name: String,
    pub short_name: Option<String>,
    pub needs_parent_word: u16,
    pub parameters: Vec<Parameter>,
    pub execute_handler_present: bool,
    pub parse_handler_present: bool,
    pub condition_handler_present: bool,
    pub flags: u32,
    pub implementation_status: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct OperatorDescriptor {
    pub descriptor_file_offset: usize,
    pub table_index: usize,
    pub code: u32,
    pub precedence: u8,
    pub spelling: String,
    pub raw_spelling_bytes: [u8; 3],
}

#[derive(Debug, Serialize)]
pub(super) struct Catalogue {
    pub schema_version: u32,
    pub source_bytes: usize,
    pub source_sha256: String,
    pub source_version_profile: &'static str,
    pub source_reference: &'static str,
    pub image_base: u32,
    pub pe_timestamp: u32,
    pub script_commands: Vec<Descriptor>,
    pub event_blocks: Vec<Descriptor>,
    pub statements: Vec<Descriptor>,
    pub operators: Vec<OperatorDescriptor>,
    pub execution_ready: bool,
    pub retail_parity_accepted: bool,
    pub unknown: Vec<&'static str>,
}

fn name(image: &Image<'_>, address: u32) -> Result<String> {
    let (_, bytes) = image.c_string(address, 128)?;
    if !bytes
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return Err("Command descriptor name contains unsupported bytes".into());
    }
    Ok(std::str::from_utf8(bytes)?.to_owned())
}

fn descriptors(
    image: &Image<'_>,
    start: u32,
    count: usize,
    first_id: u32,
) -> Result<Vec<Descriptor>> {
    if count > 1024 {
        return Err("Command descriptor count budget".into());
    }
    let (file_offset, bytes) = image.read(start, count * DESCRIPTOR_BYTES)?;
    let mut result = Vec::with_capacity(count);
    for (index, bytes) in bytes.as_chunks::<40>().0.iter().enumerate() {
        let long_name = name(image, u32_at(bytes, 0)?)?;
        let short_pointer = u32_at(bytes, 4)?;
        let parameter_count = usize::from(u16_at(bytes, 18)?);
        if parameter_count > 64 {
            return Err("Command parameter count budget".into());
        }
        let mut parameters = Vec::with_capacity(parameter_count);
        if parameter_count > 0 {
            let (parameter_offset, data) = image.read(u32_at(bytes, 20)?, parameter_count * 12)?;
            for (index, bytes) in data.as_chunks::<12>().0.iter().enumerate() {
                parameters.push(Parameter {
                    descriptor_file_offset: parameter_offset + index * 12,
                    type_name: name(image, u32_at(bytes, 0)?)?,
                    type_id: u32_at(bytes, 4)?,
                    optional_word: u32_at(bytes, 8)?,
                });
            }
        }
        result.push(Descriptor {
            descriptor_file_offset: file_offset + index * DESCRIPTOR_BYTES,
            table_index: index,
            id: first_id + index as u32,
            stored_opcode: u32_at(bytes, 8)?,
            name: long_name,
            short_name: if short_pointer == 0 {
                None
            } else {
                Some(self::name(image, short_pointer)?)
            },
            needs_parent_word: u16_at(bytes, 16)?,
            parameters,
            execute_handler_present: u32_at(bytes, 24)? != 0,
            parse_handler_present: u32_at(bytes, 28)? != 0,
            condition_handler_present: u32_at(bytes, 32)? != 0,
            flags: u32_at(bytes, 36)?,
            implementation_status: "metadata-decoded; behavior unimplemented",
        });
    }
    Ok(result)
}

fn operators(image: &Image<'_>, start: u32, count: usize) -> Result<Vec<OperatorDescriptor>> {
    if count > 64 {
        return Err("Operator descriptor count budget".into());
    }
    let (offset, bytes) = image.read(start, count * 8)?;
    let mut result = Vec::with_capacity(count);
    for (index, bytes) in bytes.as_chunks::<8>().0.iter().enumerate() {
        let raw_spelling_bytes: [u8; 3] = bytes[5..8].try_into()?;
        let end = raw_spelling_bytes
            .iter()
            .position(|byte| *byte == 0)
            .ok_or("Unterminated operator spelling")?;
        if end == 0
            || !raw_spelling_bytes[..end]
                .iter()
                .all(u8::is_ascii_punctuation)
        {
            return Err("Unsupported operator spelling".into());
        }
        result.push(OperatorDescriptor {
            descriptor_file_offset: offset + index * 8,
            table_index: index,
            code: u32_at(bytes, 0)?,
            precedence: bytes[4],
            spelling: std::str::from_utf8(&raw_spelling_bytes[..end])?.to_owned(),
            raw_spelling_bytes,
        });
    }
    Ok(result)
}

pub(super) fn inspect(path: &Path) -> Result<Catalogue> {
    let file = baseline::open_source(path)?;
    let size = file.metadata()?.len();
    if size > 64 * 1024 * 1024 {
        return Err("Executable source byte budget".into());
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    decode(&bytes)
}

fn decode(bytes: &[u8]) -> Result<Catalogue> {
    let digest = format!("{:x}", Sha256::digest(bytes));
    if digest != EXECUTABLE_SHA256 {
        return Err(format!("Unsupported executable fingerprint {digest}; descriptor locations are verified only for the pinned NV source").into());
    }
    let image = Image::parse(bytes)?;
    // Pinned xNVSE CommandTable.cpp (runtime 1.4.0.525) and GameScript.cpp.
    // These are preferred-image addresses converted through PE sections, never
    // process pointers. A different digest cannot enter this layout path.
    let script_commands = descriptors(&image, 0x01190910, 640, 0x1000)?;
    let event_blocks = descriptors(&image, 0x0118e2f0, 38, 0)?;
    let statements = descriptors(&image, 0x0118cb50, 16, 0x10)?;
    let operators = operators(&image, 0x0118cad0, 16)?;
    Ok(Catalogue {
        schema_version: 1,
        source_bytes: bytes.len(),
        source_sha256: digest,
        source_version_profile: "authorized local NV 1.4.0.525; exact executable digest",
        source_reference: "xNVSE 0ccd23ad885ddae533c1790a3fc56cd073e38de3 CommandTable.h/.cpp and GameScript.cpp",
        image_base: image.image_base,
        pe_timestamp: image.timestamp,
        script_commands,
        event_blocks,
        statements,
        operators,
        execution_ready: false,
        retail_parity_accepted: false,
        unknown: vec![
            "native return types, mutations, errors and caller behavior",
            "condition ID binding and subject semantics",
            "event timing and scheduling",
            "extension commands and other executable variants",
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_spelling_is_bounded_and_unused_bytes_remain_authored() {
        let mut bytes = crate::pe_image::tests::fixture();
        bytes[0x220..0x228].copy_from_slice(&[15, 0, 0, 0, 6, b'~', 0, 0x7b]);
        let image = Image::parse(&bytes).unwrap();
        let rows = operators(&image, 0x401020, 1).unwrap();
        assert_eq!(rows[0].code, 15);
        assert_eq!(rows[0].precedence, 6);
        assert_eq!(rows[0].spelling, "~");
        assert_eq!(rows[0].raw_spelling_bytes, [b'~', 0, 0x7b]);
        bytes[0x226..0x228].copy_from_slice(b"++");
        assert!(operators(&Image::parse(&bytes).unwrap(), 0x401020, 1).is_err());
        bytes[0x225..0x228].copy_from_slice(&[b'A', 0, 0]);
        assert!(operators(&Image::parse(&bytes).unwrap(), 0x401020, 1).is_err());
    }

    fn descriptor_fixture() -> Vec<u8> {
        let mut bytes = crate::pe_image::tests::fixture();
        let mut put32 =
            |at: usize, value: u32| bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        put32(0x220, 0x401010); // Original fixture's "hello" string.
        put32(0x228, 0x1000);
        put32(0x230, 0x00010002); // One parameter; preserve the needs-parent word 2.
        put32(0x234, 0x401050);
        put32(0x238, 1); // Presence only; never resolved/called as executable code.
        put32(0x23c, 1);
        put32(0x244, 0xa5a5);
        put32(0x250, 0x401010);
        put32(0x254, 4);
        put32(0x258, 2); // Optional word is retained, not silently reduced to bool.
        bytes
    }

    #[test]
    fn catalogue_reads_original_descriptor_fields_and_preserves_uncertain_words() {
        let bytes = descriptor_fixture();
        let image = Image::parse(&bytes).unwrap();
        let rows = descriptors(&image, 0x401020, 1, 0x1000).unwrap();
        let row = &rows[0];
        assert_eq!(row.descriptor_file_offset, 0x220);
        assert_eq!(row.id, 0x1000);
        assert_eq!(row.stored_opcode, 0x1000);
        assert_eq!(row.name, "hello");
        assert_eq!(row.short_name, None);
        assert_eq!(row.needs_parent_word, 2);
        assert_eq!(row.parameters[0].descriptor_file_offset, 0x250);
        assert_eq!(row.parameters[0].type_name, "hello");
        assert_eq!(row.parameters[0].type_id, 4);
        assert_eq!(row.parameters[0].optional_word, 2);
        assert!(row.execute_handler_present);
        assert!(row.parse_handler_present);
        assert!(!row.condition_handler_present);
        assert_eq!(row.flags, 0xa5a5);
        assert!(row.implementation_status.contains("unimplemented"));
    }

    #[test]
    fn catalogue_rejects_bad_name_parameter_pointers_and_unknown_fingerprints() {
        let original = descriptor_fixture();
        for (at, value) in [
            (0x220, 0u32),
            (0x234, 0xffffffff),
            (0x230, 0xffff0000),
            (0x250, 0x401080),
        ] {
            let mut bytes = original.clone();
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            let image = Image::parse(&bytes).unwrap();
            assert!(descriptors(&image, 0x401020, 1, 0x1000).is_err());
        }
        let error = decode(&original).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Unsupported executable fingerprint")
        );
    }
}
