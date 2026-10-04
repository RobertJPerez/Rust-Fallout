use super::{
    ControlledBlock, Controller, Data, NoteLinks, Sequence, TextKey, TextKeys,
    TransformInterpolator, charge,
};
use crate::{Result, nif_scene::cursor::Reader};

struct Decode<'a, 'b> {
    reader: Reader<'a>,
    checks: &'b mut usize,
}
impl Decode<'_, '_> {
    fn link(&mut self) -> Result<Option<u32>> {
        charge(self.checks, self.reader.source)?;
        self.reader.reference()
    }
    fn string(&mut self) -> Result<Option<u32>> {
        charge(self.checks, self.reader.source)?;
        self.reader.string()
    }
    fn bits(&mut self) -> Result<u32> {
        Ok(self.reader.float()?.to_bits())
    }
    fn vector<const N: usize>(&mut self) -> Result<[u32; N]> {
        let mut bits = [0; N];
        for value in &mut bits {
            *value = self.bits()?;
        }
        Ok(bits)
    }
    fn controller(&mut self) -> Result<Controller> {
        Ok(Controller {
            next_controller: self.link()?,
            flags: self.reader.u16()?,
            frequency_bits: self.bits()?,
            phase_bits: self.bits()?,
            start_bits: self.bits()?,
            stop_bits: self.bits()?,
            target: self.link()?,
            interpolator: self.link()?,
        })
    }
    fn packet(&mut self) -> Result<ControlledBlock> {
        charge(self.checks, self.reader.source)?;
        Ok(ControlledBlock {
            interpolator: self.link()?,
            controller: self.link()?,
            priority: self.reader.u8()?,
            node_name: self.string()?,
            property_type: self.string()?,
            controller_type: self.string()?,
            controller_id: self.string()?,
            interpolator_id: self.string()?,
        })
    }
    fn sequence(&mut self) -> Result<Sequence> {
        let name = self.string()?;
        let declared_controlled_blocks = self.reader.u32()?;
        let array_grow_by = self.reader.u32()?;
        let count = declared_controlled_blocks as usize;
        self.reader.budget(count, 29)?;
        self.reader.reserve::<ControlledBlock>(count)?;
        // Charge the product even when its source count is zero.
        charge(self.checks, self.reader.source)?;
        let mut controlled_blocks = Vec::with_capacity(count);
        for _ in 0..count {
            controlled_blocks.push(self.packet()?);
        }
        let weight_bits = self.bits()?;
        let text_keys = self.link()?;
        let cycle_type = self.reader.u32()?;
        let frequency_bits = self.bits()?;
        let start_bits = self.bits()?;
        let stop_bits = self.bits()?;
        let manager = self.link()?;
        let accum_root_name = self.string()?;
        let stream = self.reader.index.bethesda_version;
        let notes = if (24..=28).contains(&stream) {
            NoteLinks::Single {
                target: self.link()?,
            }
        } else if stream > 28 {
            let declared_count = self.reader.u16()?;
            let count = declared_count as usize;
            self.reader.budget(count, 4)?;
            self.reader.reserve::<Option<u32>>(count)?;
            charge(self.checks, self.reader.source)?;
            let mut targets = Vec::with_capacity(count);
            for _ in 0..count {
                targets.push(self.link()?);
            }
            NoteLinks::Array {
                declared_count,
                targets,
            }
        } else {
            NoteLinks::Absent
        };
        Ok(Sequence {
            name,
            declared_controlled_blocks,
            array_grow_by,
            controlled_blocks,
            weight_bits,
            text_keys,
            cycle_type,
            frequency_bits,
            start_bits,
            stop_bits,
            manager,
            accum_root_name,
            notes,
        })
    }
    fn text_keys(&mut self) -> Result<TextKeys> {
        let name = self.string()?;
        let declared_keys = self.reader.u32()?;
        let count = declared_keys as usize;
        self.reader.budget(count, 8)?;
        self.reader.reserve::<TextKey>(count)?;
        charge(self.checks, self.reader.source)?;
        let mut keys = Vec::with_capacity(count);
        for _ in 0..count {
            keys.push(TextKey {
                time_bits: self.bits()?,
                value: self.string()?,
            });
        }
        Ok(TextKeys {
            name,
            declared_keys,
            keys,
        })
    }
}

/// Shared NV NiTimeController/NiSingleInterpController layout. NiVisController
/// has no additional payload in the already-admitted 20.2.0.7 tuple.
pub(super) fn single_controller(reader: Reader<'_>, checks: &mut usize) -> Result<Controller> {
    let mut input = Decode { reader, checks };
    let controller = input.controller()?;
    input.reader.finish()?;
    Ok(controller)
}

pub(super) fn decode(reader: Reader<'_>, kind: &str, checks: &mut usize) -> Result<Data> {
    let mut input = Decode { reader, checks };
    let data = match kind {
        "NiTransformController" => Data::TransformController {
            controller: input.controller()?,
        },
        "NiControllerSequence" => Data::ControllerSequence {
            sequence: input.sequence()?,
        },
        "NiTransformInterpolator" => Data::TransformInterpolator {
            interpolator: TransformInterpolator {
                translation_bits: input.vector()?,
                rotation_wxyz_bits: input.vector()?,
                scale_bits: input.bits()?,
                data: input.link()?,
            },
        },
        "NiTextKeyExtraData" => Data::TextKeyExtraData {
            text_keys: input.text_keys()?,
        },
        _ => unreachable!("caller selects four admitted exact classes"),
    };
    input.reader.finish()?;
    Ok(data)
}
