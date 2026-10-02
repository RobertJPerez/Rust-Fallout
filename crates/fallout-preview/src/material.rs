//! Fixed render states for the inspection adapter. Lighting and effect controllers
//! are still separate work; the source decoder keeps every original property bit.
use bevy::{
    asset::embedded_asset,
    mesh::MeshVertexBufferLayoutRef,
    pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline},
    prelude::*,
    render::render_resource::{
        AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, CompareFunction,
        Face, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};
use serde::Serialize;

pub type InspectionMaterial = ExtendedMaterial<StandardMaterial, SourceExtension>;

#[derive(Clone, Copy, Debug, Serialize, Reflect)]
pub struct Raster {
    pub alpha_flags: u16,
    pub alpha_threshold: u8,
    pub draw_mode: u8,
    pub depth_test: bool,
    pub depth_write: bool,
}

impl Default for Raster {
    fn default() -> Self {
        Self {
            alpha_flags: 0,
            alpha_threshold: 0,
            draw_mode: 1,
            depth_test: true,
            depth_write: true,
        }
    }
}

impl Raster {
    pub fn alpha_mode(self) -> AlphaMode {
        if self.alpha_flags & 1 != 0 {
            AlphaMode::Blend
        } else if self.alpha_flags & 0x200 != 0 {
            AlphaMode::Mask(0.)
        } else {
            AlphaMode::Opaque
        }
    }

    pub fn validate(self) -> crate::model::Result<()> {
        if self.alpha_flags & 1 != 0
            && ((self.alpha_flags >> 1) & 15 > 10 || (self.alpha_flags >> 5) & 15 > 10)
        {
            return Err(format!(
                "unsupported source alpha blend factors: 0x{:04x}",
                self.alpha_flags
            )
            .into());
        }
        Ok(())
    }

    pub fn extension(self) -> SourceExtension {
        SourceExtension {
            alpha: UVec4::new(
                u32::from(self.alpha_flags),
                u32::from(self.alpha_threshold),
                0,
                0,
            ),
            raster: self,
        }
    }
}

/// Pipeline keys include only fixed state, so changing a threshold does not build
/// another pipeline. Its value lives in the material uniform instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RasterKey {
    blend: Option<(u8, u8)>,
    depth_test: bool,
    depth_write: bool,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[bind_group_data(RasterKey)]
pub struct SourceExtension {
    #[uniform(100)]
    alpha: UVec4,
    raster: Raster,
}

impl From<&SourceExtension> for RasterKey {
    fn from(value: &SourceExtension) -> Self {
        let flags = value.raster.alpha_flags;
        Self {
            blend: (flags & 1 != 0)
                .then_some((((flags >> 1) & 15) as u8, ((flags >> 5) & 15) as u8)),
            depth_test: value.raster.depth_test,
            depth_write: value.raster.depth_write,
        }
    }
}

fn factor(source: u8) -> BlendFactor {
    match source {
        0 => BlendFactor::One,
        1 => BlendFactor::Zero,
        2 => BlendFactor::Src,
        3 => BlendFactor::OneMinusSrc,
        4 => BlendFactor::Dst,
        5 => BlendFactor::OneMinusDst,
        6 => BlendFactor::SrcAlpha,
        7 => BlendFactor::OneMinusSrcAlpha,
        8 => BlendFactor::DstAlpha,
        9 => BlendFactor::OneMinusDstAlpha,
        10 => BlendFactor::SrcAlphaSaturated,
        _ => unreachable!("raster factors validated before material creation"),
    }
}

impl MaterialExtension for SourceExtension {
    fn fragment_shader() -> ShaderRef {
        "embedded://fallout_preview/inspection.wgsl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(target) = descriptor
            .fragment
            .as_mut()
            .and_then(|f| f.targets.first_mut())
            .and_then(Option::as_mut)
        {
            target.blend = key.bind_group_data.blend.map(|(src, dst)| {
                let color = BlendComponent {
                    src_factor: factor(src),
                    dst_factor: factor(dst),
                    operation: BlendOperation::Add,
                };
                // The saturated factor is defined as one for the alpha channel.
                let alpha = BlendComponent {
                    src_factor: if src == 10 {
                        BlendFactor::One
                    } else {
                        factor(src)
                    },
                    dst_factor: if dst == 10 {
                        BlendFactor::One
                    } else {
                        factor(dst)
                    },
                    operation: BlendOperation::Add,
                };
                BlendState { color, alpha }
            });
        }
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_write_enabled = Some(key.bind_group_data.depth_write);
            if !key.bind_group_data.depth_test {
                depth.depth_compare = Some(CompareFunction::Always);
            }
        }
        Ok(())
    }
}

pub fn adapt(base: StandardMaterial, raster: Raster) -> InspectionMaterial {
    let cull_mode = match raster.draw_mode {
        2 => Some(Face::Front),
        3 => None,
        _ => Some(Face::Back),
    };
    InspectionMaterial {
        base: StandardMaterial {
            alpha_mode: raster.alpha_mode(),
            cull_mode,
            double_sided: raster.draw_mode == 3,
            unlit: true,
            ..base
        },
        extension: raster.extension(),
    }
}

pub struct InspectionPlugin;
impl Plugin for InspectionPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "inspection.wgsl");
        app.add_plugins(MaterialPlugin::<InspectionMaterial>::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_darkening_and_additive_blends_keep_distinct_factors() {
        for (flags, expected) in [(0x10ed, (6, 7)), (0x1043, (1, 2)), (0x100d, (6, 0))] {
            let raster = Raster {
                alpha_flags: flags,
                ..default()
            };
            raster.validate().unwrap();
            assert_eq!(RasterKey::from(&raster.extension()).blend, Some(expected));
        }
        assert!(
            Raster {
                alpha_flags: 0x1f,
                ..default()
            }
            .validate()
            .is_err()
        );
        let first = Raster {
            alpha_flags: 0x12ec,
            alpha_threshold: 1,
            ..default()
        }
        .extension();
        let second = Raster {
            alpha_flags: 0x12ec,
            alpha_threshold: 254,
            ..default()
        }
        .extension();
        assert_eq!(RasterKey::from(&first), RasterKey::from(&second));
    }
}
