//! Failure-compounding acceptance battery (bead 6nb.9): seed the workflow
//! with (a) a deliberately broken cross-crate golden modeled EXACTLY on the
//! real powi incident (bead 4xnt) and (b) a falsifier hit on a wrong
//! certificate constant; both must produce minimized replayable cases,
//! neighborhood boundary evidence, permanent regression families with
//! tracking references, and a content-addressed manifest whose hash is
//! frozen as a golden (identical in both build modes and on both ISAs —
//! integer/`to_bits` arithmetic only).

use fs_bisect::compound::{
    Canon, CompoundError, FailureCase, FamilyProvenance, InvariantClass,
    MAX_CANONICAL_MEMBER_BYTES, MAX_IDENTIFIER_BYTES, MAX_MINIMIZE_STEPS, MAX_NEIGHBOR_PROBES,
    MAX_SHRINK_CANDIDATES_PER_STEP, RegressionFamily, Shrink, compound, minimize,
    probe_neighborhood,
};

fn modeled_golden_hash(bytes: &[u8]) -> u64 {
    let mut acc: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        acc ^= u64::from(byte);
        acc = acc.wrapping_mul(0x0000_0100_0000_01b3);
    }
    acc
}

fn provenance(seed: u64) -> FamilyProvenance {
    FamilyProvenance::new(
        seed,
        "fs-bisect::compound".to_string(),
        "seeded regression".to_string(),
    )
    .expect("valid provenance")
}

fn test_family_with_provenance(
    name: &str,
    invariant: InvariantClass,
    member: u64,
    tracking: Vec<String>,
    admission: Option<String>,
    provenance: FamilyProvenance,
) -> RegressionFamily<u64> {
    RegressionFamily::new(
        name.to_string(),
        invariant,
        vec![("minimized".to_string(), member)],
        tracking,
        admission,
        provenance,
    )
    .expect("valid family")
}

fn test_family(
    name: &str,
    invariant: InvariantClass,
    member: u64,
    tracking: Vec<String>,
    admission: Option<String>,
) -> RegressionFamily<u64> {
    test_family_with_provenance(name, invariant, member, tracking, admission, provenance(7))
}

// ---- Scenario (a): the powi golden break, faithfully modeled ----
//
// Two integer-power implementations with different rounding orders:
// sequential (one rounding per multiply) vs square-and-multiply. They agree
// bitwise through exponent 3 and diverge from 4 — the exact mechanism that
// made the rand_nla golden build-mode-dependent.

fn pow_sequential(x: f64, k: u32) -> f64 {
    let mut p = 1.0f64;
    for _ in 0..k {
        p *= x;
    }
    p
}

fn pow_squaring(x: f64, k: u32) -> f64 {
    let mut b = k;
    let mut a = x;
    let mut r = 1.0f64;
    loop {
        if b & 1 == 1 {
            r *= a;
        }
        b /= 2;
        if b == 0 {
            break;
        }
        a *= a;
    }
    r
}

/// A golden fixture: a sweep of exponents whose combined bits are hashed.
#[derive(Debug, Clone, PartialEq)]
struct Sweep {
    base: f64,
    exponents: Vec<u32>,
}

impl Canon for Sweep {
    fn canon(&self, out: &mut Vec<u8>) {
        self.base.canon(out);
        let exps: Vec<i64> = self.exponents.iter().map(|&e| i64::from(e)).collect();
        exps.canon(out);
    }
}

impl Shrink for Sweep {
    /// Drop-one (leftmost first), then decrement-one — so minimization
    /// walks down to the exact divergence boundary, not just any witness.
    fn shrink_candidates(&self) -> Vec<Self> {
        let mut out = Vec::new();
        if self.exponents.len() > 1 {
            for i in 0..self.exponents.len() {
                let mut exps = self.exponents.clone();
                exps.remove(i);
                out.push(Sweep {
                    base: self.base,
                    exponents: exps,
                });
            }
        }
        for i in 0..self.exponents.len() {
            if self.exponents[i] > 0 {
                let mut exps = self.exponents.clone();
                exps[i] -= 1;
                out.push(Sweep {
                    base: self.base,
                    exponents: exps,
                });
            }
        }
        out
    }
}

/// The "golden drifted" predicate: reference-chain bits != suspect-chain bits.
fn golden_breaks(s: &Sweep) -> bool {
    let feed = |f: &dyn Fn(f64, u32) -> f64| -> u64 {
        let mut bytes = Vec::new();
        for &k in &s.exponents {
            f(s.base, k).canon(&mut bytes);
        }
        modeled_golden_hash(&bytes)
    };
    feed(&pow_sequential) != feed(&pow_squaring)
}

fn powi_case() -> FailureCase<Sweep> {
    FailureCase {
        id: "powi-order-divergence".to_string(),
        seed: 0xFEED,
        input: Sweep {
            base: 0.7,
            exponents: vec![0, 1, 2, 3, 4, 5, 6, 7, 8],
        },
        invariant: InvariantClass::BuildModeDeterminism,
        contract: "fs-la::rand_nla golden (modeled)".to_string(),
        detail: "sequential vs square-multiply power chains disagree".to_string(),
    }
}

#[test]
fn powi_model_minimizes_to_the_exact_boundary() {
    // Precondition sanity: for base 0.7 the two orders agree through 6 and
    // diverge from 7 onward (an IEEE fact of the two chains — the same
    // mechanism as the real incident, whose lowerings diverged from 4).
    for k in 0..=6u32 {
        assert_eq!(
            pow_sequential(0.7, k).to_bits(),
            pow_squaring(0.7, k).to_bits(),
            "orders must agree at k={k}"
        );
    }
    assert_ne!(
        pow_sequential(0.7, 7).to_bits(),
        pow_squaring(0.7, 7).to_bits(),
        "orders must diverge at k=7"
    );

    let report = compound(
        powi_case(),
        &golden_breaks,
        &|min: &Sweep| {
            (1..=10u32)
                .map(|k| {
                    (
                        format!("k={k}"),
                        Sweep {
                            base: min.base,
                            exponents: vec![k],
                        },
                    )
                })
                .collect()
        },
        vec![
            "frankensim-epic-gauntlet-6nb.9".to_string(),
            "frankensim-powi-build-mode-determinism-4xnt".to_string(),
        ],
        Some(
            "forbid variable-exponent integer powers in deterministic paths (4xnt lint)"
                .to_string(),
        ),
        1000,
    )
    .expect("the seeded golden break must minimize");

    // Minimized to the EXACT boundary: one exponent, value 7.
    assert!(report.converged, "minimization must reach a fixpoint");
    assert_eq!(report.case.input.exponents, vec![7], "boundary is k=7");
    // Neighborhood: 1..=6 pass, 7..=10 fail — region evidence, sharp edge.
    let failing: Vec<&str> = report
        .neighborhood
        .probes
        .iter()
        .filter(|p| p.fails)
        .map(|p| p.label.as_str())
        .collect();
    assert_eq!(failing, ["k=7", "k=8", "k=9", "k=10"]);
    // The family: minimum first, then every failing neighbor; tracked.
    assert_eq!(report.family.members().len(), 5);
    assert_eq!(report.family.members()[0].0, "minimized");
    assert!(
        !report.family.tracking().is_empty(),
        "no paper trail, no family"
    );
    // Replay: every member still fails under the suspect implementation...
    let live = report.family.replay(&golden_breaks);
    assert!(live.now_passing.is_empty(), "family must be live: {live:?}");
    // ...and the SAME family goes fully stale once the bug is "fixed"
    // (both chains sequential) — stale detection is the point of replay.
    let fixed = |_: &Sweep| false;
    let stale = report.family.replay(&fixed);
    assert!(stale.still_failing.is_empty());
    assert_eq!(stale.now_passing.len(), 5);
}

/// Recorded on aarch64-apple (M4 Pro); must be identical in debug and
/// release and on x86-64 (integer/to_bits arithmetic only).
const POWI_FAMILY_MANIFEST_HASH: &str =
    "ff9c945e8f3ecbaee37e82b5d57e7da7f710644ce9d8d0095c4974815aa132b7";

#[test]
fn powi_family_manifest_is_content_addressed_and_frozen() {
    let report = compound(
        powi_case(),
        &golden_breaks,
        &|min: &Sweep| {
            (1..=8u32)
                .map(|k| {
                    (
                        format!("k={k}"),
                        Sweep {
                            base: min.base,
                            exponents: vec![k],
                        },
                    )
                })
                .collect()
        },
        vec!["frankensim-epic-gauntlet-6nb.9".to_string()],
        None,
        1000,
    )
    .expect("must minimize");
    let manifest = report.family.manifest();
    // The manifest carries the hash in its trailer and is replay-complete.
    assert!(manifest.contains("\"family\":\"powi-order-divergence\""));
    assert_eq!(manifest.lines().count(), 2 + report.family.members().len());
    println!(
        "{{\"suite\":\"fs-bisect\",\"case\":\"compound-manifest\",\"verdict\":\"info\",\"detail\":\"{}\"}}",
        report.content_hash
    );
    assert_eq!(
        report.content_hash.to_hex(),
        POWI_FAMILY_MANIFEST_HASH,
        "family bits changed: {} vs {POWI_FAMILY_MANIFEST_HASH} — bump only with \
         semantic justification (golden-evidence policy)",
        report.content_hash
    );
}

// ---- Scenario (b): a falsifier hit on a wrong certificate constant ----
//
// A toy certificate claims |Σ_{k≤n} 1/k² − π²/6| ≤ 1/(2n). The true tail
// is ~1/n, so the claim is wrong for every n — a systematic constant
// error, exactly what an independent falsifier exists to catch.

#[derive(Debug, Clone, PartialEq)]
struct TailClaim {
    n: u64,
}

impl Canon for TailClaim {
    fn canon(&self, out: &mut Vec<u8>) {
        self.n.canon(out);
    }
}

impl Shrink for TailClaim {
    fn shrink_candidates(&self) -> Vec<Self> {
        let mut out = Vec::new();
        if self.n > 1 {
            out.push(TailClaim { n: self.n / 2 });
            out.push(TailClaim { n: self.n - 1 });
        }
        out
    }
}

fn falsifier_refutes(c: &TailClaim) -> bool {
    let n = c.n;
    let mut s = 0.0f64;
    for k in 1..=n {
        let kf = k as f64;
        s += 1.0 / (kf * kf);
    }
    let truth = std::f64::consts::PI * std::f64::consts::PI / 6.0;
    let claimed_bound = 1.0 / (2.0 * n as f64);
    (s - truth).abs() > claimed_bound
}

#[test]
fn falsifier_hit_compounds_into_a_family() {
    let report = compound(
        FailureCase {
            id: "basel-tail-constant".to_string(),
            seed: 0,
            input: TailClaim { n: 4096 },
            invariant: InvariantClass::CertificateForgery,
            contract: "toy tail certificate (modeled)".to_string(),
            detail: "claimed 1/(2n) tail bound; true tail ~ 1/n".to_string(),
        },
        &falsifier_refutes,
        &|_min: &TailClaim| {
            [1u64, 2, 8, 64, 1024]
                .iter()
                .map(|&n| (format!("n={n}"), TailClaim { n }))
                .collect()
        },
        vec!["frankensim-epic-gauntlet-6nb.9".to_string()],
        Some("bound constants need a proof or a falsifier-passing margin".to_string()),
        100,
    )
    .expect("the falsifier hit must minimize");
    // Systematic error ⇒ minimizes all the way down to n = 1 and the whole
    // neighborhood fails: region evidence, not a point.
    assert!(report.converged);
    assert_eq!(report.case.input, TailClaim { n: 1 });
    assert_eq!(
        report.neighborhood.failing,
        report.neighborhood.probes.len(),
        "systematic constant error: every probe must fail"
    );
    assert!(report.family.recommended_admission().is_some());
    let live = report.family.replay(&falsifier_refutes);
    assert!(live.now_passing.is_empty());
}

// ---- G0 units: determinism, refusal, canon integrity ----

#[test]
fn minimize_is_deterministic_and_refuses_non_failures() {
    let case = powi_case();
    let a = minimize("a", &case.input, &golden_breaks, 1000).expect("fails");
    let b = minimize("b", &case.input, &golden_breaks, 1000).expect("fails");
    let canon = |s: &Sweep| {
        let mut v = Vec::new();
        s.canon(&mut v);
        v
    };
    assert_eq!(
        canon(&a.minimized),
        canon(&b.minimized),
        "bitwise-identical minimum"
    );
    assert_eq!(a.steps, b.steps);
    assert_eq!(a.tried, b.tried);
    // A passing input is a typed refusal, never a fake minimum.
    let passing = Sweep {
        base: 0.7,
        exponents: vec![1, 2, 3],
    };
    assert_eq!(
        minimize("p", &passing, &golden_breaks, 10).unwrap_err(),
        CompoundError::NotFailing {
            id: "p".to_string()
        }
    );
}

#[test]
fn canon_encoding_resists_concatenation_collisions() {
    let h = |parts: &[&str]| {
        let mut v = Vec::new();
        for p in parts {
            p.canon(&mut v);
        }
        fs_blake3::hash_bytes(&v)
    };
    assert_ne!(h(&["ab", "c"]), h(&["a", "bc"]));
    let mut v1 = Vec::new();
    vec![1u64, 2].canon(&mut v1);
    let mut v2 = Vec::new();
    vec![1u64].canon(&mut v2);
    2u64.canon(&mut v2);
    assert_ne!(
        fs_blake3::hash_bytes(&v1),
        fs_blake3::hash_bytes(&v2),
        "length prefixes must separate"
    );
}

#[test]
fn content_hash_is_sensitive_to_every_field() {
    let base = test_family(
        "f",
        InvariantClass::GoldenDrift,
        1,
        vec!["t".to_string()],
        None,
    );
    let h0 = base.content_hash();
    assert_ne!(
        h0,
        test_family(
            "g",
            InvariantClass::GoldenDrift,
            1,
            vec!["t".to_string()],
            None,
        )
        .content_hash()
    );
    assert_ne!(
        h0,
        test_family(
            "f",
            InvariantClass::EnclosureViolation,
            1,
            vec!["t".to_string()],
            None,
        )
        .content_hash()
    );
    assert_ne!(
        h0,
        test_family(
            "f",
            InvariantClass::GoldenDrift,
            2,
            vec!["t".to_string()],
            None,
        )
        .content_hash()
    );
    assert_ne!(
        h0,
        test_family(
            "f",
            InvariantClass::GoldenDrift,
            1,
            vec!["t".to_string(), "u".to_string()],
            None,
        )
        .content_hash()
    );
    assert_ne!(
        h0,
        test_family(
            "f",
            InvariantClass::GoldenDrift,
            1,
            vec!["t".to_string()],
            Some("rule".to_string()),
        )
        .content_hash()
    );
}

#[test]
fn content_hash_is_sensitive_to_provenance_fields() {
    let hash = |provenance| {
        test_family_with_provenance(
            "f",
            InvariantClass::GoldenDrift,
            1,
            vec!["t".to_string()],
            None,
            provenance,
        )
        .content_hash()
    };
    let h0 = hash(provenance(7));
    assert_ne!(h0, hash(provenance(8)), "seed is semantic");
    assert_ne!(
        h0,
        hash(
            FamilyProvenance::new(
                7,
                "fs-bisect::other-contract".to_string(),
                "seeded regression".to_string(),
            )
            .unwrap()
        ),
        "contract is semantic"
    );
    assert_ne!(
        h0,
        hash(
            FamilyProvenance::new(
                7,
                "fs-bisect::compound".to_string(),
                "different diagnosis".to_string(),
            )
            .unwrap()
        ),
        "detail is semantic"
    );
}

#[derive(Clone)]
struct WideShrink;

impl Shrink for WideShrink {
    fn shrink_candidates(&self) -> Vec<Self> {
        vec![Self; MAX_SHRINK_CANDIDATES_PER_STEP + 1]
    }
}

impl Canon for WideShrink {
    fn canon(&self, out: &mut Vec<u8>) {
        1u64.canon(out);
    }
}

struct HugeCanon;

impl Canon for HugeCanon {
    fn canon(&self, out: &mut Vec<u8>) {
        out.resize(MAX_CANONICAL_MEMBER_BYTES + 1, 0);
    }
}

struct EmptyCanon;

impl Canon for EmptyCanon {
    fn canon(&self, _out: &mut Vec<u8>) {}
}

#[test]
fn work_envelopes_refuse_at_limit_plus_one() {
    assert!(matches!(
        minimize("wide", &WideShrink, &|_| true, 1),
        Err(CompoundError::LimitExceeded {
            resource: "shrink_candidates_per_step",
            requested,
            max: MAX_SHRINK_CANDIDATES_PER_STEP,
        }) if requested == MAX_SHRINK_CANDIDATES_PER_STEP + 1
    ));
    assert!(matches!(
        minimize(
            "steps",
            &WideShrink,
            &|_| true,
            MAX_MINIMIZE_STEPS + 1,
        ),
        Err(CompoundError::LimitExceeded {
            resource: "minimize_steps",
            requested,
            max: MAX_MINIMIZE_STEPS,
        }) if requested == MAX_MINIMIZE_STEPS + 1
    ));
    let oversized_id = "x".repeat(MAX_IDENTIFIER_BYTES + 1);
    assert!(matches!(
        minimize(&oversized_id, &WideShrink, &|_| true, 0),
        Err(CompoundError::LimitExceeded {
            resource: "case_id",
            requested,
            max: MAX_IDENTIFIER_BYTES,
        }) if requested == MAX_IDENTIFIER_BYTES + 1
    ));
}

#[test]
fn neighborhood_count_and_labels_are_bounded_and_unambiguous() {
    let at_limit: Vec<(String, u64)> = (0..MAX_NEIGHBOR_PROBES)
        .map(|index| (format!("n-{index}"), index as u64))
        .collect();
    assert_eq!(
        probe_neighborhood(&at_limit, &|_| false)
            .expect("exact neighborhood cap")
            .probes
            .len(),
        MAX_NEIGHBOR_PROBES
    );
    let mut over_limit = at_limit;
    over_limit.push(("over".to_string(), 0));
    assert!(matches!(
        probe_neighborhood(&over_limit, &|_| false),
        Err(CompoundError::LimitExceeded {
            resource: "neighbor_probes",
            requested,
            max: MAX_NEIGHBOR_PROBES,
        }) if requested == MAX_NEIGHBOR_PROBES + 1
    ));
    let duplicate = vec![("same".to_string(), 1), ("same".to_string(), 2)];
    assert!(matches!(
        probe_neighborhood(&duplicate, &|_| true),
        Err(CompoundError::DuplicateIdentity {
            field: "neighbor_label",
            ..
        })
    ));
}

#[test]
fn family_construction_seals_tracking_invariants_and_canonical_size() {
    assert!(matches!(
        RegressionFamily::new(
            "Not_Kebab".to_string(),
            InvariantClass::GoldenDrift,
            vec![("minimized".to_string(), 1u64)],
            vec!["frankensim-j3q2".to_string()],
            None,
            provenance(0),
        ),
        Err(CompoundError::InvalidField {
            field: "case_id",
            ..
        })
    ));
    assert!(matches!(
        RegressionFamily::new(
            "untracked".to_string(),
            InvariantClass::GoldenDrift,
            vec![("minimized".to_string(), 1u64)],
            Vec::new(),
            None,
            provenance(0),
        ),
        Err(CompoundError::InvalidField {
            field: "tracking",
            ..
        })
    ));
    assert!(matches!(
        RegressionFamily::new(
            "wrong-first".to_string(),
            InvariantClass::GoldenDrift,
            vec![("neighbor".to_string(), 1u64)],
            vec!["frankensim-j3q2".to_string()],
            None,
            provenance(0),
        ),
        Err(CompoundError::InvalidField {
            field: "members",
            ..
        })
    ));
    assert!(matches!(
        RegressionFamily::new(
            "reserved".to_string(),
            InvariantClass::Other("golden-drift".to_string()),
            vec![("minimized".to_string(), 1u64)],
            vec!["t".to_string()],
            None,
            provenance(0),
        ),
        Err(CompoundError::InvalidField {
            field: "invariant",
            ..
        })
    ));

    assert!(matches!(
        RegressionFamily::new(
            "huge".to_string(),
            InvariantClass::Other("custom-bound".to_string()),
            vec![("minimized".to_string(), HugeCanon)],
            vec!["frankensim-j3q2".to_string()],
            None,
            provenance(0),
        ),
        Err(CompoundError::LimitExceeded {
            resource: "canonical_member_bytes",
            requested,
            max: MAX_CANONICAL_MEMBER_BYTES,
        }) if requested == MAX_CANONICAL_MEMBER_BYTES + 1
    ));
    assert!(matches!(
        RegressionFamily::new(
            "empty-canon".to_string(),
            InvariantClass::Other("custom-bound".to_string()),
            vec![("minimized".to_string(), EmptyCanon)],
            vec!["frankensim-j3q2".to_string()],
            None,
            provenance(0),
        ),
        Err(CompoundError::InvalidField {
            field: "member_canon",
            ..
        })
    ));
}

#[test]
fn invalid_family_authority_refuses_before_predicate_work() {
    use std::cell::Cell;

    let calls = Cell::new(0usize);
    let result = compound(
        FailureCase {
            id: "preflight".to_string(),
            seed: 0,
            input: WideShrink,
            invariant: InvariantClass::GoldenDrift,
            contract: "fs-bisect::compound".to_string(),
            detail: "seeded failure".to_string(),
        },
        &|_| {
            calls.set(calls.get() + 1);
            true
        },
        &|_| panic!("neighbors must not run for an untracked family"),
        Vec::new(),
        None,
        0,
    );
    assert!(matches!(
        result,
        Err(CompoundError::InvalidField {
            field: "tracking",
            ..
        })
    ));
    assert_eq!(calls.get(), 0, "authority preflight must precede work");
}

#[test]
fn manifest_escapes_fields_and_hashes_the_canonical_snapshot() {
    use std::cell::Cell;

    struct MutableCanon(Cell<u64>);
    impl Canon for MutableCanon {
        fn canon(&self, out: &mut Vec<u8>) {
            self.0.get().canon(out);
        }
    }

    let family = RegressionFamily::new(
        "family-escape".to_string(),
        InvariantClass::Other("custom-\"\\".to_string()),
        vec![
            ("minimized".to_string(), MutableCanon(Cell::new(7))),
            ("member-\"\\".to_string(), MutableCanon(Cell::new(9))),
        ],
        vec!["bead-\"\\".to_string()],
        Some("line one\n\"line two\" \\".to_string()),
        FamilyProvenance::new(
            11,
            "contract \"quoted\"".to_string(),
            "detail line one\nline two".to_string(),
        )
        .expect("valid escaped provenance"),
    )
    .expect("escapable visible identifiers");
    let before = family.content_hash();
    family.members()[0].1.0.set(8);
    assert_eq!(
        family.content_hash(),
        before,
        "content identity must use the sealed construction-time canonical bytes"
    );
    let manifest = family.manifest();
    let header = manifest.lines().next().expect("header");
    assert!(header.contains("\\\""), "quotes escaped: {header}");
    assert!(header.contains("\\\\"), "backslashes escaped: {header}");
    assert!(header.contains("\\n"), "newlines escaped: {header}");
    assert!(!header.contains('\n'), "one JSON object per line");
    assert!(manifest.ends_with('\n'));
}
