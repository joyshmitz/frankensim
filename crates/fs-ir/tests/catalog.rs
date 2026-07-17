//! G0 battery for the operator catalog (bead gp3.6): generation
//! determinism, registry/dispatch drift in both directions, executable
//! examples (documentation rots loudly), deliberate-mismatch refusal,
//! structured query fixtures, diffability, and the serve-latency
//! budget (strict variant gated for quiet perf lanes).

use fs_ir::catalog::{
    ARITH_SAME_DIMS, COMPARE_FORMS, Catalog, CatalogQuery, OperatorEntry, OperatorKind, SUGAR_VERBS,
};
use fs_ir::lower::lower;
use fs_ir::sexpr;

fn head_symbol(node: &fs_ir::Node) -> Option<&str> {
    match &node.items()?.first()?.kind {
        fs_ir::NodeKind::Symbol(name) => Some(name.as_str()),
        _ => None,
    }
}

fn parse(program: &str) -> fs_ir::Node {
    sexpr::parse(program).expect("catalog example must parse")
}

#[test]
fn cat_001_generation_is_deterministic_and_validates() {
    let first = Catalog::builtin();
    let second = Catalog::builtin();
    assert_eq!(
        first.to_canonical_jsonl(),
        second.to_canonical_jsonl(),
        "two generations must export byte-identical canonical JSONL"
    );
    first.validate().expect("the built-in catalog validates");
    assert!(
        first.entries().len() >= SUGAR_VERBS.len() + ARITH_SAME_DIMS.len() + COMPARE_FORMS.len(),
        "catalog covers every registered surface"
    );
}

#[test]
fn cat_002_every_sugar_verb_lowers_and_dispatch_knows_no_others() {
    let catalog = Catalog::builtin();
    for entry in catalog.entries() {
        let OperatorKind::SugarVerb {
            lowers_to,
            injected_defaults,
        } = &entry.kind
        else {
            continue;
        };
        // The entry's FIRST example is the minimal invocation: lowering
        // it must succeed, expand to the declared target, and inject
        // exactly the declared defaults — executed drift proof.
        let example = entry.examples.first().expect("sugar entry has an example");
        let lowered = lower(&parse(example))
            .unwrap_or_else(|error| panic!("{} example must lower: {error:?}", entry.name));
        let trace = lowered.trace;
        let head = head_symbol(&lowered.node).unwrap_or_default();
        assert_eq!(
            head, *lowers_to,
            "{}: catalog lowers_to and the real expansion disagree",
            entry.name
        );
        let step = trace
            .iter()
            .find(|step| step.verb == entry.name)
            .unwrap_or_else(|| panic!("{}: lowering recorded no trace step", entry.name));
        assert_eq!(
            step.injected, *injected_defaults,
            "{}: catalog injected_defaults and the real trace disagree",
            entry.name
        );
    }
    // The reverse direction: a head the registry does not know must
    // pass through lower untouched (no hidden sugar dispatch).
    let untouched =
        lower(&parse("(not-a-registered-verb 1 2)")).expect("unknown heads pass through");
    assert_eq!(
        head_symbol(&untouched.node),
        Some("not-a-registered-verb"),
        "unknown heads must not be expanded"
    );
    assert!(
        untouched.trace.is_empty(),
        "no trace step for unregistered heads"
    );
}

#[test]
fn cat_003_every_example_parses() {
    let catalog = Catalog::builtin();
    for entry in catalog.entries() {
        for example in entry.examples {
            let parsed = parse(example);
            assert!(
                parsed.items().is_some(),
                "{}: example {example:?} must be a form",
                entry.name
            );
        }
    }
}

#[test]
fn cat_004_deliberate_mismatch_fails_validation() {
    // A sugar entry whose target is not cataloged: the drift lint must
    // refuse — this is the "deliberately mismatched hand-annotation
    // fails the build" acceptance criterion exercised at the unit
    // level (the build-level enforcement is cat_001 + cat_002 running
    // in CI).
    let catalog = Catalog::builtin();
    let mut entries: Vec<OperatorEntry> = catalog.entries().to_vec();
    for entry in &mut entries {
        if let OperatorKind::SugarVerb { lowers_to, .. } = &mut entry.kind {
            *lowers_to = "engine.that-does-not-exist";
            break;
        }
    }
    let tampered = Catalog::from_entries_for_test(entries);
    let drifts = tampered
        .validate()
        .expect_err("a mismatched lowers_to must fail validation");
    assert!(
        drifts
            .iter()
            .any(|d| d.detail.contains("engine.that-does-not-exist")),
        "the drift names the mismatched target: {drifts:?}"
    );
}

#[test]
fn cat_005_structured_queries_return_correct_sets() {
    let catalog = Catalog::builtin();

    let sugar = catalog.query(&CatalogQuery {
        namespace: Some("sugar".to_string()),
        ..CatalogQuery::default()
    });
    assert_eq!(
        sugar.iter().map(|e| e.name).collect::<Vec<_>>(),
        SUGAR_VERBS.to_vec(),
        "namespace=sugar returns exactly the registered sugar verbs"
    );

    let ascent = catalog.query(&CatalogQuery {
        name: Some("ascent.*".to_string()),
        ..CatalogQuery::default()
    });
    assert_eq!(ascent.len(), 1);
    assert_eq!(ascent[0].name, "ascent.optimize");

    let granted = catalog.query(&CatalogQuery {
        granted_capabilities: Some(vec!["flux.*".to_string()]),
        ..CatalogQuery::default()
    });
    assert!(
        granted.iter().any(|e| e.name == "flux.free-surface-lbm"),
        "flux.* grants admit the flux operator"
    );
    assert!(
        granted.iter().all(|e| e.name != "ascent.optimize"),
        "flux.* grants do not admit ascent.optimize"
    );

    let probabilistic = catalog.query(&CatalogQuery {
        namespace: Some("query".to_string()),
        ..CatalogQuery::default()
    });
    let exceedance = probabilistic
        .iter()
        .find(|e| e.name == "exceedance")
        .expect("exceedance cataloged");
    assert!(
        matches!(
            exceedance.kind,
            OperatorKind::QoiOperator {
                probabilistic: true,
                linear: false,
                ..
            }
        ),
        "exceedance metadata derives from the live Qoi menu"
    );

    let text = catalog.query(&CatalogQuery {
        text: Some("free-surface".to_string()),
        ..CatalogQuery::default()
    });
    assert!(text.iter().any(|e| e.name == "flux.free-surface-lbm"));
}

#[test]
fn cat_006_diff_reports_added_removed_changed() {
    let catalog = Catalog::builtin();
    let current = catalog.to_canonical_jsonl();

    let unchanged = catalog.diff(&current);
    assert!(unchanged.added.is_empty());
    assert!(unchanged.removed.is_empty());
    assert!(unchanged.changed.is_empty());

    // Remove one line -> reported as added (present now, absent then);
    // tamper one line -> reported as changed.
    let without_integral: String = current
        .lines()
        .filter(|line| !line.contains("\"name\":\"integral\""))
        .map(|line| format!("{line}\n"))
        .collect();
    let diff = catalog.diff(&without_integral);
    assert_eq!(diff.added, vec!["integral".to_string()]);
    assert!(diff.removed.is_empty());

    let tampered: String = current
        .lines()
        .map(|line| {
            if line.contains("\"name\":\"integral\"") {
                format!("{}\n", line.replace("\"linear\":true", "\"linear\":false"))
            } else {
                format!("{line}\n")
            }
        })
        .collect();
    let diff = catalog.diff(&tampered);
    assert_eq!(diff.changed, vec!["integral".to_string()]);
}

#[test]
fn cat_007_query_latency_smoke() {
    // Generous default-suite bound (shared hosts are noisy); the
    // strict ≤100ms certification lives in cat_008 for quiet lanes.
    let catalog = Catalog::builtin();
    let queries = latency_query_set();
    let start = std::time::Instant::now();
    let mut total = 0usize;
    for query in &queries {
        total += catalog.query(query).len();
    }
    let elapsed = start.elapsed();
    assert!(total > 0, "the latency sweep exercises real matches");
    assert!(
        elapsed.as_millis() < 1000,
        "catalog query sweep took {elapsed:?} (default-suite smoke bound)"
    );
}

#[test]
#[ignore = "quiet-lane certification of the plan §11.5 ≤100ms serve budget; run with --ignored on an unloaded host"]
fn cat_008_query_latency_certification() {
    let catalog = Catalog::builtin();
    let queries = latency_query_set();
    let start = std::time::Instant::now();
    for _ in 0..100 {
        for query in &queries {
            std::hint::black_box(catalog.query(query));
        }
    }
    let elapsed = start.elapsed();
    let per_sweep = elapsed / 100;
    assert!(
        per_sweep.as_millis() <= 100,
        "full-catalog query sweep {per_sweep:?} exceeds the 100ms serve budget"
    );
}

fn latency_query_set() -> Vec<CatalogQuery> {
    vec![
        CatalogQuery::default(),
        CatalogQuery {
            namespace: Some("sugar".to_string()),
            ..CatalogQuery::default()
        },
        CatalogQuery {
            namespace: Some("query".to_string()),
            ..CatalogQuery::default()
        },
        CatalogQuery {
            name: Some("flux.*".to_string()),
            ..CatalogQuery::default()
        },
        CatalogQuery {
            granted_capabilities: Some(vec!["ascent.*".to_string(), "flux.*".to_string()]),
            ..CatalogQuery::default()
        },
        CatalogQuery {
            text: Some("optimize".to_string()),
            ..CatalogQuery::default()
        },
    ]
}
