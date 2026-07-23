//! Deterministic final-envelope audits for user-facing QoI claims.
//!
//! A validated model card is regional evidence. This module checks every
//! operating point against every card consumed by a QoI, partitions partial
//! sweeps exactly, and emits a typed receipt. Any exit forces the effective
//! color to an unbounded estimate; an explicit override is retained only as
//! an acknowledgement and can never restore claim strength.

use crate::cards::axis_distance_to_validity;
use fs_evidence::{Color, ModelCard, ProvenanceHash, validate_color_payload};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// One named operating point in dimensionless-group space.
#[derive(Debug, Clone, PartialEq)]
pub struct OperatingPoint {
    /// Stable point identity used in receipts and exact sweep partitions.
    pub id: String,
    /// Dimensionless group values at this point.
    pub groups: BTreeMap<String, f64>,
}

/// One user-facing quantity and the model cards consumed to produce it.
#[derive(Debug, Clone, PartialEq)]
pub struct QoiClaim {
    /// Stable quantity identity.
    pub qoi: String,
    /// Color before the final operating-envelope audit.
    pub color: Color,
    /// Names of every consumed model card.
    pub model_cards: Vec<String>,
    /// Explicit authorization to proceed despite a demotion, if supplied.
    pub override_acknowledgement: Option<OverrideAcknowledgement>,
}

/// An explicit decision to proceed with a demoted output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideAcknowledgement {
    /// Identity of the actor accepting the limitation.
    pub actor: String,
    /// Human-readable reason retained in the receipt.
    pub reason: String,
}

/// Exact coverage class for the audited operating points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EnvelopeCoverage {
    /// Every point satisfies every consumed model card.
    FullyInDomain,
    /// At least one point is in-domain and at least one is out-of-domain.
    Partial,
    /// No point satisfies every consumed model card.
    FullyOutOfDomain,
}

impl EnvelopeCoverage {
    fn name(self) -> &'static str {
        match self {
            Self::FullyInDomain => "fully-in-domain",
            Self::Partial => "partial",
            Self::FullyOutOfDomain => "fully-out-of-domain",
        }
    }
}

/// Why one card axis rejected one operating point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AxisViolationKind {
    /// The operating point omitted a constrained axis.
    Missing,
    /// The operating point supplied NaN or infinity.
    NonFinite,
    /// The value is below the inclusive lower bound.
    Below,
    /// The value is above the inclusive upper bound.
    Above,
    /// The model card itself contains unusable bounds.
    InvalidBounds,
}

impl AxisViolationKind {
    fn name(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::NonFinite => "non-finite",
            Self::Below => "below",
            Self::Above => "above",
            Self::InvalidBounds => "invalid-bounds",
        }
    }
}

/// One named, distance-scored model-card violation.
#[derive(Debug, Clone, PartialEq)]
pub struct RegimeViolation {
    /// Operating point that violated the card.
    pub point: String,
    /// Model card name.
    pub card: String,
    /// Model card semantic version.
    pub card_version: String,
    /// Constrained dimensionless axis.
    pub axis: String,
    /// Observed value, or `None` when the point omitted this axis.
    pub observed: Option<f64>,
    /// Inclusive lower validity bound.
    pub lo: f64,
    /// Inclusive upper validity bound.
    pub hi: f64,
    /// Stable reason class.
    pub kind: AxisViolationKind,
    /// Per-axis log-space distance from [`axis_distance_to_validity`].
    pub distance: f64,
}

/// Stable identity of one model card consumed by a QoI.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConsumedModelCard {
    /// Model card name.
    pub name: String,
    /// Model card semantic version.
    pub version: String,
}

/// Final audit receipt for one QoI.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputClaimReceipt {
    /// Stable quantity identity.
    pub qoi: String,
    /// Color entering the final audit.
    pub original_color: Color,
    /// Color allowed at the product boundary.
    pub effective_color: Color,
    /// Incoming color retained for the exact in-domain partition, when nonempty.
    pub in_domain_color: Option<Color>,
    /// Demoted color assigned to the exact out-of-domain partition, when nonempty.
    pub out_of_domain_color: Option<Color>,
    /// Exact envelope coverage class.
    pub coverage: EnvelopeCoverage,
    /// Points satisfying every consumed card, sorted by identity.
    pub in_domain_points: Vec<String>,
    /// Points violating at least one consumed card, sorted by identity.
    pub out_of_domain_points: Vec<String>,
    /// Consumed card identities, sorted by name and deduplicated by registry law.
    pub model_cards: Vec<ConsumedModelCard>,
    /// Named violations, canonically sorted.
    pub violations: Vec<RegimeViolation>,
    /// Optional acknowledgement; never affects `effective_color`.
    pub override_acknowledgement: Option<OverrideAcknowledgement>,
}

impl OutputClaimReceipt {
    /// Whether the final audit weakened the output to an unbounded estimate.
    #[must_use]
    pub fn demoted(&self) -> bool {
        !self.out_of_domain_points.is_empty()
    }

    /// Canonical JSON projection for ledger/report handoff.
    #[must_use]
    pub fn to_canonical_json(&self) -> String {
        let mut out = String::with_capacity(512);
        let _ = write!(
            out,
            "{{\"qoi\":{},\"coverage\":{},\"original_color\":{},\"effective_color\":{},\"in_domain_color\":{},\"out_of_domain_color\":{},",
            json_string(&self.qoi),
            json_string(self.coverage.name()),
            color_json(&self.original_color),
            color_json(&self.effective_color),
            optional_color_json(self.in_domain_color.as_ref()),
            optional_color_json(self.out_of_domain_color.as_ref()),
        );
        out.push_str("\"model_cards\":[");
        for (index, card) in self.model_cards.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "{{\"name\":{},\"version\":{}}}",
                json_string(&card.name),
                json_string(&card.version)
            );
        }
        out.push(']');
        out.push(',');
        push_string_array(&mut out, "in_domain_points", &self.in_domain_points);
        out.push(',');
        push_string_array(&mut out, "out_of_domain_points", &self.out_of_domain_points);
        out.push_str(",\"violations\":[");
        for (index, violation) in self.violations.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "{{\"point\":{},\"card\":{},\"card_version\":{},\"axis\":{},\"observed\":{},\"bounds\":[{},{}],\"kind\":{},\"distance\":{}}}",
                json_string(&violation.point),
                json_string(&violation.card),
                json_string(&violation.card_version),
                json_string(&violation.axis),
                violation
                    .observed
                    .map_or_else(|| "null".to_string(), json_f64),
                json_f64(violation.lo),
                json_f64(violation.hi),
                json_string(violation.kind.name()),
                json_f64(violation.distance),
            );
        }
        out.push_str("],\"override_acknowledgement\":");
        if let Some(acknowledgement) = &self.override_acknowledgement {
            let _ = write!(
                out,
                "{{\"actor\":{},\"reason\":{}}}",
                json_string(&acknowledgement.actor),
                json_string(&acknowledgement.reason),
            );
        } else {
            out.push_str("null");
        }
        out.push('}');
        out
    }

    /// Deterministic report-ready no-claim entry.
    #[must_use]
    pub fn no_claim_markdown(&self) -> Option<String> {
        if !self.demoted() {
            return None;
        }
        let mut out = format!(
            "- `{}`: **estimated / no dispersion claim**; coverage `{}`; {} of {} operating points outside all-card intersection.",
            self.qoi,
            self.coverage.name(),
            self.out_of_domain_points.len(),
            self.in_domain_points.len() + self.out_of_domain_points.len(),
        );
        for violation in &self.violations {
            let observed = violation
                .observed
                .map_or_else(|| "missing".to_string(), |value| format!("{value:.6e}"));
            let _ = write!(
                out,
                " `{}` / `{}` / `{}`: {} vs [{:.6e}, {:.6e}], distance {:.6e}.",
                violation.point,
                violation.card,
                violation.axis,
                observed,
                violation.lo,
                violation.hi,
                violation.distance,
            );
        }
        if let Some(acknowledgement) = &self.override_acknowledgement {
            let _ = write!(
                out,
                " Override acknowledged by `{}`: {}; acknowledgement does not restore color.",
                acknowledgement.actor, acknowledgement.reason,
            );
        }
        Some(out)
    }
}

/// Deterministic collection of final QoI audit receipts.
#[derive(Debug, Clone, PartialEq)]
pub struct ProductOutputAudit {
    /// One receipt per QoI, sorted by QoI identity.
    pub receipts: Vec<OutputClaimReceipt>,
    /// Deterministic provenance of the canonical receipt collection.
    pub provenance: ProvenanceHash,
}

/// Structural input error for a final-output audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputAuditError {
    /// At least one operating point is required.
    NoOperatingPoints,
    /// An identity is empty or contains surrounding whitespace.
    InvalidIdentity {
        /// Input field.
        field: &'static str,
        /// Offending value.
        value: String,
    },
    /// A supposedly unique identity was repeated.
    DuplicateIdentity {
        /// Input field.
        field: &'static str,
        /// Repeated value.
        value: String,
    },
    /// A QoI did not declare any model cards.
    NoModelCards {
        /// Offending QoI.
        qoi: String,
    },
    /// A QoI references a card absent from the supplied registry.
    UnknownModelCard {
        /// Offending QoI.
        qoi: String,
        /// Missing card name.
        card: String,
    },
    /// A QoI entered the product boundary with a malformed color payload.
    InvalidColor {
        /// Offending QoI.
        qoi: String,
        /// Structural color diagnosis.
        reason: String,
    },
}

impl core::fmt::Display for OutputAuditError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoOperatingPoints => {
                write!(f, "output audit requires at least one operating point")
            }
            Self::InvalidIdentity { field, value } => {
                write!(f, "invalid {field} identity {value:?}")
            }
            Self::DuplicateIdentity { field, value } => {
                write!(f, "duplicate {field} identity {value:?}")
            }
            Self::NoModelCards { qoi } => write!(f, "QoI {qoi:?} consumed no model cards"),
            Self::UnknownModelCard { qoi, card } => {
                write!(f, "QoI {qoi:?} references unknown model card {card:?}")
            }
            Self::InvalidColor { qoi, reason } => {
                write!(f, "QoI {qoi:?} has invalid color: {reason}")
            }
        }
    }
}

impl std::error::Error for OutputAuditError {}

/// Audit every QoI/card/operating-point combination at the product boundary.
///
/// # Errors
/// Refuses missing points, malformed or duplicate identities, cardless QoIs,
/// duplicate registry names, and references to absent model cards.
pub fn audit_product_output(
    registry: &[ModelCard],
    operating_points: &[OperatingPoint],
    claims: &[QoiClaim],
) -> Result<ProductOutputAudit, OutputAuditError> {
    if operating_points.is_empty() {
        return Err(OutputAuditError::NoOperatingPoints);
    }
    let cards = unique_cards(registry)?;
    let points = unique_points(operating_points)?;
    let claims = unique_claims(claims)?;

    let mut receipts = Vec::with_capacity(claims.len());
    for claim in claims {
        let model_cards = canonical_card_names(claim)?;
        let selected_cards = model_cards
            .iter()
            .map(|name| {
                cards
                    .get(name)
                    .copied()
                    .ok_or_else(|| OutputAuditError::UnknownModelCard {
                        qoi: claim.qoi.clone(),
                        card: name.clone(),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let consumed_cards = selected_cards
            .iter()
            .map(|card| ConsumedModelCard {
                name: card.name.clone(),
                version: card.version.clone(),
            })
            .collect::<Vec<_>>();
        receipts.push(audit_claim(
            claim,
            &points,
            &consumed_cards,
            &selected_cards,
        ));
    }
    receipts.sort_by(|left, right| left.qoi.cmp(&right.qoi));
    let canonical = receipts
        .iter()
        .map(OutputClaimReceipt::to_canonical_json)
        .collect::<Vec<_>>()
        .join("\n");
    Ok(ProductOutputAudit {
        receipts,
        provenance: ProvenanceHash::of_bytes(canonical.as_bytes()),
    })
}

fn unique_cards(registry: &[ModelCard]) -> Result<BTreeMap<String, &ModelCard>, OutputAuditError> {
    let mut cards = BTreeMap::new();
    for card in registry {
        validate_identity("model-card", &card.name)?;
        validate_identity("model-card-version", &card.version)?;
        for axis in card.validity.bounds().keys() {
            validate_identity("model-card-axis", axis)?;
        }
        if cards.insert(card.name.clone(), card).is_some() {
            return Err(OutputAuditError::DuplicateIdentity {
                field: "model-card",
                value: card.name.clone(),
            });
        }
    }
    Ok(cards)
}

fn unique_points(
    operating_points: &[OperatingPoint],
) -> Result<Vec<&OperatingPoint>, OutputAuditError> {
    let mut by_id = BTreeMap::new();
    for point in operating_points {
        validate_identity("operating-point", &point.id)?;
        for axis in point.groups.keys() {
            validate_identity("operating-point-axis", axis)?;
        }
        if by_id.insert(point.id.clone(), point).is_some() {
            return Err(OutputAuditError::DuplicateIdentity {
                field: "operating-point",
                value: point.id.clone(),
            });
        }
    }
    Ok(by_id.into_values().collect())
}

fn unique_claims(claims: &[QoiClaim]) -> Result<Vec<&QoiClaim>, OutputAuditError> {
    let mut by_qoi = BTreeMap::new();
    for claim in claims {
        validate_identity("qoi", &claim.qoi)?;
        validate_color_payload(&claim.color).map_err(|error| OutputAuditError::InvalidColor {
            qoi: claim.qoi.clone(),
            reason: error.to_string(),
        })?;
        if by_qoi.insert(claim.qoi.clone(), claim).is_some() {
            return Err(OutputAuditError::DuplicateIdentity {
                field: "qoi",
                value: claim.qoi.clone(),
            });
        }
        if let Some(acknowledgement) = &claim.override_acknowledgement {
            validate_identity("override-actor", &acknowledgement.actor)?;
            validate_identity("override-reason", &acknowledgement.reason)?;
        }
    }
    Ok(by_qoi.into_values().collect())
}

fn canonical_card_names(claim: &QoiClaim) -> Result<Vec<String>, OutputAuditError> {
    if claim.model_cards.is_empty() {
        return Err(OutputAuditError::NoModelCards {
            qoi: claim.qoi.clone(),
        });
    }
    let mut names = BTreeSet::new();
    for name in &claim.model_cards {
        validate_identity("model-card-reference", name)?;
        names.insert(name.clone());
    }
    Ok(names.into_iter().collect())
}

fn validate_identity(field: &'static str, value: &str) -> Result<(), OutputAuditError> {
    if value.is_empty() || value.trim() != value {
        Err(OutputAuditError::InvalidIdentity {
            field,
            value: value.to_string(),
        })
    } else {
        Ok(())
    }
}

fn audit_claim(
    claim: &QoiClaim,
    points: &[&OperatingPoint],
    model_cards: &[ConsumedModelCard],
    selected_cards: &[&ModelCard],
) -> OutputClaimReceipt {
    let mut in_domain_points = Vec::new();
    let mut out_of_domain_points = Vec::new();
    let mut violations = Vec::new();
    for point in points {
        let before = violations.len();
        for card in selected_cards {
            collect_violations(point, card, &mut violations);
        }
        if before == violations.len() {
            in_domain_points.push(point.id.clone());
        } else {
            out_of_domain_points.push(point.id.clone());
        }
    }
    let coverage = if out_of_domain_points.is_empty() {
        EnvelopeCoverage::FullyInDomain
    } else if in_domain_points.is_empty() {
        EnvelopeCoverage::FullyOutOfDomain
    } else {
        EnvelopeCoverage::Partial
    };
    violations.sort_by(|left, right| {
        left.point
            .cmp(&right.point)
            .then(left.card.cmp(&right.card))
            .then(left.axis.cmp(&right.axis))
    });
    let out_of_domain_color = if out_of_domain_points.is_empty() {
        None
    } else {
        Some(demoted_color(claim, model_cards, &violations))
    };
    let effective_color = out_of_domain_color
        .clone()
        .unwrap_or_else(|| claim.color.clone());
    let in_domain_color = (!in_domain_points.is_empty()).then(|| claim.color.clone());
    OutputClaimReceipt {
        qoi: claim.qoi.clone(),
        original_color: claim.color.clone(),
        effective_color,
        in_domain_color,
        out_of_domain_color,
        coverage,
        in_domain_points,
        out_of_domain_points,
        model_cards: model_cards.to_vec(),
        violations,
        override_acknowledgement: claim.override_acknowledgement.clone(),
    }
}

fn collect_violations(
    point: &OperatingPoint,
    card: &ModelCard,
    violations: &mut Vec<RegimeViolation>,
) {
    for (axis, &(lo, hi)) in card.validity.bounds() {
        let observed = point.groups.get(axis).copied();
        let kind = if !lo.is_finite() || !hi.is_finite() || lo > hi {
            Some(AxisViolationKind::InvalidBounds)
        } else {
            match observed {
                None => Some(AxisViolationKind::Missing),
                Some(value) if !value.is_finite() => Some(AxisViolationKind::NonFinite),
                Some(value) if value < lo => Some(AxisViolationKind::Below),
                Some(value) if value > hi => Some(AxisViolationKind::Above),
                Some(_) => None,
            }
        };
        if let Some(kind) = kind {
            violations.push(RegimeViolation {
                point: point.id.clone(),
                card: card.name.clone(),
                card_version: card.version.clone(),
                axis: axis.clone(),
                observed,
                lo,
                hi,
                kind,
                distance: axis_distance_to_validity(observed, lo, hi),
            });
        }
    }
}

fn demoted_color(
    claim: &QoiClaim,
    model_cards: &[ConsumedModelCard],
    violations: &[RegimeViolation],
) -> Color {
    let mut identity = Vec::new();
    push_identity_field(&mut identity, b"regime-output-audit-v1");
    push_identity_field(&mut identity, claim.qoi.as_bytes());
    push_identity_field(&mut identity, &claim.color.canonical_bytes());
    for card in model_cards {
        push_identity_field(&mut identity, card.name.as_bytes());
        push_identity_field(&mut identity, card.version.as_bytes());
    }
    for violation in violations {
        push_identity_field(&mut identity, violation.point.as_bytes());
        push_identity_field(&mut identity, violation.card.as_bytes());
        push_identity_field(&mut identity, violation.card_version.as_bytes());
        push_identity_field(&mut identity, violation.axis.as_bytes());
        match violation.observed {
            Some(value) => {
                push_identity_field(&mut identity, b"present");
                push_identity_field(&mut identity, &value.to_bits().to_le_bytes());
            }
            None => push_identity_field(&mut identity, b"missing"),
        }
        push_identity_field(&mut identity, &violation.lo.to_bits().to_le_bytes());
        push_identity_field(&mut identity, &violation.hi.to_bits().to_le_bytes());
        push_identity_field(&mut identity, violation.kind.name().as_bytes());
        push_identity_field(&mut identity, &violation.distance.to_bits().to_le_bytes());
    }
    Color::Estimated {
        estimator: format!(
            "regime-audit:{:016x}",
            ProvenanceHash::of_bytes(&identity).0
        ),
        dispersion: f64::INFINITY,
    }
}

fn push_identity_field(identity: &mut Vec<u8>, bytes: &[u8]) {
    identity.extend_from_slice(bytes.len().to_string().as_bytes());
    identity.push(b':');
    identity.extend_from_slice(bytes);
}

fn push_string_array(out: &mut String, key: &str, values: &[String]) {
    let _ = write!(out, "{}:[", json_string(key));
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&json_string(value));
    }
    out.push(']');
}

fn json_f64(value: f64) -> String {
    if value.is_finite() {
        format!("{value:.17e}")
    } else {
        json_string(&format!("non-finite:{value}"))
    }
}

fn color_payload_json(color: &Color) -> String {
    match color {
        Color::Verified { lo, hi } => {
            format!("{{\"interval\":[{},{}]}}", json_f64(*lo), json_f64(*hi))
        }
        Color::Validated { regime, dataset } => {
            let mut axes = String::new();
            for (index, (axis, (lo, hi))) in regime.bounds().iter().enumerate() {
                if index > 0 {
                    axes.push(',');
                }
                let _ = write!(
                    axes,
                    "{}:[{},{}]",
                    json_string(axis),
                    json_f64(*lo),
                    json_f64(*hi)
                );
            }
            format!(
                "{{\"dataset\":{},\"regime\":{{{axes}}}}}",
                json_string(dataset)
            )
        }
        Color::Estimated {
            estimator,
            dispersion,
        } => format!(
            "{{\"estimator\":{},\"dispersion\":{}}}",
            json_string(estimator),
            json_f64(*dispersion)
        ),
    }
}

fn color_json(color: &Color) -> String {
    format!(
        "{{\"name\":{},\"payload\":{}}}",
        json_string(color.name()),
        color_payload_json(color)
    )
}

fn optional_color_json(color: Option<&Color>) -> String {
    color.map_or_else(|| "null".to_string(), color_json)
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if u32::from(character) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(character));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}
