//! Sealed cost-model authority (bead 2pmb).
//!
//! [`crate::cost::CostModel`] is deliberately freely constructible —
//! it is provisional MATH. Authority is a different thing: the exact
//! roofline loader spends real validation (receipt scope, provenance
//! edges, dependency digests, build identity) before trusting a tune
//! row, and that work must survive the API boundary instead of being
//! discarded into an indistinguishable struct.
//!
//! [`SealedCostModel`] is the carrier. Its `ExactRooflineReceipt`
//! class is mintable ONLY by [`crate::oracle::cost_model_from_tune`]
//! after full validation; the fields are private, so a caller-fitted
//! model can never impersonate one. Tests and non-authoritative hints
//! use [`SealedCostModel::provisional_unaudited`], which works
//! identically but stamps every prediction — and therefore every
//! downstream admission finding and session receipt — with
//! [`CostEvidenceClass::ProvisionalUnaudited`]. Composition cannot
//! upgrade the class: authority follows the mint, not the arithmetic.

use crate::cost::{CostModel, CostPrediction, CostRefusal};

/// How a sealed model's evidence was established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostEvidenceClass {
    /// Minted by the exact roofline loader after full receipt,
    /// provenance, dependency, and build-identity validation.
    ExactRooflineReceipt,
    /// Explicitly provisional: caller-fitted observations with NO
    /// receipt validation. Useful for tests and non-authoritative
    /// hints; every consumer sees this label and must surface it.
    ProvisionalUnaudited,
}

impl CostEvidenceClass {
    /// Stable name for findings and receipts.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            CostEvidenceClass::ExactRooflineReceipt => "exact-roofline-receipt",
            CostEvidenceClass::ProvisionalUnaudited => "provisional-unaudited",
        }
    }
}

/// The validated scope a sealed model speaks for. For provisional
/// models every receipt-derived field is the literal string
/// `"provisional"` / zero — visibly not a receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostModelScope {
    kernel: String,
    shape_class: String,
    machine: Vec<u8>,
    run_receipt: String,
    op: u64,
    build_identity: String,
    recorded_at_ns: i64,
}

impl CostModelScope {
    pub(crate) fn from_validated(
        kernel: String,
        shape_class: String,
        machine: Vec<u8>,
        run_receipt: String,
        op: u64,
        build_identity: String,
        recorded_at_ns: i64,
    ) -> CostModelScope {
        CostModelScope {
            kernel,
            shape_class,
            machine,
            run_receipt,
            op,
            build_identity,
            recorded_at_ns,
        }
    }

    fn provisional(label: &str) -> CostModelScope {
        CostModelScope {
            kernel: format!("provisional:{label}"),
            shape_class: "provisional".to_string(),
            machine: Vec::new(),
            run_receipt: "provisional".to_string(),
            op: 0,
            build_identity: "provisional".to_string(),
            recorded_at_ns: 0,
        }
    }

    /// The exact tune kernel (or `provisional:<label>`).
    #[must_use]
    pub fn kernel(&self) -> &str {
        &self.kernel
    }

    /// The exact tune shape class.
    #[must_use]
    pub fn shape_class(&self) -> &str {
        &self.shape_class
    }

    /// The exact roofline machine key bytes (empty for provisional).
    #[must_use]
    pub fn machine(&self) -> &[u8] {
        &self.machine
    }

    /// The finalized-run receipt digest the row was bound to.
    #[must_use]
    pub fn run_receipt(&self) -> &str {
        &self.run_receipt
    }

    /// The ledger operation id the evidence was recorded under.
    #[must_use]
    pub fn op(&self) -> u64 {
        self.op
    }

    /// The validated build identity.
    #[must_use]
    pub fn build_identity(&self) -> &str {
        &self.build_identity
    }

    /// When the evidence operation finished (ledger nanoseconds).
    #[must_use]
    pub fn recorded_at_ns(&self) -> i64 {
        self.recorded_at_ns
    }
}

/// A cost model whose provenance survived the loader boundary.
/// Private fields: the `ExactRooflineReceipt` class cannot be forged
/// outside this crate.
#[derive(Debug, Clone)]
pub struct SealedCostModel {
    model: CostModel,
    scope: CostModelScope,
    class: CostEvidenceClass,
}

impl SealedCostModel {
    pub(crate) fn mint_exact(model: CostModel, scope: CostModelScope) -> SealedCostModel {
        SealedCostModel {
            model,
            scope,
            class: CostEvidenceClass::ExactRooflineReceipt,
        }
    }

    /// Wrap a caller-fitted model as EXPLICITLY provisional evidence.
    /// The label lands in the scope's kernel field and the class lands
    /// in every prediction; nothing downstream can mistake this for
    /// receipt-backed authority.
    #[must_use]
    pub fn provisional_unaudited(model: CostModel, label: &str) -> SealedCostModel {
        SealedCostModel {
            model,
            scope: CostModelScope::provisional(label),
            class: CostEvidenceClass::ProvisionalUnaudited,
        }
    }

    /// Observation count behind the fit.
    #[must_use]
    pub fn n_obs(&self) -> usize {
        self.model.n_obs()
    }

    /// The validated (or visibly provisional) scope.
    #[must_use]
    pub fn scope(&self) -> &CostModelScope {
        &self.scope
    }

    /// The evidence class stamped into every prediction.
    #[must_use]
    pub fn evidence_class(&self) -> CostEvidenceClass {
        self.class
    }

    /// Predict wall cost at `size`, carrying scope and class.
    ///
    /// # Errors
    /// Exactly [`CostModel::predict`]'s refusals.
    pub fn predict(&self, size: f64) -> Result<SealedCostPrediction, CostRefusal> {
        let prediction = self.model.predict(size)?;
        Ok(SealedCostPrediction {
            prediction,
            kernel: self.scope.kernel.clone(),
            shape_class: self.scope.shape_class.clone(),
            run_receipt: self.scope.run_receipt.clone(),
            build_identity: self.scope.build_identity.clone(),
            recorded_at_ns: self.scope.recorded_at_ns,
            evidence: self.class,
        })
    }
}

/// A prediction that remembers what it speaks for. The numeric bands
/// are [`CostPrediction`]; the rest is the provenance the bead 2pmb
/// audit found being dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct SealedCostPrediction {
    /// The quantile bands (P10/P50/P90, n_obs, extrapolation flag).
    pub prediction: CostPrediction,
    /// The exact tune kernel (or `provisional:<label>`).
    pub kernel: String,
    /// The exact tune shape class.
    pub shape_class: String,
    /// The finalized-run receipt digest.
    pub run_receipt: String,
    /// The validated build identity.
    pub build_identity: String,
    /// Evidence-operation completion time (ledger nanoseconds; 0 for
    /// provisional).
    pub recorded_at_ns: i64,
    /// The class stamped at mint time — never upgraded downstream.
    pub evidence: CostEvidenceClass,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::CostObservation;

    fn fitted() -> CostModel {
        let obs: Vec<CostObservation> = (1..=6)
            .map(|k| CostObservation {
                size: f64::from(k) * 256.0,
                cost_s: 0.05 * f64::from(k) * 256.0,
            })
            .collect();
        CostModel::fit(&obs).expect("fits")
    }

    #[test]
    fn provisional_mint_labels_everything_and_upgrades_nothing() {
        let sealed = SealedCostModel::provisional_unaudited(fitted(), "unit-test");
        assert_eq!(
            sealed.evidence_class(),
            CostEvidenceClass::ProvisionalUnaudited
        );
        assert_eq!(sealed.scope().kernel(), "provisional:unit-test");
        assert_eq!(sealed.scope().run_receipt(), "provisional");
        assert_eq!(sealed.scope().op(), 0);
        assert!(sealed.scope().machine().is_empty());
        let prediction = sealed.predict(512.0).expect("predicts");
        assert_eq!(
            prediction.evidence,
            CostEvidenceClass::ProvisionalUnaudited,
            "the class travels into every prediction"
        );
        assert_eq!(prediction.kernel, "provisional:unit-test");
        assert_eq!(prediction.recorded_at_ns, 0);
        // The math is untouched by the seal: bands match the raw model.
        let raw = fitted().predict(512.0).expect("raw predicts");
        assert_eq!(prediction.prediction, raw);
    }

    #[test]
    fn exact_mint_carries_the_full_validated_scope() {
        let scope = CostModelScope::from_validated(
            "simd-axpy-f64".to_string(),
            "roofline-v1:run=abc:op=41".to_string(),
            vec![7u8; 40],
            "abc".to_string(),
            41,
            "build-xyz".to_string(),
            1_784_000_000_000_000_000,
        );
        let sealed = SealedCostModel::mint_exact(fitted(), scope);
        assert_eq!(
            sealed.evidence_class(),
            CostEvidenceClass::ExactRooflineReceipt
        );
        let prediction = sealed.predict(512.0).expect("predicts");
        assert_eq!(prediction.evidence, CostEvidenceClass::ExactRooflineReceipt);
        assert_eq!(prediction.kernel, "simd-axpy-f64");
        assert_eq!(prediction.shape_class, "roofline-v1:run=abc:op=41");
        assert_eq!(prediction.run_receipt, "abc");
        assert_eq!(prediction.build_identity, "build-xyz");
        assert_eq!(prediction.recorded_at_ns, 1_784_000_000_000_000_000);
        assert_eq!(sealed.scope().op(), 41);
        assert_eq!(sealed.scope().machine(), &[7u8; 40][..]);
    }

    #[test]
    fn refusals_pass_through_the_seal() {
        let sealed = SealedCostModel::provisional_unaudited(CostModel::new(), "empty");
        assert!(
            sealed.predict(512.0).is_err(),
            "an empty model refuses through the seal exactly as raw"
        );
        assert_eq!(sealed.n_obs(), 0);
    }

    #[test]
    fn class_names_are_stable_receipt_vocabulary() {
        assert_eq!(
            CostEvidenceClass::ExactRooflineReceipt.name(),
            "exact-roofline-receipt"
        );
        assert_eq!(
            CostEvidenceClass::ProvisionalUnaudited.name(),
            "provisional-unaudited"
        );
    }
}
