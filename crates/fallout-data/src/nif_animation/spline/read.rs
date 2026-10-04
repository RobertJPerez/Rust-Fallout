use super::Data;
use crate::{Error, Result, nif_scene::cursor::Reader};

pub(super) fn charge(work: &mut usize, count: usize, source: &str) -> Result<()> {
    *work = work.checked_sub(count).ok_or_else(|| {
        Error::Unsupported(format!("{source}: spline-source work budget exceeded"))
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
        let mut values = [0; N];
        for value in &mut values {
            *value = self.bits()?;
        }
        Ok(values)
    }
    fn compact_transform(&mut self) -> Result<Data> {
        charge(self.work, 21, self.reader.source)?;
        Ok(Data::CompactTransform {
            start_bits: self.bits()?,
            stop_bits: self.bits()?,
            spline_data: self.reader.reference()?,
            basis_data: self.reader.reference()?,
            translation_bits: self.vector()?,
            rotation_wxyz_bits: self.vector()?,
            scale_bits: self.bits()?,
            translation_handle: self.reader.u32()?,
            rotation_handle: self.reader.u32()?,
            scale_handle: self.reader.u32()?,
            translation_offset_bits: self.bits()?,
            translation_half_range_bits: self.bits()?,
            rotation_offset_bits: self.bits()?,
            rotation_half_range_bits: self.bits()?,
            scale_offset_bits: self.bits()?,
            scale_half_range_bits: self.bits()?,
        })
    }
    fn control_points(&mut self) -> Result<Data> {
        charge(self.work, 1, self.reader.source)?;
        let declared_float_count = self.reader.u32()?;
        let count = declared_float_count as usize;
        self.reader.budget(count, 4)?;
        charge(self.work, count, self.reader.source)?;
        self.reader.reserve::<u32>(count)?;
        let mut float_bits = Vec::with_capacity(count);
        for _ in 0..count {
            float_bits.push(self.bits()?);
        }
        charge(self.work, 1, self.reader.source)?;
        let declared_compact_count = self.reader.u32()?;
        let count = declared_compact_count as usize;
        self.reader.budget(count, 2)?;
        charge(self.work, count, self.reader.source)?;
        self.reader.reserve::<i16>(count)?;
        let mut compact = Vec::with_capacity(count);
        for _ in 0..count {
            compact.push(self.reader.u16()? as i16);
        }
        Ok(Data::ControlPoints {
            declared_float_count,
            float_bits,
            declared_compact_count,
            compact,
        })
    }
}
pub(super) fn decode(reader: Reader<'_>, kind: &str, work: &mut usize) -> Result<Data> {
    let mut input = Decode { reader, work };
    let result = match kind {
        "NiBSplineCompTransformInterpolator" => input.compact_transform()?,
        "NiBSplineData" => input.control_points()?,
        "NiBSplineBasisData" => {
            charge(input.work, 1, input.reader.source)?;
            Data::Basis {
                num_control_points: input.reader.u32()?,
            }
        }
        _ => unreachable!("caller selects three exact source classes"),
    };
    input.reader.finish()?;
    Ok(result)
}
