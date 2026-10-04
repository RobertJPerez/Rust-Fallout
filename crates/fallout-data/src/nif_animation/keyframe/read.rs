use super::{Group, Key, QuaternionKey, Rotation, TransformKeys};
use crate::{Error, Result, nif_scene::cursor::Reader};

pub(super) fn charge(work: &mut usize, source: &str) -> Result<()> {
    charge_many(work, 1, source)
}
pub(super) fn charge_many(work: &mut usize, count: usize, source: &str) -> Result<()> {
    *work = work.checked_sub(count).ok_or_else(|| {
        Error::Unsupported(format!("{source}: transform-key work budget exceeded"))
    })?;
    Ok(())
}
struct Decode<'a, 'b> {
    reader: Reader<'a>,
    work: &'b mut usize,
}
impl Decode<'_, '_> {
    fn bits(&mut self) -> Result<u32> {
        Ok(self.reader.float()?.to_bits())
    }
    fn vector<const N: usize>(&mut self) -> Result<[u32; N]> {
        let mut result = [0; N];
        for value in &mut result {
            *value = self.bits()?;
        }
        Ok(result)
    }
    fn group<const N: usize>(&mut self) -> Result<Group<N>> {
        charge(self.work, self.reader.source)?; // Includes an empty product.
        let declared_keys = self.reader.u32()?;
        if declared_keys == 0 {
            return Ok(Group {
                declared_keys,
                key_type: None,
                keys: Vec::new(),
            });
        }
        let tag = self.reader.u32()?;
        if ![1, 2, 3, 5].contains(&tag) {
            return Err(Error::Unsupported(format!(
                "{}: unadmitted transform key-group tag {tag}",
                self.reader.source
            )));
        }
        let words = 1
            + N
            + if tag == 2 {
                2 * N
            } else if tag == 3 {
                3
            } else {
                0
            };
        self.reader.budget(declared_keys as usize, words * 4)?;
        // Admit every stored float word before allocating this source array.
        charge_many(
            self.work,
            declared_keys as usize * words,
            self.reader.source,
        )?;
        self.reader.reserve::<Key<N>>(declared_keys as usize)?;
        let mut keys = Vec::with_capacity(declared_keys as usize);
        for _ in 0..declared_keys {
            let time_bits = self.bits()?;
            let value_bits = self.vector()?;
            let (forward_bits, backward_bits) = if tag == 2 {
                (Some(self.vector()?), Some(self.vector()?))
            } else {
                (None, None)
            };
            let tbc_bits = if tag == 3 { Some(self.vector()?) } else { None };
            keys.push(Key {
                time_bits,
                value_bits,
                forward_bits,
                backward_bits,
                tbc_bits,
            });
        }
        Ok(Group {
            declared_keys,
            key_type: Some(tag),
            keys,
        })
    }
    fn rotation(&mut self, count: u32) -> Result<Rotation> {
        charge(self.work, self.reader.source)?;
        if count == 0 {
            return Ok(Rotation::Absent);
        }
        let tag = self.reader.u32()?;
        if tag == 4 {
            if count != 1 {
                return Err(Error::Unsupported(format!(
                    "{}: XYZ rotation source count other than one is unadmitted",
                    self.reader.source
                )));
            }
            return Ok(Rotation::Xyz {
                axes: [self.group()?, self.group()?, self.group()?],
            });
        }
        if ![1, 2, 3, 5].contains(&tag) {
            return Err(Error::Unsupported(format!(
                "{}: unadmitted quaternion key tag {tag}",
                self.reader.source
            )));
        }
        // Quaternion keys never have stored tangents, including source tag2.
        let width = if tag == 3 { 32 } else { 20 };
        self.reader.budget(count as usize, width)?;
        charge_many(self.work, count as usize * (width / 4), self.reader.source)?;
        self.reader.reserve::<QuaternionKey>(count as usize)?;
        let mut keys = Vec::with_capacity(count as usize);
        for _ in 0..count {
            keys.push(QuaternionKey {
                time_bits: self.bits()?,
                value_wxyz_bits: self.vector()?,
                tbc_bits: if tag == 3 { Some(self.vector()?) } else { None },
            });
        }
        Ok(Rotation::Quaternion {
            key_type: tag,
            keys,
        })
    }
}
pub(super) fn decode(reader: Reader<'_>, work: &mut usize) -> Result<TransformKeys> {
    let mut input = Decode { reader, work };
    let declared_rotation_keys = input.reader.u32()?;
    let rotation = input.rotation(declared_rotation_keys)?;
    let translations = input.group()?;
    let scales = input.group()?;
    input.reader.finish()?;
    Ok(TransformKeys {
        declared_rotation_keys,
        rotation,
        translations,
        scales,
    })
}
