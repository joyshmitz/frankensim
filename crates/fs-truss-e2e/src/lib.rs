//! fs-truss-e2e — TrussPath: a deterministic truss iterate with an advisory,
//! endpoint-checked load path. Layer: L4 (ASCENT).
//!
//! # The campaign
//!
//! A structural optimizer returns member sizes. This returns a deterministic
//! truss iterate plus explicit numerical diagnostics and an advisory account of
//! how the load travels through it, composing crates never designed to meet:
//!
//! - **Ground-structure optimization** ([`fs_truss`]): a Michell ground
//!   structure (all admissible candidate bars) is iterated toward minimum
//!   volume and equilibrium by a first-order PDHG solver, which emits a relative
//!   primal-dual gap and equilibrium-residual diagnostic. The approximate
//!   primal is not exactly feasible, so the returned volume is not a certified
//!   optimum bound.
//! - **The critical load path** ([`fs_tropical`]): the active bars form a
//!   directed acyclic graph oriented by distance-to-support; a MAX-PLUS
//!   (tropical) critical-path computation finds a connected chain of active
//!   bars from the load node to a support, and names a bottleneck only when the
//!   rounded task graph has a unique heaviest chain.
//! - **Honest colors** ([`fs_evidence`]): PDHG convergence remains `Estimated`
//!   until an exactly feasible primal upper bound is constructed. The load path
//!   also remains `Estimated` until active-set membership and member volumes
//!   carry interval separation through the tropical analysis.
//!
//! Deterministic; no dependencies beyond the composed crates.

use fs_evidence::Color;
use fs_tropical::{MAX_TASK_DAG_EDGES, MAX_TASK_DAG_NODES, TaskDag, TropicalError};
use fs_truss::{GroundRules, GroundStructure, LayoutLp, PdhgError, PdhgSettings};
use std::collections::BTreeSet;

/// Maximum grid nodes admitted to the cubic ground-structure constructor.
pub const MAX_TRUSS_CAMPAIGN_NODES: usize = 256;
/// Maximum cubic node-triplet checks admitted before ground construction.
pub const MAX_TRUSS_GROUND_CHECKS: usize = 262_144;
/// Maximum candidate members retained for one campaign solve.
pub const MAX_TRUSS_CANDIDATE_MEMBERS: usize = 512;
/// Maximum conservative scalar operations admitted to the fixed PDHG solve.
pub const MAX_TRUSS_PDHG_SCALAR_STEPS: usize = 1 << 27;

const TRUSS_PDHG_MAX_ITERS: usize = 60_000;
const TRUSS_PDHG_CHECK_EVERY: usize = 500;

/// Structured TrussPath refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrussError {
    /// One public campaign field is outside its bounded numerical domain.
    InvalidInput {
        /// Stable field name.
        field: &'static str,
        /// Stable domain requirement.
        requirement: &'static str,
    },
    /// The bounded grid produced no admissible candidate member.
    NoCandidateMembers,
    /// The thresholded active graph has no connected multi-bar load-to-support path.
    NoCompleteLoadPath,
    /// A deterministic construction or solver work budget was exceeded.
    WorkBudget {
        /// Bounded resource.
        resource: &'static str,
        /// Configured maximum.
        limit: usize,
        /// Observed request, saturated on arithmetic overflow.
        observed: usize,
    },
    /// Solver-derived path data violated its checked domain.
    InvalidLoadPath {
        /// Stable diagnosis.
        reason: &'static str,
    },
    /// The checked PDHG solver refused its controls or warm-start state.
    Solver(PdhgError),
    /// Tropical analysis refused solver-derived task data.
    Tropical(TropicalError),
}

impl core::fmt::Display for TrussError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidInput { field, requirement } => {
                write!(formatter, "truss campaign {field} {requirement}")
            }
            Self::NoCandidateMembers => {
                formatter.write_str("truss campaign has no candidate members")
            }
            Self::NoCompleteLoadPath => formatter.write_str(
                "truss campaign has no connected multi-bar active path from load to support",
            ),
            Self::WorkBudget {
                resource,
                limit,
                observed,
            } => write!(
                formatter,
                "truss campaign {resource} work {observed} exceeds limit {limit}"
            ),
            Self::InvalidLoadPath { reason } => {
                write!(formatter, "truss load-path input {reason}")
            }
            Self::Solver(error) => write!(formatter, "truss solver refused: {error}"),
            Self::Tropical(error) => write!(formatter, "truss load-path analysis refused: {error}"),
        }
    }
}

impl std::error::Error for TrussError {}

impl From<TropicalError> for TrussError {
    fn from(value: TropicalError) -> Self {
        Self::Tropical(value)
    }
}

impl From<PdhgError> for TrussError {
    fn from(value: PdhgError) -> Self {
        Self::Solver(value)
    }
}

/// The campaign report.
#[derive(Debug, Clone)]
pub struct TrussReport {
    /// Candidate bars in the ground structure.
    pub num_members: usize,
    /// Bars carrying meaningful force in the returned iterate.
    pub num_active: usize,
    /// Approximate primal volume of the returned PDHG iterate.
    pub total_volume: f64,
    /// Relative primal/dual objective separation diagnostic from PDHG.
    pub gap: f64,
    /// The equilibrium residual `‖Ax−b‖/‖b‖`.
    pub eq_residual: f64,
    /// PDHG iterations run.
    pub iters: usize,
    /// Did the iterative solver meet its gap and equilibrium-residual targets?
    pub solver_converged: bool,
    /// The advisory load path as original bar indices (load → support).
    pub critical_path: Vec<usize>,
    /// The volume carried by the critical path (tropical makespan).
    pub critical_path_volume: f64,
    /// The uniquely heaviest bar on a unique advisory path (original index).
    pub bottleneck_member: Option<usize>,
    /// The optimality color (currently `Estimated`; see no-claim boundary).
    pub optimality_color: Color,
    /// Load-path evidence (currently `Estimated`; see no-claim boundary).
    pub load_path_color: Color,
}

/// Checked advisory load-path analysis shared by native and browser campaigns.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadPathAnalysis {
    /// Original member indices, ordered from the load node to a support.
    pub members: Vec<usize>,
    /// Rounded sum of the selected member weights.
    pub weight: f64,
    /// A uniquely heaviest member when both path and weight ranking are unique.
    pub bottleneck_member: Option<usize>,
    /// Whether directed rounding separates the selected path from all rivals.
    pub path_is_unique: bool,
}

#[derive(Debug, Clone, Copy)]
struct OrientedMember {
    original: usize,
    from: usize,
    to: usize,
    weight: f64,
}

/// Extract a connected, strictly support-ward path from thresholded members.
///
/// Every retained task is reachable from `load_node` and can reach one of the
/// indexed supports. This prevents a heavy disconnected component or an
/// interior suffix from being mislabeled as a load-to-support chain.
///
/// # Errors
/// Refuses malformed indices, non-finite geometry/weights, duplicate active or
/// support identities, bounded-resource excess, and graphs without a connected
/// path of at least two bars.
#[allow(clippy::too_many_lines)] // One bounded load-to-support graph witness and verifier.
pub fn analyze_load_path(
    nodes: &[[f64; 2]],
    members: &[(usize, usize)],
    active: &[usize],
    weights: &[f64],
    load_node: usize,
    support_nodes: &[usize],
) -> Result<LoadPathAnalysis, TrussError> {
    if nodes.is_empty() || nodes.len() > MAX_TRUSS_CAMPAIGN_NODES {
        return Err(TrussError::InvalidLoadPath {
            reason: "node count must be within the campaign bound",
        });
    }
    if members.len() != weights.len() {
        return Err(TrussError::InvalidLoadPath {
            reason: "member and weight counts must match",
        });
    }
    if load_node >= nodes.len() {
        return Err(TrussError::InvalidLoadPath {
            reason: "load node is out of range",
        });
    }
    if support_nodes.is_empty() || support_nodes.len() > nodes.len() {
        return Err(TrussError::InvalidLoadPath {
            reason: "support count must be within 1..=node count",
        });
    }
    if active.len() > MAX_TASK_DAG_NODES {
        return Err(TrussError::WorkBudget {
            resource: "active member count",
            limit: MAX_TASK_DAG_NODES,
            observed: active.len(),
        });
    }
    if nodes
        .iter()
        .flatten()
        .any(|coordinate| !coordinate.is_finite())
    {
        return Err(TrussError::InvalidLoadPath {
            reason: "node coordinates must be finite",
        });
    }

    let supports: BTreeSet<usize> = support_nodes.iter().copied().collect();
    if supports.len() != support_nodes.len()
        || supports.iter().any(|&node| node >= nodes.len())
        || supports.contains(&load_node)
    {
        return Err(TrussError::InvalidLoadPath {
            reason: "supports must be unique, in range, and exclude the load node",
        });
    }
    let active_set: BTreeSet<usize> = active.iter().copied().collect();
    if active_set.len() != active.len() {
        return Err(TrussError::InvalidLoadPath {
            reason: "active member identities must be unique",
        });
    }

    let support_points: Vec<[f64; 2]> = supports.iter().map(|&index| nodes[index]).collect();
    let distance: Vec<f64> = nodes
        .iter()
        .map(|point| {
            support_points
                .iter()
                .map(|support| (point[0] - support[0]).hypot(point[1] - support[1]))
                .fold(f64::INFINITY, f64::min)
        })
        .collect();
    if distance.iter().any(|value| !value.is_finite()) {
        return Err(TrussError::InvalidLoadPath {
            reason: "distance-to-support must be finite",
        });
    }

    let mut oriented = Vec::with_capacity(active.len());
    for &member in active {
        let Some(&(a, b)) = members.get(member) else {
            return Err(TrussError::InvalidLoadPath {
                reason: "active member is out of range",
            });
        };
        if a >= nodes.len() || b >= nodes.len() || a == b {
            return Err(TrussError::InvalidLoadPath {
                reason: "active member endpoints must be distinct and in range",
            });
        }
        let weight = weights[member];
        if !weight.is_finite() || weight <= 0.0 {
            return Err(TrussError::InvalidLoadPath {
                reason: "active member weights must be finite and positive",
            });
        }
        let (from, to) = if distance[a] > distance[b] {
            (a, b)
        } else if distance[b] > distance[a] {
            (b, a)
        } else {
            // Equal-distance members do not make strictly support-ward progress.
            continue;
        };
        oriented.push(OrientedMember {
            original: member,
            from,
            to,
            weight,
        });
    }

    let mut reachable = vec![false; nodes.len()];
    reachable[load_node] = true;
    loop {
        let mut changed = false;
        for member in &oriented {
            if reachable[member.from] && !reachable[member.to] {
                reachable[member.to] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut reaches_support = vec![false; nodes.len()];
    for &support in &supports {
        reaches_support[support] = true;
    }
    loop {
        let mut changed = false;
        for member in oriented.iter().rev() {
            if reaches_support[member.to] && !reaches_support[member.from] {
                reaches_support[member.from] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    oriented.retain(|member| reachable[member.from] && reaches_support[member.to]);
    oriented.sort_by(|a, b| {
        distance[b.from]
            .total_cmp(&distance[a.from])
            .then(a.original.cmp(&b.original))
    });
    if oriented.len() < 2 {
        return Err(TrussError::NoCompleteLoadPath);
    }

    let mut starts_at = vec![Vec::new(); nodes.len()];
    for (index, member) in oriented.iter().enumerate() {
        starts_at[member.from].push(index);
    }
    let mut dag = TaskDag::new(oriented.iter().map(|member| member.weight).collect());
    let mut edge_count = 0usize;
    for (index, member) in oriented.iter().enumerate() {
        for &successor in &starts_at[member.to] {
            edge_count = edge_count.checked_add(1).ok_or(TrussError::WorkBudget {
                resource: "load-path edge count",
                limit: MAX_TASK_DAG_EDGES,
                observed: usize::MAX,
            })?;
            if edge_count > MAX_TASK_DAG_EDGES {
                return Err(TrussError::WorkBudget {
                    resource: "load-path edge count",
                    limit: MAX_TASK_DAG_EDGES,
                    observed: edge_count,
                });
            }
            dag = dag.with_edge(index, successor);
        }
    }

    let critical = dag.critical_path()?;
    let path: Vec<OrientedMember> = critical.path.iter().map(|&index| oriented[index]).collect();
    if path.len() < 2
        || path.first().is_none_or(|member| member.from != load_node)
        || path
            .last()
            .is_none_or(|member| !supports.contains(&member.to))
        || path.windows(2).any(|pair| pair[0].to != pair[1].from)
    {
        return Err(TrussError::NoCompleteLoadPath);
    }
    let bottleneck_member = dag.bottleneck()?.map(|index| oriented[index].original);
    Ok(LoadPathAnalysis {
        members: path.iter().map(|member| member.original).collect(),
        weight: critical.makespan,
        bottleneck_member,
        path_is_unique: critical.path_is_unique,
    })
}

/// Run the TrussPath campaign: a cantilever on an `nx×ny` grid over `[0,w]×[0,h]`,
/// left edge supported, a unit downward load at the free bottom corner.
///
/// # Errors
/// Returns a structured refusal for invalid/unbounded grid parameters, an empty
/// candidate set, an excessive construction/solver budget, or invalid
/// solver-derived path data.
#[allow(clippy::too_many_lines)] // One bounded campaign, diagnostics, and evidence report pipeline.
pub fn run_campaign(
    nx: usize,
    ny: usize,
    w: f64,
    h: f64,
    gap_tol: f64,
) -> Result<TrussReport, TrussError> {
    if nx < 2 || ny < 2 {
        return Err(TrussError::InvalidInput {
            field: "grid dimensions",
            requirement: "must each be at least two",
        });
    }
    let node_count = nx.checked_mul(ny).ok_or(TrussError::InvalidInput {
        field: "grid node count",
        requirement: "must fit usize and the deterministic node budget",
    })?;
    if node_count > MAX_TRUSS_CAMPAIGN_NODES {
        return Err(TrussError::InvalidInput {
            field: "grid node count",
            requirement: "exceeds the 256-node deterministic work budget",
        });
    }
    let ground_checks = node_count
        .checked_mul(node_count)
        .and_then(|square| square.checked_mul(node_count))
        .ok_or(TrussError::WorkBudget {
            resource: "ground-structure triplet checks",
            limit: MAX_TRUSS_GROUND_CHECKS,
            observed: usize::MAX,
        })?;
    if ground_checks > MAX_TRUSS_GROUND_CHECKS {
        return Err(TrussError::WorkBudget {
            resource: "ground-structure triplet checks",
            limit: MAX_TRUSS_GROUND_CHECKS,
            observed: ground_checks,
        });
    }
    let max_extent = f64::MAX.sqrt() * 0.5;
    if !w.is_finite() || w <= 0.0 || w > max_extent {
        return Err(TrussError::InvalidInput {
            field: "width",
            requirement: "must be finite, positive, and safe for squared geometry",
        });
    }
    if !h.is_finite() || h <= 0.0 || h > max_extent {
        return Err(TrussError::InvalidInput {
            field: "height",
            requirement: "must be finite, positive, and safe for squared geometry",
        });
    }
    if !gap_tol.is_finite() || gap_tol <= 0.0 || gap_tol > 1.0 {
        return Err(TrussError::InvalidInput {
            field: "gap tolerance",
            requirement: "must be finite and in 0 < gap_tol <= 1",
        });
    }
    let rules = GroundRules {
        min_len: 0.1,
        max_len: w.hypot(h) / 1.5,
        angles: Vec::new(),
        angle_tol: 1e-6,
    };
    let gs = GroundStructure::grid(nx, ny, w, h, &rules);
    let m = gs.members.len();
    if m == 0 {
        return Err(TrussError::NoCandidateMembers);
    }
    if m > MAX_TRUSS_CANDIDATE_MEMBERS {
        return Err(TrussError::WorkBudget {
            resource: "candidate member count",
            limit: MAX_TRUSS_CANDIDATE_MEMBERS,
            observed: m,
        });
    }

    // Left edge supported; unit downward load at the free bottom-right node.
    let support_nodes: Vec<usize> = (0..ny).map(|row| row * nx).collect();
    let support_set: BTreeSet<usize> = support_nodes.iter().copied().collect();
    let supported = |node: usize, _comp: usize| support_set.contains(&node);
    let load_node = nx - 1;
    let loads = |node: usize| {
        if node == load_node {
            [0.0, -1.0]
        } else {
            [0.0, 0.0]
        }
    };

    let lp = LayoutLp::assemble(&gs, &supported, &loads, 1.0);
    if lp.b.iter().all(|load| *load == 0.0) {
        return Err(TrussError::InvalidInput {
            field: "load degree of freedom",
            requirement: "must survive support elimination",
        });
    }
    // Two sparse multiply-add passes (4*nnz scalar arithmetic), the projected
    // primal update plus extrapolation (6*nvar), and the dual update (3*nrow).
    // Diagnostic checkpoints add two more SpMVs and bounded reductions.
    let per_iteration =
        lp.a.nnz()
            .checked_mul(4)
            .and_then(|steps| {
                lp.c.len()
                    .checked_mul(10)
                    .and_then(|vector_steps| steps.checked_add(vector_steps))
            })
            .and_then(|steps| {
                lp.b.len()
                    .checked_mul(3)
                    .and_then(|row_steps| steps.checked_add(row_steps))
            })
            .ok_or(TrussError::WorkBudget {
                resource: "PDHG scalar steps",
                limit: MAX_TRUSS_PDHG_SCALAR_STEPS,
                observed: usize::MAX,
            })?;
    let per_diagnostic =
        lp.a.nnz()
            .checked_mul(4)
            .and_then(|steps| {
                lp.c.len()
                    .checked_mul(6)
                    .and_then(|vector_steps| steps.checked_add(vector_steps))
            })
            .and_then(|steps| {
                lp.b.len()
                    .checked_mul(7)
                    .and_then(|row_steps| steps.checked_add(row_steps))
            })
            .and_then(|steps| steps.checked_add(16))
            .ok_or(TrussError::WorkBudget {
                resource: "PDHG scalar steps",
                limit: MAX_TRUSS_PDHG_SCALAR_STEPS,
                observed: usize::MAX,
            })?;
    let iteration_steps =
        per_iteration
            .checked_mul(TRUSS_PDHG_MAX_ITERS)
            .ok_or(TrussError::WorkBudget {
                resource: "PDHG scalar steps",
                limit: MAX_TRUSS_PDHG_SCALAR_STEPS,
                observed: usize::MAX,
            })?;
    let diagnostic_steps = per_diagnostic
        .checked_mul(TRUSS_PDHG_MAX_ITERS.div_ceil(TRUSS_PDHG_CHECK_EVERY))
        .ok_or(TrussError::WorkBudget {
            resource: "PDHG scalar steps",
            limit: MAX_TRUSS_PDHG_SCALAR_STEPS,
            observed: usize::MAX,
        })?;
    let solver_steps =
        iteration_steps
            .checked_add(diagnostic_steps)
            .ok_or(TrussError::WorkBudget {
                resource: "PDHG scalar steps",
                limit: MAX_TRUSS_PDHG_SCALAR_STEPS,
                observed: usize::MAX,
            })?;
    if solver_steps > MAX_TRUSS_PDHG_SCALAR_STEPS {
        return Err(TrussError::WorkBudget {
            resource: "PDHG scalar steps",
            limit: MAX_TRUSS_PDHG_SCALAR_STEPS,
            observed: solver_steps,
        });
    }
    let settings = PdhgSettings {
        max_iters: TRUSS_PDHG_MAX_ITERS,
        gap_tol,
        check_every: TRUSS_PDHG_CHECK_EVERY,
    };
    let (x, _y, report) = lp.solve(None, None, settings)?;

    // Member force (q⁺ − q⁻) and material volume (both split costs).
    let force = |k: usize| x[k] - x[m + k];
    let volume = |k: usize| lp.c[k] * x[k] + lp.c[m + k] * x[m + k];
    let max_force = (0..m).map(|k| force(k).abs()).fold(0.0, f64::max);
    let active_tol = 1e-3 * max_force.max(1e-12);

    let active: Vec<usize> = (0..m).filter(|&k| force(k).abs() > active_tol).collect();
    let num_active = active.len();

    let volumes: Vec<f64> = (0..m).map(volume).collect();
    let load_path = analyze_load_path(
        &gs.nodes,
        &gs.members,
        &active,
        &volumes,
        load_node,
        &support_nodes,
    )?;
    let load_path_color = Color::Estimated {
        estimator: if load_path.path_is_unique {
            "pdhg-thresholded-tropical-load-path-v1"
        } else {
            "ambiguous-pdhg-thresholded-tropical-load-path-v1"
        }
        .to_string(),
        // No interval active-set or product enclosure exists yet.
        dispersion: f64::INFINITY,
    };

    let solver_converged = report.gap.is_finite()
        && report.eq_residual.is_finite()
        && report.gap >= 0.0
        && report.eq_residual >= 0.0
        && report.gap < gap_tol
        && report.eq_residual < gap_tol;
    let optimality_color = Color::Estimated {
        estimator: "pdhg-gap-with-equilibrium-residual-v1".to_string(),
        dispersion: if report.gap.is_finite()
            && report.eq_residual.is_finite()
            && report.gap >= 0.0
            && report.eq_residual >= 0.0
        {
            report.gap.max(report.eq_residual)
        } else {
            f64::INFINITY
        },
    };

    Ok(TrussReport {
        num_members: m,
        num_active,
        total_volume: report.volume,
        gap: report.gap,
        eq_residual: report.eq_residual,
        iters: report.iters,
        solver_converged,
        critical_path: load_path.members,
        critical_path_volume: load_path.weight,
        bottleneck_member: load_path.bottleneck_member,
        optimality_color,
        load_path_color,
    })
}
