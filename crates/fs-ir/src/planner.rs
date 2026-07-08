//! GREEDY FIDELITY-LADDER PLANNER (addendum Proposal 8, bead lmp4.16;
//! [F] — behind the `ladder-planner` feature): a LADDER WALK, not a
//! general planner. Governance Rule 1 forbids opening general planning
//! as a research program; the search space is deliberately collapsed to
//! the fidelity-refinement lattice and solved greedily over the operator
//! menu `{cache, speculate, solve-rung, DWR-refine, climb}` with costs
//! LEARNED from telemetry (cold estimates fall back to a conservative
//! default). All the intelligence is inherited from the flywheel
//! underneath: certified verification (Proposal 9's verifier), the
//! content-addressed cache (Proposal 2), the fidelity-ladder registry
//! (Proposal 3), and colors on the returned interval.
//!
//! Determinism (G5): fixed operator order, deterministic tie-breaks
//! (refine preferred over climb on equal predicted cost) — a replayed
//! query reproduces the same operator sequence and interval trajectory.
//! The cannot-discharge boundary hands off cleanly to refusal semantics
//! with the best achieved certified interval — never a false in-budget
//! answer.

use std::collections::BTreeMap;

use fs_verify::estimator::verify;
use fs_verify::fem1d::{MmsProblem, Poly, gauss5, solve_p1};

/// One planner operator (the menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlanOp {
    /// Proposal 2: the content-addressed answer cache.
    CacheLookup,
    /// Proposal 9: verify a prolongated coarse answer without solving.
    Speculate,
    /// Solve at the current rung (uniform mesh).
    SolveRung,
    /// Refine ONLY where the residual indicators concentrate.
    DwrRefine,
    /// Move to the next rung (uniform, finer everywhere).
    Climb,
}

impl PlanOp {
    /// Stable name for logs and the cost table.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            PlanOp::CacheLookup => "cache",
            PlanOp::Speculate => "speculate",
            PlanOp::SolveRung => "solve-rung",
            PlanOp::DwrRefine => "dwr-refine",
            PlanOp::Climb => "climb",
        }
    }
}

/// A certified cached answer.
#[derive(Debug, Clone)]
pub struct CachedAnswer {
    /// The nodal solution.
    pub nodal: Vec<f64>,
    /// The certified energy bound it carries.
    pub bound: f64,
    /// The mesh it lives on.
    pub mesh: Vec<f64>,
}

/// The Proposal-2 cache seam (implemented over the content-addressed
/// store; the planner only needs lookup/insert semantics).
pub trait AnswerCache {
    /// A certified answer for `key` whose bound is ≤ `tol`, if any.
    fn lookup(&self, key: &str, tol: f64) -> Option<CachedAnswer>;
    /// Record a certified answer.
    fn insert(&mut self, key: &str, answer: CachedAnswer);
}

/// The trivial in-memory cache.
#[derive(Debug, Default)]
pub struct MemCache {
    items: BTreeMap<String, CachedAnswer>,
}

impl AnswerCache for MemCache {
    fn lookup(&self, key: &str, tol: f64) -> Option<CachedAnswer> {
        self.items.get(key).filter(|a| a.bound <= tol).cloned()
    }

    fn insert(&mut self, key: &str, answer: CachedAnswer) {
        self.items.insert(key.to_string(), answer);
    }
}

/// LEARNED cost table: mean observed cost (cells solved) per operator.
/// Cold entries fall back to the conservative default.
#[derive(Debug)]
pub struct CostTable {
    seen: BTreeMap<&'static str, (f64, u64)>,
    /// The cold-telemetry fallback (conservative by design).
    pub cold_default: f64,
}

impl CostTable {
    /// A table with the given conservative default.
    #[must_use]
    pub fn new(cold_default: f64) -> CostTable {
        CostTable {
            seen: BTreeMap::new(),
            cold_default,
        }
    }

    /// Record an actual cost.
    pub fn record(&mut self, op: PlanOp, cost: f64) {
        let e = self.seen.entry(op.name()).or_insert((0.0, 0));
        e.0 += cost;
        e.1 += 1;
    }

    /// Predict an operator's cost (learned mean, else the default).
    #[must_use]
    pub fn predict(&self, op: PlanOp) -> f64 {
        self.seen
            .get(op.name())
            .filter(|(_, n)| *n > 0)
            .map_or(self.cold_default, |(sum, n)| {
                #[allow(clippy::cast_precision_loss)]
                {
                    sum / *n as f64
                }
            })
    }
}

/// One executed step in the plan (the audit trail).
#[derive(Debug, Clone, PartialEq)]
pub struct OpLog {
    /// Which operator ran.
    pub op: PlanOp,
    /// Cells involved (the cost unit).
    pub cost: f64,
    /// The certified bound after the step (∞ before any verify).
    pub bound_after: f64,
}

/// The planner verdict.
#[derive(Debug, Clone)]
pub enum PlanOutcome {
    /// The query is discharged: the certified bound meets tolerance.
    Discharged {
        /// The nodal answer on its mesh.
        nodal: Vec<f64>,
        /// The final mesh.
        mesh: Vec<f64>,
        /// The certified energy bound (VERIFIED color: an equilibrated
        /// enclosure, never a DWR guess).
        bound: f64,
        /// The executed operator sequence.
        ops: Vec<OpLog>,
        /// Total cost spent (cells).
        cost: f64,
    },
    /// The budget could not discharge the query: hand off to refusal
    /// semantics with the BEST ACHIEVED certified interval — never a
    /// false in-budget answer.
    RefusedWithBest {
        /// The best certified bound achieved.
        best_bound: f64,
        /// The nodal answer that achieved it.
        best_nodal: Vec<f64>,
        /// Its mesh.
        best_mesh: Vec<f64>,
        /// The executed operator sequence.
        ops: Vec<OpLog>,
        /// Total cost spent (≤ budget + the final step's cost).
        cost: f64,
        /// What to tell the caller.
        reason: String,
    },
}

/// The 1-D elliptic problem family the v0 planner discharges (the
/// verifier's kernel class): exact solution `theta`-scaled.
#[derive(Debug, Clone)]
pub struct ProblemFamily {
    /// The exact solution polynomial at θ = 1.
    pub base: Poly,
    /// The family's kernel name (cache keys, ladder).
    pub kernel: String,
}

impl ProblemFamily {
    /// Instantiate the problem at `theta` on `mesh`.
    #[must_use]
    pub fn at(&self, theta: f64, mesh: Vec<f64>) -> MmsProblem {
        let scaled = Poly(self.base.0.iter().map(|c| c * theta).collect());
        MmsProblem::new(&self.kernel, scaled, mesh)
    }
}

fn uniform_mesh(cells: usize) -> Vec<f64> {
    #[allow(clippy::cast_precision_loss)]
    (0..=cells).map(|k| k as f64 / cells as f64).collect()
}

/// Per-element energy-residual indicators (the same integrand the
/// equilibrated verifier bounds, localized): `∫_K (c* − F − u′)²`.
fn element_indicators(problem: &MmsProblem, nodal: &[f64]) -> Vec<f64> {
    let m = &problem.mesh;
    let n = m.len();
    // The verifier's optimal constant.
    let mut c_star = 0.0f64;
    for e in 0..n - 1 {
        let h = m[e + 1] - m[e];
        let slope = (nodal[e + 1] - nodal[e]) / h;
        for (gx, gw) in gauss5(m[e], m[e + 1]) {
            c_star += gw * (problem.big_f.eval(gx) + slope);
        }
    }
    (0..n - 1)
        .map(|e| {
            let h = m[e + 1] - m[e];
            let slope = (nodal[e + 1] - nodal[e]) / h;
            let mut acc = 0.0f64;
            for (gx, gw) in gauss5(m[e], m[e + 1]) {
                let r = c_star - problem.big_f.eval(gx) - slope;
                acc += gw * r * r;
            }
            acc
        })
        .collect()
}

/// EQUIDISTRIBUTION refinement (the textbook optimal-mesh criterion):
/// split every element whose squared-residual contribution exceeds the
/// per-element target `tol²/n`, with a PER-ELEMENT depth from its own
/// gap (splitting an element into 2^d pieces cuts its contribution by
/// ~4^d in this residual model). Deterministic; converges in a couple
/// of solve rounds instead of crawling at the tail.
fn refine_to_target(mesh: &[f64], indicators: &[f64], tol: f64) -> Vec<f64> {
    let n = indicators.len().max(1);
    #[allow(clippy::cast_precision_loss)]
    let target = (tol * tol / n as f64).max(f64::MIN_POSITIVE);
    let mut out = Vec::with_capacity(mesh.len() + 16);
    for e in 0..mesh.len() - 1 {
        out.push(mesh[e]);
        if indicators[e] > target {
            let gap = indicators[e] / target;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let depth = ((gap.log2() / 2.0).ceil() as u32).clamp(1, 5);
            let pieces = 1usize << depth;
            let (a, b) = (mesh[e], mesh[e + 1]);
            for k in 1..pieces {
                #[allow(clippy::cast_precision_loss)]
                out.push(a + (b - a) * k as f64 / pieces as f64);
            }
        }
    }
    out.push(*mesh.last().expect("nonempty"));
    out
}

/// The greedy ladder walk. `rung_cells` is the fidelity lattice
/// (coarsest first); `budget_cells` is the cost budget in solved cells.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn plan(
    family: &ProblemFamily,
    theta: f64,
    tol: f64,
    budget_cells: f64,
    rung_cells: &[usize],
    cache: &mut dyn AnswerCache,
    costs: &mut CostTable,
) -> PlanOutcome {
    let key = format!("{}@theta={theta:.6e}@tol<=", family.kernel);
    let mut ops: Vec<OpLog> = Vec::new();
    let mut spent = 0.0f64;
    let mut best: Option<(f64, Vec<f64>, Vec<f64>)> = None; // (bound, nodal, mesh)
    // ---- Operator 1: the cache (zero solves on a hit).
    if let Some(hit) = cache.lookup(&key, tol) {
        ops.push(OpLog {
            op: PlanOp::CacheLookup,
            cost: 0.0,
            bound_after: hit.bound,
        });
        return PlanOutcome::Discharged {
            nodal: hit.nodal,
            mesh: hit.mesh,
            bound: hit.bound,
            ops,
            cost: 0.0,
        };
    }
    ops.push(OpLog {
        op: PlanOp::CacheLookup,
        cost: 0.0,
        bound_after: f64::INFINITY,
    });
    let mut rung = 0usize;
    let mut mesh = uniform_mesh(rung_cells[0]);
    let mut carried: Option<Vec<f64>> = None; // prolongated candidate
    loop {
        // ---- Operator 2: speculate — verify a carried candidate
        // WITHOUT solving (prolongation from the previous rung).
        if let Some(cand) = carried.take() {
            let problem = family.at(theta, mesh.clone());
            #[allow(clippy::cast_precision_loss)]
            let vcost = 0.2 * (mesh.len() - 1) as f64;
            spent += vcost;
            let rep = verify(&problem, &cand, tol);
            costs.record(PlanOp::Speculate, vcost);
            ops.push(OpLog {
                op: PlanOp::Speculate,
                cost: vcost,
                bound_after: rep.bound.hi,
            });
            if rep.accept {
                cache.insert(
                    &key,
                    CachedAnswer {
                        nodal: cand.clone(),
                        bound: rep.bound.hi,
                        mesh: mesh.clone(),
                    },
                );
                return PlanOutcome::Discharged {
                    nodal: cand,
                    mesh,
                    bound: rep.bound.hi,
                    ops,
                    cost: spent,
                };
            }
        }
        // ---- Operator 3: solve at the current rung.
        let problem = family.at(theta, mesh.clone());
        #[allow(clippy::cast_precision_loss)]
        let scost = (mesh.len() - 1) as f64;
        spent += scost;
        let nodal = solve_p1(&problem);
        let rep = verify(&problem, &nodal, tol);
        costs.record(PlanOp::SolveRung, scost);
        ops.push(OpLog {
            op: PlanOp::SolveRung,
            cost: scost,
            bound_after: rep.bound.hi,
        });
        let better = best.as_ref().is_none_or(|(b, _, _)| rep.bound.hi < *b);
        if better {
            best = Some((rep.bound.hi, nodal.clone(), mesh.clone()));
        }
        if rep.accept {
            cache.insert(
                &key,
                CachedAnswer {
                    nodal: nodal.clone(),
                    bound: rep.bound.hi,
                    mesh: mesh.clone(),
                },
            );
            return PlanOutcome::Discharged {
                nodal,
                mesh,
                bound: rep.bound.hi,
                ops,
                cost: spent,
            };
        }
        // ---- Budget check BEFORE committing to more work.
        let next_refine = costs.predict(PlanOp::DwrRefine);
        let next_climb = costs.predict(PlanOp::Climb);
        let cheapest_next = next_refine.min(next_climb);
        if spent + cheapest_next > budget_cells {
            let (b, n, m) = best.expect("at least one solve ran");
            return PlanOutcome::RefusedWithBest {
                best_bound: b,
                best_nodal: n,
                best_mesh: m,
                ops,
                cost: spent,
                reason: format!(
                    "budget {budget_cells} cells cannot fund the next operator \
                     (predicted {cheapest_next:.0}); best certified bound {b:.3e} vs \
                     tol {tol:.3e} — hand off to refusal/anytime semantics"
                ),
            };
        }
        // ---- Greedy choice: refine-where-indicated vs climb, by
        // learned predicted cost; deterministic tie-break prefers
        // DwrRefine (the cheaper-in-principle local move).
        let choose_refine = next_refine <= next_climb;
        if choose_refine {
            let indicators = element_indicators(&problem, &nodal);
            mesh = refine_to_target(&mesh, &indicators, tol);
            #[allow(clippy::cast_precision_loss)]
            let rcost = (mesh.len() - 1) as f64;
            costs.record(PlanOp::DwrRefine, rcost);
            ops.push(OpLog {
                op: PlanOp::DwrRefine,
                cost: 0.0, // the refine itself is bookkeeping; the solve pays
                bound_after: f64::INFINITY,
            });
            carried = None;
        } else if rung + 1 < rung_cells.len() {
            rung += 1;
            let fine_mesh = uniform_mesh(rung_cells[rung]);
            // Prolongate the current answer as the speculation candidate
            // (dyadic meshes: midpoint interpolation).
            let mut cand = Vec::with_capacity(fine_mesh.len());
            for (i, _) in fine_mesh.iter().enumerate() {
                if i % 2 == 0 {
                    cand.push(nodal[i / 2]);
                } else {
                    cand.push(f64::midpoint(nodal[i / 2], nodal[i / 2 + 1]));
                }
            }
            #[allow(clippy::cast_precision_loss)]
            let ccost = (fine_mesh.len() - 1) as f64;
            costs.record(PlanOp::Climb, ccost);
            ops.push(OpLog {
                op: PlanOp::Climb,
                cost: 0.0,
                bound_after: f64::INFINITY,
            });
            mesh = fine_mesh;
            carried = Some(cand);
        } else {
            // Top of the ladder and still short: refine locally anyway
            // (the only move left).
            let indicators = element_indicators(&problem, &nodal);
            mesh = refine_to_target(&mesh, &indicators, tol);
            ops.push(OpLog {
                op: PlanOp::DwrRefine,
                cost: 0.0,
                bound_after: f64::INFINITY,
            });
            carried = None;
        }
    }
}

/// The FIXED BASELINE the kill criterion measures against: a single
/// mid-rung solve, then UNIFORM refinement until the tolerance is met
/// (no cache, no speculation, no locality).
#[must_use]
pub fn baseline_uniform(
    family: &ProblemFamily,
    theta: f64,
    tol: f64,
    mid_rung_cells: usize,
    max_doublings: usize,
) -> (f64, f64) {
    let mut cells = mid_rung_cells;
    let mut spent = 0.0f64;
    for _ in 0..=max_doublings {
        let problem = family.at(theta, uniform_mesh(cells));
        #[allow(clippy::cast_precision_loss)]
        {
            spent += cells as f64;
        }
        let nodal = solve_p1(&problem);
        let rep = verify(&problem, &nodal, tol);
        if rep.accept {
            return (spent, rep.bound.hi);
        }
        cells *= 2;
    }
    (spent, f64::INFINITY)
}
