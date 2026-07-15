//! SHEAF-ADJUDICATED THREE-WAY MERGE (addendum Proposal 10, bead
//! lmp4.12 — THE CROWN JEWEL; [M], behind the `sheaf-merge` feature
//! until its Gauntlet tier + kill metric are green): the sheaf
//! machinery built for watertightness supplies useful operators, but the
//! fixed-iteration decomposition is a guarded merge heuristic rather than an
//! H¹ certifier:
//!
//! - the union cochain receives a deterministic fixed-iteration gauge
//!   reconciliation → applied automatically only when the nominal post-gauge
//!   residual independently passes the supplied tolerance;
//! - a dominant decomposition remainder is retained only as a CANDIDATE
//!   merge conflict, localized to its supra-tolerance interface cells with
//!   both caller-supplied parent labels. It proves neither H¹, non-exactness,
//!   authenticated provenance, nor that no local repair exists;
//! - NON-GEOMETRIC assignment payloads (load cases, materials) currently
//!   refuse before decomposition. Without the base assignment map, even two
//!   different branch values do not prove a three-way conflict: either value
//!   may be unchanged from the base. No assignment is silently dropped;
//! - trust is CONDITIONED on the complex's spectral gap (Proposal 5,
//!   risk R5): merges in degraded-gap regions are flagged
//!   low-confidence.

use crate::sheaf_repair::{
    SheafRepairError, SheafSkeleton, SheafSkeletonError, apply_gauge, hodge_decompose,
};
use std::collections::{BTreeMap, BTreeSet};

/// One branch's edits: a mismatch cochain plus non-geometric keyed
/// assignments (load cases, materials — the typed layer's inputs).
#[derive(Debug, Clone)]
pub struct BranchState {
    /// Caller-supplied parent label (for example, a commit or agent label).
    /// This string is not an authenticated provenance identity.
    pub provenance: String,
    /// The branch's interface mismatch cochain.
    pub mismatch: Vec<f64>,
    /// Non-geometric keyed assignments.
    pub assignments: BTreeMap<String, String>,
}

/// A dominant fixed-iteration decomposition remainder that prevents automatic
/// merge at the requested tolerance.
///
/// This candidate proves neither non-exactness nor an H¹/topology claim.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateRemainderConflict {
    /// Supra-tolerance interface cells (patch pairs) with magnitudes,
    /// strongest first. This is not the complete mathematical support.
    pub cells: Vec<((usize, usize), f64)>,
    /// Caller-supplied parent labels (X, Y); not authenticated identities.
    pub parents: (String, String),
}

/// A pairwise assignment-difference candidate: the two branch payloads carry
/// different values for the same key. Without a base assignment map this does
/// not prove that both branches edited or that a three-way conflict exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeConflict {
    /// The colliding key.
    pub key: String,
    /// X's value.
    pub x_value: String,
    /// Y's value.
    pub y_value: String,
}

/// Heuristic merge-confidence diagnostic conditioned on the current spectral
/// gap proxy (Proposal 5 / R5). This is not certification authority.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Confidence {
    /// The measured proxy meets the supplied threshold. The fixed-iteration
    /// eigensolve does not certify convergence or separation.
    Normal {
        /// The algebraic-connectivity gap.
        gap: f64,
    },
    /// Degraded measured proxy: the candidate-remainder/gauge-fit split is
    /// especially fragile here — treat the merge as provisional.
    LowGap {
        /// The measured gap.
        gap: f64,
        /// The threshold it fell below.
        threshold: f64,
    },
}

/// Local nominal post-gauge threshold observation attached to auto-resolved
/// merges. The v1 fields are not a portable replay/content-bound receipt.
///
/// This is a deterministic `f64` threshold check, not an interval
/// watertightness certificate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MergeResidualReceipt {
    /// Worst post-reconciliation interface mismatch.
    pub post_norm: f64,
    /// The tolerance it passed.
    pub tol: f64,
}

/// The merge verdict.
#[derive(Debug, Clone, PartialEq)]
pub enum MergeOutcome {
    /// Inputs could not support an honest merge decision.
    Refused {
        /// Stable, actionable refusal reason.
        reason: &'static str,
    },
    /// A boundary fast path fired (X == Y, or one branch unchanged):
    /// no decomposition needed. This proves edit equality only and carries no
    /// post-state residual receipt.
    Trivial {
        /// Which fast path.
        reason: &'static str,
        /// The merged cochain.
        merged: Vec<f64>,
    },
    /// The deterministic gauge reconciliation reached the supplied nominal
    /// residual tolerance.
    Resolved {
        /// The reconciled cochain.
        merged: Vec<f64>,
        /// The gauge offsets applied.
        gauge: Vec<f64>,
        /// The checked local nominal residual observation.
        residual_receipt: MergeResidualReceipt,
        /// Gap-conditioned trust.
        confidence: Confidence,
    },
    /// Operational numerical merge conflicts. Candidate remainders make no
    /// topology claim. The assignment field is retained for the future
    /// base-aware API; the current entry point refuses all assignment payloads
    /// before producing this variant.
    Conflicted {
        /// Dominant fixed-iteration remainders.
        candidate_remainders: Vec<CandidateRemainderConflict>,
        /// Base-aware keyed-assignment conflicts (currently always empty).
        type_conflicts: Vec<TypeConflict>,
        /// Gap-conditioned trust.
        confidence: Confidence,
    },
    /// The Sev-0 guard: reconciliation could not reach the nominal residual
    /// tolerance (for example, a coexact residue) — escalated unresolved
    /// rather than reported as resolved.
    EscalatedUnresolved {
        /// The post-reconciliation norm that failed.
        post_norm: f64,
        /// The tolerance it failed against.
        tol: f64,
        /// Diagnostic squared-norm ratios from the fixed-iteration split; not
        /// a certified orthogonal energy partition.
        fractions: (f64, f64, f64),
        /// Gap-conditioned trust, retained on every numerical adjudication.
        confidence: Confidence,
    },
}

/// Invalid or refused input to the seeded candidate-remainder diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateRateError {
    /// A rate requires a non-empty trial set.
    ZeroTrials,
    /// Edit magnitudes must be finite and non-negative.
    InvalidEditScale,
    /// The supplied skeleton cannot support the incidence operators.
    MalformedSkeleton,
    /// A generated merge refused rather than producing an adjudication.
    TrialRefused {
        /// Stable refusal reason from [`three_way_merge`].
        reason: &'static str,
    },
}

fn norm_inf(v: &[f64]) -> f64 {
    if v.iter().any(|value| !value.is_finite()) {
        f64::INFINITY
    } else {
        v.iter().fold(0.0f64, |a, &b| a.max(b.abs()))
    }
}

fn skeleton_is_well_formed(skeleton: &SheafSkeleton) -> bool {
    if skeleton.n_patches == 0 {
        return false;
    }
    let mut edges = BTreeSet::new();
    for &(u, v) in &skeleton.edges {
        if u >= v || v >= skeleton.n_patches || !edges.insert((u, v)) {
            return false;
        }
    }
    let mut triangles = BTreeSet::new();
    for &(a, b, c) in &skeleton.triangles {
        if a >= b
            || b >= c
            || c >= skeleton.n_patches
            || !triangles.insert((a, b, c))
            || !edges.contains(&(a, b))
            || !edges.contains(&(b, c))
            || !edges.contains(&(a, c))
        {
            return false;
        }
    }
    true
}

/// Dense symmetric eigenvalues by cyclic Jacobi (small matrices —
/// patch counts, not DOF counts).
#[allow(clippy::needless_range_loop)] // rotations touch (k,p),(k,q) pairs
fn jacobi_eigenvalues(mut a: Vec<Vec<f64>>) -> Vec<f64> {
    let n = a.len();
    for _sweep in 0..64 {
        let mut off = 0.0f64;
        for p in 0..n {
            for q in (p + 1)..n {
                off += a[p][q] * a[p][q];
            }
        }
        if off < 1e-24 {
            break;
        }
        for p in 0..n {
            for q in (p + 1)..n {
                if a[p][q].abs() < 1e-300 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (akp, akq) = (a[k][p], a[k][q]);
                    a[k][p] = c * akp - s * akq;
                    a[k][q] = s * akp + c * akq;
                }
                for k in 0..n {
                    let (apk, aqk) = (a[p][k], a[q][k]);
                    a[p][k] = c * apk - s * aqk;
                    a[q][k] = s * apk + c * aqk;
                }
            }
        }
    }
    let mut ev: Vec<f64> = (0..n).map(|i| a[i][i]).collect();
    ev.sort_by(f64::total_cmp);
    ev
}

/// The spectral gap (algebraic connectivity λ₂) of the weighted
/// patch-adjacency Laplacian — the Proposal-5 trust signal. Weights
/// default to 1 (e.g. interface sample counts belong here).
#[must_use]
pub fn spectral_gap(skeleton: &SheafSkeleton, weights: Option<&[f64]>) -> f64 {
    if !skeleton_is_well_formed(skeleton)
        || weights.is_some_and(|values| {
            values.len() != skeleton.edges.len()
                || values
                    .iter()
                    .any(|weight| !weight.is_finite() || *weight < 0.0)
        })
    {
        return f64::NAN;
    }
    let n = skeleton.n_patches;
    let mut lap = vec![vec![0.0f64; n]; n];
    for (k, &(u, v)) in skeleton.edges.iter().enumerate() {
        let w = weights.map_or(1.0, |ws| ws[k]);
        lap[u][u] += w;
        lap[v][v] += w;
        lap[u][v] -= w;
        lap[v][u] -= w;
    }
    if lap.iter().flatten().any(|entry| !entry.is_finite()) {
        return f64::NAN;
    }
    let ev = jacobi_eigenvalues(lap);
    // Algebraic connectivity is the SECOND sorted eigenvalue. Searching for
    // the first positive eigenvalue would falsely call a disconnected graph
    // well connected by skipping its additional zero modes.
    let Some(&lambda_2) = ev.get(1) else {
        return 0.0;
    };
    if !lambda_2.is_finite() {
        lambda_2
    } else if lambda_2 <= 1e-9 {
        0.0
    } else {
        lambda_2
    }
}

/// Detect pairwise keyed-assignment differences. Without a base assignment
/// map these are only conflict candidates, not authoritative three-way
/// conflicts (either branch may equal the base). Coupling-graph legality of a
/// future merged assignment set is fs-iface's contract at its own layer.
#[must_use]
pub fn type_conflicts(x: &BranchState, y: &BranchState) -> Vec<TypeConflict> {
    let mut out = Vec::new();
    for (k, xv) in &x.assignments {
        if let Some(yv) = y.assignments.get(k)
            && xv != yv
        {
            out.push(TypeConflict {
                key: k.clone(),
                x_value: xv.clone(),
                y_value: yv.clone(),
            });
        }
    }
    out
}

/// The three-way merge. `base` is the common ancestor's mismatch
/// cochain; `tol` is the nominal residual tolerance the reconciled state
/// must satisfy to be reported resolved; `gap_threshold` conditions trust.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn three_way_merge(
    skeleton: &SheafSkeleton,
    base: &[f64],
    x: &BranchState,
    y: &BranchState,
    weights: Option<&[f64]>,
    tol: f64,
    gap_threshold: f64,
) -> MergeOutcome {
    if !skeleton_is_well_formed(skeleton) {
        return MergeOutcome::Refused {
            reason: "malformed sheaf skeleton",
        };
    }
    if base.len() != skeleton.edges.len()
        || x.mismatch.len() != base.len()
        || y.mismatch.len() != base.len()
    {
        return MergeOutcome::Refused {
            reason: "cochain length mismatch",
        };
    }
    if !tol.is_finite() || tol < 0.0 || !gap_threshold.is_finite() || gap_threshold <= 0.0 {
        return MergeOutcome::Refused {
            reason: "residual tolerance must be finite non-negative and gap threshold finite positive",
        };
    }
    if base
        .iter()
        .chain(&x.mismatch)
        .chain(&y.mismatch)
        .any(|value| !value.is_finite())
    {
        return MergeOutcome::Refused {
            reason: "cochains must be finite",
        };
    }
    if weights.is_some_and(|values| {
        values.len() != skeleton.edges.len()
            || values
                .iter()
                .any(|weight| !weight.is_finite() || *weight < 0.0)
    }) {
        return MergeOutcome::Refused {
            reason: "weights must match edges and be finite non-negative values",
        };
    }
    // This v1 API carries only the base mismatch cochain, not the base
    // assignment map, and none of its success variants can return merged
    // assignments. Even pairwise-different branch values do not prove a
    // three-way conflict because either may equal the unknown base. Refuse all
    // assignment payloads rather than inventing conflict authority or dropping
    // state.
    if !x.assignments.is_empty() || !y.assignments.is_empty() {
        return MergeOutcome::Refused {
            reason: "base-aware assignment merge is not represented",
        };
    }
    // Boundary fast paths: no decomposition, no false ceremony.
    // Compare in place so an O(1) result does not allocate whole bit-vectors.
    let same_bits = |left: &[f64], right: &[f64]| {
        left.len() == right.len()
            && left
                .iter()
                .zip(right)
                .all(|(a, b)| a.to_bits() == b.to_bits())
    };
    if same_bits(&x.mismatch, &y.mismatch) {
        return MergeOutcome::Trivial {
            reason: "branches identical",
            merged: x.mismatch.clone(),
        };
    }
    if same_bits(&x.mismatch, base) {
        return MergeOutcome::Trivial {
            reason: "X unchanged from base",
            merged: y.mismatch.clone(),
        };
    }
    if same_bits(&y.mismatch, base) {
        return MergeOutcome::Trivial {
            reason: "Y unchanged from base",
            merged: x.mismatch.clone(),
        };
    }
    // Nontrivial numerical adjudication is the first path that consumes gap
    // confidence; trivial equality must not pay for the dense Jacobi proxy.
    let gap = spectral_gap(skeleton, weights);
    if !gap.is_finite() {
        return MergeOutcome::Refused {
            reason: "spectral-gap arithmetic is non-finite",
        };
    }
    let confidence = if gap < gap_threshold {
        Confidence::LowGap {
            gap,
            threshold: gap_threshold,
        }
    } else {
        Confidence::Normal { gap }
    };
    // The naive union of edits at the cochain level: X + Y − B.
    let union: Vec<f64> = x
        .mismatch
        .iter()
        .zip(&y.mismatch)
        .zip(base)
        .map(|((a, b), c)| a + b - c)
        .collect();
    if union.iter().any(|value| !value.is_finite()) {
        return MergeOutcome::Refused {
            reason: "merged cochain arithmetic is non-finite",
        };
    }
    let split = match hodge_decompose(skeleton, &union) {
        Ok(split) => split,
        Err(
            SheafRepairError::NumericalOverflow { .. }
            | SheafRepairError::Skeleton(SheafSkeletonError::NumericalOverflow { .. }),
        ) => {
            return MergeOutcome::Refused {
                reason: "decomposition arithmetic is non-finite",
            };
        }
        Err(_) => {
            return MergeOutcome::Refused {
                reason: "decomposition refused malformed, non-finite, or exhausted input",
            };
        }
    };
    if split
        .exact
        .iter()
        .chain(&split.potential)
        .chain(&split.coexact)
        .chain(&split.harmonic)
        .any(|value| !value.is_finite())
        || [split.fractions.0, split.fractions.1, split.fractions.2]
            .into_iter()
            .any(|value| !value.is_finite())
    {
        return MergeOutcome::Refused {
            reason: "decomposition arithmetic is non-finite",
        };
    }
    // Deterministic fixed-iteration gauge reconciliation. Resolution is
    // authorized only by the nominal residual check below, not by a
    // cohomology classification.
    let merged = match apply_gauge(skeleton, &union, &split.potential) {
        Ok(merged) => merged,
        Err(
            SheafRepairError::NumericalOverflow { .. }
            | SheafRepairError::Skeleton(SheafSkeletonError::NumericalOverflow { .. }),
        ) => {
            return MergeOutcome::Refused {
                reason: "post-gauge arithmetic is non-finite",
            };
        }
        Err(_) => {
            return MergeOutcome::Refused {
                reason: "post-gauge construction refused malformed, non-finite, or exhausted input",
            };
        }
    };
    if merged.iter().any(|value| !value.is_finite()) {
        return MergeOutcome::Refused {
            reason: "post-gauge arithmetic is non-finite",
        };
    }
    // Sev-0 post-state check: nominal values only. This is not the interval
    // watertightness certificate from the base sheaf path.
    let post_norm = norm_inf(&merged);
    if post_norm <= tol {
        return MergeOutcome::Resolved {
            merged,
            gauge: split.potential,
            residual_receipt: MergeResidualReceipt { post_norm, tol },
            confidence,
        };
    }
    // The nominal residual check failed. A dominant fixed-iteration remainder
    // is retained as an operational merge-conflict candidate only; anything
    // else (for example a coexact circulation) escalates unresolved.
    let harmonic_norm = norm_inf(&split.harmonic);
    let coexact_norm = norm_inf(&split.coexact);
    if harmonic_norm > tol && harmonic_norm >= coexact_norm {
        let mut cells: Vec<((usize, usize), f64)> = skeleton
            .edges
            .iter()
            .zip(&split.harmonic)
            .filter(|(_, h)| h.abs() > tol)
            .map(|(&e, &h)| (e, h.abs()))
            .collect();
        cells.sort_by(|a, b| b.1.total_cmp(&a.1));
        return MergeOutcome::Conflicted {
            candidate_remainders: vec![CandidateRemainderConflict {
                cells,
                parents: (x.provenance.clone(), y.provenance.clone()),
            }],
            type_conflicts: Vec::new(),
            confidence,
        };
    }
    MergeOutcome::EscalatedUnresolved {
        post_norm,
        tol,
        fractions: split.fractions,
        confidence,
    }
}

/// Run seeded random three-way merges and measure the candidate-remainder
/// conflict rate.
///
/// This is one diagnostic input to Proposal 10's broader kill criterion. It
/// deliberately counts only [`CandidateRemainderConflict`] outcomes; callers
/// must separately retain escalations, refusals, and type conflicts before
/// claiming the full fraction of merges that could not auto-reconcile. It does
/// not count certified H¹ classes.
pub fn candidate_remainder_conflict_rate(
    skeleton: &SheafSkeleton,
    trials: usize,
    edit_scale: f64,
    seed: u64,
) -> Result<f64, CandidateRateError> {
    if trials == 0 {
        return Err(CandidateRateError::ZeroTrials);
    }
    if !edit_scale.is_finite() || edit_scale < 0.0 {
        return Err(CandidateRateError::InvalidEditScale);
    }
    if !skeleton_is_well_formed(skeleton) {
        return Err(CandidateRateError::MalformedSkeleton);
    }
    let mut state = seed;
    let mut lcg = move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((state >> 11) as f64) / (1u64 << 53) as f64 - 0.5
    };
    let n_edges = skeleton.edges.len();
    let n_patches = skeleton.n_patches;
    let mut conflicts = 0usize;
    for _ in 0..trials {
        // Seeded synthetic edits: each branch re-gauges random patch values
        // (coboundary-style work), then receives small interface noise.
        let mut edit = |scale: f64| -> Result<Vec<f64>, CandidateRateError> {
            let offsets: Vec<f64> = (0..n_patches).map(|_| scale * lcg()).collect();
            let mut m = skeleton
                .d0(&offsets)
                .map_err(|_| CandidateRateError::TrialRefused {
                    reason: "seeded edit incidence refused",
                })?;
            // A small amount of independent interface noise.
            for v in &mut m {
                *v += 0.01 * scale * lcg();
                if !v.is_finite() {
                    return Err(CandidateRateError::TrialRefused {
                        reason: "seeded edit arithmetic is non-finite",
                    });
                }
            }
            Ok(m)
        };
        let base = vec![0.0f64; n_edges];
        let x = BranchState {
            provenance: "trial-x".to_string(),
            mismatch: edit(edit_scale)?,
            assignments: BTreeMap::new(),
        };
        let y = BranchState {
            provenance: "trial-y".to_string(),
            mismatch: edit(edit_scale)?,
            assignments: BTreeMap::new(),
        };
        let out = three_way_merge(skeleton, &base, &x, &y, None, 0.05 * edit_scale, 1e-6);
        match out {
            MergeOutcome::Conflicted {
                candidate_remainders,
                ..
            } if !candidate_remainders.is_empty() => conflicts += 1,
            MergeOutcome::Refused { reason } => {
                return Err(CandidateRateError::TrialRefused { reason });
            }
            _ => {}
        }
    }
    #[allow(clippy::cast_precision_loss)]
    {
        Ok(conflicts as f64 / trials as f64)
    }
}
