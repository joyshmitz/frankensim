//! FALSIFIER PAIRING (addendum Proposal 6): no certificate ships without
//! an attached INDEPENDENT falsifier — a different algorithm on a
//! different code path that would catch the certificate being wrong —
//! and falsification compute is allocated by CONSEQUENCE × DOUBT.
//! Certificates prove what the model claims; falsifiers probe whether
//! the claims connect to reality; the gap between them is where
//! simulation systems silently rot. Popper as infrastructure.
//!
//! The base plan's ray-parity check on watertightness was the first
//! instance of this rule; this module promotes the instinct to
//! architecture: a registry (a class CANNOT register without ≥1
//! falsifier), the consequence×doubt budget allocator with honest
//! cold-start boundaries, hit→tombstone+bug wiring, and per-class YIELD
//! tracking so every falsifier pays rent.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Doubt never reaches zero: even a perfect record keeps a floor of
/// falsification pressure (the record could be luck or blind spots).
pub const DOUBT_FLOOR: f64 = 0.05;

/// Cold-start doubt: a class with NO history in a regime is maximally
/// doubted, never trusted by default.
pub const DOUBT_COLD_START: f64 = 1.0;

/// A claim with no downstream dependents still gets a minimal-but-
/// nonzero consequence weight (someone may read it directly).
pub const CONSEQUENCE_FLOOR: f64 = 0.01;

/// Yield threshold: at or above this many runs in a quarter, a class
/// with ZERO hits has "meaningful volume" and its budget share decays.
pub const RENT_VOLUME: u64 = 100;

/// Budget-share multiplier applied per rent review to yield-less
/// falsifiers (never to zero: the pairing rule itself is not killable).
pub const RENT_DECAY: f64 = 0.5;

/// Floor on the decayed share multiplier.
pub const RENT_SHARE_FLOOR: f64 = 0.1;

/// One registered falsifier: an independent check on a certificate
/// class, running a DIFFERENT algorithm on a DIFFERENT code path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FalsifierSpec {
    /// Stable falsifier name (e.g. "ray-parity-sampling").
    pub name: String,
    /// The independent method, stated (audit text).
    pub method: String,
}

/// Registration failure (the no-falsifier-no-ship rule at its source).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FalsifyError {
    /// A certificate class tried to register without any falsifier.
    NoFalsifier {
        /// The offending class.
        class: String,
    },
    /// Duplicate class registration.
    Duplicate {
        /// The class.
        class: String,
    },
    /// A query named an unregistered class.
    Unknown {
        /// The class.
        class: String,
    },
}

impl core::fmt::Display for FalsifyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FalsifyError::NoFalsifier { class } => write!(
                f,
                "certificate class {class:?} cannot register without a falsifier \
                 (no-falsifier-no-ship)"
            ),
            FalsifyError::Duplicate { class } => write!(f, "class {class:?} already registered"),
            FalsifyError::Unknown { class } => write!(f, "unknown certificate class {class:?}"),
        }
    }
}

impl std::error::Error for FalsifyError {}

/// The falsifier registry: certificate class → its independent checks.
#[derive(Debug, Default)]
pub struct FalsifierRegistry {
    classes: BTreeMap<String, Vec<FalsifierSpec>>,
}

impl FalsifierRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        FalsifierRegistry::default()
    }

    /// The STARTING REGISTRY from the proposal: one independent falsifier
    /// per shipped certificate class.
    #[must_use]
    pub fn standard() -> Self {
        let mut r = FalsifierRegistry::new();
        let pairs: [(&str, &str, &str); 6] = [
            (
                "watertightness",
                "ray-parity-sampling",
                "random rays must cross the surface an even number of times \
                 (independent of the sheaf/winding certificate path)",
            ),
            (
                "conservation",
                "global-flux-audit",
                "independent global flux balance on a DIFFERENT quadrature rule",
            ),
            (
                "adjoint-gradient",
                "finite-difference-spot-check",
                "central differences along random directions, independent of the tape",
            ),
            (
                "surrogate-accept",
                "held-out-point-evaluation",
                "full-fidelity evaluation at points the surrogate never saw",
            ),
            (
                "symmetry-block-solve",
                "occasional-full-solve",
                "solve the UNREDUCED system on random instances and compare",
            ),
            (
                "validated-color",
                "held-out-experimental-anchor",
                "compare against experimental anchors withheld from calibration",
            ),
        ];
        for (class, name, method) in pairs {
            r.register(
                class,
                vec![FalsifierSpec {
                    name: name.to_string(),
                    method: method.to_string(),
                }],
            )
            .expect("standard registry is well-formed");
        }
        r
    }

    /// Register a certificate class with its falsifiers. REFUSES an
    /// empty falsifier list — that is the rule.
    ///
    /// # Errors
    /// [`FalsifyError::NoFalsifier`] / [`FalsifyError::Duplicate`].
    pub fn register(
        &mut self,
        class: &str,
        falsifiers: Vec<FalsifierSpec>,
    ) -> Result<(), FalsifyError> {
        if falsifiers.is_empty() {
            return Err(FalsifyError::NoFalsifier {
                class: class.to_string(),
            });
        }
        if self.classes.contains_key(class) {
            return Err(FalsifyError::Duplicate {
                class: class.to_string(),
            });
        }
        self.classes.insert(class.to_string(), falsifiers);
        Ok(())
    }

    /// The falsifiers for a class.
    ///
    /// # Errors
    /// [`FalsifyError::Unknown`].
    pub fn falsifiers(&self, class: &str) -> Result<&[FalsifierSpec], FalsifyError> {
        self.classes
            .get(class)
            .map(Vec::as_slice)
            .ok_or_else(|| FalsifyError::Unknown {
                class: class.to_string(),
            })
    }

    /// THE GAUNTLET GATE (no-falsifier-no-ship): every shipped
    /// certificate class must be registered with ≥1 falsifier. Returns
    /// the violating classes (empty = gate passes).
    #[must_use]
    pub fn ship_gate(&self, shipped_classes: &[&str]) -> Vec<String> {
        shipped_classes
            .iter()
            .filter(|c| !self.classes.contains_key(**c))
            .map(|c| (*c).to_string())
            .collect()
    }
}

/// Per-(class, regime) falsification history: the DOUBT source.
#[derive(Debug, Default)]
pub struct FalsifierHistory {
    /// (class, regime-key) → (passes, hits, compute spent).
    rows: BTreeMap<(String, String), (u64, u64, f64)>,
    /// Rent-decay multipliers per class (1.0 until decayed).
    share: BTreeMap<String, f64>,
}

/// A falsifier HIT: the certificate was wrong. Automatically a tombstone
/// AND a bug report against the certificate's estimator.
#[derive(Debug, Clone, PartialEq)]
pub struct FalsifierHit {
    /// Certificate class.
    pub class: String,
    /// Regime key the claim lived in.
    pub regime: String,
    /// Which falsifier caught it.
    pub falsifier: String,
    /// What disagreed (audit text).
    pub detail: String,
}

/// The tombstone record a hit generates (Proposal E consumes these).
#[derive(Debug, Clone, PartialEq)]
pub struct Tombstone {
    /// Canonical JSON payload.
    pub json: String,
}

/// The estimator bug report a hit generates.
#[derive(Debug, Clone, PartialEq)]
pub struct EstimatorBug {
    /// Canonical JSON payload.
    pub json: String,
}

impl FalsifierHistory {
    /// An empty history.
    #[must_use]
    pub fn new() -> Self {
        FalsifierHistory::default()
    }

    /// Record a PASS (the falsifier found nothing) with its compute cost.
    pub fn record_pass(&mut self, class: &str, regime: &str, compute_s: f64) {
        let row = self
            .rows
            .entry((class.to_string(), regime.to_string()))
            .or_insert((0, 0, 0.0));
        row.0 += 1;
        row.2 += compute_s;
    }

    /// Record a HIT: returns the tombstone + estimator bug report the
    /// wiring REQUIRES (callers ledger both).
    pub fn record_hit(&mut self, hit: &FalsifierHit, compute_s: f64) -> (Tombstone, EstimatorBug) {
        let row = self
            .rows
            .entry((hit.class.clone(), hit.regime.clone()))
            .or_insert((0, 0, 0.0));
        row.1 += 1;
        row.2 += compute_s;
        let mut t = String::from("{\"kind\":\"tombstone\",\"source\":\"falsifier-hit\"");
        let _ = write!(
            t,
            ",\"class\":\"{}\",\"regime\":\"{}\",\"falsifier\":\"{}\",\"detail\":{:?}}}",
            hit.class, hit.regime, hit.falsifier, hit.detail
        );
        let mut b = String::from("{\"kind\":\"estimator-bug\"");
        let _ = write!(
            b,
            ",\"class\":\"{}\",\"regime\":\"{}\",\"caught_by\":\"{}\",\"evidence\":{:?}}}",
            hit.class, hit.regime, hit.falsifier, hit.detail
        );
        (Tombstone { json: t }, EstimatorBug { json: b })
    }

    /// DOUBT for a class in a regime: `1 − pass rate`, with the
    /// cold-start boundary (no history → maximum doubt) and the floor
    /// (a perfect record never reaches zero doubt).
    #[must_use]
    pub fn doubt(&self, class: &str, regime: &str) -> f64 {
        match self.rows.get(&(class.to_string(), regime.to_string())) {
            None => DOUBT_COLD_START,
            Some((passes, hits, _)) => {
                let total = passes + hits;
                if total == 0 {
                    return DOUBT_COLD_START;
                }
                #[allow(clippy::cast_precision_loss)]
                let raw = 1.0 - (*passes as f64) / (total as f64);
                raw.max(DOUBT_FLOOR)
            }
        }
    }

    /// YIELD for a class: (true catches, compute spent, runs) across all
    /// regimes — the rent measurement.
    #[must_use]
    pub fn yield_of(&self, class: &str) -> (u64, f64, u64) {
        let mut hits = 0u64;
        let mut compute = 0.0f64;
        let mut runs = 0u64;
        for ((c, _), (p, h, s)) in &self.rows {
            if c == class {
                hits += h;
                compute += s;
                runs += p + h;
            }
        }
        (hits, compute, runs)
    }

    /// The current budget-share multiplier for a class.
    #[must_use]
    pub fn share(&self, class: &str) -> f64 {
        self.share.get(class).copied().unwrap_or(1.0)
    }

    /// The quarterly RENT REVIEW: classes with zero hits at meaningful
    /// volume have their share decayed (never below the floor — the
    /// pairing rule itself is not killable). Returns decayed classes.
    pub fn rent_review(&mut self) -> Vec<(String, f64)> {
        let classes: std::collections::BTreeSet<String> =
            self.rows.keys().map(|(c, _)| c.clone()).collect();
        let mut decayed = Vec::new();
        for class in classes {
            let (hits, _, runs) = self.yield_of(&class);
            if hits == 0 && runs >= RENT_VOLUME {
                let cur = self.share(&class);
                let next = (cur * RENT_DECAY).max(RENT_SHARE_FLOOR);
                self.share.insert(class.clone(), next);
                decayed.push((class, next));
            }
        }
        decayed
    }
}

/// One claim awaiting falsification budget.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaimContext {
    /// Certificate class.
    pub class: String,
    /// Regime key.
    pub regime: String,
    /// Downstream decision weight (DAG dependents; the ledger scores it).
    pub consequence: f64,
}

/// Allocate a job's reserved falsification budget across its claims by
/// `consequence × doubt × rent-share`, normalized. Boundaries: zero
/// claims → zero spend (empty vector); a claim with no dependents gets
/// the consequence floor; allocation is monotone in both factors.
#[must_use]
pub fn allocate_budget(
    total_budget_s: f64,
    claims: &[ClaimContext],
    history: &FalsifierHistory,
) -> Vec<f64> {
    if claims.is_empty() || total_budget_s <= 0.0 {
        return vec![0.0; claims.len()];
    }
    let weights: Vec<f64> = claims
        .iter()
        .map(|c| {
            let consequence = c.consequence.max(CONSEQUENCE_FLOOR);
            let doubt = history.doubt(&c.class, &c.regime);
            consequence * doubt * history.share(&c.class)
        })
        .collect();
    let total: f64 = weights.iter().sum();
    if total <= 0.0 {
        return vec![0.0; claims.len()];
    }
    weights.iter().map(|w| total_budget_s * w / total).collect()
}
