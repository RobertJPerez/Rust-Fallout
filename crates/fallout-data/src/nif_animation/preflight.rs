//! Allocation-free bounds for the existing immutable container reader. This is
//! metadata preflight, not a second accepted container/schema implementation.
use super::Limits;
use crate::{Error, Result, malformed, nif};

const HEADER: &[u8] = b"Gamebryo File Format, Version 20.2.0.7\n";

struct Scan<'a> {
    bytes: &'a [u8],
    source: &'a str,
    position: usize,
}
impl Scan<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8]> {
        if count > self.bytes.len() - self.position {
            return Err(malformed(
                self.source,
                self.position as u64,
                "animation container preflight exceeds input",
            ));
        }
        let start = self.position;
        self.position += count;
        Ok(&self.bytes[start..self.position])
    }
    fn u32(&mut self) -> Result<usize> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("four bytes")) as usize)
    }
    fn count(&self, count: usize, width: usize, maximum: usize) -> Result<()> {
        if count > maximum || count > (self.bytes.len() - self.position) / width {
            return Err(malformed(
                self.source,
                self.position as u64,
                "animation container table exceeds input/count budget",
            ));
        }
        Ok(())
    }
}

fn add(total: &mut usize, count: usize, width: usize, source: &str) -> Result<()> {
    *total = count
        .checked_mul(width)
        .and_then(|n| total.checked_add(n))
        .ok_or_else(|| {
            Error::Unsupported(format!(
                "{source}: animation container storage charge overflow"
            ))
        })?;
    Ok(())
}

/// Charges predictable returned index storage and concurrently live temporary
/// table vectors before `nif::inspect` allocates. Existing map allocator overhead
/// stays count-bounded; these charges are not a process-memory measurement.
pub(super) fn storage(bytes: &[u8], source: &str, limits: Limits) -> Result<usize> {
    if bytes.len() > limits.input_bytes {
        return Err(Error::Unsupported(format!(
            "{source}: animation input byte budget exceeded"
        )));
    }
    // Preserve the existing adapter's unsupported-header/tuple diagnostics.
    if !bytes.starts_with(HEADER) {
        return nif::inspect(bytes, source).map(|_| 0);
    }
    let mut scan = Scan {
        bytes,
        source,
        position: HEADER.len(),
    };
    let version = scan.u32()?;
    let endian = scan.take(1)?[0];
    let user = scan.u32()?;
    let blocks = scan.u32()?;
    let stream = scan.u32()?;
    if (version, endian, user) != (0x1402_0007, 1, 11)
        || ![14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34].contains(&stream)
    {
        return nif::inspect(bytes, source).map(|_| 0);
    }
    scan.count(blocks, 6, limits.blocks.min(1_000_000))?;
    let mut retained = 0;
    add(
        &mut retained,
        blocks,
        std::mem::size_of::<nif::Block>(),
        source,
    )?;
    add(&mut retained, 3, std::mem::size_of::<Vec<u8>>(), source)?;
    for _ in 0..3 {
        let length = scan.take(1)?[0] as usize;
        scan.take(length)?;
        add(&mut retained, length, 1, source)?;
    }
    let types = u16::from_le_bytes(scan.take(2)?.try_into().expect("two bytes")) as usize;
    scan.count(types, 4, 16_384)?;
    add(&mut retained, types, std::mem::size_of::<String>(), source)?;
    // At most one retained block-count key per declared type. Charge its logical
    // record/string even when an unused type will not enter the existing map.
    add(
        &mut retained,
        types,
        std::mem::size_of::<(String, usize)>(),
        source,
    )?;
    let mut maximum_type_bytes = 0;
    for _ in 0..types {
        let length = scan.u32()?;
        if length == 0 || length > 1024 {
            return Err(malformed(
                source,
                scan.position as u64,
                "animation block type name exceeds budget",
            ));
        }
        scan.take(length)?;
        add(&mut retained, length, 2, source)?;
        maximum_type_bytes = maximum_type_bytes.max(length);
    }
    scan.count(blocks, 6, limits.blocks.min(1_000_000))?;
    scan.take(blocks * 2)?;
    let mut payload_bytes = 0;
    for _ in 0..blocks {
        add(&mut payload_bytes, scan.u32()?, 1, source)?;
    }
    let strings = scan.u32()?;
    let maximum_string_bytes = scan.u32()?;
    scan.count(strings, 4, 1_000_000)?;
    add(
        &mut retained,
        strings,
        std::mem::size_of::<Vec<u8>>(),
        source,
    )?;
    let mut string_bytes = 0;
    for _ in 0..strings {
        let length = scan.u32()?;
        if length > maximum_string_bytes || length > 16 * 1024 * 1024 {
            return Err(malformed(
                source,
                scan.position as u64,
                "animation string exceeds source budget",
            ));
        }
        add(&mut string_bytes, length, 1, source)?;
        if string_bytes > 16 * 1024 * 1024 {
            return Err(malformed(
                source,
                scan.position as u64,
                "animation string table exceeds source budget",
            ));
        }
        scan.take(length)?;
    }
    add(&mut retained, string_bytes, 1, source)?;
    let groups = scan.u32()?;
    scan.count(groups, 4, 1_000_000)?;
    add(&mut retained, groups, std::mem::size_of::<u32>(), source)?;
    scan.take(groups * 4)?;
    scan.take(payload_bytes)?;
    let roots = scan.u32()?;
    scan.count(roots, 4, 1_000_000)?;
    add(
        &mut retained,
        roots,
        std::mem::size_of::<Option<u32>>(),
        source,
    )?;
    scan.take(roots * 4)?;
    if scan.position != bytes.len() {
        return Err(malformed(
            source,
            scan.position as u64,
            "unconsumed animation container footer bytes",
        ));
    }
    let mut temporary = maximum_type_bytes;
    add(
        &mut temporary,
        blocks,
        std::mem::size_of::<u16>() + std::mem::size_of::<usize>(),
        source,
    )?;
    if retained
        .checked_add(temporary)
        .is_none_or(|n| n > limits.array_bytes)
    {
        return Err(Error::Unsupported(format!(
            "{source}: animation container storage budget exceeded before allocation"
        )));
    }
    Ok(retained)
}
