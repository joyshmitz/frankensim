//! Typed kernels for the Walking Fish Group benchmark family.
//!
//! This first production slice implements the normalized WFG4 composition for
//! arbitrary valid objective, position-parameter, and distance-parameter
//! counts.  Its transformation and shape equations follow the corrected WFG
//! toolkit as represented by jMetal revision
//! `ea7e882f6b8f94b99535921674e62cda7986f20e`.  Inputs are already normalized
//! to `[0, 1]`; accepting the heterogeneous canonical bounds
//! `z_i in [0, 2(i + 1)]` is deliberately left to a later adapter.
//!
//! Determinism is structural within the evaluator: reductions have a fixed
//! left-to-right order and transcendental calls use [`fs_math::det`].  This
//! module does not yet claim the complete WFG1-WFG9 suite, executable parity
//! with an external oracle, optimizer convergence, cancellation coverage,
//! cross-ISA bit stability, or performance evidence.

#![deny(unsafe_code)]

const CORRECTION_EPSILON: f64 = 1.0e-10;
const S_MULTI_A: f64 = 30.0;
const S_MULTI_B: f64 = 10.0;
const S_MULTI_CENTER: f64 = 0.35;

/// Structured refusal from [`Wfg4::new`] or [`Wfg4::evaluate_normalized`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WfgError {
    /// A multiobjective WFG problem needs at least two objectives.
    TooFewObjectives {
        /// Supplied objective count.
        objectives: usize,
    },
    /// Each of the `M - 1` position groups must contain a parameter.
    TooFewPositionParameters {
        /// Supplied position-parameter count.
        position_parameters: usize,
        /// Supplied objective count.
        objectives: usize,
    },
    /// Position parameters cannot be divided into equal objective groups.
    PositionParametersNotDivisible {
        /// Supplied position-parameter count.
        position_parameters: usize,
        /// Required group count, equal to `objectives - 1`.
        groups: usize,
    },
    /// The WFG distance block must not be empty.
    NoDistanceParameters,
    /// The total decision dimension could not be represented by `usize`.
    DimensionOverflow {
        /// Supplied position-parameter count.
        position_parameters: usize,
        /// Supplied distance-parameter count.
        distance_parameters: usize,
    },
    /// The normalized decision vector has the wrong dimension.
    WrongInputLength {
        /// Dimension admitted by the problem specification.
        expected: usize,
        /// Supplied decision-vector length.
        actual: usize,
    },
    /// A normalized coordinate is NaN or infinite.
    NonFiniteInput {
        /// Zero-based coordinate index.
        component: usize,
        /// Exact IEEE-754 payload.
        bits: u64,
    },
    /// A finite normalized coordinate lies outside `[0, 1]`.
    InputOutOfRange {
        /// Zero-based coordinate index.
        component: usize,
        /// Exact IEEE-754 payload.
        bits: u64,
    },
    /// Storage for a validated evaluation could not be reserved.
    AllocationFailed {
        /// Stable name of the requested vector.
        what: &'static str,
        /// Number of `f64` elements requested.
        elements: usize,
    },
}

impl core::fmt::Display for WfgError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::TooFewObjectives { objectives } => write!(
                formatter,
                "WFG4 requires at least two objectives, received {objectives}"
            ),
            Self::TooFewPositionParameters {
                position_parameters,
                objectives,
            } => write!(
                formatter,
                "WFG4 with {objectives} objectives requires at least {} position parameters, received {position_parameters}",
                objectives.saturating_sub(1)
            ),
            Self::PositionParametersNotDivisible {
                position_parameters,
                groups,
            } => write!(
                formatter,
                "WFG4 position count {position_parameters} is not divisible by its {groups} objective groups"
            ),
            Self::NoDistanceParameters => {
                formatter.write_str("WFG4 requires at least one distance parameter")
            }
            Self::DimensionOverflow {
                position_parameters,
                distance_parameters,
            } => write!(
                formatter,
                "WFG4 decision dimension {position_parameters} + {distance_parameters} overflowed usize"
            ),
            Self::WrongInputLength { expected, actual } => write!(
                formatter,
                "WFG4 expected {expected} normalized coordinates, received {actual}"
            ),
            Self::NonFiniteInput { component, bits } => write!(
                formatter,
                "WFG4 normalized coordinate {component} is non-finite (bits 0x{bits:016x})"
            ),
            Self::InputOutOfRange { component, bits } => write!(
                formatter,
                "WFG4 normalized coordinate {component} lies outside [0, 1] (bits 0x{bits:016x})"
            ),
            Self::AllocationFailed { what, elements } => write!(
                formatter,
                "WFG4 could not reserve {elements} elements for {what}"
            ),
        }
    }
}

impl std::error::Error for WfgError {}

/// A validated normalized WFG4 problem definition.
///
/// WFG4 partitions its `k` position parameters equally among `M - 1`
/// reductions and reduces all `l` distance parameters into the final
/// coordinate.  Construction refuses dimensions for which that partition is
/// not exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wfg4 {
    objectives: usize,
    position_parameters: usize,
    distance_parameters: usize,
    dimension: usize,
    position_group_size: usize,
}

impl Wfg4 {
    /// Validate a normalized WFG4 problem definition.
    ///
    /// # Errors
    ///
    /// Returns a structured refusal when the objective count, position
    /// partition, distance count, or checked total dimension is invalid.
    pub fn new(
        objectives: usize,
        position_parameters: usize,
        distance_parameters: usize,
    ) -> Result<Self, WfgError> {
        if objectives < 2 {
            return Err(WfgError::TooFewObjectives { objectives });
        }
        let groups = objectives - 1;
        if position_parameters < groups {
            return Err(WfgError::TooFewPositionParameters {
                position_parameters,
                objectives,
            });
        }
        if !position_parameters.is_multiple_of(groups) {
            return Err(WfgError::PositionParametersNotDivisible {
                position_parameters,
                groups,
            });
        }
        if distance_parameters == 0 {
            return Err(WfgError::NoDistanceParameters);
        }
        let Some(dimension) = position_parameters.checked_add(distance_parameters) else {
            return Err(WfgError::DimensionOverflow {
                position_parameters,
                distance_parameters,
            });
        };

        Ok(Self {
            objectives,
            position_parameters,
            distance_parameters,
            dimension,
            position_group_size: position_parameters / groups,
        })
    }

    /// Number of objective values produced by each evaluation.
    #[must_use]
    pub const fn objectives(self) -> usize {
        self.objectives
    }

    /// Number of position-related decision parameters.
    #[must_use]
    pub const fn position_parameters(self) -> usize {
        self.position_parameters
    }

    /// Number of distance-related decision parameters.
    #[must_use]
    pub const fn distance_parameters(self) -> usize {
        self.distance_parameters
    }

    /// Total normalized decision dimension, `k + l`.
    #[must_use]
    pub const fn dimension(self) -> usize {
        self.dimension
    }

    /// Evaluate one normalized decision vector.
    ///
    /// The returned intermediate vectors make benchmark receipts and
    /// independent recomputation possible without duplicating private
    /// transformation logic.
    ///
    /// # Errors
    ///
    /// Returns a structured refusal for the wrong decision dimension,
    /// non-finite or out-of-domain coordinates, or a failed output-storage
    /// reservation.  No transformation is evaluated before input admission.
    pub fn evaluate_normalized(&self, input: &[f64]) -> Result<WfgEvaluation, WfgError> {
        self.validate_input(input)?;

        let mut transformed = reserved_vec("transformed coordinates", self.dimension)?;
        for &value in input {
            transformed.push(s_multi(value));
        }

        let mut reduced = reserved_vec("reduced coordinates", self.objectives)?;
        for group in 0..self.objectives - 1 {
            let start = group * self.position_group_size;
            let end = start + self.position_group_size;
            reduced.push(equal_weight_reduction(&transformed[start..end]));
        }
        reduced.push(equal_weight_reduction(
            &transformed[self.position_parameters..],
        ));

        // WFG4 has A_i = 1, so the standard x reconstruction is exactly t.
        let shape = concave_shape(&reduced)?;
        let distance = reduced[self.objectives - 1];
        let mut objectives = reserved_vec("objectives", self.objectives)?;
        for (index, &shape_value) in shape.iter().enumerate() {
            let scale = 2.0 * (index + 1) as f64;
            objectives.push(scale.mul_add(shape_value, distance));
        }

        Ok(WfgEvaluation {
            transformed,
            reduced,
            shape,
            objectives,
        })
    }

    fn validate_input(self, input: &[f64]) -> Result<(), WfgError> {
        if input.len() != self.dimension {
            return Err(WfgError::WrongInputLength {
                expected: self.dimension,
                actual: input.len(),
            });
        }
        for (component, &value) in input.iter().enumerate() {
            if !value.is_finite() {
                return Err(WfgError::NonFiniteInput {
                    component,
                    bits: value.to_bits(),
                });
            }
            if !(0.0..=1.0).contains(&value) {
                return Err(WfgError::InputOutOfRange {
                    component,
                    bits: value.to_bits(),
                });
            }
        }
        Ok(())
    }
}

/// One normalized WFG4 evaluation with replay-relevant intermediates.
#[derive(Debug, Clone, PartialEq)]
pub struct WfgEvaluation {
    transformed: Vec<f64>,
    reduced: Vec<f64>,
    shape: Vec<f64>,
    objectives: Vec<f64>,
}

impl WfgEvaluation {
    /// Per-coordinate `s_multi(30, 10, 0.35)` outputs.
    #[must_use]
    pub fn transformed(&self) -> &[f64] {
        &self.transformed
    }

    /// The `M` fixed-order, equal-weight reductions.
    #[must_use]
    pub fn reduced(&self) -> &[f64] {
        &self.reduced
    }

    /// The `M` concave WFG shape values before scale and distance are applied.
    #[must_use]
    pub fn shape(&self) -> &[f64] {
        &self.shape
    }

    /// The scaled WFG4 objective vector.
    #[must_use]
    pub fn objectives(&self) -> &[f64] {
        &self.objectives
    }

    /// Consume the evaluation and return its objective vector.
    #[must_use]
    pub fn into_objectives(self) -> Vec<f64> {
        self.objectives
    }
}

fn reserved_vec(what: &'static str, elements: usize) -> Result<Vec<f64>, WfgError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(elements)
        .map_err(|_| WfgError::AllocationFailed { what, elements })?;
    Ok(values)
}

/// Snap only roundoff-sized excursions at the WFG unit-interval boundaries.
/// This is not a general clamp.
fn correct_to_01(value: f64) -> f64 {
    if (-CORRECTION_EPSILON..=0.0).contains(&value) {
        0.0
    } else if (1.0..=1.0 + CORRECTION_EPSILON).contains(&value) {
        1.0
    } else {
        value
    }
}

/// Canonical WFG `s_multi(y, A=30, B=10, C=0.35)` transformation.
fn s_multi(value: f64) -> f64 {
    let denominator = if value <= S_MULTI_CENTER {
        2.0 * S_MULTI_CENTER
    } else {
        2.0 * (S_MULTI_CENTER - 1.0)
    };
    let ratio = (value - S_MULTI_CENTER).abs() / denominator;
    let phase = (4.0 * S_MULTI_A + 2.0) * core::f64::consts::PI * (0.5 - ratio);
    let quadratic = 4.0 * S_MULTI_B * ratio * ratio;
    correct_to_01((1.0 + fs_math::det::cos(phase) + quadratic) / (S_MULTI_B + 2.0))
}

fn equal_weight_reduction(values: &[f64]) -> f64 {
    debug_assert!(!values.is_empty());
    correct_to_01(values.iter().sum::<f64>() / values.len() as f64)
}

fn concave_shape(reduced: &[f64]) -> Result<Vec<f64>, WfgError> {
    let objectives = reduced.len();
    let mut sine_prefix = reserved_vec("concave sine prefix", objectives)?;
    let mut product = 1.0;
    sine_prefix.push(product);
    for &coordinate in &reduced[..objectives - 1] {
        product *= fs_math::det::sin(coordinate * core::f64::consts::FRAC_PI_2);
        sine_prefix.push(product);
    }

    let mut shape = reserved_vec("concave shape", objectives)?;
    for objective in 1..=objectives {
        let prefix_index = objectives - objective;
        let value = if objective == 1 {
            sine_prefix[prefix_index]
        } else {
            sine_prefix[prefix_index]
                * fs_math::det::cos(reduced[prefix_index] * core::f64::consts::FRAC_PI_2)
        };
        shape.push(value);
    }
    Ok(shape)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f64 = 1.0e-11;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= TOLERANCE,
            "actual={actual:.17e}, expected={expected:.17e}"
        );
    }

    fn assert_slice_close(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len());
        for (&actual, &expected) in actual.iter().zip(expected) {
            assert_close(actual, expected);
        }
    }

    #[test]
    fn specification_refuses_malformed_dimensions() {
        assert_eq!(
            Wfg4::new(1, 1, 1).unwrap_err(),
            WfgError::TooFewObjectives { objectives: 1 }
        );
        assert_eq!(
            Wfg4::new(4, 2, 1).unwrap_err(),
            WfgError::TooFewPositionParameters {
                position_parameters: 2,
                objectives: 4,
            }
        );
        assert_eq!(
            Wfg4::new(4, 4, 1).unwrap_err(),
            WfgError::PositionParametersNotDivisible {
                position_parameters: 4,
                groups: 3,
            }
        );
        assert_eq!(
            Wfg4::new(3, 4, 0).unwrap_err(),
            WfgError::NoDistanceParameters
        );
        assert_eq!(
            Wfg4::new(2, usize::MAX, 1).unwrap_err(),
            WfgError::DimensionOverflow {
                position_parameters: usize::MAX,
                distance_parameters: 1,
            }
        );
    }

    #[test]
    fn input_admission_is_exact_and_structured() {
        let problem = Wfg4::new(3, 4, 2).unwrap();
        assert_eq!(problem.objectives(), 3);
        assert_eq!(problem.position_parameters(), 4);
        assert_eq!(problem.distance_parameters(), 2);
        assert_eq!(problem.dimension(), 6);

        assert_eq!(
            problem.evaluate_normalized(&[0.0; 5]).unwrap_err(),
            WfgError::WrongInputLength {
                expected: 6,
                actual: 5,
            }
        );

        let mut input = [0.0; 6];
        input[2] = f64::NAN;
        assert_eq!(
            problem.evaluate_normalized(&input).unwrap_err(),
            WfgError::NonFiniteInput {
                component: 2,
                bits: f64::NAN.to_bits(),
            }
        );
        input[2] = f64::INFINITY;
        assert_eq!(
            problem.evaluate_normalized(&input).unwrap_err(),
            WfgError::NonFiniteInput {
                component: 2,
                bits: f64::INFINITY.to_bits(),
            }
        );
        input[2] = -f64::EPSILON;
        assert_eq!(
            problem.evaluate_normalized(&input).unwrap_err(),
            WfgError::InputOutOfRange {
                component: 2,
                bits: (-f64::EPSILON).to_bits(),
            }
        );
        input[2] = 1.0 + f64::EPSILON;
        assert_eq!(
            problem.evaluate_normalized(&input).unwrap_err(),
            WfgError::InputOutOfRange {
                component: 2,
                bits: (1.0 + f64::EPSILON).to_bits(),
            }
        );
    }

    #[test]
    fn correction_snaps_only_boundary_roundoff() {
        assert_eq!(correct_to_01(-CORRECTION_EPSILON), 0.0);
        assert_eq!(correct_to_01(-0.0).to_bits(), 0.0f64.to_bits());
        assert_eq!(correct_to_01(0.0), 0.0);
        assert_eq!(correct_to_01(1.0), 1.0);
        assert_eq!(correct_to_01(1.0 + CORRECTION_EPSILON), 1.0);
        assert_eq!(
            correct_to_01(-2.0 * CORRECTION_EPSILON),
            -2.0 * CORRECTION_EPSILON
        );
        assert_eq!(
            correct_to_01(1.0 + 2.0 * CORRECTION_EPSILON),
            1.0 + 2.0 * CORRECTION_EPSILON
        );
    }

    #[test]
    fn canonical_s_multi_anchors_match_corrected_toolkit() {
        assert_close(s_multi(0.0), 1.0);
        assert_close(s_multi(S_MULTI_CENTER), 0.0);
        assert_close(s_multi(1.0), 1.0);
    }

    #[test]
    fn canonical_m3_k4_l20_extreme_is_recovered() {
        let problem = Wfg4::new(3, 4, 20).unwrap();
        let evaluation = problem
            .evaluate_normalized(&vec![S_MULTI_CENTER; problem.dimension()])
            .unwrap();

        assert_slice_close(evaluation.transformed(), &[0.0; 24]);
        assert_slice_close(evaluation.reduced(), &[0.0, 0.0, 0.0]);
        assert_slice_close(evaluation.shape(), &[0.0, 0.0, 1.0]);
        assert_slice_close(evaluation.objectives(), &[0.0, 0.0, 6.0]);
    }

    #[test]
    fn interior_half_reduction_catches_shape_index_and_scale_mutants() {
        let problem = Wfg4::new(3, 4, 2).unwrap();
        let evaluation = problem
            .evaluate_normalized(&[0.35, 0.0, 0.35, 0.0, 0.35, 0.0])
            .unwrap();

        assert_slice_close(evaluation.reduced(), &[0.5, 0.5, 0.5]);
        assert_slice_close(evaluation.shape(), &[0.5, 0.5, 0.5_f64.sqrt()]);
        assert_slice_close(
            evaluation.objectives(),
            &[1.5, 2.5, 0.5 + 3.0 * 2.0_f64.sqrt()],
        );
    }

    #[test]
    fn pinned_reference_interior_probe_matches_full_pipeline() {
        // Frozen from an independent f64 port of the corrected equations at
        // jMetal revision ea7e882f6b8f94b99535921674e62cda7986f20e.
        // Non-anchor coordinates make A/B/phase transformation mutants
        // observable; asymmetric position and distance blocks make slicing,
        // shape, scale, and distance mutants observable in the same KAT.
        let problem = Wfg4::new(3, 4, 20).unwrap();
        let input = [
            0.00, 0.37, 0.74, 0.10, 0.47, 0.84, 0.20, 0.57, 0.94, 0.30, 0.67, 0.03, 0.40, 0.77,
            0.13, 0.50, 0.87, 0.23, 0.60, 0.97, 0.33, 0.70, 0.06, 0.43,
        ];
        let evaluation = problem.evaluate_normalized(&input).unwrap();

        for (index, expected) in [
            (0, 1.0),
            (1, 0.006_941_067_221_959_935),
            (2, 0.409_084_749_531_246_85),
            (3, 0.489_959_990_197_517_96),
            (4, 0.168_487_021_762_804_94),
            (23, 0.093_942_962_058_725_45),
        ] {
            assert_close(evaluation.transformed()[index], expected);
        }
        assert_slice_close(
            evaluation.reduced(),
            &[
                0.503_470_533_610_979_9,
                0.449_522_369_864_382_43,
                0.354_308_186_433_816_06,
            ],
        );
        assert_slice_close(
            evaluation.shape(),
            &[
                0.461_320_042_089_157_9,
                0.540_957_680_606_666,
                0.703_241_499_457_699_8,
            ],
        );
        assert_slice_close(
            evaluation.objectives(),
            &[
                1.276_948_270_612_131_8,
                2.518_138_908_860_48,
                4.573_757_183_180_015,
            ],
        );
    }

    #[test]
    fn concave_anchors_generalize_across_objective_counts() {
        for objectives in [2, 3, 5] {
            let position_parameters = 2 * (objectives - 1);
            let problem = Wfg4::new(objectives, position_parameters, 2).unwrap();

            let center = problem
                .evaluate_normalized(&vec![S_MULTI_CENTER; problem.dimension()])
                .unwrap();
            let mut expected_center = vec![0.0; objectives];
            expected_center[objectives - 1] = 2.0 * objectives as f64;
            assert_slice_close(center.objectives(), &expected_center);

            let boundary = problem
                .evaluate_normalized(&vec![0.0; problem.dimension()])
                .unwrap();
            let mut expected_boundary = vec![1.0; objectives];
            expected_boundary[0] = 3.0;
            assert_slice_close(boundary.transformed(), &vec![1.0; problem.dimension()]);
            assert_slice_close(boundary.objectives(), &expected_boundary);
        }
    }

    #[test]
    fn asymmetric_groups_use_the_exact_declared_slices() {
        let problem = Wfg4::new(4, 6, 3).unwrap();
        let input = [0.05, 0.15, 0.25, 0.45, 0.65, 0.75, 0.85, 0.95, 0.35];
        let evaluation = problem.evaluate_normalized(&input).unwrap();
        let transformed: Vec<f64> = input.into_iter().map(s_multi).collect();
        let expected = [
            (transformed[0] + transformed[1]) / 2.0,
            (transformed[2] + transformed[3]) / 2.0,
            (transformed[4] + transformed[5]) / 2.0,
            (transformed[6] + transformed[7] + transformed[8]) / 3.0,
        ];
        assert_slice_close(evaluation.reduced(), &expected);
    }

    #[test]
    fn two_element_reduction_groups_are_bitwise_pair_swap_invariant() {
        let problem = Wfg4::new(3, 4, 2).unwrap();
        let input = [0.07, 0.21, 0.46, 0.72, 0.18, 0.91];
        let permuted = [0.21, 0.07, 0.72, 0.46, 0.91, 0.18];
        let first = problem.evaluate_normalized(&input).unwrap();
        let second = problem.evaluate_normalized(&permuted).unwrap();

        assert_eq!(first.reduced(), second.reduced());
        assert_eq!(first.shape(), second.shape());
        assert_eq!(first.objectives(), second.objectives());
    }

    #[test]
    fn wrong_center_and_same_objective_shape_mutants_are_observable() {
        let problem = Wfg4::new(3, 4, 2).unwrap();
        let center_input = [S_MULTI_CENTER; 6];
        let canonical_center = problem.evaluate_normalized(&center_input).unwrap();

        let wrong_center_distance = center_input[4..]
            .iter()
            .map(|&value| {
                let denominator = if value <= 0.5 { 1.0 } else { -1.0 };
                let ratio = (value - 0.5).abs() / denominator;
                let phase = (4.0 * S_MULTI_A + 2.0) * core::f64::consts::PI * (0.5 - ratio);
                let quadratic = 4.0 * S_MULTI_B * ratio * ratio;
                correct_to_01((1.0 + fs_math::det::cos(phase) + quadratic) / (S_MULTI_B + 2.0))
            })
            .sum::<f64>()
            / 2.0;
        assert!(wrong_center_distance > 0.05);

        let interior = problem
            .evaluate_normalized(&[0.07, 0.21, 0.46, 0.72, 0.18, 0.91])
            .unwrap();
        let convex_mutant_first = (1.0
            - fs_math::det::cos(interior.reduced()[0] * core::f64::consts::FRAC_PI_2))
            * (1.0 - fs_math::det::cos(interior.reduced()[1] * core::f64::consts::FRAC_PI_2));
        assert_ne!(convex_mutant_first.to_bits(), interior.shape()[0].to_bits());
        assert_close(canonical_center.reduced()[2], 0.0);
    }

    #[test]
    fn repeated_evaluation_is_bitwise_identical() {
        let problem = Wfg4::new(5, 8, 7).unwrap();
        let input: Vec<f64> = (0..problem.dimension())
            .map(|index| ((index * 37 + 19) % 101) as f64 / 100.0)
            .collect();
        let first = problem.evaluate_normalized(&input).unwrap();
        let second = problem.evaluate_normalized(&input).unwrap();

        assert_eq!(first.transformed(), second.transformed());
        assert_eq!(first.reduced(), second.reduced());
        assert_eq!(first.shape(), second.shape());
        assert_eq!(first.objectives(), second.objectives());
        assert_eq!(first.clone().into_objectives(), second.into_objectives());
    }
}
