use crate::{Result, malformed, nif::NifIndex};

/// Each reader is confined to one block, but errors point into the original file.
pub(crate) struct Reader<'a> {
    pub data: &'a [u8],
    pub base: usize,
    pub position: usize,
    pub source: &'a str,
    pub index: &'a NifIndex,
    pub array_bytes_left: &'a mut usize,
}

impl Reader<'_> {
    pub fn fail(&self, reason: impl Into<String>) -> crate::Error {
        malformed(self.source, (self.base + self.position) as u64, reason)
    }

    pub fn take(&mut self, count: usize) -> Result<&[u8]> {
        if count > self.data.len() - self.position {
            return Err(self.fail("NIF field exceeds its block"));
        }
        let start = self.position;
        self.position += count;
        Ok(&self.data[start..self.position])
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }
    pub fn boolean(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(self.fail("noncanonical NIF boolean")),
        }
    }
    pub fn float(&mut self) -> Result<f32> {
        let value = f32::from_bits(self.u32()?);
        if !value.is_finite() {
            return Err(self.fail("nonfinite NIF float"));
        }
        Ok(value)
    }
    pub fn vector<const N: usize>(&mut self) -> Result<[f32; N]> {
        let mut value = [0.; N];
        for item in &mut value {
            *item = self.float()?;
        }
        Ok(value)
    }
    pub fn budget(&self, count: usize, width: usize) -> Result<()> {
        if count > 2_000_000 || count > (self.data.len() - self.position) / width {
            return Err(self.fail("NIF array exceeds block or element budget"));
        }
        Ok(())
    }
    pub fn vectors<const N: usize>(&mut self, count: usize) -> Result<Vec<[f32; N]>> {
        self.budget(count, 4 * N)?;
        self.reserve::<[f32; N]>(count)?;
        (0..count).map(|_| self.vector()).collect()
    }
    pub fn indices(&mut self, count: usize, vertices: u16) -> Result<Vec<u16>> {
        self.budget(count, 2)?;
        self.reserve::<u16>(count)?;
        (0..count)
            .map(|_| {
                let index = self.u16()?;
                if index >= vertices {
                    return Err(self.fail("vertex index out of range"));
                }
                Ok(index)
            })
            .collect()
    }
    fn link(&mut self, count: usize, kind: &str) -> Result<Option<u32>> {
        let value = self.u32()?;
        if value == u32::MAX {
            return Ok(None);
        }
        if value as usize >= count {
            return Err(self.fail(format!("{kind} index out of range")));
        }
        Ok(Some(value))
    }
    pub fn reference(&mut self) -> Result<Option<u32>> {
        self.link(self.index.blocks.len(), "block")
    }
    pub fn string(&mut self) -> Result<Option<u32>> {
        self.link(self.index.strings.len(), "string")
    }
    pub fn references(&mut self) -> Result<Vec<Option<u32>>> {
        let count = self.u32()? as usize;
        self.budget(count, 4)?;
        self.reserve::<Option<u32>>(count)?;
        (0..count).map(|_| self.reference()).collect()
    }
    pub fn byte_string(&mut self) -> Result<Vec<u8>> {
        let count = self.u32()? as usize;
        self.budget(count, 1)?;
        self.reserve::<u8>(count)?;
        Ok(self.take(count)?.to_vec())
    }
    pub fn finish(&self) -> Result<()> {
        if self.position != self.data.len() {
            return Err(self.fail("unconsumed bytes in supported NIF block"));
        }
        Ok(())
    }

    pub fn reserve<T>(&mut self, count: usize) -> Result<()> {
        let bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .and_then(|bytes| self.array_bytes_left.checked_sub(bytes))
            .ok_or_else(|| self.fail("NIF payload array storage budget exceeded"))?;
        *self.array_bytes_left = bytes;
        Ok(())
    }
}
