//! VALUE-OF-INFORMATION QUERIES (addendum Proposal C, bead knh1.6;
//! [F] — behind the `voi-queries` feature): THE IGNORANCE MARKET, v0
//! as a RANKED LIST. Across everything the ledger is uncertain about,
//! where does one dollar of evidence most change the downstream
//! decision? Decision sensitivity (does the decision FLIP inside the
//! node's interval?) is computed from CACHED surrogate sweeps, crossed
//! with a PRICED PROBE MENU that unifies computational and physical
//! experiments, ranked by FLIP-PROBABILITY-PER-DOLLAR.
//!
//! MYOPIC one-step VoI ONLY (the proposal's own discipline: full
//! sequential VoI is intractable and myopic captures most of the
//! value). The output surfaces as (i) the hint on query results — the
//! upgrade the fs-ir anytime module's CONTRACT reserved for Proposal C
//! — and (ii) the scheduler for discrepancy probes.
//!
//! THE KILL CRITERION AS CODE: [`audit_verdict`] compares
//! VoI-recommended purchases against agent-chosen alternatives at
//! matched cost; if recommendations do not measurably outperform on
//! realized decision changes, VoI DEMOTES ITSELF to a reporting
//! feature.

/// One uncertainty node touching a live decision: a named quantity the
/// ledger only knows to an interval.
#[derive(Debug, Clone, PartialEq)]
pub struct UncertaintyNode {
    /// Ledger name.
    pub name: String,
    /// Current uncertainty interval.
    pub lo: f64,
    /// Upper end.
    pub hi: f64,
    /// Nominal (decision-time) value.
    pub nominal: f64,
}

/// A live decision over the uncertain quantities: v0 is a threshold
/// verdict on a cheap cached surrogate `margin(values) > 0`.
pub struct LiveDecision<'a> {
    /// The cached surrogate margin (cheap by Proposals 9/2).
    pub margin: &'a dyn Fn(&[f64]) -> f64,
    /// Node count.
    pub arity: usize,
}

impl LiveDecision<'_> {
    /// The nominal verdict.
    #[must_use]
    pub fn nominal_verdict(&self, nodes: &[UncertaintyNode]) -> bool {
        let vals: Vec<f64> = nodes.iter().map(|n| n.nominal).collect();
        (self.margin)(&vals) > 0.0
    }

    /// DECISION SENSITIVITY of one node: sweep the node's interval on
    /// the cached surrogate (others at nominal, `grid` points) and
    /// return the fraction of the interval where the verdict differs
    /// from nominal — the myopic flip probability under the uniform
    /// interval measure (v0's declared prior).
    #[must_use]
    pub fn flip_probability(&self, nodes: &[UncertaintyNode], node_idx: usize, grid: usize) -> f64 {
        let base = self.nominal_verdict(nodes);
        let mut vals: Vec<f64> = nodes.iter().map(|n| n.nominal).collect();
        let node = &nodes[node_idx];
        let mut flips = 0usize;
        for k in 0..grid {
            #[allow(clippy::cast_precision_loss)]
            let t = (k as f64 + 0.5) / grid as f64;
            vals[node_idx] = node.lo + t * (node.hi - node.lo);
            if ((self.margin)(&vals) > 0.0) != base {
                flips += 1;
            }
        }
        vals[node_idx] = node.nominal;
        #[allow(clippy::cast_precision_loss)]
        {
            flips as f64 / grid as f64
        }
    }
}

/// The kind of evidence purchase — the menu UNIFIES computational and
/// physical experiments (the epistemic-engine identity made concrete).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    /// Climb a fidelity rung / refine / add solver accuracy.
    Computational,
    /// Wind-tunnel anchor, CT scan, strain gauge — reality as evidence.
    Physical,
}

/// One priced probe: buying it SHRINKS a node's interval around its
/// nominal by `shrink` (0 < shrink < 1).
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    /// Menu name ("climb-to-rung-96", "wind-tunnel-anchor", ...).
    pub name: String,
    /// Which node it tightens.
    pub target: String,
    /// Price in dollars.
    pub cost: f64,
    /// Post-probe interval width as a fraction of the current width.
    pub shrink: f64,
    /// Computational or physical.
    pub kind: ProbeKind,
}

/// One ranked purchase: the myopic VoI score.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedPurchase {
    /// The probe.
    pub probe: Probe,
    /// Flip probability before the purchase.
    pub flip_before: f64,
    /// Expected flip probability after the purchase.
    pub flip_after: f64,
    /// THE SCORE: expected flip-probability reduction per dollar.
    pub score: f64,
}

/// Rank the probe menu by flip-probability-per-dollar for the live
/// decision — MYOPIC one-step VoI (each probe is evaluated against the
/// CURRENT state only; no sequential tree).
#[must_use]
pub fn rank_purchases(
    decision: &LiveDecision<'_>,
    nodes: &[UncertaintyNode],
    menu: &[Probe],
    grid: usize,
) -> Vec<RankedPurchase> {
    let mut ranked: Vec<RankedPurchase> = menu
        .iter()
        .filter_map(|probe| {
            let idx = nodes.iter().position(|n| n.name == probe.target)?;
            let flip_before = decision.flip_probability(nodes, idx, grid);
            // Myopic post-probe state: the interval shrinks around the
            // nominal by the probe's factor.
            let mut post = nodes.to_vec();
            let n = &nodes[idx];
            let half = (n.hi - n.lo) / 2.0 * probe.shrink;
            post[idx].lo = n.nominal - half;
            post[idx].hi = n.nominal + half;
            let flip_after = decision.flip_probability(&post, idx, grid);
            let score = (flip_before - flip_after).max(0.0) / probe.cost.max(1e-9);
            Some(RankedPurchase {
                probe: probe.clone(),
                flip_before,
                flip_after,
                score,
            })
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.probe.cost.total_cmp(&b.probe.cost))
            .then(a.probe.name.cmp(&b.probe.name))
    });
    ranked
}

/// Surface the top purchase as the QUERY-RESULT HINT — the Proposal-8
/// anytime-hint shape, now priced by decision impact instead of the
/// O(h) extrapolation the fs-ir CONTRACT flagged as interim.
#[must_use]
pub fn hint_for_query(ranked: &[RankedPurchase]) -> String {
    match ranked.first() {
        Some(top) if top.score > 0.0 => format!(
            "highest-value evidence: {} (${:.0}) — expected flip-probability drop \
             {:.3} -> {:.3} on '{}' ({:.4}/$)",
            top.probe.name,
            top.probe.cost,
            top.flip_before,
            top.flip_after,
            top.probe.target,
            top.score
        ),
        _ => "no purchase on the menu changes the decision — spend nothing".to_string(),
    }
}

/// THE PROBE SCHEDULER: greedy top-k purchases under a dollar budget
/// (the discrepancy-probe scheduler surface for color-probes).
#[must_use]
pub fn schedule_probes(ranked: &[RankedPurchase], budget: f64) -> Vec<RankedPurchase> {
    let mut remaining = budget;
    let mut out = Vec::new();
    for r in ranked {
        if r.score > 0.0 && r.probe.cost <= remaining {
            remaining -= r.probe.cost;
            out.push(r.clone());
        }
    }
    out
}

/// One prospective-audit record: a VoI-recommended purchase vs the
/// agent-chosen alternative at MATCHED COST, with the realized outcome
/// (did the evidence change the decision?).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    /// Did the RECOMMENDED purchase realize a decision change?
    pub recommended_changed_decision: bool,
    /// Did the agent-chosen alternative?
    pub alternative_changed_decision: bool,
}

/// The audit verdict — the kill criterion as code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditVerdict {
    /// Recommendations measurably outperform: keep scheduling by VoI.
    KeepScheduling,
    /// They do not: DEMOTE VoI to a reporting feature.
    DemoteToReporting,
}

/// Compare realized decision-change rates at matched cost. VoI keeps
/// its scheduling authority only while it MEASURABLY outperforms.
#[must_use]
pub fn audit_verdict(records: &[AuditRecord]) -> AuditVerdict {
    if records.is_empty() {
        return AuditVerdict::DemoteToReporting; // no evidence, no authority
    }
    let rec = records
        .iter()
        .filter(|r| r.recommended_changed_decision)
        .count();
    let alt = records
        .iter()
        .filter(|r| r.alternative_changed_decision)
        .count();
    if rec > alt {
        AuditVerdict::KeepScheduling
    } else {
        AuditVerdict::DemoteToReporting
    }
}
