//! Enclose internally evaluated CPU binary64 positions in the source skin frame.
use super::pose;
use crate::{Result, nif_animation};
use pose::Budget;
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const CONTRACT: &str = "engineering-source-bound-posed-skin-bounds-v1";
pub const FRAME: &str = "existing-cpu-source-skin-root-binary64-positions";

#[derive(Clone, Copy, Debug)]
pub enum Pose {
    Stored,
    Sampled {
        controller_policy: pose::ControllerPolicy,
        animation: nif_animation::pose::Request,
    },
}
#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub expected_source_sha256: [u8; 32],
    pub skin: pose::Request,
    pub pose: Pose,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub stored: pose::Limits,
    pub sampled: pose::CombinedLimits,
    pub input_bytes: usize,
    pub vertices: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub extra_array_bytes: usize,
    pub extra_work_units: usize,
    pub array_bytes: usize,
    pub work_units: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            stored: Default::default(),
            sampled: Default::default(),
            input_bytes: 64 * 1024 * 1024,
            vertices: 65_535,
            decoder_array_admission_bytes: 768 * 1024 * 1024,
            decoder_check_admission_units: 96_000_000,
            extra_array_bytes: 8 * 1024 * 1024,
            extra_work_units: 128_000_000,
            array_bytes: 80 * 1024 * 1024,
            work_units: 146_000_000,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Coordinates {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub min_f64_bits: [u64; 3],
    pub max_f64_bits: [u64; 3],
}
#[derive(Debug, Serialize)]
pub struct Evaluation {
    pub contract: &'static str,
    pub frame: &'static str,
    pub arithmetic: &'static str,
    pub source_sha256: String,
    pub geometry: u32,
    pub geometry_data: u32,
    pub instance: u32,
    pub skin_data: u32,
    pub skeleton_root: u32,
    pub weights: pose::WeightPolicy,
    pub mode: &'static str,
    pub controller_policy: Option<pose::ControllerPolicy>,
    pub sample: Option<nif_animation::pose::ObjectPose>,
    pub unapplied_controllers: Vec<pose::UnappliedController>,
    pub vertices: usize,
    pub coordinates: Coordinates,
    /// Placement is retained, never applied to these source-root endpoints.
    pub skin_to_source_world: pose::Affine,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub pose_retained_bytes: usize,
    pub pose_work_units: usize,
    pub extra_retained_bytes: usize,
    pub extra_work_units: usize,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub retail_behavior_verified: bool,
}

struct Charges<'a> {
    extra: Budget<'a>,
    aggregate: Budget<'a>,
}
impl Charges<'_> {
    fn reserve<T>(&mut self, count: usize) -> Result<()> {
        self.extra.reserve::<T>(count)?;
        self.aggregate.reserve::<T>(count)
    }
    fn charge(&mut self, count: usize) -> Result<()> {
        self.extra.charge(count)?;
        self.aggregate.charge(count)
    }
}

/// No midpoint, extent, transform or outward-neighbor arithmetic is needed:
/// endpoints are actual internally evaluated binary64 values. Total order keeps
/// negative zero as the lower zero and positive zero as the upper zero.
fn enclose(points: &[[f64; 3]], charges: &mut Charges<'_>) -> Result<Coordinates> {
    charges.charge(points.len().checked_mul(9).ok_or_else(|| {
        charges
            .aggregate
            .fail("bounds vertex comparison work product overflow")
    })?)?;
    let first = *points
        .first()
        .ok_or_else(|| charges.aggregate.fail("posed bounds require vertices"))?;
    let mut min = first;
    let mut max = first;
    for point in points {
        for axis in 0..3 {
            let value = point[axis];
            if !value.is_finite() {
                return Err(charges
                    .aggregate
                    .fail("posed bounds coordinate is not finite"));
            }
            if value.total_cmp(&min[axis]).is_lt() {
                min[axis] = value;
            }
            if value.total_cmp(&max[axis]).is_gt() {
                max[axis] = value;
            }
        }
    }
    Ok(Coordinates {
        min,
        max,
        min_f64_bits: min.map(f64::to_bits),
        max_f64_bits: max.map(f64::to_bits),
    })
}

pub fn evaluate(
    bytes: &[u8],
    source: &str,
    request: Request,
    limits: Limits,
) -> Result<Evaluation> {
    let mut charges = Charges {
        extra: Budget {
            source,
            storage: limits.extra_array_bytes,
            work: limits.extra_work_units,
        },
        aggregate: Budget {
            source,
            storage: limits.array_bytes,
            work: limits.work_units,
        },
    };
    let phase_input_limit = match request.pose {
        Pose::Stored => limits.stored.source.partition.skin.scene.input_bytes,
        Pose::Sampled { .. } => limits
            .sampled
            .skin
            .source
            .partition
            .skin
            .scene
            .input_bytes
            .min(limits.sampled.animation.scene.input_bytes)
            .min(limits.sampled.animation.keys.animation.input_bytes),
    };
    if bytes.len() > limits.input_bytes.min(phase_input_limit) {
        return Err(charges
            .aggregate
            .fail("posed bounds source input byte budget exceeded"));
    }
    charges.charge(bytes.len().checked_mul(6).ok_or_else(|| {
        charges
            .aggregate
            .fail("bounds source byte work product overflow")
    })?)?;
    if <[u8; 32]>::from(Sha256::digest(bytes)) != request.expected_source_sha256 {
        return Err(charges.aggregate.fail("posed bounds source SHA256 differs"));
    }
    let (skin_limits, additional_arrays, additional_checks) = match request.pose {
        Pose::Stored => (limits.stored, 0, 0),
        Pose::Sampled { .. } => {
            let animation = limits.sampled.animation;
            let arrays = animation
                .keys
                .max_combined_retained_bytes
                .checked_add(animation.scene.array_bytes)
                .ok_or_else(|| charges.aggregate.fail("bounds decoder array sum overflow"))?;
            let checks = [
                animation.keys.animation.reference_checks,
                animation.keys.key_work,
                animation.sampling.validation_work,
                animation.sampling.sampling_work,
            ]
            .into_iter()
            .try_fold(0usize, usize::checked_add)
            .ok_or_else(|| charges.aggregate.fail("bounds decoder check sum overflow"))?;
            (limits.sampled.skin, arrays, checks)
        }
    };
    let skin_source = skin_limits.source;
    let arrays = [
        additional_arrays,
        skin_source.partition.skin.scene.array_bytes,
        skin_source.partition.skin.skin_array_bytes,
        skin_source.partition.array_bytes,
        skin_source.array_bytes,
    ]
    .into_iter()
    .try_fold(0usize, usize::checked_add)
    .filter(|n| *n <= limits.decoder_array_admission_bytes)
    .ok_or_else(|| {
        charges
            .aggregate
            .fail("bounds decoder array admission exceeded")
    })?;
    let checks = [
        additional_checks,
        skin_source.partition.skin.weight_index_checks,
        skin_source.partition.index_checks,
        skin_source.graph_checks,
    ]
    .into_iter()
    .try_fold(0usize, usize::checked_add)
    .filter(|n| *n <= limits.decoder_check_admission_units)
    .ok_or_else(|| {
        charges
            .aggregate
            .fail("bounds decoder check admission exceeded")
    })?;
    charges.reserve::<Evaluation>(1)?;
    let (skin, sample, policy, mode, pose_retained, pose_work) = match request.pose {
        Pose::Stored => {
            let skin = pose::evaluate(
                bytes,
                source,
                request.skin,
                pose::Limits {
                    array_bytes: limits.stored.array_bytes.min(charges.aggregate.storage),
                    work_units: limits.stored.work_units.min(charges.aggregate.work),
                    ..limits.stored
                },
            )?;
            let retained = skin.retained_bytes;
            let work = skin.work_units;
            (skin, None, None, "stored", retained, work)
        }
        Pose::Sampled {
            controller_policy,
            animation,
        } => {
            let evaluated = pose::evaluate_sampled(
                bytes,
                source,
                pose::SampledRequest {
                    expected_source_sha256: request.expected_source_sha256,
                    skin: request.skin,
                    controller_policy,
                },
                animation,
                pose::CombinedLimits {
                    array_bytes: limits.sampled.array_bytes.min(charges.aggregate.storage),
                    work_units: limits.sampled.work_units.min(charges.aggregate.work),
                    ..limits.sampled
                },
            )?;
            (
                evaluated.skin,
                Some(evaluated.sample),
                Some(controller_policy),
                "sampled",
                evaluated.retained_bytes,
                evaluated.work_units,
            )
        }
    };
    // Includes every charged deformation intermediate even though positions,
    // palettes, normals and weight sums are dropped after endpoint selection.
    charges.aggregate.reserve::<u8>(pose_retained)?;
    charges.aggregate.charge(pose_work)?;
    if skin.positions.len() > limits.vertices {
        return Err(charges
            .aggregate
            .fail("posed bounds vertex count budget exceeded"));
    }
    let coordinates = enclose(&skin.positions, &mut charges)?;
    Ok(Evaluation {
        contract: CONTRACT,
        frame: FRAME,
        arithmetic: "finite-binary64-output-coordinate-endpoint-selection-no-extent-arithmetic",
        source_sha256: skin.source_sha256,
        geometry: skin.geometry,
        geometry_data: skin.geometry_data,
        instance: skin.instance,
        skin_data: skin.skin_data,
        skeleton_root: skin.skeleton_root,
        weights: skin.weights,
        mode,
        controller_policy: policy,
        sample,
        unapplied_controllers: skin.unapplied_controllers,
        vertices: skin.positions.len(),
        coordinates,
        skin_to_source_world: skin.skin_to_source_world,
        decoder_array_admission_bytes: arrays,
        decoder_check_admission_units: checks,
        pose_retained_bytes: pose_retained,
        pose_work_units: pose_work,
        extra_retained_bytes: limits.extra_array_bytes - charges.extra.storage,
        extra_work_units: limits.extra_work_units - charges.extra.work,
        retained_bytes: limits.array_bytes - charges.aggregate.storage,
        work_units: limits.work_units - charges.aggregate.work,
        retail_behavior_verified: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn charges(work: usize) -> Charges<'static> {
        Charges {
            extra: Budget {
                source: "endpoint arithmetic",
                storage: 0,
                work,
            },
            aggregate: Budget {
                source: "endpoint arithmetic",
                storage: 0,
                work,
            },
        }
    }

    #[test]
    fn finite_extremes_subnormals_and_signed_zero_are_selected_without_extent_overflow() {
        let tiny = f64::from_bits(1);
        let points = [[f64::MAX, tiny, 0.], [-f64::MAX, -tiny, -0.], [0., 0., 0.]];
        let bounds = enclose(&points, &mut charges(27)).unwrap();
        assert_eq!(bounds.min, [-f64::MAX, -tiny, -0.]);
        assert_eq!(bounds.max, [f64::MAX, tiny, 0.]);
        assert_eq!(
            bounds.min_f64_bits,
            [-f64::MAX, -tiny, -0.].map(f64::to_bits)
        );
        assert_eq!(bounds.max_f64_bits, [f64::MAX, tiny, 0.].map(f64::to_bits));
        for points in [points, [points[2], points[1], points[0]]] {
            let reversed = enclose(&points, &mut charges(27)).unwrap();
            assert_eq!(reversed.min_f64_bits, bounds.min_f64_bits);
            assert_eq!(reversed.max_f64_bits, bounds.max_f64_bits);
        }
    }

    #[test]
    fn nonfinite_empty_and_either_comparison_budget_refuse_without_partial_bounds() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(enclose(&[[1., 2., 3.], [4., bad, 6.]], &mut charges(18)).is_err());
        }
        assert!(enclose(&[], &mut charges(0)).is_err());
        for aggregate_under in [false, true] {
            let mut budget = charges(18);
            if aggregate_under {
                budget.aggregate.work -= 1;
            } else {
                budget.extra.work -= 1;
            }
            // The complete scan is charged before touching even a nonfinite row.
            let error = enclose(&[[0.; 3], [f64::INFINITY; 3]], &mut budget).unwrap_err();
            assert!(
                error.to_string().contains("work budget exceeded"),
                "{error}"
            );
        }
    }
}
