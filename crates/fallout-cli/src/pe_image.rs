//! A bounded view of initialized bytes in a PE32 image. This is an offline data
//! reader, not a loader: it never maps executable pages or calls a source address.
//! Field evidence: Microsoft's PE specification, linked in docs/command-catalogue.md.
use super::Result;
use std::ops::Range;

#[derive(Debug)]
struct Section {
    rva: u32,
    virtual_bytes: u32,
    raw: Range<usize>,
}

#[derive(Debug)]
pub(super) struct Image<'a> {
    bytes: &'a [u8],
    pub image_base: u32,
    pub timestamp: u32,
    header_bytes: usize,
    sections: Vec<Section>,
}

fn span(bytes: &[u8], offset: usize, length: usize) -> Result<&[u8]> {
    let end = offset.checked_add(length).ok_or("PE extent overflow")?;
    bytes
        .get(offset..end)
        .ok_or_else(|| format!("PE field at 0x{offset:X} exceeds input").into())
}

pub(super) fn u16_at(bytes: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(span(bytes, offset, 2)?.try_into()?))
}

pub(super) fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(span(bytes, offset, 4)?.try_into()?))
}

impl<'a> Image<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() > 64 * 1024 * 1024 || span(bytes, 0, 2)? != b"MZ" {
            return Err("PE32 input magic or byte budget".into());
        }
        let pe = u32_at(bytes, 0x3c)? as usize;
        if pe < 64 || span(bytes, pe, 24)?[..4] != *b"PE\0\0" {
            return Err("PE signature or DOS-header overlap".into());
        }
        if u16_at(bytes, pe + 4)? != 0x14c {
            return Err("Offline NV table reader requires an i386 image".into());
        }
        let count = usize::from(u16_at(bytes, pe + 6)?);
        if count == 0 || count > 96 {
            return Err("PE section count outside 1..=96".into());
        }
        let optional_bytes = usize::from(u16_at(bytes, pe + 20)?);
        let optional = span(bytes, pe + 24, optional_bytes)?;
        if optional.len() < 96 || u16_at(optional, 0)? != 0x10b {
            return Err("Offline NV table reader requires a complete PE32 optional header".into());
        }
        let image_base = u32_at(optional, 28)?;
        let image_bytes = u32_at(optional, 56)?;
        let header_bytes = u32_at(optional, 60)? as usize;
        let table = pe + 24 + optional_bytes;
        let headers_end = table + count * 40;
        if image_base % 65536 != 0
            || header_bytes < headers_end
            || header_bytes > bytes.len()
            || header_bytes > image_bytes as usize
            || image_base.checked_add(image_bytes).is_none()
        {
            return Err("PE image/header extent or base is invalid".into());
        }
        let mut sections = Vec::with_capacity(count);
        for entry in span(bytes, table, count * 40)?.as_chunks::<40>().0 {
            let virtual_bytes = u32_at(entry, 8)?;
            let rva = u32_at(entry, 12)?;
            let raw_bytes = u32_at(entry, 16)? as usize;
            let raw_start = u32_at(entry, 20)? as usize;
            let raw_end = raw_start
                .checked_add(raw_bytes)
                .ok_or("PE raw extent overflow")?;
            let loaded_bytes = virtual_bytes.max(raw_bytes as u32);
            let virtual_end = rva
                .checked_add(loaded_bytes)
                .ok_or("PE virtual extent overflow")?;
            if rva < header_bytes as u32
                || virtual_end > image_bytes
                || raw_end > bytes.len()
                || (raw_bytes != 0 && raw_start < header_bytes)
            {
                return Err("PE section overlaps headers or exceeds source/image".into());
            }
            sections.push(Section {
                rva,
                virtual_bytes: loaded_bytes,
                raw: raw_start..raw_end,
            });
        }
        // Reject ambiguous lookup even when a file happens to put the desired
        // descriptor in the first overlapping section. Never guess a winner.
        sections.sort_by_key(|section| section.rva);
        for pair in sections.windows(2) {
            if pair[0].rva + pair[0].virtual_bytes > pair[1].rva {
                return Err("Overlapping PE virtual sections".into());
            }
        }
        let mut raw: Vec<_> = sections
            .iter()
            .filter(|s| !s.raw.is_empty())
            .map(|s| s.raw.clone())
            .collect();
        raw.sort_by_key(|range| range.start);
        if raw.windows(2).any(|pair| pair[0].end > pair[1].start) {
            return Err("Overlapping PE raw sections".into());
        }
        Ok(Self {
            bytes,
            image_base,
            timestamp: u32_at(bytes, pe + 8)?,
            header_bytes,
            sections,
        })
    }

    fn initialized_tail(&self, address: u32) -> Result<(usize, usize)> {
        let rva = address
            .checked_sub(self.image_base)
            .ok_or("PE address precedes image base")?;
        if (rva as usize) < self.header_bytes {
            return Ok((rva as usize, self.header_bytes));
        }
        let next = self.sections.partition_point(|section| section.rva <= rva);
        let section = self
            .sections
            .get(next.checked_sub(1).ok_or("PE address has no section")?)
            .ok_or("PE address has no section")?;
        let delta = (rva - section.rva) as usize;
        if delta >= section.raw.len() {
            return Err("PE address is uninitialized or outside a section".into());
        }
        Ok((section.raw.start + delta, section.raw.end))
    }

    pub fn read(&self, address: u32, length: usize) -> Result<(usize, &'a [u8])> {
        let (offset, boundary) = self.initialized_tail(address)?;
        if length > boundary - offset {
            return Err("PE read crosses initialized section boundary".into());
        }
        Ok((offset, span(self.bytes, offset, length)?))
    }

    /// Keep original string bytes; descriptor names are ASCII in the tested
    /// executable, but a general PE image does not imply any string encoding.
    pub fn c_string(&self, address: u32, maximum: usize) -> Result<(usize, &'a [u8])> {
        let (offset, boundary) = self.initialized_tail(address)?;
        let length =
            (boundary - offset).min(maximum.checked_add(1).ok_or("PE string budget overflow")?);
        let bytes = span(self.bytes, offset, length)?;
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .ok_or("PE string is unterminated or exceeds budget")?;
        Ok((offset, &bytes[..end]))
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    fn put16(bytes: &mut [u8], at: usize, value: u16) {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn fixture() -> Vec<u8> {
        let mut bytes = vec![0; 0x280];
        bytes[..2].copy_from_slice(b"MZ");
        put32(&mut bytes, 0x3c, 0x80);
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        put16(&mut bytes, 0x84, 0x14c);
        put16(&mut bytes, 0x86, 1);
        put16(&mut bytes, 0x94, 96);
        put16(&mut bytes, 0x98, 0x10b);
        put32(&mut bytes, 0xb4, 0x400000);
        put32(&mut bytes, 0xd0, 0x2000);
        put32(&mut bytes, 0xd4, 0x200);
        put32(&mut bytes, 0x100, 0x100);
        put32(&mut bytes, 0x104, 0x1000);
        put32(&mut bytes, 0x108, 0x80);
        put32(&mut bytes, 0x10c, 0x200);
        bytes[0x210..0x216].copy_from_slice(b"hello\0");
        bytes
    }

    #[test]
    fn pe_reads_map_initialized_rvas_and_reject_zero_fill_or_crossing() {
        let bytes = fixture();
        let image = Image::parse(&bytes).unwrap();
        assert_eq!(image.read(0x401010, 5).unwrap(), (0x210, &b"hello"[..]));
        assert_eq!(image.c_string(0x401010, 5).unwrap(), (0x210, &b"hello"[..]));
        assert!(image.c_string(0x401010, 4).is_err());
        assert!(image.read(0x401080, 1).is_err());
        assert!(image.read(0x40107f, 2).is_err());
        assert!(image.read(0x3fffff, 1).is_err());
        assert!(image.read(0x400300, 1).is_err());
        assert!(image.read(0xffffffff, 1).is_err());
        assert_eq!(image.read(0x400000, 2).unwrap().1, b"MZ");
    }

    #[test]
    fn pe_rejects_truncation_overlaps_and_wrong_variants_without_panicking() {
        let bytes = fixture();
        for end in 0..bytes.len() {
            assert!(Image::parse(&bytes[..end]).is_err());
        }
        for (at, value) in [
            (0x3c, u32::MAX),
            (0xd4, 1),
            (0x104, 1),
            (0x108, u32::MAX),
            (0x10c, 1),
        ] {
            let mut bad = bytes.clone();
            put32(&mut bad, at, value);
            assert!(Image::parse(&bad).is_err());
        }
        for (at, value) in [(0x84, 0x8664), (0x86, 97), (0x94, 95), (0x98, 0x20b)] {
            let mut bad = bytes.clone();
            put16(&mut bad, at, value);
            assert!(Image::parse(&bad).is_err());
        }
        let mut overlap = bytes.clone();
        put16(&mut overlap, 0x86, 2);
        overlap.copy_within(0xf8..0x120, 0x120);
        assert!(Image::parse(&overlap).is_err());
    }

    #[test]
    fn pe_mutation_sweep_never_panics_or_reads_outside_input() {
        let original = fixture();
        for at in 0..original.len() {
            for value in [0, 0xff, 0x80] {
                let mut bytes = original.clone();
                bytes[at] = value;
                if let Ok(image) = Image::parse(&bytes) {
                    for address in [0, 0x400000, 0x401000, 0x40107f, 0x401080, 0xffffffff] {
                        if let Ok((offset, data)) = image.read(address, 1) {
                            assert_eq!(data, &bytes[offset..offset + 1]);
                        }
                    }
                }
            }
        }
    }
}
