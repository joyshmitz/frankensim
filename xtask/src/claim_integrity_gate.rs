//! Claim-integrity promotion gate (bead
//! `frankensim-extreal-program-f85xj.2.3`).
//!
//! The E02 synthesis's first immediate action: *any bug that can emit a
//! stronger epistemic claim than the evidence supports should block public
//! capability promotion.* Without a machine gate that is an intention, not a
//! control. This is the enforcement seam between the claim-integrity inventory
//! (`docs/CLAIM_INTEGRITY.md`, bead `.2.1`) and the capability maturity
//! registry (`capability-maturity.json`, bead `.16.1`).
//!
//! The rule, in one sentence: **a capability may not be promoted while an open
//! `severity:default-path` claim-integrity defect is in scope for it.**
//!
//! Three design commitments, each of which is a refusal rather than a
//! convenience:
//!
//! 1. **Fail closed on unreadable evidence.** If `.beads/issues.jsonl` cannot
//!    be read or a row cannot be parsed, the gate REFUSES. Another agent may be
//!    flushing the store mid-read; a gate that passed because it could not look
//!    would itself be the defect class it exists to enforce.
//! 2. **Fail closed on ambiguous scope.** A defect with no `crate:` scope
//!    blocks EVERY promotion, not none. An unscoped defect is one nobody has
//!    localised, which is a reason for more caution, not less.
//! 3. **Demotions are always allowed.** Lowering a claim is how the registry
//!    stays honest and must never be procedurally harder than raising one.
//!
//! Scope matching is deliberately coarse: a defect blocks a capability when
//! they name any crate in common. Coarse means over-blocking, which is the
//! safe direction — a promotion delayed by an unrelated-looking defect costs a
//! conversation, while a promotion admitted past a real one costs the claim.

use crate::maturity::{self, CapabilityLevels};
use crate::{PolicyNote, Violation};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const CHECK: &str = "claim-integrity-gate";
const BEADS_FILE: &str = ".beads/issues.jsonl";
const CLASS_LABEL: &str = "claim-integrity";
const GATING_SEVERITY: &str = "severity:default-path";
const CRATE_LABEL_PREFIX: &str = "crate:";
/// Beads rows are large; refuse rather than allocate without bound.
const MAX_BEADS_BYTES: u64 = 64 * 1024 * 1024;

pub struct GateReport {
    pub violations: Vec<Violation>,
    pub decisions: Vec<PolicyNote>,
}

fn violation(entity: &str, detail: String) -> Violation {
    Violation {
        check: CHECK,
        crate_name: entity.to_string(),
        detail,
    }
}

fn note(entity: &str, verdict: &'static str, detail: String) -> PolicyNote {
    PolicyNote {
        check: CHECK,
        crate_name: entity.to_string(),
        verdict,
        detail,
    }
}

/// One open, gating claim-integrity defect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatingDefect {
    pub id: String,
    /// Crate scopes from `crate:` labels. EMPTY MEANS GLOBAL — the defect
    /// blocks every promotion, because an unlocalised defect is not a
    /// harmless one.
    pub scopes: BTreeSet<String>,
}

impl GatingDefect {
    fn blocks(&self, capability_crates: &BTreeSet<String>) -> bool {
        self.scopes.is_empty() || self.scopes.intersection(capability_crates).next().is_some()
    }

    fn scope_text(&self) -> String {
        if self.scopes.is_empty() {
            "<unscoped: blocks globally>".to_string()
        } else {
            self.scopes.iter().cloned().collect::<Vec<_>>().join(", ")
        }
    }
}

/// Extract a top-level JSON string field from one beads row.
fn row_str<'a>(row: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\":\"");
    let start = row.find(&needle)? + needle.len();
    let rest = &row[start..];
    let mut end = 0;
    let bytes = rest.as_bytes();
    while end < bytes.len() {
        match bytes[end] {
            b'\\' => end += 2,
            b'"' => return Some(&rest[..end]),
            _ => end += 1,
        }
    }
    None
}

/// Extract the `labels` array from one beads row.
fn row_labels(row: &str) -> BTreeSet<String> {
    let mut labels = BTreeSet::new();
    let Some(start) = row.find("\"labels\":[") else {
        return labels;
    };
    let rest = &row[start + "\"labels\":[".len()..];
    let Some(end) = rest.find(']') else {
        return labels;
    };
    for piece in rest[..end].split(',') {
        let piece = piece.trim().trim_matches('"');
        if !piece.is_empty() {
            labels.insert(piece.to_string());
        }
    }
    labels
}

/// Read the open, gating claim-integrity defects. Returns `Err` when the store
/// cannot be trusted — the caller must treat that as a refusal, never as an
/// empty inventory.
pub fn gating_defects(source: &str) -> Result<Vec<GatingDefect>, String> {
    let mut defects = Vec::new();
    for (index, row) in source.lines().enumerate() {
        let row = row.trim();
        if row.is_empty() {
            continue;
        }
        if !row.starts_with('{') || !row.ends_with('}') {
            return Err(format!(
                "{BEADS_FILE} line {} is not a complete JSON object; the store may be mid-flush \
                 (read-then-validate: refusing rather than reporting a short inventory)",
                index + 1
            ));
        }
        let labels = row_labels(row);
        if !labels.contains(CLASS_LABEL) {
            continue;
        }
        let Some(id) = row_str(row, "id") else {
            return Err(format!("{BEADS_FILE} line {} has no id", index + 1));
        };
        // Only bugs are inventory; a claim-integrity epic/task/feature is E02
        // program work (docs/CLAIM_INTEGRITY.md). Without this split the gate
        // would count its own epic and block every promotion forever.
        if row_str(row, "issue_type") != Some("bug") {
            continue;
        }
        if row_str(row, "status") == Some("closed") {
            continue;
        }
        if !labels.contains(GATING_SEVERITY) {
            continue;
        }
        defects.push(GatingDefect {
            id: id.to_string(),
            scopes: labels
                .iter()
                .filter_map(|label| label.strip_prefix(CRATE_LABEL_PREFIX))
                .map(str::to_string)
                .collect(),
        });
    }
    Ok(defects)
}

/// Evaluate promotions against the gating inventory. Pure, so the unit tests
/// can drive it with fixtures instead of the live tree.
pub fn evaluate(
    promotions: &BTreeMap<String, (String, String, BTreeSet<String>)>,
    defects: &[GatingDefect],
    violations: &mut Vec<Violation>,
    decisions: &mut Vec<PolicyNote>,
) {
    for (capability, (from, to, crates)) in promotions {
        let blocking: Vec<&GatingDefect> = defects
            .iter()
            .filter(|defect| defect.blocks(crates))
            .collect();
        if blocking.is_empty() {
            decisions.push(note(
                capability,
                "promotion-admitted",
                format!(
                    "promotion {from} -> {to} has no open severity:default-path claim-integrity \
                     defect in scope"
                ),
            ));
            continue;
        }
        for defect in blocking {
            violations.push(violation(
                capability,
                format!(
                    "promotion {from} -> {to} is BLOCKED by open claim-integrity defect {} \
                     (scope: {}) — a capability may not be promoted while a defect that can emit \
                     a stronger claim than its evidence supports is in scope for it \
                     (docs/CLAIM_INTEGRITY.md, bead f85xj.2.3)",
                    defect.id,
                    defect.scope_text()
                ),
            ));
        }
    }
}

/// The gate: read the registry's promotions and the beads inventory, and
/// refuse any promotion with a gating defect in scope.
pub fn check_claim_integrity_gate(root: &Path) -> GateReport {
    let mut violations = Vec::new();
    let mut decisions = Vec::new();

    let CapabilityLevels {
        current,
        committed,
        crates,
    } = match maturity::capability_levels(root) {
        Ok(levels) => levels,
        Err(detail) => {
            violations.push(violation("<repo>", detail));
            return GateReport {
                violations,
                decisions,
            };
        }
    };

    // Promotions only. Demotions and introductions never gate.
    let mut promotions = BTreeMap::new();
    for (id, level) in &current {
        let Some(before) = committed.get(id) else {
            continue;
        };
        let (Some(before_index), Some(now_index)) =
            (maturity::level_rank(before), maturity::level_rank(level))
        else {
            continue;
        };
        if now_index > before_index {
            promotions.insert(
                id.clone(),
                (
                    before.clone(),
                    level.clone(),
                    crates.get(id).cloned().unwrap_or_default(),
                ),
            );
        }
    }

    if promotions.is_empty() {
        decisions.push(note(
            "<repo>",
            "no-promotion",
            "no capability is being promoted; the claim-integrity gate has nothing to weigh"
                .to_string(),
        ));
        return GateReport {
            violations,
            decisions,
        };
    }

    // A promotion is on the table, so the inventory must be readable. Refuse
    // rather than admit a promotion we could not check.
    let path = root.join(BEADS_FILE);
    let metadata = std::fs::metadata(&path);
    if let Ok(metadata) = &metadata
        && metadata.len() > MAX_BEADS_BYTES
    {
        violations.push(violation(
            "<repo>",
            format!(
                "{BEADS_FILE} is {} bytes, over the {MAX_BEADS_BYTES}-byte bound; refusing to \
                 admit a promotion against an inventory this check will not read",
                metadata.len()
            ),
        ));
        return GateReport {
            violations,
            decisions,
        };
    }
    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            violations.push(violation(
                "<repo>",
                format!(
                    "{BEADS_FILE} is unreadable ({error}) while {} promotion(s) are pending; a \
                     gate that cannot read the inventory must refuse, because an inventory that \
                     has not been read is not an empty inventory",
                    promotions.len()
                ),
            ));
            return GateReport {
                violations,
                decisions,
            };
        }
    };

    let defects = match gating_defects(&source) {
        Ok(defects) => defects,
        Err(detail) => {
            violations.push(violation("<repo>", detail));
            return GateReport {
                violations,
                decisions,
            };
        }
    };

    decisions.push(note(
        "<repo>",
        "inventory",
        format!(
            "{} promotion(s) pending against {} open severity:default-path claim-integrity \
             defect(s)",
            promotions.len(),
            defects.len()
        ),
    ));
    evaluate(&promotions, &defects, &mut violations, &mut decisions);

    GateReport {
        violations,
        decisions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, kind: &str, status: &str, labels: &[&str]) -> String {
        let labels = labels
            .iter()
            .map(|l| format!("\"{l}\""))
            .collect::<Vec<_>>()
            .join(",");
        format!(r#"{{"id":"{id}","issue_type":"{kind}","status":"{status}","labels":[{labels}]}}"#)
    }

    fn scopes(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    fn promotion(
        capability: &str,
        crates: &[&str],
    ) -> BTreeMap<String, (String, String, BTreeSet<String>)> {
        let mut map = BTreeMap::new();
        map.insert(
            capability.to_string(),
            ("L2".to_string(), "L3".to_string(), scopes(crates)),
        );
        map
    }

    fn run(
        promotions: &BTreeMap<String, (String, String, BTreeSet<String>)>,
        defects: &[GatingDefect],
    ) -> (Vec<Violation>, Vec<PolicyNote>) {
        let (mut v, mut d) = (Vec::new(), Vec::new());
        evaluate(promotions, defects, &mut v, &mut d);
        (v, d)
    }

    #[test]
    fn no_defects_admits_the_promotion() {
        let (v, d) = run(&promotion("cap", &["fs-x"]), &[]);
        assert!(v.is_empty(), "{v:?}");
        assert!(d.iter().any(|n| n.verdict == "promotion-admitted"));
    }

    #[test]
    fn an_in_scope_defect_blocks_the_promotion() {
        let defect = GatingDefect {
            id: "bug-1".to_string(),
            scopes: scopes(&["fs-x"]),
        };
        let (v, _) = run(&promotion("cap", &["fs-x", "fs-y"]), &[defect]);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].detail
                .contains("BLOCKED by open claim-integrity defect bug-1")
        );
    }

    #[test]
    fn an_out_of_scope_defect_does_not_block() {
        let defect = GatingDefect {
            id: "bug-1".to_string(),
            scopes: scopes(&["fs-unrelated"]),
        };
        let (v, d) = run(&promotion("cap", &["fs-x"]), &[defect]);
        assert!(v.is_empty(), "{v:?}");
        assert!(d.iter().any(|n| n.verdict == "promotion-admitted"));
    }

    #[test]
    fn an_unscoped_defect_blocks_globally() {
        // Ambiguous scope fails CLOSED: a defect nobody has localised is a
        // reason for more caution, not less.
        let defect = GatingDefect {
            id: "bug-global".to_string(),
            scopes: BTreeSet::new(),
        };
        let (v, _) = run(&promotion("cap", &["fs-anything"]), &[defect]);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].detail.contains("<unscoped: blocks globally>"));
    }

    #[test]
    fn only_open_gating_bugs_enter_the_inventory() {
        let source = [
            // gating: open bug, class label, default-path severity
            row(
                "open-p0",
                "bug",
                "open",
                &[CLASS_LABEL, GATING_SEVERITY, "crate:fs-a"],
            ),
            // in_progress still counts as open exposure
            row(
                "wip-p0",
                "bug",
                "in_progress",
                &[CLASS_LABEL, GATING_SEVERITY],
            ),
            // closed: not exposure
            row(
                "closed-p0",
                "bug",
                "closed",
                &[CLASS_LABEL, GATING_SEVERITY],
            ),
            // gated severity: not the gating class
            row("gated", "bug", "open", &[CLASS_LABEL, "severity:gated"]),
            // program bead: type is the discriminator, must NOT gate
            row("epic", "epic", "open", &[CLASS_LABEL, GATING_SEVERITY]),
            row("task", "task", "open", &[CLASS_LABEL, GATING_SEVERITY]),
            // unrelated bug
            row("other", "bug", "open", &["something-else"]),
        ]
        .join("\n");
        let defects = gating_defects(&source).expect("well-formed rows parse");
        let ids: Vec<&str> = defects.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, vec!["open-p0", "wip-p0"], "{defects:?}");
        assert_eq!(defects[0].scopes, scopes(&["fs-a"]));
        assert!(defects[1].scopes.is_empty(), "unscoped stays unscoped");
    }

    #[test]
    fn a_truncated_row_refuses_rather_than_undercounting() {
        // Another agent flushing the store mid-read must never look like a
        // clean inventory.
        let source = format!(
            "{}\n{{\"id\":\"half\",\"issue_type\":\"bu",
            row("ok", "bug", "open", &[CLASS_LABEL, GATING_SEVERITY])
        );
        let error = gating_defects(&source).expect_err("a truncated row must refuse");
        assert!(error.contains("mid-flush"), "{error}");
    }

    #[test]
    fn demotions_and_introductions_never_reach_the_gate() {
        // `evaluate` only ever sees promotions; this pins the contract that a
        // demotion cannot produce a violation even with a global defect open.
        let defect = GatingDefect {
            id: "bug-global".to_string(),
            scopes: BTreeSet::new(),
        };
        let (v, d) = run(&BTreeMap::new(), &[defect]);
        assert!(
            v.is_empty(),
            "an empty promotion set cannot be blocked: {v:?}"
        );
        assert!(d.is_empty());
    }
}
