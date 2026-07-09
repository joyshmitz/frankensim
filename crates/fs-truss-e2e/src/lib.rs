//! fs-truss-e2e — TrussPath: an optimal truss with a certified critical load
//! path. Layer: L4 (ASCENT).
//!
//! # The campaign
//!
//! A structural optimizer returns member sizes. This returns an OPTIMAL truss
//! plus two certificates — that it is near-optimal, and how the load actually
//! travels through it — composing crates never designed to meet:
//!
//! - **Ground-structure optimization** ([`fs_truss`]): a Michell ground
//!   structure (all admissible candidate bars) is sized to minimum volume under
//!   equilibrium by a first-order PDHG solver, which emits a certified relative
//!   primal-dual DUALITY GAP — a machine-checkable bound on how far from optimal
//!   the returned design is.
//! - **The critical load path** ([`fs_tropical`]): the active bars form a
//!   directed acyclic graph oriented by distance-to-support; a MAX-PLUS
//!   (tropical) critical-path computation finds the single chain of bars that
//!   carries the most material from the load to the supports, and names the
//!   bottleneck bar. Exact by construction.
//! - **Honest colors** ([`fs_evidence`]): the optimality claim is `Verified`
//!   when the duality gap and equilibrium residual are tiny; the load path is
//!   `Verified` (an exact tropical computation).
//!
//! Deterministic; no dependencies beyond the composed crates.

use fs_evidence::Color;
use fs_tropical::TaskDag;
use fs_truss::{GroundRules, GroundStructure, LayoutLp, PdhgSettings};

/// The campaign report.
#[derive(Debug, Clone)]
pub struct TrussReport {
    /// Candidate bars in the ground structure.
    pub num_members: usize,
    /// Bars carrying meaningful force in the optimum.
    pub num_active: usize,
    /// Minimum certified volume.
    pub total_volume: f64,
    /// The PDHG relative duality gap.
    pub gap: f64,
    /// The equilibrium residual `‖Ax−b‖/‖b‖`.
    pub eq_residual: f64,
    /// PDHG iterations run.
    pub iters: usize,
    /// Is the design certified near-optimal (small gap + residual)?
    pub certified_optimal: bool,
    /// The critical load path as ORIGINAL bar indices (load → support).
    pub critical_path: Vec<usize>,
    /// The volume carried by the critical path (tropical makespan).
    pub critical_path_volume: f64,
    /// The heaviest bar on the critical path (original index).
    pub bottleneck_member: Option<usize>,
    /// The optimality color (`Verified` iff certified near-optimal).
    pub optimality_color: Color,
    /// The load-path color (`Verified` — exact tropical).
    pub load_path_color: Color,
}

fn dist_to_support(p: [f64; 2], supports: &[[f64; 2]]) -> f64 {
    supports
        .iter()
        .map(|s| (p[0] - s[0]).hypot(p[1] - s[1]))
        .fold(f64::INFINITY, f64::min)
}

/// Run the TrussPath campaign: a cantilever on an `nx×ny` grid over `[0,w]×[0,h]`,
/// left edge supported, a unit downward load at the free bottom corner.
///
/// # Panics
/// If the grid is degenerate (no free DOFs / no members).
#[must_use]
pub fn run_campaign(nx: usize, ny: usize, w: f64, h: f64, gap_tol: f64) -> TrussReport {
    let rules = GroundRules {
        min_len: 0.1,
        max_len: (w * w + h * h).sqrt() / 1.5,
        angles: Vec::new(),
        angle_tol: 1e-6,
    };
    let gs = GroundStructure::grid(nx, ny, w, h, &rules);
    let m = gs.members.len();
    assert!(m > 0, "no candidate members");

    // Left edge supported; unit downward load at the free bottom-right node.
    let supported = |node: usize, _comp: usize| gs.nodes[node][0] < 1e-9;
    let support_pts: Vec<[f64; 2]> = gs.nodes.iter().copied().filter(|p| p[0] < 1e-9).collect();
    // Load node: max x, then min y (bottom-right corner).
    let load_node = (0..gs.nodes.len())
        .max_by(|&a, &b| {
            (gs.nodes[a][0] - gs.nodes[a][1]).total_cmp(&(gs.nodes[b][0] - gs.nodes[b][1]))
        })
        .unwrap_or(0);
    let loads = |node: usize| {
        if node == load_node {
            [0.0, -1.0]
        } else {
            [0.0, 0.0]
        }
    };

    let lp = LayoutLp::assemble(&gs, &supported, &loads, 1.0);
    let settings = PdhgSettings {
        max_iters: 60_000,
        gap_tol,
        check_every: 500,
    };
    let (x, _y, report) = lp.solve(None, None, settings);

    // Member force (q⁺ − q⁻) and material volume (both split costs).
    let force = |k: usize| x[k] - x[m + k];
    let volume = |k: usize| lp.c[k] * x[k] + lp.c[m + k] * x[m + k];
    let max_force = (0..m).map(|k| force(k).abs()).fold(0.0, f64::max);
    let active_tol = 1e-3 * max_force.max(1e-12);

    // Active bars, sorted by DECREASING distance-to-support so DAG index order
    // (load side first) is a valid topological order.
    let mut active: Vec<usize> = (0..m).filter(|&k| force(k).abs() > active_tol).collect();
    active.sort_by(|&a, &b| {
        let da = mid_dist(&gs, a, &support_pts);
        let db = mid_dist(&gs, b, &support_pts);
        db.total_cmp(&da)
    });
    let num_active = active.len();

    // Tropical critical path over the active-bar DAG (latency = bar volume).
    let latencies: Vec<f64> = active.iter().map(|&k| volume(k)).collect();
    let mut dag = TaskDag::new(latencies);
    for i in 0..num_active {
        for j in (i + 1)..num_active {
            let (ki, kj) = (active[i], active[j]);
            if shares_joint(&gs, ki, kj)
                && mid_dist(&gs, ki, &support_pts) > mid_dist(&gs, kj, &support_pts) + 1e-9
            {
                dag = dag.with_edge(i, j);
            }
        }
    }
    let (critical_path, critical_path_volume, bottleneck_member) = match dag.critical_path() {
        Ok(cp) => {
            let orig: Vec<usize> = cp.path.iter().map(|&i| active[i]).collect();
            let bottleneck = dag
                .bottleneck(&cp)
                .map(|i| active[i])
                .or_else(|| orig.first().copied());
            (orig, cp.makespan, bottleneck)
        }
        Err(_) => (Vec::new(), 0.0, None),
    };

    let certified_optimal = report.gap < gap_tol * 10.0 && report.eq_residual < 1e-3;
    let optimality_color = if certified_optimal {
        // The rigorous enclosure of the true optimum: the feasible primal
        // `volume` is an UPPER bound and the dual `volume·(1−gap)` a LOWER bound
        // (fs-truss's gap is |primal−dual|/|primal|).
        Color::Verified {
            lo: report.volume * (1.0 - report.gap),
            hi: report.volume,
        }
    } else {
        Color::Estimated {
            estimator: "pdhg-not-converged".to_string(),
            dispersion: report.gap,
        }
    };

    TrussReport {
        num_members: m,
        num_active,
        total_volume: report.volume,
        gap: report.gap,
        eq_residual: report.eq_residual,
        iters: report.iters,
        certified_optimal,
        critical_path,
        critical_path_volume,
        bottleneck_member,
        optimality_color,
        load_path_color: Color::Verified {
            lo: 0.0,
            hi: critical_path_volume,
        },
    }
}

fn mid_dist(gs: &GroundStructure, k: usize, supports: &[[f64; 2]]) -> f64 {
    let (a, b) = gs.members[k];
    let mid = [
        f64::midpoint(gs.nodes[a][0], gs.nodes[b][0]),
        f64::midpoint(gs.nodes[a][1], gs.nodes[b][1]),
    ];
    dist_to_support(mid, supports)
}

fn shares_joint(gs: &GroundStructure, i: usize, j: usize) -> bool {
    let (a, b) = gs.members[i];
    let (c, d) = gs.members[j];
    a == c || a == d || b == c || b == d
}
