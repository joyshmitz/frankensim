//! Claim-state lint (bead 06yc): README prose must not drift from code.
//!
//! Public capability prose already has machine counterparts in the tree
//! (golden-hash constants, test function names, crate directories). This
//! check verifies the three cheapest, highest-yield couplings:
//!
//! 1. Every 16-hex-digit hash cited in README.md exists verbatim
//!    (underscore-insensitive, case-insensitive) somewhere under
//!    `crates/*/src` or `crates/*/tests` — a hash quoted in prose that no
//!    longer matches any recorded golden is stale evidence language.
//! 2. Every backticked `fs-<name>` crate reference in README.md exists as
//!    `crates/fs-<name>/` (wildcards like `fs-rep-*` and paths containing
//!    `::` or `_` are skipped — they are module prose, not crate names).
//! 3. Every backticked `*_hash` symbol in README.md exists as a
//!    `fn <name>` in some crate source or test — sentinel names in prose
//!    must be real tests.
//!
//! Rule 4 (huq.18) derives README inventory counts from the tree, and rule 5
//! (f85xj.2.1) keeps the claim-integrity defect class defined and its label
//! taxonomy documented — see the section further down for why that definition
//! is load-bearing rather than decorative.
//!
//! The deeper claim-state machinery (landed flags, no-claim rows, site
//! generation from evidence packages) belongs to huq.15.1; this lint is
//! the repo-level drift stop until that exists.

use crate::Violation;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Normalize a hash token: strip `0x`, underscores, lowercase.
fn norm_hash(tok: &str) -> String {
    tok.trim_start_matches("0x")
        .chars()
        .filter(|c| *c != '_')
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Extract candidate 64-bit hash literals (16 hex digits after
/// normalization) from a text.
fn hashes_in(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (idx, _) in text.match_indices("0x") {
        let tail: String = text[idx + 2..]
            .chars()
            .take_while(|c| c.is_ascii_hexdigit() || *c == '_')
            .collect();
        let norm = norm_hash(&tail);
        if norm.len() == 16 {
            out.insert(norm);
        }
    }
    out
}

/// Backticked tokens in a markdown text.
fn backticked(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else { break };
        out.push(&after[..close]);
        rest = &after[close + 1..];
    }
    out
}

/// Walk all `.rs` files under `crates/*/{src,tests}`.
fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(root.join("crates")) else {
        return files;
    };
    let mut stack: Vec<PathBuf> = entries
        .flatten()
        .flat_map(|e| [e.path().join("src"), e.path().join("tests")])
        .filter(|p| p.is_dir())
        .collect();
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "rs") {
                files.push(p);
            }
        }
    }
    files
}

/// The number immediately preceding `pat` on `line` (digits touching the
/// pattern), if any.
fn count_before(line: &str, pat: &str) -> Option<usize> {
    let pos = line.find(pat)?;
    let digits: String = line[..pos]
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    digits.parse::<usize>().ok()
}

fn workspace_fs_member_count(manifest: &str) -> Option<usize> {
    let mut lines = manifest.lines();
    lines.find(|line| line.trim() == "members = [")?;
    let mut count = 0usize;
    for line in lines {
        let entry = line.trim();
        if entry == "]" {
            return Some(count);
        }
        let entry = entry.strip_suffix(',').unwrap_or(entry).trim();
        let entry = entry.strip_prefix('"')?.strip_suffix('"')?;
        if entry.starts_with("crates/fs-") {
            count = count.checked_add(1)?;
        }
    }
    None
}

fn tracked_file_count(root: &Path, pathspec: &str) -> Option<usize> {
    let output = std::process::Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["ls-files", "--", pathspec])
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count()
    })
}

/// huq.18: README inventory counts (crate/contract/test-file numbers in
/// the badges and the What-Exists table) must equal the tree's actual
/// counts — counts are DERIVED, never hand-promoted, so drift turns the
/// gate red instead of aging silently.
fn check_inventory_counts(root: &Path, readme: &str) -> Vec<Violation> {
    let mut violations = Vec::new();
    let crate_dirs: Vec<PathBuf> = std::fs::read_dir(root.join("crates")).map_or_else(
        |_| Vec::new(),
        |rd| rd.flatten().map(|e| e.path()).collect(),
    );
    let filesystem_crate_count = crate_dirs
        .iter()
        .filter(|p| p.join("Cargo.toml").is_file())
        .count();
    let filesystem_contract_count = crate_dirs
        .iter()
        .filter(|p| p.join("CONTRACT.md").is_file())
        .count();
    let filesystem_test_file_count: usize = crate_dirs
        .iter()
        .filter_map(|p| std::fs::read_dir(p.join("tests")).ok())
        .flat_map(std::iter::Iterator::flatten)
        .filter(|f| f.path().extension().is_some_and(|x| x == "rs"))
        .count();
    // Public inventory describes a clean clone. Untracked scratch probes must
    // neither inflate README claims nor make the gate nondeterministic. The
    // filesystem fallback keeps isolated non-git unit fixtures useful.
    let crate_count =
        tracked_file_count(root, "crates/*/Cargo.toml").unwrap_or(filesystem_crate_count);
    let contract_count =
        tracked_file_count(root, "crates/*/CONTRACT.md").unwrap_or(filesystem_contract_count);
    let test_file_count =
        tracked_file_count(root, "crates/*/tests/*.rs").unwrap_or(filesystem_test_file_count);
    let workspace_crate_count = std::fs::read_to_string(root.join("Cargo.toml"))
        .ok()
        .and_then(|manifest| workspace_fs_member_count(&manifest));
    if workspace_crate_count.is_none() {
        violations.push(Violation {
            check: "claim-state",
            crate_name: "Cargo.toml".to_string(),
            detail: "cannot derive native fs-* workspace-member count from [workspace].members"
                .to_string(),
        });
    }
    let workspace_crate_count = workspace_crate_count.unwrap_or(usize::MAX);
    let checks = [
        (
            "%20fs--%2A%20crates",
            workspace_crate_count,
            "fs-* crates (badge)",
        ),
        (
            " native `fs-*` workspace crates",
            workspace_crate_count,
            "native fs-* workspace crates",
        ),
        (
            " `fs-*` crate directories",
            crate_count,
            "fs-* crate directories",
        ),
        (" fs-* crates", crate_count, "fs-* crates (layout)"),
        (" `CONTRACT.md` files", contract_count, "CONTRACT.md files"),
        (" crate test files", test_file_count, "crate test files"),
        (
            " crate-level conformance",
            test_file_count,
            "crate test files (What Exists table)",
        ),
        (
            "%20crate%20test%20files",
            test_file_count,
            "crate test files (badge)",
        ),
    ];
    for line in readme.lines() {
        for (pat, actual, what) in checks {
            if let Some(claimed) = count_before(line, pat)
                && claimed != actual
            {
                violations.push(Violation {
                    check: "claim-state",
                    crate_name: "README.md".to_string(),
                    detail: format!(
                        "README claims {claimed} {what} but the tree has {actual} — counts \
                         are derived, never hand-promoted (bead huq.18)"
                    ),
                });
            }
        }
        // Contracts badge: `contracts-<n>%20of%20<m>%20crates`.
        if let Some(at) = line.find("badge/contracts-") {
            let tail = &line[at + "badge/contracts-".len()..];
            let n: String = tail.chars().take_while(char::is_ascii_digit).collect();
            let m = tail
                .find("%20of%20")
                .map(|p| &tail[p + "%20of%20".len()..])
                .map(|t| {
                    t.chars()
                        .take_while(char::is_ascii_digit)
                        .collect::<String>()
                });
            if let (Ok(n), Some(Ok(m))) = (n.parse::<usize>(), m.map(|s| s.parse::<usize>()))
                && (n != contract_count || m != crate_count)
            {
                violations.push(Violation {
                    check: "claim-state",
                    crate_name: "README.md".to_string(),
                    detail: format!(
                        "README contracts badge says {n} of {m} but the tree has \
                         {contract_count} CONTRACT.md files across {crate_count} crates \
                         (bead huq.18)"
                    ),
                });
            }
        }
        if line.contains("| Contracts |") || line.contains("`CONTRACT.md` files for") {
            let numbers: Vec<usize> = line
                .split(|ch: char| !ch.is_ascii_digit())
                .filter(|token| !token.is_empty())
                .filter_map(|token| token.parse().ok())
                .collect();
            if numbers.len() >= 2 && numbers[numbers.len() - 2..] != [contract_count, crate_count] {
                violations.push(Violation {
                    check: "claim-state",
                    crate_name: "README.md".to_string(),
                    detail: format!(
                        "README contract inventory says {} of {} but the tree has \
                         {contract_count} CONTRACT.md files across {crate_count} crates \
                         (bead huq.18)",
                        numbers[numbers.len() - 2],
                        numbers[numbers.len() - 1]
                    ),
                });
            }
        }
    }
    violations
}

// ---------------------------------------------------------------------------
// Claim-integrity defect class (bead frankensim-extreal-program-f85xj.2.1).
//
// The class definition is the input the E02 sweep and promotion gate consume
// verbatim, so it must not silently disappear, lose a decision-rule section,
// or drift out of agreement with the label taxonomy the gate queries. Code is
// the single source of truth for the canonical severity labels; the definition
// doc and the CONVENTIONS taxonomy must both name exactly these.
//
// This lint proves the definition is present and structurally intact. It does
// not, and cannot, judge whether an audit was performed honestly — that is
// what the sweep's recorded verdicts and the gate drills are for. Claiming
// otherwise here would itself be a claim-integrity defect.
// ---------------------------------------------------------------------------

/// Canonical severity labels. The gate and the inventory script accept exactly
/// these; adding a severity means changing this array and both documents.
pub const CLAIM_INTEGRITY_SEVERITY_LABELS: [&str; 3] = [
    "severity:default-path",
    "severity:gated",
    "severity:doc-only",
];

/// The mandatory class-membership label; `br list -l <label>` is the inventory.
pub const CLAIM_INTEGRITY_LABEL: &str = "claim-integrity";

const CLAIM_INTEGRITY_DOC: &str = "docs/CLAIM_INTEGRITY.md";
const CLAIM_INTEGRITY_CONVENTIONS: &str = "docs/CONVENTIONS.md";
const CLAIM_INTEGRITY_INVENTORY_SCRIPT: &str = "scripts/ci/claim_integrity_inventory.sh";

/// Sections the definition must keep. Each one is consumed by a downstream
/// bead: decision rules and audit method by the `.2.2` sweep, severity rules
/// and label taxonomy by the `.2.3` gate, known instances by both as the
/// known-answer set.
const CLAIM_INTEGRITY_REQUIRED_SECTIONS: [&str; 6] = [
    "## Definition",
    "## Decision rules",
    "## Severity rules",
    "## Label taxonomy",
    "## Audit method",
    "## Known instances",
];

/// The CONVENTIONS taxonomy section heading that must point agents at the
/// definition.
const CLAIM_INTEGRITY_CONVENTIONS_SECTION: &str = "## Claim-integrity defect class";

fn claim_integrity_violation(file: &str, detail: String) -> Violation {
    Violation {
        check: "claim-state",
        crate_name: file.to_string(),
        detail,
    }
}

/// Lint the claim-integrity definition and its taxonomy (bead f85xj.2.1).
fn check_claim_integrity_docs(root: &Path) -> Vec<Violation> {
    let mut violations = Vec::new();

    let Ok(definition) = std::fs::read_to_string(root.join(CLAIM_INTEGRITY_DOC)) else {
        violations.push(claim_integrity_violation(
            CLAIM_INTEGRITY_DOC,
            format!(
                "{CLAIM_INTEGRITY_DOC} is missing — the claim-integrity defect class is the \
                 definition the E02 sweep and promotion gate consume verbatim; without it the \
                 gate counts an inventory it cannot define (bead f85xj.2.1)"
            ),
        ));
        return violations;
    };

    for section in CLAIM_INTEGRITY_REQUIRED_SECTIONS {
        if !definition.contains(section) {
            violations.push(claim_integrity_violation(
                CLAIM_INTEGRITY_DOC,
                format!(
                    "{CLAIM_INTEGRITY_DOC} lost required section {section:?} — downstream beads \
                     (.2.2 sweep, .2.3 gate) consume these sections verbatim (bead f85xj.2.1)"
                ),
            ));
        }
    }

    for label in CLAIM_INTEGRITY_SEVERITY_LABELS {
        if !definition.contains(label) {
            violations.push(claim_integrity_violation(
                CLAIM_INTEGRITY_DOC,
                format!(
                    "{CLAIM_INTEGRITY_DOC} does not name canonical severity label {label:?} — the \
                     doc and xtask must agree on the label set the gate queries (bead f85xj.2.1)"
                ),
            ));
        }
    }

    // The inventory script is named by the definition as its enforcement arm;
    // a definition citing a script that does not exist overstates enforcement.
    if definition.contains(CLAIM_INTEGRITY_INVENTORY_SCRIPT)
        && !root.join(CLAIM_INTEGRITY_INVENTORY_SCRIPT).is_file()
    {
        violations.push(claim_integrity_violation(
            CLAIM_INTEGRITY_DOC,
            format!(
                "{CLAIM_INTEGRITY_DOC} cites {CLAIM_INTEGRITY_INVENTORY_SCRIPT} as its enforcement \
                 arm but that script does not exist — documented enforcement that cannot run is \
                 itself an overstated claim (bead f85xj.2.1)"
            ),
        ));
    }

    let Ok(conventions) = std::fs::read_to_string(root.join(CLAIM_INTEGRITY_CONVENTIONS)) else {
        violations.push(claim_integrity_violation(
            CLAIM_INTEGRITY_CONVENTIONS,
            format!("{CLAIM_INTEGRITY_CONVENTIONS} is missing (bead f85xj.2.1)"),
        ));
        return violations;
    };

    if !conventions.contains(CLAIM_INTEGRITY_CONVENTIONS_SECTION) {
        violations.push(claim_integrity_violation(
            CLAIM_INTEGRITY_CONVENTIONS,
            format!(
                "{CLAIM_INTEGRITY_CONVENTIONS} lost section \
                 {CLAIM_INTEGRITY_CONVENTIONS_SECTION:?} — the label taxonomy must be discoverable \
                 where agents read conventions, not only in the definition (bead f85xj.2.1)"
            ),
        ));
    }
    if !conventions.contains(CLAIM_INTEGRITY_LABEL) {
        violations.push(claim_integrity_violation(
            CLAIM_INTEGRITY_CONVENTIONS,
            format!(
                "{CLAIM_INTEGRITY_CONVENTIONS} does not name the {CLAIM_INTEGRITY_LABEL:?} label \
                 (bead f85xj.2.1)"
            ),
        ));
    }
    for label in CLAIM_INTEGRITY_SEVERITY_LABELS {
        if !conventions.contains(label) {
            violations.push(claim_integrity_violation(
                CLAIM_INTEGRITY_CONVENTIONS,
                format!(
                    "{CLAIM_INTEGRITY_CONVENTIONS} taxonomy omits canonical severity label \
                     {label:?} — an undocumented severity is one the sweep will not apply \
                     (bead f85xj.2.1)"
                ),
            ));
        }
    }

    violations
}

/// README claim-state lint: see module docs for the three rules.
pub fn check_claims(root: &Path) -> Vec<Violation> {
    let mut violations = Vec::new();
    let Ok(readme) = std::fs::read_to_string(root.join("README.md")) else {
        violations.push(Violation {
            check: "claim-state",
            crate_name: "<repo>".to_string(),
            detail: "README.md missing at workspace root".to_string(),
        });
        return violations;
    };

    // Corpus: all code text (sources + tests) for hash and fn lookups.
    let mut code_hashes: BTreeSet<String> = BTreeSet::new();
    let mut code_text = String::new();
    for f in rust_files(root) {
        if let Ok(t) = std::fs::read_to_string(&f) {
            code_hashes.extend(hashes_in(&t));
            code_text.push_str(&t);
            code_text.push('\n');
        }
    }

    // Rule 4 (huq.18): README inventory counts are derived, never
    // hand-promoted.
    violations.extend(check_inventory_counts(root, &readme));

    // Rule 5 (f85xj.2.1): the claim-integrity defect class stays defined and
    // its taxonomy stays documented where agents read conventions.
    violations.extend(check_claim_integrity_docs(root));

    // Rule 1: cited hashes exist in code.
    for h in hashes_in(&readme) {
        if !code_hashes.contains(&h) {
            violations.push(Violation {
                check: "claim-state",
                crate_name: "README.md".to_string(),
                detail: format!(
                    "README cites hash 0x{h} but no crate source/test contains it — the prose \
                     is stale relative to the recorded goldens (re-check the sentinel it \
                     describes; golden bumps must update citing prose, bead 06yc)"
                ),
            });
        }
    }

    // Rules 2 and 3 over backticked tokens.
    for tok in backticked(&readme) {
        // Rule 2: crate references.
        if let Some(name) = tok.strip_prefix("fs-") {
            let clean = name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            if clean && !name.is_empty() && !root.join("crates").join(tok).is_dir() {
                violations.push(Violation {
                    check: "claim-state",
                    crate_name: "README.md".to_string(),
                    detail: format!(
                        "README references crate `{tok}` but crates/{tok}/ does not exist \
                         (renamed or removed crate leaves stale capability prose, bead 06yc)"
                    ),
                });
            }
        }
        // Rule 3: sentinel test symbols.
        if tok.ends_with("_hash")
            && tok
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            && !code_text.contains(&format!("fn {tok}"))
        {
            violations.push(Violation {
                check: "claim-state",
                crate_name: "README.md".to_string(),
                detail: format!(
                    "README names sentinel `{tok}` but no `fn {tok}` exists in any crate \
                     source/test (bead 06yc)"
                ),
            });
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_backtick_extraction() {
        let hs = hashes_in("golden `0xeef1_0550_7daf_c0d5` and 0xDEAD (too short)");
        assert!(hs.contains("eef105507dafc0d5"));
        assert_eq!(hs.len(), 1);
        assert_eq!(backticked("a `b` c `d-e`"), vec!["b", "d-e"]);
    }

    #[test]
    fn workspace_member_count_excludes_tools_and_nested_workspaces() {
        let manifest = r#"
[workspace]
members = [
    "crates/fs-a",
    "crates/fs-b",
    "xtask",
]
"#;
        assert_eq!(workspace_fs_member_count(manifest), Some(2));
        assert_eq!(workspace_fs_member_count("[workspace]\n"), None);
    }

    /// Build a minimal docs pair that satisfies the claim-integrity lint, so
    /// each negative case below differs from green by exactly one mutation.
    fn claim_integrity_fixture(base: &Path) {
        let mut definition = String::new();
        for section in CLAIM_INTEGRITY_REQUIRED_SECTIONS {
            definition.push_str(section);
            definition.push_str("\n\nbody\n\n");
        }
        for label in CLAIM_INTEGRITY_SEVERITY_LABELS {
            definition.push_str(&format!("- `{label}`\n"));
        }
        let mut conventions = format!("{CLAIM_INTEGRITY_CONVENTIONS_SECTION}\n\n");
        conventions.push_str(&format!("label `{CLAIM_INTEGRITY_LABEL}`\n"));
        for label in CLAIM_INTEGRITY_SEVERITY_LABELS {
            conventions.push_str(&format!("- `{label}`\n"));
        }
        let write = |rel: &str, text: &str| {
            let path = base.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write(CLAIM_INTEGRITY_DOC, &definition);
        write(CLAIM_INTEGRITY_CONVENTIONS, &conventions);
    }

    #[test]
    fn claim_integrity_lint_accepts_a_complete_definition_and_taxonomy() {
        let base = std::env::temp_dir().join(format!("fsim-ci-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        claim_integrity_fixture(&base);
        let violations = check_claim_integrity_docs(&base);
        assert!(violations.is_empty(), "expected clean: {violations:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn claim_integrity_lint_fails_closed_on_each_single_mutation() {
        let base = std::env::temp_dir().join(format!("fsim-ci-mut-{}", std::process::id()));

        // A missing definition is one violation, not a silent pass: the gate
        // must never count an inventory whose class it cannot define.
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("docs")).unwrap();
        std::fs::write(base.join(CLAIM_INTEGRITY_CONVENTIONS), "irrelevant").unwrap();
        let missing = check_claim_integrity_docs(&base);
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(missing[0].detail.contains("is missing"));

        // Dropping any one required section is caught by name.
        for dropped in CLAIM_INTEGRITY_REQUIRED_SECTIONS {
            let _ = std::fs::remove_dir_all(&base);
            claim_integrity_fixture(&base);
            let text = std::fs::read_to_string(base.join(CLAIM_INTEGRITY_DOC)).unwrap();
            std::fs::write(
                base.join(CLAIM_INTEGRITY_DOC),
                text.replace(dropped, "## Removed"),
            )
            .unwrap();
            let violations = check_claim_integrity_docs(&base);
            assert!(
                violations.iter().any(|v| v.detail.contains(dropped)),
                "dropping {dropped:?} must be caught: {violations:?}"
            );
        }

        // Dropping any one canonical severity label is caught in both files,
        // because doc and taxonomy must agree on the set the gate queries.
        for label in CLAIM_INTEGRITY_SEVERITY_LABELS {
            let _ = std::fs::remove_dir_all(&base);
            claim_integrity_fixture(&base);
            for file in [CLAIM_INTEGRITY_DOC, CLAIM_INTEGRITY_CONVENTIONS] {
                let text = std::fs::read_to_string(base.join(file)).unwrap();
                std::fs::write(base.join(file), text.replace(label, "severity:unknown")).unwrap();
            }
            let violations = check_claim_integrity_docs(&base);
            assert!(
                violations
                    .iter()
                    .filter(|v| v.detail.contains(label))
                    .count()
                    >= 2,
                "dropping {label:?} must be caught in both files: {violations:?}"
            );
        }

        // The CONVENTIONS taxonomy section must stay discoverable.
        let _ = std::fs::remove_dir_all(&base);
        claim_integrity_fixture(&base);
        let text = std::fs::read_to_string(base.join(CLAIM_INTEGRITY_CONVENTIONS)).unwrap();
        std::fs::write(
            base.join(CLAIM_INTEGRITY_CONVENTIONS),
            text.replace(CLAIM_INTEGRITY_CONVENTIONS_SECTION, "## Something else"),
        )
        .unwrap();
        let violations = check_claim_integrity_docs(&base);
        assert!(
            violations
                .iter()
                .any(|v| v.detail.contains(CLAIM_INTEGRITY_CONVENTIONS_SECTION)),
            "{violations:?}"
        );

        // Citing an enforcement script that does not exist is itself an
        // overstated claim.
        let _ = std::fs::remove_dir_all(&base);
        claim_integrity_fixture(&base);
        let text = std::fs::read_to_string(base.join(CLAIM_INTEGRITY_DOC)).unwrap();
        std::fs::write(
            base.join(CLAIM_INTEGRITY_DOC),
            format!("{text}\nrun {CLAIM_INTEGRITY_INVENTORY_SCRIPT} for the report\n"),
        )
        .unwrap();
        let violations = check_claim_integrity_docs(&base);
        assert!(
            violations
                .iter()
                .any(|v| v.detail.contains(CLAIM_INTEGRITY_INVENTORY_SCRIPT)),
            "{violations:?}"
        );
        std::fs::create_dir_all(base.join("scripts/ci")).unwrap();
        std::fs::write(base.join(CLAIM_INTEGRITY_INVENTORY_SCRIPT), "#!/bin/sh\n").unwrap();
        assert!(
            check_claim_integrity_docs(&base).is_empty(),
            "materializing the cited script must clear the violation"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn claims_check_end_to_end_on_fixture_tree() {
        let base = std::env::temp_dir().join(format!("fsim-claims-test-{}", std::process::id()));
        let mk = |rel: &str, content: &str| {
            let p = base.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        };
        mk(
            "Cargo.toml",
            "[workspace]\nmembers = [\n    \"crates/fs-real\",\n]\n",
        );
        mk(
            "crates/fs-real/src/lib.rs",
            "pub const G: u64 = 0x1111_2222_3333_4444;\n",
        );
        mk(
            "crates/fs-real/tests/battery.rs",
            "fn real_golden_hash() {}\n",
        );
        // Seeded drift: stale hash, missing crate, missing sentinel fn.
        mk(
            "README.md",
            concat!(
                "Good: `fs-real` golden `0x1111_2222_3333_4444` via `real_golden_hash`.\n",
                "Stale hash 0xaaaa_bbbb_cccc_dddd.\n",
                "Gone crate `fs-vanished`.\n",
                "Gone sentinel `ghost_golden_hash`.\n",
            ),
        );
        // Rule 5's docs are present and complete so this case still isolates
        // the three seeded README drifts; the claim-integrity lint has its own
        // mutation tests above.
        claim_integrity_fixture(&base);
        let v = check_claims(&base);
        assert_eq!(v.len(), 3, "exactly the three seeded drifts: {v:?}");
        assert!(v.iter().any(|x| x.detail.contains("aaaabbbbccccdddd")));
        assert!(v.iter().any(|x| x.detail.contains("fs-vanished")));
        assert!(v.iter().any(|x| x.detail.contains("ghost_golden_hash")));
        let _ = std::fs::remove_dir_all(&base);
    }
}
