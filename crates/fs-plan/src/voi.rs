//! VALUE-OF-INFORMATION QUERIES (addendum Proposal C, bead knh1.6;
//! \[F\] — behind the `voi-queries` feature): THE IGNORANCE MARKET, v0
//! as a RANKED LIST. Across everything the ledger is uncertain about,
//! where does one dollar of evidence most change the downstream
//! decision? Decision sensitivity (does the decision FLIP inside the
//! node's interval?) is computed from CACHED surrogate sweeps, crossed
//! with a PRICED PROBE MENU that unifies computational and physical
//! experiments, ranked by SAMPLED FLIP-FRACTION REDUCTION PER DOLLAR.
//!
//! MYOPIC one-step VoI ONLY (the proposal's own discipline: full
//! sequential VoI is intractable and myopic captures most of the
//! value). The output surfaces as (i) the hint on query results — the
//! upgrade the fs-ir anytime module's CONTRACT reserved for Proposal C
//! — and (ii) the scheduler for discrepancy probes.
//!
//! THE KILL CRITERION AS CODE: [`audit_scheduling`] feeds validated
//! matched-cost outcomes to an anytime-valid pairwise e-process. Only a
//! successful bounded audit can mint the private-construction capability
//! accepted by [`schedule_probes`]; otherwise VoI remains reporting-only.

use std::collections::BTreeSet;

use fs_blake3::{ContentHash, hash_domain};
use fs_eproc::{LossSpan, PairwiseRace};

/// Maximum uncertainty nodes in one myopic VoI request.
pub const MAX_VOI_NODES: usize = 256;
/// Maximum probe menu entries (and scheduled ranked entries).
pub const MAX_VOI_PROBES: usize = 1024;
/// Maximum visible-ASCII byte length of node, probe, target, and audit names.
pub const MAX_VOI_NAME_BYTES: usize = 128;
/// Maximum interval-sweep grid size.
pub const MAX_VOI_GRID: usize = 1024;
/// Maximum surrogate evaluations admitted by one public VoI call.
pub const MAX_VOI_EVALUATIONS: usize = 4096;
/// Maximum matched-cost observations admitted by one prospective audit.
pub const MAX_VOI_AUDIT_RECORDS: usize = 4096;
/// Fixed anytime-valid false-activation level for VoI scheduling authority.
pub const VOI_AUDIT_ALPHA: f64 = 0.05;

const RANKED_MENU_CONTEXT_DOMAIN: &str = "frankensim.fs-plan.voi-ranked-menu.v1";
const AUDIT_CONTEXT_DOMAIN: &str = "frankensim.fs-plan.voi-audit.v1";

/// Why a VoI query, audit, or schedule was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum VoiError {
    /// A bounded collection falls outside its admitted size range.
    SizeLimit {
        /// Collection being validated.
        collection: &'static str,
        /// Supplied element count.
        count: usize,
        /// Inclusive lower bound.
        min: usize,
        /// Inclusive upper bound.
        max: usize,
    },
    /// The surrogate's declared arity differs from the node vector.
    ArityMismatch {
        /// Declared surrogate arity.
        arity: usize,
        /// Supplied node count.
        node_count: usize,
    },
    /// A node/probe/target/audit identity is not bounded visible ASCII.
    InvalidName {
        /// Name category.
        kind: &'static str,
        /// Position in its collection.
        index: usize,
        /// Supplied UTF-8 byte length.
        bytes: usize,
        /// Inclusive byte limit.
        max_bytes: usize,
    },
    /// A supposedly unique name occurs more than once.
    DuplicateName {
        /// Name category.
        kind: &'static str,
        /// Duplicate value (already bounded by [`MAX_VOI_NAME_BYTES`]).
        name: String,
    },
    /// An interval is nonfinite, unordered, too wide for finite
    /// arithmetic, or excludes its nominal value.
    InvalidInterval {
        /// Node name.
        node: String,
        /// Lower endpoint.
        lo: f64,
        /// Nominal value.
        nominal: f64,
        /// Upper endpoint.
        hi: f64,
    },
    /// A surrogate returned a nonfinite decision margin.
    NonFiniteMargin {
        /// Returned margin.
        value: f64,
    },
    /// A sensitivity request names a missing node.
    NodeIndexOutOfRange {
        /// Supplied node index.
        node_idx: usize,
        /// Supplied node count.
        node_count: usize,
    },
    /// The sweep grid is zero or exceeds the declared cap.
    InvalidGrid {
        /// Supplied grid size.
        grid: usize,
        /// Inclusive upper bound.
        max: usize,
    },
    /// A request would exceed the surrogate-evaluation budget.
    EvaluationLimitExceeded {
        /// Required evaluations.
        requested: usize,
        /// Inclusive limit.
        max: usize,
    },
    /// A probe has an invalid numeric field.
    InvalidProbeValue {
        /// Probe name.
        probe: String,
        /// Invalid field (`cost` or `shrink`).
        field: &'static str,
        /// Supplied value.
        value: f64,
    },
    /// A probe target resolves to zero or multiple nodes.
    TargetResolution {
        /// Probe name.
        probe: String,
        /// Requested target.
        target: String,
        /// Number of matching nodes.
        matches: usize,
    },
    /// The scheduling budget is nonfinite or negative.
    InvalidBudget {
        /// Supplied budget.
        budget: f64,
    },
    /// An audit observation has a malformed finite matched-cost pair.
    InvalidAuditCost {
        /// Observation identity.
        observation: String,
        /// Recommended-purchase cost.
        recommended_cost: f64,
        /// Alternative-purchase cost.
        alternative_cost: f64,
    },
    /// An audit compares a purchase with itself.
    InvalidAuditPair {
        /// Observation identity.
        observation: String,
    },
    /// An audit repeats an observation identity and could double-count evidence.
    DuplicateAuditObservation {
        /// Repeated observation identity.
        observation: String,
    },
    /// Scheduling was requested without an anytime-valid authority capability.
    MissingSchedulingAuthority,
    /// Finite inputs could not produce a finite, monotone result.
    ArithmeticRefusal {
        /// Operation that failed.
        operation: &'static str,
        /// Bounded subject name.
        subject: String,
    },
}

impl core::fmt::Display for VoiError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::SizeLimit {
                collection,
                count,
                min,
                max,
            } => write!(
                f,
                "{collection} has {count} entries; expected an inclusive range of {min}..={max}"
            ),
            Self::ArityMismatch { arity, node_count } => write!(
                f,
                "surrogate declares arity {arity}, but {node_count} uncertainty node(s) were supplied"
            ),
            Self::InvalidName {
                kind,
                index,
                bytes,
                max_bytes,
            } => write!(
                f,
                "{kind} name at index {index} is not nonempty visible ASCII or is {bytes} bytes long (limit {max_bytes})"
            ),
            Self::DuplicateName { kind, name } => {
                write!(f, "duplicate {kind} name {name:?}")
            }
            Self::InvalidInterval {
                node,
                lo,
                nominal,
                hi,
            } => write!(
                f,
                "node {node:?} has invalid interval [{lo:?}, {hi:?}] with nominal {nominal:?}"
            ),
            Self::NonFiniteMargin { value } => {
                write!(f, "surrogate returned nonfinite margin {value:?}")
            }
            Self::NodeIndexOutOfRange {
                node_idx,
                node_count,
            } => write!(
                f,
                "node index {node_idx} is out of range for {node_count} node(s)"
            ),
            Self::InvalidGrid { grid, max } => {
                write!(f, "sweep grid {grid} is outside 1..={max}")
            }
            Self::EvaluationLimitExceeded { requested, max } => write!(
                f,
                "VoI request needs {requested} surrogate evaluations; the limit is {max}"
            ),
            Self::InvalidProbeValue {
                probe,
                field,
                value,
            } => write!(f, "probe {probe:?} has invalid {field} {value:?}"),
            Self::TargetResolution {
                probe,
                target,
                matches,
            } => write!(
                f,
                "probe {probe:?} target {target:?} resolves to {matches} uncertainty node(s), expected exactly one"
            ),
            Self::InvalidBudget { budget } => {
                write!(
                    f,
                    "probe budget must be finite and non-negative, got {budget:?}"
                )
            }
            Self::InvalidAuditCost {
                observation,
                recommended_cost,
                alternative_cost,
            } => write!(
                f,
                "audit observation {observation:?} requires equal finite positive matched costs, got {recommended_cost:?} and {alternative_cost:?}"
            ),
            Self::InvalidAuditPair { observation } => write!(
                f,
                "audit observation {observation:?} compares a purchase with itself"
            ),
            Self::DuplicateAuditObservation { observation } => write!(
                f,
                "audit observation {observation:?} appears more than once"
            ),
            Self::MissingSchedulingAuthority => write!(
                f,
                "VoI scheduling requires a live anytime-valid audit authority capability"
            ),
            Self::ArithmeticRefusal { operation, subject } => {
                write!(
                    f,
                    "{operation} for {subject:?} did not remain finite and monotone"
                )
            }
        }
    }
}

impl std::error::Error for VoiError {}

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

fn validate_size(
    collection: &'static str,
    count: usize,
    min: usize,
    max: usize,
) -> Result<(), VoiError> {
    if (min..=max).contains(&count) {
        Ok(())
    } else {
        Err(VoiError::SizeLimit {
            collection,
            count,
            min,
            max,
        })
    }
}

fn validate_name(kind: &'static str, index: usize, name: &str) -> Result<(), VoiError> {
    if name.is_empty()
        || name.len() > MAX_VOI_NAME_BYTES
        || !name.bytes().all(|byte| byte.is_ascii_graphic())
    {
        Err(VoiError::InvalidName {
            kind,
            index,
            bytes: name.len(),
            max_bytes: MAX_VOI_NAME_BYTES,
        })
    } else {
        Ok(())
    }
}

fn validate_nodes(decision: &LiveDecision<'_>, nodes: &[UncertaintyNode]) -> Result<(), VoiError> {
    validate_size("uncertainty nodes", nodes.len(), 1, MAX_VOI_NODES)?;
    if decision.arity != nodes.len() {
        return Err(VoiError::ArityMismatch {
            arity: decision.arity,
            node_count: nodes.len(),
        });
    }
    let mut names = BTreeSet::new();
    for (index, node) in nodes.iter().enumerate() {
        validate_name("uncertainty node", index, &node.name)?;
        if !names.insert(node.name.as_str()) {
            return Err(VoiError::DuplicateName {
                kind: "uncertainty node",
                name: node.name.clone(),
            });
        }
        let width = node.hi - node.lo;
        if !node.lo.is_finite()
            || !node.hi.is_finite()
            || !node.nominal.is_finite()
            || node.lo > node.hi
            || node.nominal < node.lo
            || node.nominal > node.hi
            || !width.is_finite()
        {
            return Err(VoiError::InvalidInterval {
                node: node.name.clone(),
                lo: node.lo,
                nominal: node.nominal,
                hi: node.hi,
            });
        }
    }
    Ok(())
}

fn validate_grid(grid: usize) -> Result<(), VoiError> {
    if (1..=MAX_VOI_GRID).contains(&grid) {
        Ok(())
    } else {
        Err(VoiError::InvalidGrid {
            grid,
            max: MAX_VOI_GRID,
        })
    }
}

fn validate_evaluations(requested: usize) -> Result<(), VoiError> {
    if requested <= MAX_VOI_EVALUATIONS {
        Ok(())
    } else {
        Err(VoiError::EvaluationLimitExceeded {
            requested,
            max: MAX_VOI_EVALUATIONS,
        })
    }
}

fn evaluate_margin(decision: &LiveDecision<'_>, values: &[f64]) -> Result<f64, VoiError> {
    let margin = (decision.margin)(values);
    if margin.is_finite() {
        Ok(margin)
    } else {
        Err(VoiError::NonFiniteMargin { value: margin })
    }
}

fn nominal_values(nodes: &[UncertaintyNode]) -> Vec<f64> {
    nodes.iter().map(|node| node.nominal).collect()
}

fn node_at(nodes: &[UncertaintyNode], node_idx: usize) -> Result<&UncertaintyNode, VoiError> {
    nodes.get(node_idx).ok_or(VoiError::NodeIndexOutOfRange {
        node_idx,
        node_count: nodes.len(),
    })
}

fn nominal_verdict_validated(
    decision: &LiveDecision<'_>,
    nodes: &[UncertaintyNode],
) -> Result<bool, VoiError> {
    Ok(evaluate_margin(decision, &nominal_values(nodes))? > 0.0)
}

fn flip_probability_validated(
    decision: &LiveDecision<'_>,
    nodes: &[UncertaintyNode],
    node_idx: usize,
    lo: f64,
    hi: f64,
    grid: usize,
) -> Result<f64, VoiError> {
    let base = nominal_verdict_validated(decision, nodes)?;
    let mut values = nominal_values(nodes);
    let node = node_at(nodes, node_idx)?;
    let width = hi - lo;
    let mut flips = 0usize;
    for k in 0..grid {
        #[allow(clippy::cast_precision_loss)]
        let t = (k as f64 + 0.5) / grid as f64;
        let sample = lo + t * width;
        if !sample.is_finite() {
            return Err(VoiError::ArithmeticRefusal {
                operation: "interval sweep",
                subject: node.name.clone(),
            });
        }
        *values
            .get_mut(node_idx)
            .ok_or(VoiError::NodeIndexOutOfRange {
                node_idx,
                node_count: nodes.len(),
            })? = sample;
        if (evaluate_margin(decision, &values)? > 0.0) != base {
            flips += 1;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let probability = flips as f64 / grid as f64;
    Ok(probability)
}

impl LiveDecision<'_> {
    /// The nominal verdict.
    ///
    /// # Errors
    /// [`VoiError`] when node/arity/interval invariants fail or the
    /// cached surrogate returns a nonfinite margin.
    pub fn nominal_verdict(&self, nodes: &[UncertaintyNode]) -> Result<bool, VoiError> {
        validate_nodes(self, nodes)?;
        validate_evaluations(1)?;
        nominal_verdict_validated(self, nodes)
    }

    /// DECISION SENSITIVITY of one node: sweep the node's interval on
    /// the cached surrogate (others at nominal, `grid` points) and
    /// return the fraction of MIDPOINT GRID SAMPLES where the verdict differs
    /// from nominal. This is a myopic estimate under the uniform interval
    /// measure, not a certified probability.
    ///
    /// # Errors
    /// [`VoiError`] when the request is malformed, exceeds the declared
    /// sweep/evaluation limits, or the surrogate returns a nonfinite
    /// margin.
    pub fn flip_probability(
        &self,
        nodes: &[UncertaintyNode],
        node_idx: usize,
        grid: usize,
    ) -> Result<f64, VoiError> {
        validate_nodes(self, nodes)?;
        let node = node_at(nodes, node_idx)?;
        validate_grid(grid)?;
        let evaluations = grid
            .checked_add(1)
            .ok_or_else(|| VoiError::ArithmeticRefusal {
                operation: "sweep evaluation count",
                subject: node.name.clone(),
            })?;
        validate_evaluations(evaluations)?;
        flip_probability_validated(self, nodes, node_idx, node.lo, node.hi, grid)
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
    probe: Probe,
    /// Grid-sampled flip fraction before the purchase.
    flip_before: f64,
    /// Grid-sampled flip fraction after the declared contraction.
    flip_after: f64,
    /// THE SCORE: sampled flip-fraction reduction per dollar.
    score: f64,
}

impl RankedPurchase {
    /// The validated probe purchase.
    #[must_use]
    pub fn probe(&self) -> &Probe {
        &self.probe
    }

    /// Grid-sampled flip fraction before the purchase.
    #[must_use]
    pub fn flip_before(&self) -> f64 {
        self.flip_before
    }

    /// Grid-sampled flip fraction after the declared contraction.
    #[must_use]
    pub fn flip_after(&self) -> f64 {
        self.flip_after
    }

    /// Grid-sampled flip-fraction reduction per dollar.
    #[must_use]
    pub fn score(&self) -> f64 {
        self.score
    }
}

/// A complete, canonical ranking for one validated supplied
/// uncertainty/menu/grid snapshot. Rows and context are private so safe callers
/// cannot omit, splice, or reorder scheduling authority after ranking.
#[derive(Debug, PartialEq)]
pub struct RankedMenu {
    rows: Vec<RankedPurchase>,
    context_id: ContentHash,
    grid: usize,
}

impl RankedMenu {
    /// Number of ranked purchases in the complete menu.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// A ranked menu produced by [`rank_purchases`] is never empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Inspect one canonical row without exposing mutable membership.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&RankedPurchase> {
        self.rows.get(index)
    }

    /// Inspect the highest-ranked purchase.
    #[must_use]
    pub fn top(&self) -> Option<&RankedPurchase> {
        self.rows.first()
    }

    /// Iterate over canonical rows for reporting only.
    #[must_use]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &RankedPurchase> {
        self.rows.iter()
    }

    /// Midpoint grid used for every sampled flip estimate in this menu.
    #[must_use]
    pub fn grid(&self) -> usize {
        self.grid
    }

    /// BLAKE3 identity of the validated node/menu/grid snapshot.
    ///
    /// This binds supplied content but does not identify callback code, prove
    /// catalog completeness, or prove that the snapshot remains current;
    /// callers must compare it with their ledger/session snapshot before use.
    #[must_use]
    pub fn context_id(&self) -> ContentHash {
        self.context_id
    }
}

/// Structured, grid-qualified query hint. Its private optional purchase keeps
/// the no-sampled-change state distinct from an authoritative zero claim.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryHint {
    context_id: ContentHash,
    grid: usize,
    purchase: Option<RankedPurchase>,
}

impl QueryHint {
    /// Ranked snapshot identity supporting this estimate.
    #[must_use]
    pub fn context_id(&self) -> ContentHash {
        self.context_id
    }

    /// Midpoint grid supporting this estimate.
    #[must_use]
    pub fn grid(&self) -> usize {
        self.grid
    }

    /// Estimated top purchase, or `None` when no sampled row changed the
    /// decision on this grid. `None` is not a proof that no purchase can help.
    #[must_use]
    pub fn purchase(&self) -> Option<&RankedPurchase> {
        self.purchase.as_ref()
    }

    /// Safe deterministic text. Identifiers are escaped and every finite
    /// scalar uses Rust's shortest round-tripping representation.
    #[must_use]
    pub fn render_text(&self) -> String {
        match &self.purchase {
            Some(top) => format!(
                "estimated top evidence on the supplied menu from a {}-point midpoint sweep: {} (${}) - sampled flip fraction {} -> {} on {} ({}/$)",
                self.grid,
                escape_text(&top.probe.name),
                top.probe.cost,
                top.flip_before,
                top.flip_after,
                escape_text(&top.probe.target),
                top.score,
            ),
            None => format!(
                "no sampled purchase changed the decision on the {}-point midpoint sweep; this estimate does not prove that further evidence has zero value",
                self.grid
            ),
        }
    }

    /// Strict JSON rendering for logs and evidence payloads.
    #[must_use]
    pub fn to_json(&self) -> String {
        let context = self.context_id.to_hex();
        match &self.purchase {
            Some(top) => format!(
                "{{\"schema\":\"fs-plan.voi-hint.v1\",\"kind\":\"estimated_purchase\",\"context\":\"{context}\",\"grid\":{},\"probe\":{},\"target\":{},\"cost_dollars\":{},\"sampled_flip_before\":{},\"sampled_flip_after\":{},\"score_per_dollar\":{}}}",
                self.grid,
                json_string(&top.probe.name),
                json_string(&top.probe.target),
                top.probe.cost,
                top.flip_before,
                top.flip_after,
                top.score,
            ),
            None => format!(
                "{{\"schema\":\"fs-plan.voi-hint.v1\",\"kind\":\"no_sampled_change\",\"context\":\"{context}\",\"grid\":{},\"authoritative_zero\":false}}",
                self.grid
            ),
        }
    }
}

impl core::fmt::Display for QueryHint {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.render_text())
    }
}

fn escape_text(value: &str) -> String {
    value.chars().flat_map(char::escape_default).collect()
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for byte in value.bytes() {
        match byte {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            _ => out.push(char::from(byte)),
        }
    }
    out.push('"');
    out
}

fn compare_ranked(a: &RankedPurchase, b: &RankedPurchase) -> core::cmp::Ordering {
    b.score
        .total_cmp(&a.score)
        .then(a.probe.cost.total_cmp(&b.probe.cost))
        .then(a.probe.name.cmp(&b.probe.name))
}

fn validate_probe(index: usize, probe: &Probe) -> Result<(), VoiError> {
    validate_name("probe", index, &probe.name)?;
    validate_name("probe target", index, &probe.target)?;
    if !probe.cost.is_finite() || probe.cost <= 0.0 {
        return Err(VoiError::InvalidProbeValue {
            probe: probe.name.clone(),
            field: "cost",
            value: probe.cost,
        });
    }
    if !probe.shrink.is_finite() || probe.shrink <= 0.0 || probe.shrink >= 1.0 {
        return Err(VoiError::InvalidProbeValue {
            probe: probe.name.clone(),
            field: "shrink",
            value: probe.shrink,
        });
    }
    Ok(())
}

fn validate_menu(nodes: &[UncertaintyNode], menu: &[Probe]) -> Result<Vec<usize>, VoiError> {
    validate_size("probe menu", menu.len(), 1, MAX_VOI_PROBES)?;
    let mut names = BTreeSet::new();
    let mut targets = Vec::with_capacity(menu.len());
    for (index, probe) in menu.iter().enumerate() {
        validate_probe(index, probe)?;
        if !names.insert(probe.name.as_str()) {
            return Err(VoiError::DuplicateName {
                kind: "probe",
                name: probe.name.clone(),
            });
        }
        let mut matched = None;
        let mut matches = 0usize;
        for (node_idx, node) in nodes.iter().enumerate() {
            if node.name == probe.target {
                matches += 1;
                matched = Some(node_idx);
            }
        }
        let Some(node_idx) = matched.filter(|_| matches == 1) else {
            return Err(VoiError::TargetResolution {
                probe: probe.name.clone(),
                target: probe.target.clone(),
                matches,
            });
        };
        targets.push(node_idx);
    }
    Ok(targets)
}

fn push_u32(out: &mut Vec<u8>, value: usize, subject: &'static str) -> Result<(), VoiError> {
    let value = u32::try_from(value).map_err(|_| VoiError::ArithmeticRefusal {
        operation: "VoI context length",
        subject: subject.to_string(),
    })?;
    out.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn push_text(out: &mut Vec<u8>, value: &str, subject: &'static str) -> Result<(), VoiError> {
    push_u32(out, value.len(), subject)?;
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn ranked_menu_context(
    nodes: &[UncertaintyNode],
    menu: &[Probe],
    grid: usize,
) -> Result<ContentHash, VoiError> {
    let mut canonical = Vec::new();
    canonical.extend_from_slice(&1u32.to_le_bytes());
    push_u32(&mut canonical, grid, "grid")?;
    push_u32(&mut canonical, nodes.len(), "uncertainty nodes")?;
    for node in nodes {
        push_text(&mut canonical, &node.name, "uncertainty node name")?;
        canonical.extend_from_slice(&node.lo.to_bits().to_le_bytes());
        canonical.extend_from_slice(&node.nominal.to_bits().to_le_bytes());
        canonical.extend_from_slice(&node.hi.to_bits().to_le_bytes());
    }
    let mut canonical_menu: Vec<&Probe> = menu.iter().collect();
    canonical_menu.sort_by(|left, right| left.name.cmp(&right.name));
    push_u32(&mut canonical, canonical_menu.len(), "probe menu")?;
    for probe in canonical_menu {
        push_text(&mut canonical, &probe.name, "probe name")?;
        push_text(&mut canonical, &probe.target, "probe target")?;
        canonical.extend_from_slice(&probe.cost.to_bits().to_le_bytes());
        canonical.extend_from_slice(&probe.shrink.to_bits().to_le_bytes());
        canonical.push(match probe.kind {
            ProbeKind::Computational => 0,
            ProbeKind::Physical => 1,
        });
    }
    Ok(hash_domain(RANKED_MENU_CONTEXT_DOMAIN, &canonical))
}

#[derive(Debug, Clone, Copy)]
struct PreparedProbe {
    node_idx: usize,
    post_lo: f64,
    post_hi: f64,
}

fn prepare_probes(
    nodes: &[UncertaintyNode],
    menu: &[Probe],
    targets: &[usize],
) -> Result<Vec<PreparedProbe>, VoiError> {
    let mut prepared = Vec::with_capacity(menu.len());
    for (probe, &node_idx) in menu.iter().zip(targets) {
        let node = node_at(nodes, node_idx)?;
        let contracted_left = (node.nominal - node.lo) * probe.shrink;
        let contracted_right = (node.hi - node.nominal) * probe.shrink;
        let post_lo = node.nominal - contracted_left;
        let post_hi = node.nominal + contracted_right;
        let post_width = post_hi - post_lo;
        let expected_width = (node.hi - node.lo) * probe.shrink;
        if !contracted_left.is_finite()
            || !contracted_right.is_finite()
            || !post_lo.is_finite()
            || !post_hi.is_finite()
            || !post_width.is_finite()
            || !expected_width.is_finite()
            || (node.nominal > node.lo && contracted_left == 0.0)
            || (node.hi > node.nominal && contracted_right == 0.0)
            || post_lo < node.lo
            || post_lo > node.nominal
            || post_hi < node.nominal
            || post_hi > node.hi
            || (node.hi > node.lo && post_width == 0.0)
        {
            return Err(VoiError::ArithmeticRefusal {
                operation: "post-probe interval contraction",
                subject: probe.name.clone(),
            });
        }
        prepared.push(PreparedProbe {
            node_idx,
            post_lo,
            post_hi,
        });
    }
    Ok(prepared)
}

/// Rank the probe menu by sampled flip-fraction reduction per dollar for the live
/// decision — MYOPIC one-step VoI (each probe is evaluated against the
/// CURRENT state only; no sequential tree).
///
/// # Errors
/// [`VoiError`] when the decision, node set, menu, targets, grid, probe
/// economics, callback margins, or derived arithmetic are invalid.
pub fn rank_purchases(
    decision: &LiveDecision<'_>,
    nodes: &[UncertaintyNode],
    menu: &[Probe],
    grid: usize,
) -> Result<RankedMenu, VoiError> {
    validate_nodes(decision, nodes)?;
    validate_grid(grid)?;
    let targets = validate_menu(nodes, menu)?;
    let evaluations = grid
        .checked_add(1)
        .and_then(|per_sweep| per_sweep.checked_mul(2))
        .and_then(|per_probe| menu.len().checked_mul(per_probe))
        .ok_or_else(|| VoiError::ArithmeticRefusal {
            operation: "ranking evaluation count",
            subject: "probe menu".to_string(),
        })?;
    validate_evaluations(evaluations)?;
    // All input-derived intervals are prepared before the first callback, so
    // a malformed later probe cannot leave observable partial callback work.
    let prepared = prepare_probes(nodes, menu, &targets)?;
    let context_id = ranked_menu_context(nodes, menu, grid)?;

    let mut ranked = Vec::with_capacity(menu.len());
    for (probe, prepared) in menu.iter().zip(&prepared) {
        let node = node_at(nodes, prepared.node_idx)?;
        let flip_before =
            flip_probability_validated(decision, nodes, prepared.node_idx, node.lo, node.hi, grid)?;
        let flip_after = flip_probability_validated(
            decision,
            nodes,
            prepared.node_idx,
            prepared.post_lo,
            prepared.post_hi,
            grid,
        )?;
        let score = (flip_before - flip_after).max(0.0) / probe.cost;
        if !score.is_finite() || score < 0.0 {
            return Err(VoiError::ArithmeticRefusal {
                operation: "sampled flip-fraction-per-dollar score",
                subject: probe.name.clone(),
            });
        }
        ranked.push(RankedPurchase {
            probe: probe.clone(),
            flip_before,
            flip_after,
            score,
        });
    }
    ranked.sort_by(compare_ranked);
    Ok(RankedMenu {
        rows: ranked,
        context_id,
        grid,
    })
}

/// Surface a structured QUERY-RESULT HINT. Every scalar is explicitly a
/// grid-sampled estimate; a sampled zero is never rendered as proof that no
/// evidence could change the decision.
#[must_use]
pub fn hint_for_query(ranked: &RankedMenu) -> QueryHint {
    QueryHint {
        context_id: ranked.context_id,
        grid: ranked.grid,
        purchase: ranked.rows.iter().find(|row| row.score > 0.0).cloned(),
    }
}

/// The audit verdict for reporting. This enum is not scheduling authority;
/// only the private-construction [`SchedulingAuthority`] capability is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditVerdict {
    /// Anytime-valid evidence crossed the fixed activation threshold.
    KeepScheduling,
    /// Evidence is absent, insufficient, or has not crossed the threshold.
    DemoteToReporting,
}

/// One validated matched-cost prospective-audit observation.
///
/// Fields are private so raw booleans and unmatched prices cannot enter the
/// e-process without identity, provenance, and economic validation.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchedAuditRecord {
    observation_id: String,
    recommended_id: String,
    alternative_id: String,
    provenance: String,
    matched_cost: f64,
    recommended_changed_decision: bool,
    alternative_changed_decision: bool,
}

impl MatchedAuditRecord {
    /// Construct one matched-cost comparison.
    ///
    /// # Errors
    /// [`VoiError`] unless identities/provenance are bounded visible ASCII,
    /// candidates differ, and both finite positive costs are bit-identical.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        observation_id: impl Into<String>,
        recommended_id: impl Into<String>,
        alternative_id: impl Into<String>,
        provenance: impl Into<String>,
        recommended_cost: f64,
        alternative_cost: f64,
        recommended_changed_decision: bool,
        alternative_changed_decision: bool,
    ) -> Result<Self, VoiError> {
        let observation_id = observation_id.into();
        let recommended_id = recommended_id.into();
        let alternative_id = alternative_id.into();
        let provenance = provenance.into();
        for (kind, value) in [
            ("audit observation", observation_id.as_str()),
            ("recommended purchase", recommended_id.as_str()),
            ("alternative purchase", alternative_id.as_str()),
            ("audit provenance", provenance.as_str()),
        ] {
            validate_name(kind, 0, value)?;
        }
        if recommended_id == alternative_id {
            return Err(VoiError::InvalidAuditPair {
                observation: observation_id,
            });
        }
        if !recommended_cost.is_finite()
            || recommended_cost <= 0.0
            || recommended_cost.to_bits() != alternative_cost.to_bits()
        {
            return Err(VoiError::InvalidAuditCost {
                observation: observation_id,
                recommended_cost,
                alternative_cost,
            });
        }
        Ok(Self {
            observation_id,
            recommended_id,
            alternative_id,
            provenance,
            matched_cost: recommended_cost,
            recommended_changed_decision,
            alternative_changed_decision,
        })
    }

    /// Stable observation identity used to prevent duplicate evidence.
    #[must_use]
    pub fn observation_id(&self) -> &str {
        &self.observation_id
    }

    /// Recommended-purchase identity.
    #[must_use]
    pub fn recommended_id(&self) -> &str {
        &self.recommended_id
    }

    /// Matched alternative-purchase identity.
    #[must_use]
    pub fn alternative_id(&self) -> &str {
        &self.alternative_id
    }

    /// Caller-supplied provenance identity.
    #[must_use]
    pub fn provenance(&self) -> &str {
        &self.provenance
    }

    /// Exact matched cost.
    #[must_use]
    pub fn matched_cost(&self) -> f64 {
        self.matched_cost
    }

    /// Whether the recommended purchase changed the realized decision.
    #[must_use]
    pub fn recommended_changed_decision(&self) -> bool {
        self.recommended_changed_decision
    }

    /// Whether the matched alternative changed the realized decision.
    #[must_use]
    pub fn alternative_changed_decision(&self) -> bool {
        self.alternative_changed_decision
    }
}

/// Unforgeable-in-safe-Rust scheduling capability minted only by a successful
/// bounded e-process audit. Construction and fields are module-private. The
/// capability proves that supplied records crossed policy; ledger
/// authentication of those records is a separate boundary.
#[derive(Debug)]
pub struct SchedulingAuthority {
    audit_context_id: ContentHash,
    observations: usize,
    log_e_value: f64,
}

/// One authorized, single-epoch scheduling decision. The receipt retains the
/// ranked snapshot, audit evidence root, and exact budget transition instead of
/// returning a provenance-free probe row.
#[derive(Debug, PartialEq)]
pub struct ScheduledPurchase {
    purchase: RankedPurchase,
    ranked_context_id: ContentHash,
    ranked_grid: usize,
    audit_context_id: ContentHash,
    audit_observations: usize,
    audit_log_e_value: f64,
    budget_dollars: f64,
    remaining_budget_dollars: f64,
}

impl ScheduledPurchase {
    /// Authorized purchase.
    #[must_use]
    pub fn purchase(&self) -> &RankedPurchase {
        &self.purchase
    }

    /// Ranked node/menu/grid snapshot identity.
    #[must_use]
    pub fn ranked_context_id(&self) -> ContentHash {
        self.ranked_context_id
    }

    /// Midpoint grid supporting the sampled purchase score.
    #[must_use]
    pub fn ranked_grid(&self) -> usize {
        self.ranked_grid
    }

    /// Anytime-valid matched-audit evidence identity.
    #[must_use]
    pub fn audit_context_id(&self) -> ContentHash {
        self.audit_context_id
    }

    /// Matched-cost observation count supporting authority.
    #[must_use]
    pub fn audit_observations(&self) -> usize {
        self.audit_observations
    }

    /// Final log e-value supporting authority.
    #[must_use]
    pub fn audit_log_e_value(&self) -> f64 {
        self.audit_log_e_value
    }

    /// Admitted scheduling budget in dollars.
    #[must_use]
    pub fn budget_dollars(&self) -> f64 {
        self.budget_dollars
    }

    /// Exact remaining budget in dollars after this one purchase.
    #[must_use]
    pub fn remaining_budget_dollars(&self) -> f64 {
        self.remaining_budget_dollars
    }
}

impl SchedulingAuthority {
    /// Content identity of the matched-cost evidence prefix.
    #[must_use]
    pub fn audit_context_id(&self) -> ContentHash {
        self.audit_context_id
    }

    /// Number of observations supporting activation.
    #[must_use]
    pub fn observations(&self) -> usize {
        self.observations
    }

    /// Final log e-value supporting activation.
    #[must_use]
    pub fn log_e_value(&self) -> f64 {
        self.log_e_value
    }
}

/// Bounded prospective-audit result. A reporting verdict is intentionally
/// separate from the optional private-construction scheduling capability.
#[derive(Debug)]
pub struct AuditReport {
    audit_context_id: ContentHash,
    observations: usize,
    log_e_value: f64,
    authority: Option<SchedulingAuthority>,
}

impl AuditReport {
    /// Reporting verdict.
    #[must_use]
    pub fn verdict(&self) -> AuditVerdict {
        if self.authority.is_some() {
            AuditVerdict::KeepScheduling
        } else {
            AuditVerdict::DemoteToReporting
        }
    }

    /// Scheduling capability, absent until the fixed anytime-valid threshold
    /// is satisfied.
    #[must_use]
    pub fn authority(&self) -> Option<&SchedulingAuthority> {
        self.authority.as_ref()
    }

    /// Content identity of the canonical evidence prefix.
    #[must_use]
    pub fn audit_context_id(&self) -> ContentHash {
        self.audit_context_id
    }

    /// Number of matched-cost observations evaluated.
    #[must_use]
    pub fn observations(&self) -> usize {
        self.observations
    }

    /// Final log e-value, useful for reporting progress before activation.
    #[must_use]
    pub fn log_e_value(&self) -> f64 {
        self.log_e_value
    }
}

fn audit_context(records: &[&MatchedAuditRecord]) -> Result<ContentHash, VoiError> {
    let mut canonical = Vec::new();
    canonical.extend_from_slice(&1u32.to_le_bytes());
    canonical.extend_from_slice(&VOI_AUDIT_ALPHA.to_bits().to_le_bytes());
    push_u32(
        &mut canonical,
        MAX_VOI_AUDIT_RECORDS,
        "maximum audit records",
    )?;
    push_u32(&mut canonical, records.len(), "audit records")?;
    for record in records {
        push_text(&mut canonical, &record.observation_id, "audit observation")?;
        push_text(
            &mut canonical,
            &record.recommended_id,
            "recommended purchase",
        )?;
        push_text(
            &mut canonical,
            &record.alternative_id,
            "alternative purchase",
        )?;
        push_text(&mut canonical, &record.provenance, "audit provenance")?;
        canonical.extend_from_slice(&record.matched_cost.to_bits().to_le_bytes());
        canonical.push(u8::from(record.recommended_changed_decision));
        canonical.push(u8::from(record.alternative_changed_decision));
    }
    Ok(hash_domain(AUDIT_CONTEXT_DOMAIN, &canonical))
}

/// Evaluate a canonical matched-cost evidence prefix with an anytime-valid
/// pairwise e-process. Input order is non-authoritative: observation identity
/// defines the deterministic replay order.
///
/// Records are caller-supplied and content-bound, not ledger-authenticated.
/// See `frankensim-wk4m` for signed outcomes, freshness, and expiry.
///
/// # Errors
/// [`VoiError`] when the bounded record limit or unique-identity invariant is
/// violated, or the e-process produces invalid arithmetic.
pub fn audit_scheduling(records: &[MatchedAuditRecord]) -> Result<AuditReport, VoiError> {
    validate_size("VoI audit records", records.len(), 0, MAX_VOI_AUDIT_RECORDS)?;
    let mut canonical: Vec<&MatchedAuditRecord> = records.iter().collect();
    canonical.sort_by(|left, right| left.observation_id.cmp(&right.observation_id));
    for pair in canonical.windows(2) {
        if pair[0].observation_id == pair[1].observation_id {
            return Err(VoiError::DuplicateAuditObservation {
                observation: pair[0].observation_id.clone(),
            });
        }
    }
    let audit_context_id = audit_context(&canonical)?;
    let mut race = PairwiseRace::new(LossSpan::ONE);
    for record in &canonical {
        let recommended_loss = f64::from(u8::from(!record.recommended_changed_decision));
        let alternative_loss = f64::from(u8::from(!record.alternative_changed_decision));
        race.observe(recommended_loss, alternative_loss)
            .map_err(|_| VoiError::ArithmeticRefusal {
                operation: "VoI matched-cost e-process",
                subject: record.observation_id.clone(),
            })?;
    }
    let log_e_value = race.log_e_value();
    if !log_e_value.is_finite() {
        return Err(VoiError::ArithmeticRefusal {
            operation: "VoI audit log e-value",
            subject: audit_context_id.to_hex(),
        });
    }
    let authority = race
        .a_beats_b(VOI_AUDIT_ALPHA)
        .then_some(SchedulingAuthority {
            audit_context_id,
            observations: records.len(),
            log_e_value,
        });
    Ok(AuditReport {
        audit_context_id,
        observations: records.len(),
        log_e_value,
        authority,
    })
}

/// Execute at most one highest-value affordable purchase for this ranking
/// epoch. The caller must obtain new evidence, update the uncertainty snapshot,
/// and rerank before another purchase.
///
/// # Errors
/// [`VoiError`] when the budget is malformed, no scheduling authority was
/// minted, or finite positive cost cannot decrease the budget monotonically.
pub fn schedule_probes(
    ranked: RankedMenu,
    budget: f64,
    authority: Option<&SchedulingAuthority>,
) -> Result<Option<ScheduledPurchase>, VoiError> {
    if !budget.is_finite() || budget < 0.0 {
        return Err(VoiError::InvalidBudget { budget });
    }
    let authority = authority.ok_or(VoiError::MissingSchedulingAuthority)?;
    let ranked_context_id = ranked.context_id;
    let ranked_grid = ranked.grid;
    let Some(purchase) = ranked
        .rows
        .into_iter()
        .find(|row| row.score > 0.0 && row.probe.cost <= budget)
    else {
        return Ok(None);
    };
    let remaining = budget - purchase.probe.cost;
    if !remaining.is_finite() || remaining < 0.0 || remaining >= budget {
        return Err(VoiError::ArithmeticRefusal {
            operation: "remaining-budget subtraction",
            subject: purchase.probe.name.clone(),
        });
    }
    Ok(Some(ScheduledPurchase {
        purchase,
        ranked_context_id,
        ranked_grid,
        audit_context_id: authority.audit_context_id,
        audit_observations: authority.observations,
        audit_log_e_value: authority.log_e_value,
        budget_dollars: budget,
        remaining_budget_dollars: remaining,
    }))
}
