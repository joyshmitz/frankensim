//! I10.1 G0/G3 conformance for the authority-separated multi-case schema.
//!
//! Tests use deterministic JSON diagnostics so the central batch verifier can
//! retain exact refusal/identity context.  No test treats a hash as laboratory
//! authentication or an identifiability theorem.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use fs_blake3::{ContentHash, hash_domain};
use fs_evidence::ValidityDomain;
use fs_evidence::vv::*;
use fs_matdb::{
    ClaimSet, ConstitutiveModelCard, InitialStatePolicy, LawId, LawParameter, MATDB_SCHEMA_VERSION,
    MaterialCard, MaterialStateId, Provenance,
};
use fs_material::identifiability::*;
use fs_qty::{Dims, QuantitySpec};

const STRESS: Dims = Dims([-1, 1, -2, 0, 0, 0]);
const DIMENSIONLESS: Dims = Dims([0; 6]);
const TEST_HASH_DOMAIN: &str = "org.frankensim.fs-material.identifiability-authority-test.v1";

fn escape_json(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                write!(&mut escaped, "\\u{:04x}", character as u32)
                    .expect("writing JSON escape to String cannot fail");
            }
            character => escaped.push(character),
        }
    }
    escaped
}

fn log(case: &str, verdict: &str, detail: &str) {
    println!(
        "{{\"suite\":\"fs-material/identifiability-authority\",\
         \"case\":\"{}\",\"verdict\":\"{}\",\"detail\":\"{}\"}}",
        escape_json(case),
        escape_json(verdict),
        escape_json(detail),
    );
}

fn hash(label: &str) -> ContentHash {
    hash_domain(TEST_HASH_DOMAIN, label.as_bytes())
}

fn artifact(value: &str) -> ArtifactId {
    ArtifactId::try_new(value).expect("fixture artifact token")
}

fn qoi(value: &str) -> QoiId {
    QoiId::try_new(value).expect("fixture QoI token")
}

fn unit(value: &str) -> UnitId {
    UnitId::try_new(value).expect("fixture unit token")
}

fn axis(value: &str) -> AxisId {
    AxisId::try_new(value).expect("fixture axis token")
}

fn role(value: &str) -> ParameterRoleId {
    ParameterRoleId::try_new(value).expect("fixture parameter role")
}

fn case_id(value: &str) -> CaseId {
    CaseId::try_new(value).expect("fixture case id")
}

fn source_key(value: &str) -> SourceKey {
    SourceKey::try_new(value).expect("fixture source key")
}

fn channel(value: &str) -> ObservationChannelId {
    ObservationChannelId::try_new(value).expect("fixture channel")
}

fn header(id: &str, capability: &str) -> ArtifactHeader {
    header_with_units(id, capability, &["Pa"])
}

fn header_with_units(id: &str, capability: &str, units: &[&str]) -> ArtifactHeader {
    ArtifactHeader::try_new(
        artifact(id),
        units.iter().map(|value| unit(value)).collect(),
        SeedDeclaration::Fixed(0x1d3_171f),
        DeclaredBudget::Limit(1.0e-9),
        DeclaredBudget::Limit(30_000),
        DeclaredBudget::Limit(32 << 20),
        vec![("fixture".to_string(), "1".to_string())],
        vec![capability.to_string()],
    )
    .expect("Five Explicits fixture")
}

fn read_u32_le(bytes: &[u8], at: &mut usize, field: &str) -> u32 {
    let end = *at + 4;
    let encoded: [u8; 4] = bytes
        .get(*at..end)
        .unwrap_or_else(|| panic!("missing {field} at byte {}", *at))
        .try_into()
        .expect("four-byte canonical u32");
    *at = end;
    u32::from_le_bytes(encoded)
}

fn read_u64_le(bytes: &[u8], at: &mut usize, field: &str) -> u64 {
    let end = *at + 8;
    let encoded: [u8; 8] = bytes
        .get(*at..end)
        .unwrap_or_else(|| panic!("missing {field} at byte {}", *at))
        .try_into()
        .expect("eight-byte canonical u64");
    *at = end;
    u64::from_le_bytes(encoded)
}

fn read_text<'a>(bytes: &'a [u8], at: &mut usize, field: &str) -> &'a str {
    let len = usize::try_from(read_u32_le(bytes, at, field)).expect("u32 fits usize");
    let end = *at + len;
    let value = std::str::from_utf8(
        bytes
            .get(*at..end)
            .unwrap_or_else(|| panic!("truncated {field} at byte {}", *at)),
    )
    .unwrap_or_else(|error| panic!("non-UTF-8 {field}: {error}"));
    *at = end;
    value
}

/// Assert the exact identity-mode header grammar and return the first byte
/// after it. This is intentionally independent of the production decoder: it
/// pins the projection marker, field order, collection framing, and numeric
/// endianness that the identity declarations promise.
fn assert_identity_header_layout(bytes: &[u8], magic: &[u8], header: &ArtifactHeader) -> usize {
    assert!(bytes.starts_with(magic), "identity wire magic moved");
    let mut at = magic.len();
    assert_eq!(read_u32_le(bytes, &mut at, "schema version"), 1);
    assert_eq!(
        bytes.get(at),
        Some(&0),
        "identity header marker must be zero"
    );
    at += 1;

    assert_eq!(
        read_u32_le(bytes, &mut at, "header unit count"),
        u32::try_from(header.units().len()).expect("bounded unit count"),
    );
    for expected in header.units() {
        assert_eq!(read_text(bytes, &mut at, "header unit"), expected.as_str());
    }

    match header.seed() {
        SeedDeclaration::Fixed(seed) => {
            assert_eq!(bytes.get(at), Some(&0), "fixed-seed tag moved");
            at += 1;
            assert_eq!(read_u64_le(bytes, &mut at, "seed"), *seed);
        }
        SeedDeclaration::NotApplicable { .. } => {
            panic!("wire-layout fixture unexpectedly uses a non-numeric seed")
        }
    }
    match header.accuracy() {
        DeclaredBudget::Limit(value) => {
            assert_eq!(bytes.get(at), Some(&0), "accuracy-limit tag moved");
            at += 1;
            assert_eq!(read_u64_le(bytes, &mut at, "accuracy"), value.to_bits());
        }
        DeclaredBudget::NotApplicable { .. } => {
            panic!("wire-layout fixture unexpectedly omits accuracy")
        }
    }
    for (field, budget) in [
        ("time budget", header.time_ms()),
        ("memory budget", header.memory_bytes()),
    ] {
        match budget {
            DeclaredBudget::Limit(value) => {
                assert_eq!(bytes.get(at), Some(&0), "{field} limit tag moved");
                at += 1;
                assert_eq!(read_u64_le(bytes, &mut at, field), *value);
            }
            DeclaredBudget::NotApplicable { .. } => {
                panic!("wire-layout fixture unexpectedly omits {field}")
            }
        }
    }

    assert_eq!(
        read_u32_le(bytes, &mut at, "header version count"),
        u32::try_from(header.versions().len()).expect("bounded version count"),
    );
    for (component, version) in header.versions() {
        assert_eq!(read_text(bytes, &mut at, "version component"), component);
        assert_eq!(read_text(bytes, &mut at, "version value"), version);
    }
    assert_eq!(
        read_u32_le(bytes, &mut at, "header capability count"),
        u32::try_from(header.capabilities().len()).expect("bounded capability count"),
    );
    for capability in header.capabilities() {
        assert_eq!(read_text(bytes, &mut at, "capability"), capability);
    }
    at
}

fn project_exact_header_to_identity(
    exact: &[u8],
    magic: &[u8],
    header: &ArtifactHeader,
) -> Vec<u8> {
    assert!(exact.starts_with(magic), "exact transport magic moved");
    let marker_at = magic.len() + 4;
    assert_eq!(
        exact.get(marker_at),
        Some(&1),
        "exact header marker must be one"
    );
    let mut id_len_at = marker_at + 1;
    let id_len = usize::try_from(read_u32_le(exact, &mut id_len_at, "artifact id length"))
        .expect("artifact id length fits usize");
    assert_eq!(id_len, header.id().as_str().len());
    let id_end = id_len_at + id_len;
    assert_eq!(
        exact.get(id_len_at..id_end),
        Some(header.id().as_str().as_bytes()),
        "exact artifact label moved",
    );

    let mut projected = Vec::with_capacity(exact.len() - 4 - id_len);
    projected.extend_from_slice(&exact[..marker_at]);
    projected.push(0);
    projected.extend_from_slice(&exact[id_end..]);
    projected
}

fn context() -> ContextOfUse {
    ContextOfUse::try_new(
        header_with_units("context-1", "fixture.context", &["Pa", "K"]),
        "Choose a constitutive calibration that predicts stress response.",
        vec![
            QoiSpec::try_new(
                qoi("stress"),
                "axial stress",
                unit("Pa"),
                AcceptanceCriterion::ClosedRange {
                    lo: -2.0e9,
                    hi: 2.0e9,
                },
            )
            .expect("stress QoI"),
            QoiSpec::try_new(
                qoi("tangent"),
                "algorithmic tangent",
                unit("Pa"),
                AcceptanceCriterion::ClosedRange {
                    lo: -2.0e9,
                    hi: 2.0e9,
                },
            )
            .expect("tangent QoI"),
        ],
        ApplicabilityDomain::try_new(
            vec![
                NumericDomainAxis::try_new(axis("temperature"), unit("K"), 250.0, 450.0)
                    .expect("temperature applicability"),
            ],
            Vec::new(),
        )
        .expect("applicability domain"),
        ApplicabilityPolicy::Demote,
    )
    .expect("context fixture")
}

fn model_cards() -> (MaterialCard, ConstitutiveModelCard) {
    let mut parameters = BTreeMap::new();
    parameters.insert(
        "yield_stress".to_string(),
        LawParameter {
            value: 276.0e6,
            dims: STRESS,
        },
    );
    parameters.insert(
        "hardening_modulus".to_string(),
        LawParameter {
            value: 1.2e9,
            dims: STRESS,
        },
    );
    let model = ConstitutiveModelCard {
        law: LawId("j2-identifiability-authority-fixture".to_string()),
        law_version: 3,
        parameters,
        state_schema_version: 2,
        initial_state: InitialStatePolicy::ZeroInternalState,
        validity: ValidityDomain::unconstrained().with("temperature", 250.0, 450.0),
        sources: vec![hash("model-source")],
        provenance: Provenance {
            source: "authority fixture".to_string(),
            license: "test-only".to_string(),
            artifact: Some(hash("model-provenance")),
        },
    };
    let material = MaterialCard::assemble(
        MaterialStateId {
            chemistry: "AA6061".to_string(),
            phase: "wrought".to_string(),
            process: "T6".to_string(),
            revision: 0,
        },
        ClaimSet::new(),
        vec![model.clone()],
    )
    .expect("material card fixture");
    (material, model)
}

fn source(value: &str, kind: SourceKind, content: ContentHash) -> SourceRef {
    let (domain, version) = match kind {
        SourceKind::ContextOfUse
        | SourceKind::ExperimentArtifact
        | SourceKind::CalibrationSplit => (VV_ARTIFACT_SOURCE_DOMAIN, VV_SCHEMA_VERSION),
        SourceKind::MaterialCard => (MATERIAL_CARD_SOURCE_DOMAIN, MATDB_SCHEMA_VERSION),
        SourceKind::ConstitutiveModelCard => {
            (CONSTITUTIVE_MODEL_CARD_SOURCE_DOMAIN, MATDB_SCHEMA_VERSION)
        }
        _ => (TEST_HASH_DOMAIN, 1),
    };
    SourceRef::try_new(source_key(value), kind, content, domain, version)
        .expect("source reference fixture")
}

fn frame(case: &str) -> FrameBinding {
    FrameBinding::try_new(
        artifact(&format!("frame-{case}")),
        hash(&format!("frame-transform-{case}")),
        "right-handed-cartesian",
    )
    .expect("frame fixture")
}

fn protocol(case: &str) -> ProtocolBinding {
    ProtocolBinding::try_new(
        artifact(&format!("protocol-{case}")),
        7,
        2,
        3,
        hash(&format!("load-path-{case}")),
        hash(&format!("environment-path-{case}")),
        hash(&format!("time-grid-{case}")),
        artifact(&format!("clock-{case}")),
    )
    .expect("protocol fixture")
}

fn specimen(case: &str, frame: FrameBinding) -> SpecimenBinding {
    SpecimenBinding::try_new(
        artifact(&format!("specimen-{case}")),
        hash(&format!("geometry-{case}")),
        hash(&format!("process-{case}")),
        hash(&format!("preparation-{case}")),
        frame,
    )
    .expect("specimen fixture")
}

fn parameter(
    name: &str,
    treatment: ParameterTreatment,
    coverage: InfluenceCoverage,
    prior_version: u32,
) -> StudyParameter {
    let domain = if name == "yield_stress" {
        ParameterDomain::try_new(1.0e6, 1.0e9).expect("yield domain")
    } else {
        ParameterDomain::try_new(1.0e7, 5.0e9).expect("hardening domain")
    };
    StudyParameter::try_new(
        role(name),
        QuantitySpec::dimensional(STRESS),
        domain,
        if name == "yield_stress" {
            ParameterPurpose::Estimand
        } else {
            ParameterPurpose::Nuisance
        },
        treatment,
        ParameterOwnerBinding::ConstitutiveModel,
        ParameterScopeBinding::Global,
        PriorPolicy::Distribution(ParameterPrior::Uniform {
            version: prior_version,
            domain,
        }),
        coverage,
    )
    .expect("study parameter fixture")
}

#[derive(Debug, Clone, Copy, Default)]
struct ProblemOptions {
    reverse_cases: bool,
    missing_hardening_influence: bool,
    dangling_operator: bool,
    dense_with_bounded_marginal: bool,
    overlapping_gauges: bool,
    bad_constraint_units: bool,
    derived_cycle: bool,
    retrospective_reuse: bool,
    declared_sharing: bool,
    bad_observation_endpoint: bool,
    self_correlation: bool,
    alternate_graph_domain: bool,
    context_contract_mutation: u8,
    second_case_complementary: bool,
    claim_domain_in_problem: bool,
    parameter_prior_version: u32,
    valid_constraint: bool,
    yield_log_scale: bool,
    one_gauge: bool,
    external_noise: bool,
    alternate_sharing_justification: bool,
}

struct ProblemFixture {
    context: ContextOfUse,
    material: MaterialCard,
    model: ConstitutiveModelCard,
    graph: ContentHash,
    document: Result<IdentifiabilityProblemDocument, IdentifiabilityError>,
}

#[derive(Debug, Clone, Copy)]
enum ProblemRoot {
    Context,
    Material,
    Model,
    Graph,
}

fn make_case(
    name: &str,
    purpose: CasePurpose,
    qoi_name: &str,
    channel_name: &str,
    bounded_noise: bool,
    retrospective: bool,
    experiment_key: &str,
) -> StudyCaseDocument {
    let frame = frame(name);
    let protocol = protocol(name);
    let rows = if retrospective {
        ObservationRows::Retrospective(BTreeSet::from([ObservationId::try_new(format!(
            "row-{name}"
        ))
        .expect("row fixture")]))
    } else {
        ObservationRows::Prospective
    };
    let observation = StudyObservation::try_new(
        channel(channel_name),
        qoi(qoi_name),
        unit("Pa"),
        QuantitySpec::dimensional(STRESS),
        frame.clone(),
        format!("node-{name}"),
        "stress-output",
        source_key(if name == "a" {
            "operator-a"
        } else {
            "operator-b"
        }),
        source_key(if name == "a" {
            "aggregation-a"
        } else {
            "aggregation-b"
        }),
        source_key(if name == "a" { "sensor-a" } else { "sensor-b" }),
        artifact(&format!("instrument-{name}")),
        artifact(&format!("clock-{name}")),
        4,
        if bounded_noise {
            MarginalNoiseSpec::Bounded { half_width: 1.0 }
        } else {
            MarginalNoiseSpec::Gaussian {
                standard_deviation: 2.0e5,
            }
        },
        MissingnessAssumption::Unknown {
            reason: "missingness has not yet been characterized".to_string(),
        },
        None,
        7,
        3,
        rows,
    )
    .expect("observation fixture");
    let data = if retrospective {
        CaseDataDeclaration::Retrospective {
            experiment: source_key(experiment_key),
            split: source_key(if name == "a" { "split-a" } else { "split-b" }),
            parser: source_key("parser"),
            preprocessing: source_key("preprocessing"),
            parser_version: 2,
            split_grouping: artifact("split-by-specimen"),
        }
    } else {
        CaseDataDeclaration::Prospective
    };
    StudyCaseDocument::try_new(
        case_id(name),
        purpose,
        InitialStateBinding::Zero { schema_version: 2 },
        specimen(name, frame),
        protocol,
        source_key(if name == "a" {
            "forward-a"
        } else {
            "forward-b"
        }),
        data,
        vec![observation],
        vec![(
            channel(channel_name),
            StudyDiscrepancy::Uncharacterized {
                reason: "no discrepancy family is admitted for this channel".to_string(),
            },
        )],
    )
    .expect("case fixture")
}

fn problem_fixture(options: ProblemOptions) -> ProblemFixture {
    let context = context();
    let (material, model) = model_cards();
    let graph = hash("constitutive-graph");
    let context_hash = context.content_hash().expect("context hashes");
    let context_source = match options.context_contract_mutation {
        0 => source("context", SourceKind::ContextOfUse, context_hash),
        1 => source("context", SourceKind::ContextOfUse, hash("wrong-context")),
        2 => SourceRef::try_new(
            source_key("context"),
            SourceKind::ContextOfUse,
            context_hash,
            TEST_HASH_DOMAIN,
            VV_SCHEMA_VERSION,
        )
        .expect("wrong-domain context reference"),
        3 => SourceRef::try_new(
            source_key("context"),
            SourceKind::ContextOfUse,
            context_hash,
            VV_ARTIFACT_SOURCE_DOMAIN,
            VV_SCHEMA_VERSION + 1,
        )
        .expect("wrong-version context reference"),
        other => panic!("unsupported context contract mutation {other}"),
    };
    let graph_source = if options.alternate_graph_domain {
        SourceRef::try_new(
            source_key("graph"),
            SourceKind::ConstitutiveGraph,
            graph,
            "org.frankensim.test.alternate-graph-domain.v1",
            1,
        )
        .expect("alternate graph-domain reference")
    } else {
        source("graph", SourceKind::ConstitutiveGraph, graph)
    };
    let mut sources = vec![
        context_source,
        source(
            "material",
            SourceKind::MaterialCard,
            material.content_hash(),
        ),
        source(
            "model",
            SourceKind::ConstitutiveModelCard,
            model.content_hash(),
        ),
        graph_source,
        source("forward-a", SourceKind::ForwardModel, hash("forward-a")),
        source("forward-b", SourceKind::ForwardModel, hash("forward-b")),
        source(
            "operator-a",
            SourceKind::ObservationOperator,
            hash("operator-a"),
        ),
        source(
            "operator-b",
            SourceKind::ObservationOperator,
            hash("operator-b"),
        ),
        source(
            "aggregation-a",
            SourceKind::ObservationOperator,
            hash("aggregation-a"),
        ),
        source(
            "aggregation-b",
            SourceKind::ObservationOperator,
            hash("aggregation-b"),
        ),
        source("sensor-a", SourceKind::Metrology, hash("sensor-a")),
        source("sensor-b", SourceKind::Metrology, hash("sensor-b")),
    ];
    if options.dangling_operator {
        sources.retain(|source| source.key().as_str() != "operator-b");
    }
    if options.retrospective_reuse {
        sources.extend([
            source(
                "experiment-shared",
                SourceKind::ExperimentArtifact,
                hash("experiment-shared"),
            ),
            source("split-a", SourceKind::CalibrationSplit, hash("split-a")),
            source("split-b", SourceKind::CalibrationSplit, hash("split-b")),
            source("parser", SourceKind::Parser, hash("parser")),
            source(
                "preprocessing",
                SourceKind::Preprocessing,
                hash("preprocessing"),
            ),
        ]);
        if options.declared_sharing {
            sources.push(source(
                "joint-likelihood",
                SourceKind::Likelihood,
                hash("joint-likelihood"),
            ));
        }
    }
    let mut cases = vec![
        make_case(
            "a",
            CasePurpose::Calibration,
            "stress",
            "stress",
            options.dense_with_bounded_marginal,
            options.retrospective_reuse,
            "experiment-shared",
        ),
        make_case(
            "b",
            if options.second_case_complementary {
                CasePurpose::Complementary {
                    reason: "case b supplies a complementary excitation".to_string(),
                }
            } else {
                CasePurpose::SymmetryBreaking
            },
            "tangent",
            "tangent",
            false,
            options.retrospective_reuse,
            "experiment-shared",
        ),
    ];
    if options.reverse_cases {
        cases.reverse();
    }
    let mut parameters = vec![
        parameter(
            "yield_stress",
            ParameterTreatment::Estimated,
            InfluenceCoverage::Declared,
            options.parameter_prior_version.max(1),
        ),
        parameter(
            "hardening_modulus",
            ParameterTreatment::Marginalized,
            InfluenceCoverage::Declared,
            options.parameter_prior_version.max(1),
        ),
    ];
    if options.derived_cycle {
        sources.push(source(
            "derived-definition",
            SourceKind::Constraint,
            hash("derived-definition"),
        ));
        for (name, parent) in [("derived-a", "derived-b"), ("derived-b", "derived-a")] {
            parameters.push(
                StudyParameter::try_new(
                    role(name),
                    QuantitySpec::dimensional(DIMENSIONLESS),
                    ParameterDomain::try_new(0.0, 1.0).expect("derived domain"),
                    ParameterPurpose::Hyperparameter,
                    ParameterTreatment::Derived {
                        definition: source_key("derived-definition"),
                        parents: BTreeSet::from([role(parent)]),
                    },
                    ParameterOwnerBinding::Population {
                        hierarchy: source_key("derived-definition"),
                    },
                    ParameterScopeBinding::Global,
                    PriorPolicy::NotApplicable {
                        reason: "derived values do not own independent priors".to_string(),
                    },
                    InfluenceCoverage::Declared,
                )
                .expect("derived parameter fixture"),
            );
        }
    }
    let mut influences = vec![InfluenceDeclaration::new(
        InfluenceId::try_new("yield-to-stress").expect("influence id"),
        role("yield_stress"),
        if options.yield_log_scale {
            DistributionFunctional::LogScale {
                observation: ObservationKey::new(case_id("a"), channel("stress")),
            }
        } else {
            DistributionFunctional::Location {
                observation: ObservationKey::new(case_id("a"), channel("stress")),
            }
        },
        InfluenceRepresentation::Direct,
    )];
    if !options.missing_hardening_influence {
        let tangent = ObservationKey::new(
            if options.bad_observation_endpoint {
                case_id("missing")
            } else {
                case_id("b")
            },
            channel("tangent"),
        );
        influences.push(InfluenceDeclaration::new(
            InfluenceId::try_new("hardening-to-tangent").expect("influence id"),
            role("hardening_modulus"),
            if options.self_correlation {
                DistributionFunctional::Correlation {
                    left: tangent.clone(),
                    right: tangent,
                }
            } else {
                DistributionFunctional::Location {
                    observation: tangent,
                }
            },
            InfluenceRepresentation::Direct,
        ));
    }
    let mut constraints = Vec::new();
    if options.valid_constraint {
        constraints.push(JointConstraint::new(
            ConstraintId::try_new("stress-balance").expect("constraint id"),
            JointConstraintKind::Affine {
                terms: vec![
                    AffineConstraintTerm::try_new(
                        role("yield_stress"),
                        1.0,
                        QuantitySpec::dimensional(DIMENSIONLESS),
                    )
                    .expect("yield term"),
                    AffineConstraintTerm::try_new(
                        role("hardening_modulus"),
                        -1.0,
                        QuantitySpec::dimensional(DIMENSIONLESS),
                    )
                    .expect("hardening term"),
                ],
                relation: ConstraintRelation::LessOrEqual,
                rhs_si: 0.0,
                residual_quantity: QuantitySpec::dimensional(STRESS),
            },
        ));
    }
    if options.bad_constraint_units {
        constraints.push(JointConstraint::new(
            ConstraintId::try_new("bad-units").expect("constraint id"),
            JointConstraintKind::Affine {
                terms: vec![
                    AffineConstraintTerm::try_new(
                        role("yield_stress"),
                        1.0,
                        QuantitySpec::dimensional(DIMENSIONLESS),
                    )
                    .expect("term"),
                    AffineConstraintTerm::try_new(
                        role("hardening_modulus"),
                        -1.0,
                        QuantitySpec::dimensional(DIMENSIONLESS),
                    )
                    .expect("term"),
                ],
                relation: ConstraintRelation::Equal,
                rhs_si: 0.0,
                residual_quantity: QuantitySpec::dimensional(DIMENSIONLESS),
            },
        ));
    }
    let mut gauges = Vec::new();
    if options.one_gauge {
        sources.push(source(
            "single-gauge-action",
            SourceKind::GaugeAction,
            hash("single-gauge-action"),
        ));
        gauges.push(
            GaugeDeclaration::try_new(
                GaugeClassId::try_new("single-gauge").expect("gauge id"),
                BTreeSet::from([role("yield_stress"), role("hardening_modulus")]),
                source_key("single-gauge-action"),
                GaugeKind::Continuous { dimension: 1 },
                GaugeHandling::Retained {
                    reason: "the fixture retains one declared gauge".to_string(),
                },
            )
            .expect("single gauge"),
        );
    }
    if options.claim_domain_in_problem {
        sources.extend([
            source(
                "claim-domain",
                SourceKind::ExternalManifold,
                hash("claim-domain"),
            ),
            source(
                "claim-domain-action",
                SourceKind::GaugeAction,
                hash("claim-domain-action"),
            ),
        ]);
        gauges.push(
            GaugeDeclaration::try_new(
                GaugeClassId::try_new("claim-domain-gauge").expect("gauge id"),
                BTreeSet::from([role("yield_stress"), role("hardening_modulus")]),
                source_key("claim-domain-action"),
                GaugeKind::Stratified {
                    strata: source_key("claim-domain"),
                },
                GaugeHandling::Retained {
                    reason: "the claim-domain strata remain explicit in this fixture".to_string(),
                },
            )
            .expect("claim-domain gauge"),
        );
    }
    if options.overlapping_gauges {
        for index in 0..2 {
            let action = format!("gauge-action-{index}");
            sources.push(source(&action, SourceKind::GaugeAction, hash(&action)));
            gauges.push(
                GaugeDeclaration::try_new(
                    GaugeClassId::try_new(format!("gauge-{index}")).expect("gauge id"),
                    BTreeSet::from([role("yield_stress"), role("hardening_modulus")]),
                    source_key(&action),
                    GaugeKind::Continuous { dimension: 1 },
                    GaugeHandling::Retained {
                        reason: "the test intentionally retains this gauge".to_string(),
                    },
                )
                .expect("gauge fixture"),
            );
        }
    }
    let joint_noise = if options.dense_with_bounded_marginal {
        sources.push(source(
            "correlation-model",
            SourceKind::Likelihood,
            hash("correlation-model"),
        ));
        JointNoiseModel::DenseCorrelation {
            order: vec![
                ObservationKey::new(case_id("a"), channel("stress")),
                ObservationKey::new(case_id("b"), channel("tangent")),
            ],
            correlation: CovarianceMatrix::try_new(2, vec![1.0, 0.0, 1.0])
                .expect("correlation matrix"),
            model: source_key("correlation-model"),
        }
    } else if options.external_noise {
        sources.push(source(
            "external-noise",
            SourceKind::Likelihood,
            hash("external-noise"),
        ));
        JointNoiseModel::ExternalKernel {
            model: source_key("external-noise"),
        }
    } else {
        JointNoiseModel::Independent
    };
    let data_reuse = if options.retrospective_reuse && options.declared_sharing {
        DataReusePolicy::Shared {
            groups: vec![
                DataSharingGroup::try_new(
                    BTreeSet::from([case_id("a"), case_id("b")]),
                    source_key("joint-likelihood"),
                    if options.alternate_sharing_justification {
                        "the exact same shared campaign is justified by an alternate audit note"
                    } else {
                        "the cases intentionally derive complementary channels from one raw campaign"
                    },
                )
                .expect("sharing group"),
            ],
        }
    } else {
        DataReusePolicy::Disjoint
    };
    let document = IdentifiabilityProblemDocument::try_new(
        source_key("context"),
        source_key("material"),
        source_key("model"),
        source_key("graph"),
        sources,
        parameters,
        constraints,
        cases,
        influences,
        gauges,
        joint_noise,
        data_reuse,
    );
    ProblemFixture {
        context,
        material,
        model,
        graph,
        document,
    }
}

fn rekey_problem_root(
    fixture: ProblemFixture,
    root: ProblemRoot,
    replacement: &str,
) -> ProblemFixture {
    let ProblemFixture {
        context,
        material,
        model,
        graph,
        document,
    } = fixture;
    let document = document.expect("problem before root rekey");
    let old_key = match root {
        ProblemRoot::Context => document.context_source(),
        ProblemRoot::Material => document.material_source(),
        ProblemRoot::Model => document.model_source(),
        ProblemRoot::Graph => document.graph_source(),
    }
    .clone();
    let replacement = source_key(replacement);
    let sources = document
        .sources()
        .values()
        .map(|source| {
            if source.key() == &old_key {
                SourceRef::try_new(
                    replacement.clone(),
                    source.kind(),
                    source.expected_hash(),
                    source.content_hash_domain(),
                    source.contract_version(),
                )
                .expect("rekeyed source")
            } else {
                source.clone()
            }
        })
        .collect();
    let context_source = if matches!(root, ProblemRoot::Context) {
        replacement.clone()
    } else {
        document.context_source().clone()
    };
    let material_source = if matches!(root, ProblemRoot::Material) {
        replacement.clone()
    } else {
        document.material_source().clone()
    };
    let model_source = if matches!(root, ProblemRoot::Model) {
        replacement.clone()
    } else {
        document.model_source().clone()
    };
    let graph_source = if matches!(root, ProblemRoot::Graph) {
        replacement
    } else {
        document.graph_source().clone()
    };
    let document = IdentifiabilityProblemDocument::try_new(
        context_source,
        material_source,
        model_source,
        graph_source,
        sources,
        document.parameters().values().cloned().collect(),
        document.constraints().values().cloned().collect(),
        document.cases().values().cloned().collect(),
        document.influences().values().cloned().collect(),
        document.gauges().values().cloned().collect(),
        document.joint_noise().clone(),
        document.data_reuse().clone(),
    );
    ProblemFixture {
        context,
        material,
        model,
        graph,
        document,
    }
}

fn unresolved_problem_identity(document: &IdentifiabilityProblemDocument) -> ContentHash {
    hash_domain(
        IDENTIFIABILITY_PROBLEM_IDENTITY_DOMAIN,
        &document.canonical_bytes().expect("canonical problem bytes"),
    )
}

fn opaque_source_preimage(source: &SourceRef) -> &[u8] {
    if source.kind() == SourceKind::ConstitutiveGraph {
        b"constitutive-graph"
    } else {
        source.key().as_str().as_bytes()
    }
}

fn opaque_resolutions(document: &IdentifiabilityProblemDocument) -> SourceResolutionSet {
    let entries = document
        .sources()
        .values()
        .filter(|source| {
            !matches!(
                source.kind(),
                SourceKind::ContextOfUse
                    | SourceKind::MaterialCard
                    | SourceKind::ConstitutiveModelCard
                    | SourceKind::ExperimentArtifact
                    | SourceKind::CalibrationSplit
            )
        })
        .map(|source| {
            SourceResolution::verify(
                source,
                opaque_source_preimage(source),
                AuthorityDisposition::ContentVerified,
            )
            .expect("opaque resolution fixture")
        })
        .collect();
    SourceResolutionSet::try_new(entries).expect("resolution set fixture")
}

fn admit_fixture(fixture: ProblemFixture) -> AdmittedIdentifiabilityProblem {
    admit_fixture_with_authority(fixture, false)
}

fn admit_fixture_with_authority(
    fixture: ProblemFixture,
    authenticated: bool,
) -> AdmittedIdentifiabilityProblem {
    admit_fixture_with_authority_and_order(fixture, authenticated, false)
}

fn admit_fixture_with_authority_and_order(
    fixture: ProblemFixture,
    authenticated: bool,
    reverse_resolutions: bool,
) -> AdmittedIdentifiabilityProblem {
    let ProblemFixture {
        context,
        material,
        model,
        graph: _,
        document,
    } = fixture;
    let document = document.expect("problem structurally admits");
    let mut entries = if authenticated {
        document
            .sources()
            .values()
            .filter(|source| {
                !matches!(
                    source.kind(),
                    SourceKind::ContextOfUse
                        | SourceKind::MaterialCard
                        | SourceKind::ConstitutiveModelCard
                        | SourceKind::ExperimentArtifact
                        | SourceKind::CalibrationSplit
                )
            })
            .map(|source| {
                SourceResolution::verify(
                    source,
                    opaque_source_preimage(source),
                    AuthorityDisposition::ExternalTrustReceipt {
                        trust_receipt: hash(&format!("trust-{}", source.key())),
                    },
                )
                .expect("authenticated resolution")
            })
            .collect::<Vec<_>>()
    } else {
        opaque_resolutions(&document)
            .entries()
            .values()
            .cloned()
            .collect::<Vec<_>>()
    };
    if reverse_resolutions {
        entries.reverse();
    }
    let opaque = SourceResolutionSet::try_new(entries).expect("canonical resolution set");
    AdmittedIdentifiabilityProblem::resolve_and_admit(
        document,
        ProblemSourceBundle::new(&context, &material, &model, BTreeMap::new(), opaque),
    )
    .expect("source-resolved problem admits")
}

fn admit_fixture_with_single_external_authority(
    fixture: ProblemFixture,
    trusted_key: &str,
) -> AdmittedIdentifiabilityProblem {
    let ProblemFixture {
        context,
        material,
        model,
        graph: _,
        document,
    } = fixture;
    let document = document.expect("problem structurally admits");
    let mut changed = 0;
    let entries = document
        .sources()
        .values()
        .filter(|source| {
            !matches!(
                source.kind(),
                SourceKind::ContextOfUse
                    | SourceKind::MaterialCard
                    | SourceKind::ConstitutiveModelCard
                    | SourceKind::ExperimentArtifact
                    | SourceKind::CalibrationSplit
            )
        })
        .map(|source| {
            let authority = if source.key().as_str() == trusted_key {
                changed += 1;
                AuthorityDisposition::ExternalTrustReceipt {
                    trust_receipt: hash(&format!("trust-{trusted_key}")),
                }
            } else {
                AuthorityDisposition::ContentVerified
            };
            SourceResolution::verify(source, opaque_source_preimage(source), authority)
                .expect("single-authority resolution")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        changed, 1,
        "exactly one requested source authority must move"
    );
    let opaque = SourceResolutionSet::try_new(entries).expect("single-authority resolution set");
    AdmittedIdentifiabilityProblem::resolve_and_admit(
        document,
        ProblemSourceBundle::new(&context, &material, &model, BTreeMap::new(), opaque),
    )
    .expect("single-authority problem admits")
}

fn execution_header(seed: u64) -> ArtifactHeader {
    ArtifactHeader::try_new(
        artifact("execution-1"),
        vec![unit("Pa")],
        SeedDeclaration::Fixed(seed),
        DeclaredBudget::Limit(1.0e-9),
        DeclaredBudget::Limit(30_000),
        DeclaredBudget::Limit(32 << 20),
        vec![("fixture".to_string(), "1".to_string())],
        vec!["identifiability.execute".to_string()],
    )
    .expect("execution header")
}

fn execution_header_with_semantics(
    units: &[&str],
    seed: u64,
    accuracy: f64,
    time_ms: u64,
    memory_bytes: u64,
    version: &str,
    extra_capability: bool,
) -> ArtifactHeader {
    let mut capabilities = vec!["identifiability.execute".to_string()];
    if extra_capability {
        capabilities.push("identifiability.symbolic".to_string());
    }
    ArtifactHeader::try_new(
        artifact("execution-1"),
        units.iter().map(|value| unit(value)).collect(),
        SeedDeclaration::Fixed(seed),
        DeclaredBudget::Limit(accuracy),
        DeclaredBudget::Limit(time_ms),
        DeclaredBudget::Limit(memory_bytes),
        vec![("fixture".to_string(), version.to_string())],
        capabilities,
    )
    .expect("execution semantic header")
}

fn assessment_header_with_semantics(
    units: &[&str],
    seed: u64,
    accuracy: f64,
    time_ms: u64,
    memory_bytes: u64,
    version: &str,
    extra_capability: bool,
) -> ArtifactHeader {
    let mut capabilities = vec!["identifiability.assess".to_string()];
    if extra_capability {
        capabilities.push("identifiability.explain".to_string());
    }
    ArtifactHeader::try_new(
        artifact("assessment-1"),
        units.iter().map(|value| unit(value)).collect(),
        SeedDeclaration::Fixed(seed),
        DeclaredBudget::Limit(accuracy),
        DeclaredBudget::Limit(time_ms),
        DeclaredBudget::Limit(memory_bytes),
        vec![("fixture".to_string(), version.to_string())],
        capabilities,
    )
    .expect("assessment semantic header")
}

fn coordinate(name: &str, affine: bool) -> ParameterCoordinate {
    let domain = if name == "yield_stress" {
        ParameterDomain::try_new(1.0e6, 1.0e9).expect("domain")
    } else {
        ParameterDomain::try_new(1.0e7, 5.0e9).expect("domain")
    };
    let (quantity, coordinate_domain, transform, suffix) = if affine {
        let (lo, hi) = domain.bounds();
        (
            QuantitySpec::dimensional(DIMENSIONLESS),
            ParameterDomain::try_new(lo / 1.0e6, hi / 1.0e6).expect("scaled domain"),
            CoordinateTransform::Affine {
                scale: 1.0e6,
                scale_quantity: QuantitySpec::dimensional(STRESS),
                offset: 0.0,
            },
            "mpa",
        )
    } else {
        (
            QuantitySpec::dimensional(STRESS),
            domain,
            CoordinateTransform::Identity,
            "si",
        )
    };
    ParameterCoordinate::try_new(
        CoordinateId::try_new(format!("{name}-{suffix}")).expect("coordinate id"),
        quantity,
        coordinate_domain,
        transform,
    )
    .expect("coordinate fixture")
}

fn resolved_sources(sources: &[&SourceRef], external_trust: bool) -> SourceResolutionSet {
    SourceResolutionSet::try_new(
        sources
            .iter()
            .map(|source| {
                let authority = if external_trust {
                    AuthorityDisposition::ExternalTrustReceipt {
                        trust_receipt: hash(&format!("execution-trust-{}", source.key())),
                    }
                } else {
                    AuthorityDisposition::ContentVerified
                };
                SourceResolution::verify(source, source.key().as_str().as_bytes(), authority)
                    .expect("content-verified source fixture")
            })
            .collect(),
    )
    .expect("closed content-verified source set")
}

fn execution(
    problem: &AdmittedIdentifiabilityProblem,
    affine: bool,
    seed: u64,
    tolerance: f64,
    wrong_action: bool,
) -> Result<IdentifiabilityExecutionPlan, IdentifiabilityError> {
    execution_with_axes(
        problem,
        affine,
        seed,
        tolerance,
        wrong_action,
        BTreeSet::from([
            RequestedClaimAxis::Structural,
            RequestedClaimAxis::Local,
            RequestedClaimAxis::Generic,
            RequestedClaimAxis::Global,
            RequestedClaimAxis::Practical,
        ]),
    )
}

fn execution_with_axes(
    problem: &AdmittedIdentifiabilityProblem,
    affine: bool,
    seed: u64,
    tolerance: f64,
    wrong_action: bool,
    requested_axes: BTreeSet<RequestedClaimAxis>,
) -> Result<IdentifiabilityExecutionPlan, IdentifiabilityError> {
    execution_with_axes_and_authority(
        problem,
        affine,
        seed,
        tolerance,
        wrong_action,
        requested_axes,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn execution_with_axes_and_authority(
    problem: &AdmittedIdentifiabilityProblem,
    affine: bool,
    seed: u64,
    tolerance: f64,
    wrong_action: bool,
    requested_axes: BTreeSet<RequestedClaimAxis>,
    external_trust: bool,
) -> Result<IdentifiabilityExecutionPlan, IdentifiabilityError> {
    let analyzer = source("analyzer", SourceKind::Analyzer, hash("analyzer"));
    let build = source("build", SourceKind::Build, hash("build"));
    let derivatives = source(
        "derivatives",
        SourceKind::DerivativeProvider,
        hash("derivatives"),
    );
    let quadrature = source("quadrature", SourceKind::Analyzer, hash("quadrature"));
    let initialization = source(
        "initialization",
        SourceKind::Assumption,
        hash("initialization"),
    );
    let stopping = source("stopping", SourceKind::Assumption, hash("stopping"));
    let determinism = source("determinism", SourceKind::Assumption, hash("determinism"));
    let mut authority_sources = vec![
        &analyzer,
        &build,
        &derivatives,
        &initialization,
        &stopping,
        &determinism,
    ];
    if !wrong_action {
        authority_sources.push(&quadrature);
    }
    let source_authority = resolved_sources(&authority_sources, external_trust);
    IdentifiabilityExecutionPlan::try_new(
        execution_header(seed),
        problem,
        analyzer,
        build,
        Some(derivatives),
        requested_axes,
        vec![
            (
                role("yield_stress"),
                ParameterExecutionAction::Optimize {
                    coordinate: coordinate("yield_stress", affine),
                },
            ),
            (
                role("hardening_modulus"),
                if wrong_action {
                    ParameterExecutionAction::Optimize {
                        coordinate: coordinate("hardening_modulus", affine),
                    }
                } else {
                    ParameterExecutionAction::Marginalize {
                        coordinate: coordinate("hardening_modulus", affine),
                        integrator: quadrature,
                    }
                },
            ),
        ],
        IdentifiabilityNumericalPolicy::try_new(
            tolerance,
            0.0,
            1.0e12,
            ArithmeticPolicy::CertifiedInterval,
        )?,
        initialization,
        stopping,
        determinism,
        source_authority,
    )
}

#[derive(Clone)]
struct ExecutionParts {
    header: ArtifactHeader,
    analyzer: SourceRef,
    build: SourceRef,
    derivative_provider: Option<SourceRef>,
    requested_axes: BTreeSet<RequestedClaimAxis>,
    actions: Vec<(ParameterRoleId, ParameterExecutionAction)>,
    numerical: IdentifiabilityNumericalPolicy,
    initialization: SourceRef,
    stopping: SourceRef,
    determinism: SourceRef,
}

impl ExecutionParts {
    fn from_plan(plan: &IdentifiabilityExecutionPlan) -> Self {
        Self {
            header: plan.header().clone(),
            analyzer: plan.analyzer().clone(),
            build: plan.build().clone(),
            derivative_provider: plan.derivative_provider().cloned(),
            requested_axes: plan.requested_axes().clone(),
            actions: plan
                .actions()
                .iter()
                .map(|(role, action)| (role.clone(), action.clone()))
                .collect(),
            numerical: plan.numerical_policy().clone(),
            initialization: plan.initialization().clone(),
            stopping: plan.stopping().clone(),
            determinism: plan.determinism_contract().clone(),
        }
    }

    fn build(
        self,
        problem: &AdmittedIdentifiabilityProblem,
        external_trust: bool,
    ) -> IdentifiabilityExecutionPlan {
        let source_authority = {
            let mut sources = vec![
                &self.analyzer,
                &self.build,
                &self.initialization,
                &self.stopping,
                &self.determinism,
            ];
            if let Some(provider) = &self.derivative_provider {
                sources.push(provider);
            }
            for (_, action) in &self.actions {
                if let ParameterExecutionAction::Marginalize { integrator, .. } = action {
                    sources.push(integrator);
                }
            }
            resolved_sources(&sources, external_trust)
        };
        IdentifiabilityExecutionPlan::try_new(
            self.header,
            problem,
            self.analyzer,
            self.build,
            self.derivative_provider,
            self.requested_axes,
            self.actions,
            self.numerical,
            self.initialization,
            self.stopping,
            self.determinism,
            source_authority,
        )
        .expect("rebuilt execution plan")
    }
}

#[derive(Clone)]
struct AssessmentParts {
    header: ArtifactHeader,
    claims: Vec<TypedIdentifiabilityClaim>,
    evidence: Vec<(ClaimId, ClaimAssessment)>,
    source_authority: SourceResolutionSet,
}

impl AssessmentParts {
    fn from_assessment(assessment: &IdentifiabilityAssessment) -> Self {
        Self {
            header: assessment.header().clone(),
            claims: assessment.claims().values().cloned().collect(),
            evidence: assessment
                .evidence()
                .iter()
                .map(|(id, value)| (id.clone(), value.clone()))
                .collect(),
            source_authority: assessment.source_authority().clone(),
        }
    }

    fn build(
        self,
        problem: &AdmittedIdentifiabilityProblem,
        execution: &IdentifiabilityExecutionPlan,
    ) -> IdentifiabilityAssessment {
        IdentifiabilityAssessment::try_new(
            self.header,
            problem,
            execution,
            self.claims,
            self.evidence,
            self.source_authority,
        )
        .expect("rebuilt assessment")
    }
}

fn assessment(
    problem: &AdmittedIdentifiabilityProblem,
    execution: &IdentifiabilityExecutionPlan,
    receipt_label: &str,
) -> IdentifiabilityAssessment {
    assessment_result(problem, execution, receipt_label).expect("assessment fixture")
}

fn assessment_result(
    problem: &AdmittedIdentifiabilityProblem,
    execution: &IdentifiabilityExecutionPlan,
    receipt_label: &str,
) -> Result<IdentifiabilityAssessment, IdentifiabilityError> {
    assessment_result_with_domain_authority(
        problem,
        execution,
        receipt_label,
        AuthorityDisposition::ContentVerified,
    )
}

fn assessment_result_with_domain_authority(
    problem: &AdmittedIdentifiabilityProblem,
    execution: &IdentifiabilityExecutionPlan,
    receipt_label: &str,
    domain_authority: AuthorityDisposition,
) -> Result<IdentifiabilityAssessment, IdentifiabilityError> {
    let claim_id = ClaimId::try_new("yield-structural-global").expect("claim id");
    let domain = problem
        .document()
        .sources()
        .get(&source_key("claim-domain"))
        .cloned()
        .unwrap_or_else(|| source("claim-domain", SourceKind::Assumption, hash("claim-domain")));
    let method = execution.analyzer().clone();
    let receipt = source(
        "claim-receipt",
        SourceKind::EvidenceReceipt,
        hash(receipt_label),
    );
    let source_authority = SourceResolutionSet::try_new(
        [
            SourceResolution::verify(&domain, b"claim-domain", domain_authority)
                .expect("claim-domain source resolution"),
            SourceResolution::verify(&method, b"analyzer", AuthorityDisposition::ContentVerified)
                .expect("execution-analyzer source resolution"),
            SourceResolution::verify(
                &receipt,
                receipt_label.as_bytes(),
                AuthorityDisposition::ContentVerified,
            )
            .expect("claim-receipt source resolution"),
        ]
        .into(),
    )
    .expect("assessment source authority");
    IdentifiabilityAssessment::try_new(
        header("assessment-1", "identifiability.assess"),
        problem,
        execution,
        vec![TypedIdentifiabilityClaim::new(
            claim_id.clone(),
            InformationRegime::StructuralExactModel,
            IdentifiabilityExtent::Global,
            ClaimQuantifier::ForAll { domain },
            ScalarDomain::Real,
            ClaimSubject::Parameter(role("yield_stress")),
            ClaimScope::WholeCampaign,
        )],
        vec![(
            claim_id,
            ClaimAssessment::ClaimedEstablished {
                method,
                receipt,
                tolerance: 1.0e-8,
            },
        )],
        source_authority,
    )
}

fn two_claim_assessment(
    problem: &AdmittedIdentifiabilityProblem,
    execution: &IdentifiabilityExecutionPlan,
) -> IdentifiabilityAssessment {
    let method = execution.analyzer().clone();
    let domain_left = source(
        "claim-domain-left",
        SourceKind::Assumption,
        hash("claim-domain-left"),
    );
    let domain_right = source(
        "claim-domain-right",
        SourceKind::Assumption,
        hash("claim-domain-right"),
    );
    let receipt_left = source(
        "claim-receipt-left",
        SourceKind::EvidenceReceipt,
        hash("claim-receipt-left"),
    );
    let receipt_right = source(
        "claim-receipt-right",
        SourceKind::EvidenceReceipt,
        hash("claim-receipt-right"),
    );
    let left_id = ClaimId::try_new("claim-left").expect("left claim id");
    let right_id = ClaimId::try_new("claim-right").expect("right claim id");
    let claims = vec![
        TypedIdentifiabilityClaim::new(
            left_id.clone(),
            InformationRegime::StructuralExactModel,
            IdentifiabilityExtent::Global,
            ClaimQuantifier::ForAll {
                domain: domain_left.clone(),
            },
            ScalarDomain::Real,
            ClaimSubject::Parameter(role("yield_stress")),
            ClaimScope::WholeCampaign,
        ),
        TypedIdentifiabilityClaim::new(
            right_id.clone(),
            InformationRegime::StructuralExactModel,
            IdentifiabilityExtent::Global,
            ClaimQuantifier::ForAll {
                domain: domain_right.clone(),
            },
            ScalarDomain::Real,
            ClaimSubject::Parameter(role("hardening_modulus")),
            ClaimScope::WholeCampaign,
        ),
    ];
    let evidence = vec![
        (
            left_id,
            ClaimAssessment::ClaimedEstablished {
                method: method.clone(),
                receipt: receipt_left.clone(),
                tolerance: 1.0e-8,
            },
        ),
        (
            right_id,
            ClaimAssessment::ClaimedEstablished {
                method: method.clone(),
                receipt: receipt_right.clone(),
                tolerance: 1.0e-8,
            },
        ),
    ];
    let source_authority = resolved_sources(
        &[
            &domain_left,
            &domain_right,
            &method,
            &receipt_left,
            &receipt_right,
        ],
        false,
    );
    IdentifiabilityAssessment::try_new(
        header("assessment-2", "identifiability.assess"),
        problem,
        execution,
        claims,
        evidence,
        source_authority,
    )
    .expect("two-claim assessment")
}

#[test]
fn structural_global_claim_requires_both_preregistered_axes() {
    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    for axes in [
        BTreeSet::from([RequestedClaimAxis::Structural]),
        BTreeSet::from([RequestedClaimAxis::Global]),
    ] {
        let plan = execution_with_axes(&problem, false, 17, 1.0e-10, false, axes)
            .expect("partial-axis execution");
        assert!(matches!(
            assessment_result(&problem, &plan, "partial-axis-receipt"),
            Err(IdentifiabilityError::InvalidText {
                field: "unrequested claim axis",
                ..
            })
        ));
    }
    let plan = execution_with_axes(
        &problem,
        false,
        17,
        1.0e-10,
        false,
        BTreeSet::from([RequestedClaimAxis::Structural, RequestedClaimAxis::Global]),
    )
    .expect("fully preregistered execution");
    assessment_result(&problem, &plan, "both-axes-receipt").expect("both claim axes admit");
    log(
        "claim-axis-product",
        "pass",
        "structural/global obligations are conjunctive rather than precedence-selected",
    );
}

#[test]
fn problem_roundtrip_is_canonical_but_remains_unresolved() {
    let document = problem_fixture(ProblemOptions::default())
        .document
        .expect("valid problem");
    let bytes = document.canonical_bytes().expect("problem encodes");
    let decoded = IdentifiabilityProblemDocument::from_canonical_bytes(&bytes)
        .expect("unresolved problem decodes");
    assert_eq!(decoded, document);
    assert_eq!(decoded.cases().len(), 2);
    log(
        "problem-roundtrip-unresolved",
        "pass",
        "decode returned only an unresolved multi-case document",
    );
}

#[test]
fn case_and_registry_input_order_are_nonsemantic() {
    let left = problem_fixture(ProblemOptions::default())
        .document
        .expect("left problem");
    let right = problem_fixture(ProblemOptions {
        reverse_cases: true,
        ..ProblemOptions::default()
    })
    .document
    .expect("right problem");
    assert_eq!(
        left.canonical_bytes().expect("left bytes"),
        right.canonical_bytes().expect("right bytes")
    );
    log(
        "case-order",
        "pass",
        "canonical maps erase caller insertion order without erasing cases",
    );
}

#[test]
fn source_resolution_input_order_is_nonsemantic() {
    let forward = admit_fixture_with_authority_and_order(
        problem_fixture(ProblemOptions::default()),
        false,
        false,
    );
    let reverse = admit_fixture_with_authority_and_order(
        problem_fixture(ProblemOptions::default()),
        false,
        true,
    );
    assert_eq!(forward.id(), reverse.id());
    assert_eq!(forward.source_admission_id(), reverse.source_admission_id());
    assert_eq!(
        forward
            .source_admission_canonical_bytes()
            .expect("forward authority bytes"),
        reverse
            .source_admission_canonical_bytes()
            .expect("reverse authority bytes"),
    );
    log(
        "source-resolution-order",
        "pass",
        "source authority canonicalization erases caller insertion order",
    );
}

#[test]
fn multi_case_campaign_qualifies_local_channel_names() {
    let problem = problem_fixture(ProblemOptions::default())
        .document
        .expect("problem");
    assert!(
        problem
            .cases()
            .contains_key(&CaseId::try_new("a").expect("case"))
    );
    assert!(
        problem
            .cases()
            .contains_key(&CaseId::try_new("b").expect("case"))
    );
    assert_ne!(
        ObservationKey::new(case_id("a"), channel("stress")),
        ObservationKey::new(case_id("b"), channel("stress"))
    );
    log(
        "composite-observation-key",
        "pass",
        "case qualification prevents cross-protocol channel aliasing",
    );
}

#[test]
fn dangling_composite_observation_endpoint_refuses() {
    let result = problem_fixture(ProblemOptions {
        bad_observation_endpoint: true,
        ..ProblemOptions::default()
    })
    .document;
    assert!(matches!(
        result,
        Err(IdentifiabilityError::UnknownReference {
            field: "composite observation key",
            ..
        })
    ));
    log(
        "dangling-observation",
        "pass",
        "unknown case/channel refused",
    );
}

#[test]
fn disconnected_free_parameter_refuses_without_false_theorem() {
    let result = problem_fixture(ProblemOptions {
        missing_hardening_influence: true,
        ..ProblemOptions::default()
    })
    .document;
    assert!(matches!(
        result,
        Err(IdentifiabilityError::DisconnectedEstimatedParameter { .. })
    ));
    log(
        "disconnected-parameter",
        "pass",
        "schema connectivity refused without claiming nonzero sensitivity",
    );
}

#[test]
fn dangling_source_key_refuses_before_authority_admission() {
    let result = problem_fixture(ProblemOptions {
        dangling_operator: true,
        ..ProblemOptions::default()
    })
    .document;
    assert!(matches!(
        result,
        Err(IdentifiabilityError::UnknownReference {
            field: "observation operator",
            ..
        })
    ));
    log("dangling-source", "pass", "source registry closure refused");
}

#[test]
fn derived_parameter_cycles_refuse() {
    let result = problem_fixture(ProblemOptions {
        derived_cycle: true,
        ..ProblemOptions::default()
    })
    .document;
    assert!(matches!(
        result,
        Err(IdentifiabilityError::InvalidNumeric {
            field: "derived parameter graph",
            ..
        })
    ));
    log(
        "derived-cycle",
        "pass",
        "derived parameter DAG is fail-closed",
    );
}

#[test]
fn joint_constraint_units_are_checked_term_by_term() {
    let result = problem_fixture(ProblemOptions {
        bad_constraint_units: true,
        ..ProblemOptions::default()
    })
    .document;
    assert!(matches!(
        result,
        Err(IdentifiabilityError::InvalidNumeric {
            field: "affine constraint units",
            ..
        })
    ));
    log(
        "constraint-units",
        "pass",
        "coefficient times parameter must equal residual dimensions",
    );
}

#[test]
fn overlapping_v1_gauges_refuse_instead_of_composing_by_order() {
    let result = problem_fixture(ProblemOptions {
        overlapping_gauges: true,
        ..ProblemOptions::default()
    })
    .document;
    assert!(matches!(
        result,
        Err(IdentifiabilityError::InvalidGauge { .. })
    ));
    log(
        "overlapping-gauges",
        "pass",
        "v1 refuses undeclared groupoid composition",
    );
}

#[test]
fn self_correlation_is_not_an_identifiability_route() {
    let result = problem_fixture(ProblemOptions {
        self_correlation: true,
        ..ProblemOptions::default()
    })
    .document;
    assert!(matches!(
        result,
        Err(IdentifiabilityError::InvalidNumeric {
            field: "correlation functional",
            ..
        })
    ));
    log(
        "self-correlation",
        "pass",
        "constant self-correlation cannot masquerade as sensitivity",
    );
}

#[test]
fn derivative_units_are_derived_from_functional_and_parameter() {
    let problem = problem_fixture(ProblemOptions::default())
        .document
        .expect("problem");
    let quantity = problem
        .influence_derivative_quantity(&InfluenceId::try_new("yield-to-stress").expect("influence"))
        .expect("derived quantity");
    assert_eq!(quantity, QuantitySpec::dimensional(DIMENSIONLESS));
    log(
        "derived-influence-units",
        "pass",
        "caller cannot inject contradictory derivative dimensions",
    );
}

#[test]
fn dense_correlation_refuses_marginals_without_finite_standard_deviation() {
    let result = problem_fixture(ProblemOptions {
        dense_with_bounded_marginal: true,
        ..ProblemOptions::default()
    })
    .document;
    assert!(matches!(
        result,
        Err(IdentifiabilityError::Covariance { .. })
    ));
    log(
        "dense-correlation-marginals",
        "pass",
        "bounded noise was not silently converted into Gaussian scale",
    );
}

#[test]
fn accidental_raw_experiment_reuse_refuses_under_disjoint_policy() {
    let result = problem_fixture(ProblemOptions {
        retrospective_reuse: true,
        ..ProblemOptions::default()
    })
    .document;
    assert!(matches!(
        result,
        Err(IdentifiabilityError::InvalidText {
            field: "data reuse policy",
            ..
        })
    ));
    log(
        "accidental-data-reuse",
        "pass",
        "same experiment cannot be double-counted under Disjoint",
    );
}

#[test]
fn declared_raw_reuse_requires_joint_likelihood_and_justification() {
    let result = problem_fixture(ProblemOptions {
        retrospective_reuse: true,
        declared_sharing: true,
        ..ProblemOptions::default()
    })
    .document;
    assert!(result.is_ok());
    log(
        "declared-data-reuse",
        "pass",
        "shared campaign admitted only through explicit group and likelihood",
    );
}

#[test]
fn wrong_concrete_source_hash_refuses_problem_identity() {
    for mutation in 1..=3 {
        let fixture = problem_fixture(ProblemOptions {
            context_contract_mutation: mutation,
            ..ProblemOptions::default()
        });
        let ProblemFixture {
            context,
            material,
            model,
            document,
            ..
        } = fixture;
        let document = document.expect("structural document");
        let opaque = opaque_resolutions(&document);
        let result = AdmittedIdentifiabilityProblem::resolve_and_admit(
            document,
            ProblemSourceBundle::new(&context, &material, &model, BTreeMap::new(), opaque),
        );
        assert!(matches!(
            result,
            Err(IdentifiabilityError::SourceMismatch { .. })
        ));
    }
    log(
        "wrong-concrete-source",
        "pass",
        "typed context hash, digest domain, and contract version are resolver-derived",
    );
}

#[test]
fn opaque_resolution_cannot_replay_across_hash_domains() {
    let good = problem_fixture(ProblemOptions::default());
    let good_document = good.document.expect("good document");
    let resolutions = opaque_resolutions(&good_document);
    let bad = problem_fixture(ProblemOptions {
        alternate_graph_domain: true,
        ..ProblemOptions::default()
    });
    let bad_document = bad.document.expect("alternate-domain document");
    let result = AdmittedIdentifiabilityProblem::resolve_and_admit(
        bad_document,
        ProblemSourceBundle::new(
            &bad.context,
            &bad.material,
            &bad.model,
            BTreeMap::new(),
            resolutions,
        ),
    );
    assert!(matches!(
        result,
        Err(IdentifiabilityError::SourceMismatch {
            field: "opaque source resolution"
        })
    ));
    log(
        "cross-domain-resolution-replay",
        "pass",
        "a digest verified under one domain cannot authorize an equal digest under another",
    );
}

#[test]
fn unverified_opaque_source_cannot_mint_problem_id() {
    let fixture = problem_fixture(ProblemOptions::default());
    let ProblemFixture {
        context,
        material,
        model,
        graph: _,
        document,
    } = fixture;
    let document = document.expect("document");
    let entries = document
        .sources()
        .values()
        .filter(|source| {
            !matches!(
                source.kind(),
                SourceKind::ContextOfUse
                    | SourceKind::MaterialCard
                    | SourceKind::ConstitutiveModelCard
                    | SourceKind::ExperimentArtifact
                    | SourceKind::CalibrationSplit
            )
        })
        .map(|source| {
            if source.key().as_str() == "forward-a" {
                SourceResolution::unresolved(source, "resolver has not fetched this artifact")
                    .expect("unresolved diagnostic")
            } else {
                let preimage = if source.key().as_str() == "graph" {
                    b"constitutive-graph".as_slice()
                } else {
                    source.key().as_str().as_bytes()
                };
                SourceResolution::verify(source, preimage, AuthorityDisposition::ContentVerified)
                    .expect("resolution")
            }
        })
        .collect();
    let opaque = SourceResolutionSet::try_new(entries).expect("resolution set");
    let result = AdmittedIdentifiabilityProblem::resolve_and_admit(
        document,
        ProblemSourceBundle::new(&context, &material, &model, BTreeMap::new(), opaque),
    );
    assert!(matches!(
        result,
        Err(IdentifiabilityError::InvalidText {
            field: "source authority",
            ..
        })
    ));
    log(
        "unverified-source",
        "pass",
        "content reference alone did not grant source authority",
    );
}

#[test]
fn problem_and_source_admission_identities_separate_question_from_trust_envelope() {
    let content_only =
        admit_fixture_with_authority(problem_fixture(ProblemOptions::default()), false);
    let authenticated =
        admit_fixture_with_authority(problem_fixture(ProblemOptions::default()), true);
    assert_eq!(content_only.id(), authenticated.id());
    assert_ne!(
        content_only.source_admission_id(),
        authenticated.source_admission_id()
    );
    log(
        "problem-vs-authority-identity",
        "pass",
        "trust receipt moves authority envelope without rewriting physical question",
    );
}

#[test]
fn identifiability_problem_identity_bindings_have_exact_mutation_evidence() {
    let baseline_document = problem_fixture(ProblemOptions::default())
        .document
        .expect("baseline problem");
    let baseline = unresolved_problem_identity(&baseline_document);
    let variants = [
        (
            "context_source",
            rekey_problem_root(
                problem_fixture(ProblemOptions::default()),
                ProblemRoot::Context,
                "context-rekeyed",
            )
            .document
            .expect("context-root mutation"),
        ),
        (
            "material_source",
            rekey_problem_root(
                problem_fixture(ProblemOptions::default()),
                ProblemRoot::Material,
                "material-rekeyed",
            )
            .document
            .expect("material-root mutation"),
        ),
        (
            "model_source",
            rekey_problem_root(
                problem_fixture(ProblemOptions::default()),
                ProblemRoot::Model,
                "model-rekeyed",
            )
            .document
            .expect("model-root mutation"),
        ),
        (
            "graph_source",
            rekey_problem_root(
                problem_fixture(ProblemOptions::default()),
                ProblemRoot::Graph,
                "graph-rekeyed",
            )
            .document
            .expect("graph-root mutation"),
        ),
        (
            "sources",
            problem_fixture(ProblemOptions {
                alternate_graph_domain: true,
                ..ProblemOptions::default()
            })
            .document
            .expect("source-registry mutation"),
        ),
        (
            "parameters",
            problem_fixture(ProblemOptions {
                parameter_prior_version: 2,
                ..ProblemOptions::default()
            })
            .document
            .expect("parameter-registry mutation"),
        ),
        (
            "constraints",
            problem_fixture(ProblemOptions {
                valid_constraint: true,
                ..ProblemOptions::default()
            })
            .document
            .expect("constraint-registry mutation"),
        ),
        (
            "cases",
            problem_fixture(ProblemOptions {
                second_case_complementary: true,
                ..ProblemOptions::default()
            })
            .document
            .expect("case-registry mutation"),
        ),
        (
            "influences",
            problem_fixture(ProblemOptions {
                yield_log_scale: true,
                ..ProblemOptions::default()
            })
            .document
            .expect("influence-registry mutation"),
        ),
        (
            "gauges",
            problem_fixture(ProblemOptions {
                one_gauge: true,
                ..ProblemOptions::default()
            })
            .document
            .expect("gauge-registry mutation"),
        ),
        (
            "joint_noise",
            problem_fixture(ProblemOptions {
                external_noise: true,
                ..ProblemOptions::default()
            })
            .document
            .expect("joint-noise mutation"),
        ),
    ];
    for (field, variant) in variants {
        assert_ne!(
            baseline,
            unresolved_problem_identity(&variant),
            "problem semantic field {field} did not move identity",
        );
    }

    let shared_left = problem_fixture(ProblemOptions {
        retrospective_reuse: true,
        declared_sharing: true,
        ..ProblemOptions::default()
    })
    .document
    .expect("shared-data baseline");
    let shared_right = problem_fixture(ProblemOptions {
        retrospective_reuse: true,
        declared_sharing: true,
        alternate_sharing_justification: true,
        ..ProblemOptions::default()
    })
    .document
    .expect("shared-data mutation");
    assert_ne!(
        unresolved_problem_identity(&shared_left),
        unresolved_problem_identity(&shared_right),
        "data_reuse semantic field did not move identity",
    );
    assert_eq!(baseline_document.schema_version(), 1);
    log(
        "problem-identity-semantic-fields",
        "pass",
        "every direct problem field has independent or validity-coupled mutation evidence",
    );
}

#[test]
fn identifiability_source_admission_identity_bindings_have_exact_mutation_evidence() {
    let content_only =
        admit_fixture_with_authority(problem_fixture(ProblemOptions::default()), false);
    let external_trust = admit_fixture_with_single_external_authority(
        problem_fixture(ProblemOptions::default()),
        "forward-a",
    );
    assert_eq!(content_only.id(), external_trust.id());
    assert_ne!(
        content_only.source_admission_id(),
        external_trust.source_admission_id(),
    );
    assert_ne!(
        content_only
            .source_admission_canonical_bytes()
            .expect("content-only source-admission bytes"),
        external_trust
            .source_admission_canonical_bytes()
            .expect("external-trust source-admission bytes"),
    );
    let authority_deltas = content_only
        .source_resolutions()
        .iter()
        .filter_map(|(key, resolution)| {
            (external_trust.source_resolutions().get(key) != Some(resolution)).then_some(key)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        authority_deltas
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec!["forward-a"],
        "one exact authority disposition must move the source-admission identity",
    );
    let different_problem = admit_fixture(problem_fixture(ProblemOptions {
        second_case_complementary: true,
        ..ProblemOptions::default()
    }));
    assert_ne!(content_only.id(), different_problem.id());
    assert_eq!(
        content_only.source_resolutions(),
        different_problem.source_resolutions(),
        "problem-only mutation must retain the exact resolution registry",
    );
    assert_ne!(
        content_only.source_admission_id(),
        different_problem.source_admission_id(),
        "problem_id must move SourceAdmissionId independently of authority disposition",
    );
    assert_eq!(
        content_only.source_admission_id().digest().as_bytes().len(),
        32
    );
    log(
        "source-admission-identity-semantic-fields",
        "pass",
        "problem id and exact resolution authority independently move SourceAdmissionId",
    );
}

#[test]
fn source_admission_id_is_stable_across_execution_variants() {
    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    let source_admission_id = problem.source_admission_id();
    let si = execution(&problem, false, 17, 1.0e-10, false).expect("SI execution");
    let affine = execution(&problem, true, 18, 1.0e-8, false).expect("affine execution");
    assert_ne!(si.id().expect("SI id"), affine.id().expect("affine id"));
    assert_eq!(problem.source_admission_id(), source_admission_id);
    log(
        "source-admission-execution-noninterference",
        "pass",
        "execution coordinates, seeds, and tolerances cannot rewrite source admission",
    );
}

#[test]
fn coordinates_do_not_move_problem_identity() {
    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    let identity = problem.id();
    let si = execution(&problem, false, 17, 1.0e-10, false).expect("SI plan");
    let affine = execution(&problem, true, 17, 1.0e-10, false).expect("affine plan");
    assert_eq!(problem.id(), identity);
    assert_ne!(si.id().expect("SI id"), affine.id().expect("affine id"));
    log(
        "coordinate-noninterference",
        "pass",
        "coordinates move execution only, never ProblemId",
    );
}

#[test]
fn seed_and_tolerance_move_execution_identity() {
    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    let baseline = execution(&problem, false, 17, 1.0e-10, false).expect("baseline");
    let seed = execution(&problem, false, 18, 1.0e-10, false).expect("seed variant");
    let tolerance = execution(&problem, false, 17, 1.0e-8, false).expect("tol variant");
    assert_ne!(baseline.id().expect("id"), seed.id().expect("id"));
    assert_ne!(baseline.id().expect("id"), tolerance.id().expect("id"));
    log(
        "execution-semantic-fields",
        "pass",
        "Five Explicits and numerical policy are execution semantics",
    );
}

#[test]
fn execution_source_authority_moves_execution_identity() {
    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    let axes = BTreeSet::from([
        RequestedClaimAxis::Structural,
        RequestedClaimAxis::Local,
        RequestedClaimAxis::Generic,
        RequestedClaimAxis::Global,
        RequestedClaimAxis::Practical,
    ]);
    let content_only =
        execution_with_axes_and_authority(&problem, false, 17, 1.0e-10, false, axes.clone(), false)
            .expect("content-verified execution");
    let external_trust =
        execution_with_axes_and_authority(&problem, false, 17, 1.0e-10, false, axes, true)
            .expect("externally trusted execution");
    assert_ne!(
        content_only.id().expect("content-only id"),
        external_trust.id().expect("external-trust id"),
    );
    log(
        "execution-source-authority-identity",
        "pass",
        "execution authority is transitive identity state, not an unverified annotation",
    );
}

#[test]
fn identifiability_execution_identity_bindings_have_exact_mutation_evidence() {
    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    let baseline = execution(&problem, false, 17, 1.0e-10, false).expect("baseline");
    let baseline_id = baseline.id().expect("baseline id");
    let baseline_parts = ExecutionParts::from_plan(&baseline);
    let assert_moves = |field: &str, parts: ExecutionParts, external_trust: bool| {
        let variant = parts.build(&problem, external_trust);
        assert_ne!(
            baseline_id,
            variant.id().expect("variant execution id"),
            "execution semantic field {field} did not move identity",
        );
    };

    for (field, header) in [
        (
            "header.units",
            execution_header_with_semantics(&["K", "Pa"], 17, 1.0e-9, 30_000, 32 << 20, "1", false),
        ),
        (
            "header.seed",
            execution_header_with_semantics(&["Pa"], 18, 1.0e-9, 30_000, 32 << 20, "1", false),
        ),
        (
            "header.accuracy",
            execution_header_with_semantics(&["Pa"], 17, 1.0e-8, 30_000, 32 << 20, "1", false),
        ),
        (
            "header.time_ms",
            execution_header_with_semantics(&["Pa"], 17, 1.0e-9, 30_001, 32 << 20, "1", false),
        ),
        (
            "header.memory_bytes",
            execution_header_with_semantics(
                &["Pa"],
                17,
                1.0e-9,
                30_000,
                (32 << 20) + 1,
                "1",
                false,
            ),
        ),
        (
            "header.versions",
            execution_header_with_semantics(&["Pa"], 17, 1.0e-9, 30_000, 32 << 20, "2", false),
        ),
        (
            "header.capabilities",
            execution_header_with_semantics(&["Pa"], 17, 1.0e-9, 30_000, 32 << 20, "1", true),
        ),
    ] {
        let mut parts = baseline_parts.clone();
        parts.header = header;
        assert_moves(field, parts, false);
    }

    let mut parts = baseline_parts.clone();
    parts.analyzer = source("analyzer-v2", SourceKind::Analyzer, hash("analyzer-v2"));
    assert_moves("analyzer", parts, false);
    let mut parts = baseline_parts.clone();
    parts.build = source("build-v2", SourceKind::Build, hash("build-v2"));
    assert_moves("build", parts, false);
    let mut parts = baseline_parts.clone();
    parts.derivative_provider = None;
    assert_moves("derivative_provider", parts, false);
    let mut parts = baseline_parts.clone();
    parts.requested_axes.remove(&RequestedClaimAxis::Generic);
    assert_moves("requested_axes", parts, false);
    let mut parts = baseline_parts.clone();
    for (role, action) in &mut parts.actions {
        if role.as_str() == "yield_stress" {
            *action = ParameterExecutionAction::Optimize {
                coordinate: coordinate("yield_stress", true),
            };
        }
    }
    assert_moves("actions", parts, false);
    let mut parts = baseline_parts.clone();
    parts.numerical = IdentifiabilityNumericalPolicy::try_new(
        1.0e-8,
        0.0,
        1.0e12,
        ArithmeticPolicy::CertifiedInterval,
    )
    .expect("numerical mutation");
    assert_moves("numerical", parts, false);
    let mut parts = baseline_parts.clone();
    parts.initialization = source(
        "initialization-v2",
        SourceKind::Assumption,
        hash("initialization-v2"),
    );
    assert_moves("initialization", parts, false);
    let mut parts = baseline_parts.clone();
    parts.stopping = source("stopping-v2", SourceKind::Assumption, hash("stopping-v2"));
    assert_moves("stopping", parts, false);
    let mut parts = baseline_parts.clone();
    parts.determinism = source(
        "determinism-v2",
        SourceKind::Assumption,
        hash("determinism-v2"),
    );
    assert_moves("determinism_contract", parts, false);
    assert_moves("source_authority", baseline_parts.clone(), true);

    let physical_variant = admit_fixture(problem_fixture(ProblemOptions {
        second_case_complementary: true,
        ..ProblemOptions::default()
    }));
    let physical_execution = baseline_parts.clone().build(&physical_variant, false);
    assert_ne!(baseline.problem_id(), physical_execution.problem_id());
    assert_ne!(
        baseline_id,
        physical_execution.id().expect("physical variant id")
    );

    let authority_variant =
        admit_fixture_with_authority(problem_fixture(ProblemOptions::default()), true);
    let authority_execution = baseline_parts.build(&authority_variant, false);
    assert_eq!(baseline.problem_id(), authority_execution.problem_id());
    assert_ne!(
        baseline.source_admission_id(),
        authority_execution.source_admission_id(),
    );
    assert_ne!(
        baseline_id,
        authority_execution.id().expect("authority variant id")
    );
    assert_eq!(baseline.schema_version(), 1);
    log(
        "execution-identity-semantic-fields",
        "pass",
        "every execution field and header projection has direct mutation evidence",
    );
}

#[test]
fn execution_action_input_order_is_nonsemantic() {
    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    let baseline = execution(&problem, false, 17, 1.0e-10, false).expect("baseline");
    let mut reversed_actions = baseline
        .actions()
        .iter()
        .map(|(role, action)| (role.clone(), action.clone()))
        .collect::<Vec<_>>();
    reversed_actions.reverse();
    let rebuilt = IdentifiabilityExecutionPlan::try_new(
        baseline.header().clone(),
        &problem,
        baseline.analyzer().clone(),
        baseline.build().clone(),
        baseline.derivative_provider().cloned(),
        baseline.requested_axes().clone(),
        reversed_actions,
        baseline.numerical_policy().clone(),
        baseline.initialization().clone(),
        baseline.stopping().clone(),
        baseline.determinism_contract().clone(),
        baseline.source_authority().clone(),
    )
    .expect("reordered execution");
    assert_eq!(
        baseline.id().expect("baseline id"),
        rebuilt.id().expect("rebuilt id")
    );
    assert_eq!(
        baseline.canonical_bytes().expect("baseline transport"),
        rebuilt.canonical_bytes().expect("rebuilt transport"),
    );
    log(
        "execution-action-order",
        "pass",
        "parameter actions are keyed semantics rather than caller sequence semantics",
    );
}

#[test]
fn execution_action_must_match_physical_treatment() {
    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    let result = execution(&problem, false, 17, 1.0e-10, true);
    assert!(matches!(
        result,
        Err(IdentifiabilityError::InvalidText {
            field: "execution parameter treatment",
            ..
        })
    ));
    log(
        "treatment-action-coverage",
        "pass",
        "marginalized parameter cannot silently become optimized",
    );
}

#[test]
fn execution_roundtrip_requires_the_exact_admitted_problem() {
    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    let plan = execution(&problem, false, 17, 1.0e-10, false).expect("execution");
    let bytes = plan.canonical_bytes().expect("execution bytes");
    assert!(matches!(
        IdentifiabilityExecutionPlan::from_canonical_bytes(
            &bytes,
            &problem,
            &SourceResolutionSet::default(),
        ),
        Err(IdentifiabilityError::SourceMismatch {
            field: "execution source-resolution replay",
        })
    ));
    let decoded = IdentifiabilityExecutionPlan::from_canonical_bytes(
        &bytes,
        &problem,
        plan.source_authority(),
    )
    .expect("execution roundtrip");
    assert_eq!(decoded, plan);
    log(
        "execution-roundtrip",
        "pass",
        "transport revalidates ProblemId and SourceAdmissionId",
    );
}

#[test]
fn artifact_labels_do_not_move_execution_or_assessment_identity() {
    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    let plan = execution(&problem, false, 17, 1.0e-10, false).expect("execution");
    let execution_id = plan.id().expect("execution id");
    let mut execution_bytes = plan.canonical_bytes().expect("execution transport");
    let execution_label = b"execution-1";
    let execution_label_at = execution_bytes
        .windows(execution_label.len())
        .position(|window| window == execution_label)
        .expect("execution artifact label in exact transport");
    execution_bytes[execution_label_at..execution_label_at + execution_label.len()]
        .copy_from_slice(b"execution-2");
    let relabeled_execution = IdentifiabilityExecutionPlan::from_canonical_bytes(
        &execution_bytes,
        &problem,
        plan.source_authority(),
    )
    .expect("relabeled execution transport");
    assert_eq!(
        relabeled_execution.id().expect("relabeled id"),
        execution_id
    );
    assert_ne!(
        relabeled_execution
            .canonical_bytes()
            .expect("exact transport"),
        plan.canonical_bytes().expect("baseline exact transport"),
    );

    let assessment = assessment(&problem, &plan, "receipt");
    let assessment_id = assessment.id().expect("assessment id");
    let mut assessment_bytes = assessment.canonical_bytes().expect("assessment transport");
    let assessment_label = b"assessment-1";
    let assessment_label_at = assessment_bytes
        .windows(assessment_label.len())
        .position(|window| window == assessment_label)
        .expect("assessment artifact label in exact transport");
    assessment_bytes[assessment_label_at..assessment_label_at + assessment_label.len()]
        .copy_from_slice(b"assessment-2");
    let relabeled_assessment = IdentifiabilityAssessment::from_canonical_bytes(
        &assessment_bytes,
        &problem,
        &plan,
        assessment.source_authority(),
    )
    .expect("relabeled assessment transport");
    assert_eq!(
        relabeled_assessment.id().expect("relabeled id"),
        assessment_id,
    );
    log(
        "artifact-label-nonsemantic",
        "pass",
        "exact transport retains ledger labels while scientific identities exclude them",
    );
}

#[test]
fn evidence_changes_assessment_not_problem_or_execution() {
    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    let plan = execution(&problem, false, 17, 1.0e-10, false).expect("execution");
    let problem_id = problem.id();
    let execution_id = plan.id().expect("execution id");
    let left = assessment(&problem, &plan, "receipt-left");
    let right = assessment(&problem, &plan, "receipt-right");
    assert_ne!(left.id().expect("left id"), right.id().expect("right id"));
    assert_eq!(problem.id(), problem_id);
    assert_eq!(plan.id().expect("execution id"), execution_id);
    log(
        "assessment-noninterference",
        "pass",
        "evidence cannot rewrite problem or execution preimages",
    );
}

#[test]
fn identifiability_assessment_identity_bindings_have_exact_mutation_evidence() {
    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    let plan = execution(&problem, false, 17, 1.0e-10, false).expect("execution");
    let baseline = assessment(&problem, &plan, "receipt-left");
    let baseline_id = baseline.id().expect("baseline assessment id");
    let baseline_parts = AssessmentParts::from_assessment(&baseline);
    let assert_moves = |field: &str, parts: AssessmentParts| {
        let variant = parts.build(&problem, &plan);
        assert_eq!(baseline.problem_id(), variant.problem_id());
        assert_eq!(baseline.execution_id(), variant.execution_id());
        assert_ne!(
            baseline_id,
            variant.id().expect("variant assessment id"),
            "assessment semantic field {field} did not move identity",
        );
    };

    const ASSESSMENT_SEED: u64 = 0x1d3_171f;
    for (field, header) in [
        (
            "header.units",
            assessment_header_with_semantics(
                &["K", "Pa"],
                ASSESSMENT_SEED,
                1.0e-9,
                30_000,
                32 << 20,
                "1",
                false,
            ),
        ),
        (
            "header.seed",
            assessment_header_with_semantics(
                &["Pa"],
                ASSESSMENT_SEED + 1,
                1.0e-9,
                30_000,
                32 << 20,
                "1",
                false,
            ),
        ),
        (
            "header.accuracy",
            assessment_header_with_semantics(
                &["Pa"],
                ASSESSMENT_SEED,
                1.0e-8,
                30_000,
                32 << 20,
                "1",
                false,
            ),
        ),
        (
            "header.time_ms",
            assessment_header_with_semantics(
                &["Pa"],
                ASSESSMENT_SEED,
                1.0e-9,
                30_001,
                32 << 20,
                "1",
                false,
            ),
        ),
        (
            "header.memory_bytes",
            assessment_header_with_semantics(
                &["Pa"],
                ASSESSMENT_SEED,
                1.0e-9,
                30_000,
                (32 << 20) + 1,
                "1",
                false,
            ),
        ),
        (
            "header.versions",
            assessment_header_with_semantics(
                &["Pa"],
                ASSESSMENT_SEED,
                1.0e-9,
                30_000,
                32 << 20,
                "2",
                false,
            ),
        ),
        (
            "header.capabilities",
            assessment_header_with_semantics(
                &["Pa"],
                ASSESSMENT_SEED,
                1.0e-9,
                30_000,
                32 << 20,
                "1",
                true,
            ),
        ),
    ] {
        let mut parts = baseline_parts.clone();
        parts.header = header;
        assert_eq!(parts.claims, baseline_parts.claims);
        assert_eq!(parts.evidence, baseline_parts.evidence);
        assert_eq!(parts.source_authority, baseline_parts.source_authority);
        assert_moves(field, parts);
    }

    let baseline_claim = baseline.claims().values().next().expect("baseline claim");
    let claim_variant = TypedIdentifiabilityClaim::new(
        baseline_claim.id().clone(),
        baseline_claim.information(),
        baseline_claim.extent(),
        baseline_claim.quantifier().clone(),
        ScalarDomain::Complex,
        baseline_claim.subject().clone(),
        baseline_claim.scope().clone(),
    );
    let mut parts = baseline_parts.clone();
    *parts
        .claims
        .iter_mut()
        .find(|claim| claim.id() == baseline_claim.id())
        .expect("claim mutation target") = claim_variant;
    assert_eq!(parts.evidence, baseline_parts.evidence);
    assert_eq!(parts.source_authority, baseline_parts.source_authority);
    assert_eq!(parts.header, baseline_parts.header);
    assert_moves("claims", parts);

    let mut parts = baseline_parts.clone();
    let (_, conclusion) = parts
        .evidence
        .iter_mut()
        .find(|(id, _)| id == baseline_claim.id())
        .expect("evidence mutation target");
    let (method, receipt) = match conclusion.clone() {
        ClaimAssessment::ClaimedEstablished {
            method, receipt, ..
        } => (method, receipt),
        _ => panic!("baseline evidence unexpectedly changed variant"),
    };
    *conclusion = ClaimAssessment::ClaimedEstablished {
        method,
        receipt,
        tolerance: 2.0e-8,
    };
    assert_eq!(parts.claims, baseline_parts.claims);
    assert_eq!(parts.source_authority, baseline_parts.source_authority);
    assert_eq!(parts.header, baseline_parts.header);
    assert_moves("evidence", parts);

    let receipt = match baseline
        .evidence()
        .values()
        .next()
        .expect("baseline evidence")
    {
        ClaimAssessment::ClaimedEstablished { receipt, .. } => receipt.clone(),
        _ => panic!("baseline evidence unexpectedly changed variant"),
    };
    let mut authority_entries = baseline
        .source_authority()
        .entries()
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let replacement = SourceResolution::verify(
        &receipt,
        b"receipt-left",
        AuthorityDisposition::ExternalTrustReceipt {
            trust_receipt: hash("assessment-receipt-external-trust"),
        },
    )
    .expect("assessment-exclusive trusted receipt resolution");
    let mut replacement_count = 0;
    for resolution in &mut authority_entries {
        if resolution.key() == receipt.key() {
            *resolution = replacement.clone();
            replacement_count += 1;
        }
    }
    assert_eq!(
        replacement_count, 1,
        "one assessment-exclusive authority moved"
    );
    let mut parts = baseline_parts.clone();
    parts.source_authority =
        SourceResolutionSet::try_new(authority_entries).expect("authority mutation");
    assert_eq!(parts.claims, baseline_parts.claims);
    assert_eq!(parts.evidence, baseline_parts.evidence);
    assert_eq!(parts.header, baseline_parts.header);
    assert_moves("source_authority", parts);

    let execution_variant =
        execution(&problem, false, 18, 1.0e-10, false).expect("execution-id variant");
    let execution_assessment = assessment(&problem, &execution_variant, "receipt-left");
    assert_eq!(baseline.problem_id(), execution_assessment.problem_id());
    assert_ne!(baseline.execution_id(), execution_assessment.execution_id());
    assert_eq!(baseline.header(), execution_assessment.header());
    assert_eq!(baseline.claims(), execution_assessment.claims());
    assert_eq!(baseline.evidence(), execution_assessment.evidence());
    assert_eq!(
        baseline.source_authority(),
        execution_assessment.source_authority(),
    );
    assert_ne!(
        baseline_id,
        execution_assessment
            .id()
            .expect("execution variant assessment id"),
    );

    let problem_variant = admit_fixture(problem_fixture(ProblemOptions {
        second_case_complementary: true,
        ..ProblemOptions::default()
    }));
    let variant_plan =
        execution(&problem_variant, false, 17, 1.0e-10, false).expect("problem variant plan");
    let problem_assessment = assessment(&problem_variant, &variant_plan, "receipt-left");
    assert_ne!(baseline.problem_id(), problem_assessment.problem_id());
    assert_ne!(baseline.execution_id(), problem_assessment.execution_id());
    assert_eq!(baseline.header(), problem_assessment.header());
    assert_eq!(baseline.claims(), problem_assessment.claims());
    assert_eq!(baseline.evidence(), problem_assessment.evidence());
    assert_eq!(
        baseline.source_authority(),
        problem_assessment.source_authority(),
    );
    assert_ne!(
        baseline_id,
        problem_assessment
            .id()
            .expect("problem variant assessment id"),
    );

    let authority_problem = admit_fixture_with_single_external_authority(
        problem_fixture(ProblemOptions::default()),
        "forward-a",
    );
    assert_eq!(problem.id(), authority_problem.id());
    assert_ne!(
        problem.source_admission_id(),
        authority_problem.source_admission_id(),
    );
    let authority_plan =
        execution(&authority_problem, false, 17, 1.0e-10, false).expect("authority plan");
    let authority_assessment = assessment(&authority_problem, &authority_plan, "receipt-left");
    assert_ne!(baseline.execution_id(), authority_assessment.execution_id());
    assert_eq!(baseline.header(), authority_assessment.header());
    assert_eq!(baseline.claims(), authority_assessment.claims());
    assert_eq!(baseline.evidence(), authority_assessment.evidence());
    assert_eq!(
        baseline.source_authority(),
        authority_assessment.source_authority(),
    );
    assert_ne!(
        baseline_id,
        authority_assessment
            .id()
            .expect("authority-envelope assessment id"),
    );

    const ASSESSMENT_MAGIC: &[u8] = b"fs-material-identifiability-assessment\0";
    let mut stale = baseline.canonical_bytes().expect("assessment transport");
    stale[ASSESSMENT_MAGIC.len()..ASSESSMENT_MAGIC.len() + 4].copy_from_slice(&2_u32.to_le_bytes());
    assert!(matches!(
        IdentifiabilityAssessment::from_canonical_bytes(
            &stale,
            &problem,
            &plan,
            baseline.source_authority(),
        ),
        Err(IdentifiabilityError::UnsupportedSchemaVersion { .. })
    ));
    log(
        "assessment-identity-semantic-fields",
        "pass",
        "every direct field and each validity-coupled parent projection has mutation evidence",
    );
}

#[test]
fn assessment_authority_must_agree_with_problem_on_transitive_overlap() {
    let problem = admit_fixture(problem_fixture(ProblemOptions {
        claim_domain_in_problem: true,
        ..ProblemOptions::default()
    }));
    let execution = execution(&problem, false, 17, 1.0e-10, false).expect("execution");
    assessment_result(&problem, &execution, "matching-domain-authority")
        .expect("matching problem/assessment authority admits");
    let result = assessment_result_with_domain_authority(
        &problem,
        &execution,
        "conflicting-domain-authority",
        AuthorityDisposition::ExternalTrustReceipt {
            trust_receipt: hash("conflicting-domain-trust"),
        },
    );
    assert!(matches!(
        result,
        Err(IdentifiabilityError::SourceMismatch {
            field: "assessment/problem source authority",
        })
    ));
    log(
        "assessment-problem-authority-overlap",
        "pass",
        "assessment cannot relabel authority for a source already admitted by the problem",
    );
}

#[test]
fn assessment_authority_must_agree_with_execution_on_transitive_overlap() {
    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    let execution = execution_with_axes_and_authority(
        &problem,
        false,
        17,
        1.0e-10,
        false,
        BTreeSet::from([
            RequestedClaimAxis::Structural,
            RequestedClaimAxis::Local,
            RequestedClaimAxis::Generic,
            RequestedClaimAxis::Global,
            RequestedClaimAxis::Practical,
        ]),
        true,
    )
    .expect("externally trusted execution");
    let result = assessment_result(&problem, &execution, "execution-overlap-receipt");
    assert!(matches!(
        result,
        Err(IdentifiabilityError::SourceMismatch {
            field: "assessment/execution source authority",
        })
    ));
    log(
        "assessment-execution-authority-overlap",
        "pass",
        "assessment cannot relabel authority for its execution analyzer source",
    );
}

#[test]
fn assessment_input_order_is_nonsemantic() {
    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    let execution = execution(&problem, false, 17, 1.0e-10, false).expect("execution");
    let baseline = two_claim_assessment(&problem, &execution);
    let mut claims = baseline.claims().values().cloned().collect::<Vec<_>>();
    let mut evidence = baseline
        .evidence()
        .iter()
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect::<Vec<_>>();
    let mut resolutions = baseline
        .source_authority()
        .entries()
        .values()
        .cloned()
        .collect::<Vec<_>>();
    assert!(claims.len() >= 2, "claim order test needs multiple claims");
    assert!(
        evidence.len() >= 2,
        "evidence order test needs multiple entries"
    );
    assert!(
        resolutions.len() >= 2,
        "authority order test needs multiple resolutions"
    );
    let baseline_claims = claims.clone();
    let baseline_evidence = evidence.clone();
    let baseline_resolutions = resolutions.clone();
    let assert_same = |field: &str,
                       claims: Vec<TypedIdentifiabilityClaim>,
                       evidence: Vec<(ClaimId, ClaimAssessment)>,
                       resolutions: Vec<SourceResolution>| {
        let rebuilt = IdentifiabilityAssessment::try_new(
            baseline.header().clone(),
            &problem,
            &execution,
            claims,
            evidence,
            SourceResolutionSet::try_new(resolutions).expect("reordered assessment authority"),
        )
        .unwrap_or_else(|error| panic!("reordered {field} refused: {error}"));
        assert_eq!(
            baseline.id().expect("baseline assessment id"),
            rebuilt.id().expect("rebuilt assessment id"),
            "caller order for {field} moved assessment identity",
        );
        assert_eq!(
            baseline.canonical_bytes().expect("baseline transport"),
            rebuilt.canonical_bytes().expect("rebuilt transport"),
            "caller order for {field} moved canonical transport",
        );
    };

    claims.reverse();
    assert_ne!(
        claims, baseline_claims,
        "claim reversal must be non-vacuous"
    );
    assert_same(
        "claims",
        claims,
        baseline_evidence.clone(),
        baseline_resolutions.clone(),
    );
    evidence.reverse();
    assert_ne!(
        evidence, baseline_evidence,
        "evidence reversal must be non-vacuous"
    );
    assert_same(
        "evidence",
        baseline_claims.clone(),
        evidence,
        baseline_resolutions.clone(),
    );
    resolutions.reverse();
    assert_ne!(
        resolutions, baseline_resolutions,
        "authority reversal must be non-vacuous"
    );
    assert_same(
        "source authority",
        baseline_claims,
        baseline_evidence,
        resolutions,
    );
    log(
        "assessment-input-order",
        "pass",
        "claims, evidence, and authority are canonical keyed collections",
    );
}

#[test]
fn assessment_roundtrip_preserves_product_typed_claim() {
    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    let plan = execution(&problem, false, 17, 1.0e-10, false).expect("execution");
    let assessment = assessment(&problem, &plan, "receipt");
    let bytes = assessment.canonical_bytes().expect("assessment bytes");
    let decoded = IdentifiabilityAssessment::from_canonical_bytes(
        &bytes,
        &problem,
        &plan,
        assessment.source_authority(),
    )
    .expect("assessment roundtrip");
    assert_eq!(decoded, assessment);
    log(
        "assessment-roundtrip",
        "pass",
        "regime, extent, quantifier, scalar domain, subject, and scope retained",
    );
}

#[test]
fn identity_domains_and_wire_magics_are_stage_separated() {
    const PROBLEM_MAGIC: &[u8] = b"fs-material-identifiability-problem\0";
    const SOURCE_ADMISSION_MAGIC: &[u8] = b"fs-material-identifiability-source-admission\0";
    const EXECUTION_MAGIC: &[u8] = b"fs-material-identifiability-execution\0";
    const ASSESSMENT_MAGIC: &[u8] = b"fs-material-identifiability-assessment\0";

    let domains = [
        IDENTIFIABILITY_PROBLEM_IDENTITY_DOMAIN,
        IDENTIFIABILITY_SOURCE_ADMISSION_IDENTITY_DOMAIN,
        IDENTIFIABILITY_EXECUTION_IDENTITY_DOMAIN,
        IDENTIFIABILITY_ASSESSMENT_IDENTITY_DOMAIN,
    ];
    assert_eq!(domains.into_iter().collect::<BTreeSet<_>>().len(), 4);
    assert_eq!(
        domains
            .into_iter()
            .map(|domain| hash_domain(domain, b"same-stage-preimage"))
            .collect::<BTreeSet<_>>()
            .len(),
        4,
    );

    let document = problem_fixture(ProblemOptions::default())
        .document
        .expect("problem");
    assert!(
        document
            .canonical_bytes()
            .expect("problem transport")
            .starts_with(PROBLEM_MAGIC)
    );
    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    assert!(
        problem
            .source_admission_canonical_bytes()
            .expect("source admission transport")
            .starts_with(SOURCE_ADMISSION_MAGIC)
    );
    let execution = execution(&problem, false, 17, 1.0e-10, false).expect("execution");
    assert!(
        execution
            .canonical_bytes()
            .expect("execution transport")
            .starts_with(EXECUTION_MAGIC)
    );
    let assessment = assessment(&problem, &execution, "domain-receipt");
    assert!(
        assessment
            .canonical_bytes()
            .expect("assessment transport")
            .starts_with(ASSESSMENT_MAGIC)
    );
    log(
        "identity-domain-and-magic-separation",
        "pass",
        "all four authority stages have distinct domains and exact wire magics",
    );
}

#[test]
fn identifiability_identity_preimages_have_exact_wire_layout() {
    const PROBLEM_MAGIC: &[u8] = b"fs-material-identifiability-problem\0";
    const SOURCE_ADMISSION_MAGIC: &[u8] = b"fs-material-identifiability-source-admission\0";
    const EXECUTION_MAGIC: &[u8] = b"fs-material-identifiability-execution\0";
    const ASSESSMENT_MAGIC: &[u8] = b"fs-material-identifiability-assessment\0";

    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    let problem_bytes = problem
        .document()
        .canonical_bytes()
        .expect("problem identity preimage");
    assert!(problem_bytes.starts_with(PROBLEM_MAGIC));
    let mut problem_at = PROBLEM_MAGIC.len();
    assert_eq!(
        read_u32_le(&problem_bytes, &mut problem_at, "problem version"),
        IDENTIFIABILITY_PROBLEM_IDENTITY_VERSION,
    );
    for expected in [
        problem.document().context_source(),
        problem.document().material_source(),
        problem.document().model_source(),
        problem.document().graph_source(),
    ] {
        assert_eq!(
            read_text(&problem_bytes, &mut problem_at, "problem root source"),
            expected.as_str(),
            "problem root-binding order moved",
        );
    }
    assert_eq!(
        read_u32_le(&problem_bytes, &mut problem_at, "source registry count"),
        u32::try_from(problem.document().sources().len()).expect("bounded source count"),
    );
    let known_parameter_bound = 1.0e6_f64.to_bits().to_le_bytes();
    assert!(
        problem_bytes
            .windows(known_parameter_bound.len())
            .any(|window| window == known_parameter_bound),
        "problem numeric fields must use canonical f64 little-endian encoding",
    );
    assert_eq!(
        hash_domain(IDENTIFIABILITY_PROBLEM_IDENTITY_DOMAIN, &problem_bytes),
        problem.id().digest(),
    );

    let source_admission_bytes = problem
        .source_admission_canonical_bytes()
        .expect("source-admission identity preimage");
    assert!(source_admission_bytes.starts_with(SOURCE_ADMISSION_MAGIC));
    let mut admission_at = SOURCE_ADMISSION_MAGIC.len();
    assert_eq!(
        read_u32_le(
            &source_admission_bytes,
            &mut admission_at,
            "source-admission version",
        ),
        IDENTIFIABILITY_SOURCE_ADMISSION_IDENTITY_VERSION,
    );
    let problem_id_end = admission_at + 32;
    assert_eq!(
        source_admission_bytes.get(admission_at..problem_id_end),
        Some(problem.id().digest().as_bytes().as_slice()),
        "source admission must place ProblemId before the resolution registry",
    );
    admission_at = problem_id_end;
    assert_eq!(
        read_u32_le(
            &source_admission_bytes,
            &mut admission_at,
            "source-admission resolution count",
        ),
        u32::try_from(problem.source_resolutions().len()).expect("bounded resolution count"),
    );
    assert_eq!(
        hash_domain(
            IDENTIFIABILITY_SOURCE_ADMISSION_IDENTITY_DOMAIN,
            &source_admission_bytes,
        ),
        problem.source_admission_id().digest(),
    );

    let execution = execution(&problem, false, 17, 1.0e-10, false).expect("execution");
    let exact_execution = execution
        .canonical_bytes()
        .expect("exact execution transport");
    let execution_preimage =
        project_exact_header_to_identity(&exact_execution, EXECUTION_MAGIC, execution.header());
    let execution_body_at =
        assert_identity_header_layout(&execution_preimage, EXECUTION_MAGIC, execution.header());
    assert_eq!(
        execution_preimage.get(execution_body_at..execution_body_at + 32),
        Some(execution.problem_id().digest().as_bytes().as_slice()),
        "execution identity must place ProblemId immediately after the projected header",
    );
    assert_eq!(
        execution_preimage.get(execution_body_at + 32..execution_body_at + 64),
        Some(
            execution
                .source_admission_id()
                .digest()
                .as_bytes()
                .as_slice()
        ),
        "execution identity must place SourceAdmissionId after ProblemId",
    );
    assert_eq!(
        hash_domain(
            IDENTIFIABILITY_EXECUTION_IDENTITY_DOMAIN,
            &execution_preimage,
        ),
        execution.id().expect("execution id").digest(),
    );
    let mut malformed_execution = exact_execution;
    malformed_execution[EXECUTION_MAGIC.len() + 4] = 0;
    assert!(matches!(
        IdentifiabilityExecutionPlan::from_canonical_bytes(
            &malformed_execution,
            &problem,
            execution.source_authority(),
        ),
        Err(IdentifiabilityError::Canonical { .. })
    ));

    let assessment = assessment(&problem, &execution, "layout-receipt");
    let exact_assessment = assessment
        .canonical_bytes()
        .expect("exact assessment transport");
    let assessment_preimage =
        project_exact_header_to_identity(&exact_assessment, ASSESSMENT_MAGIC, assessment.header());
    let assessment_body_at =
        assert_identity_header_layout(&assessment_preimage, ASSESSMENT_MAGIC, assessment.header());
    assert_eq!(
        assessment_preimage.get(assessment_body_at..assessment_body_at + 32),
        Some(assessment.problem_id().digest().as_bytes().as_slice()),
        "assessment identity must place ProblemId immediately after the projected header",
    );
    assert_eq!(
        assessment_preimage.get(assessment_body_at + 32..assessment_body_at + 64),
        Some(assessment.execution_id().digest().as_bytes().as_slice()),
        "assessment identity must place ExecutionId after ProblemId",
    );
    let mut claims_at = assessment_body_at + 64;
    assert_eq!(
        read_u32_le(&assessment_preimage, &mut claims_at, "claim count"),
        u32::try_from(assessment.claims().len()).expect("bounded claim count"),
    );
    assert_eq!(
        hash_domain(
            IDENTIFIABILITY_ASSESSMENT_IDENTITY_DOMAIN,
            &assessment_preimage,
        ),
        assessment.id().expect("assessment id").digest(),
    );
    let mut malformed_assessment = exact_assessment;
    malformed_assessment[ASSESSMENT_MAGIC.len() + 4] = 0;
    assert!(matches!(
        IdentifiabilityAssessment::from_canonical_bytes(
            &malformed_assessment,
            &problem,
            &execution,
            assessment.source_authority(),
        ),
        Err(IdentifiabilityError::Canonical { .. })
    ));
    log(
        "identity-wire-layout",
        "pass",
        "domains, version/count framing, numeric endianness, parent order, and header projection are exact",
    );
}

#[test]
fn trailing_bytes_and_stale_versions_refuse() {
    const MAGIC: &[u8] = b"fs-material-identifiability-problem\0";
    let problem = problem_fixture(ProblemOptions::default())
        .document
        .expect("problem");
    let mut trailing = problem.canonical_bytes().expect("bytes");
    trailing.push(0);
    assert!(IdentifiabilityProblemDocument::from_canonical_bytes(&trailing).is_err());
    let mut stale = problem.canonical_bytes().expect("bytes");
    stale[MAGIC.len()..MAGIC.len() + 4].copy_from_slice(&2_u32.to_le_bytes());
    assert!(matches!(
        IdentifiabilityProblemDocument::from_canonical_bytes(&stale),
        Err(IdentifiabilityError::UnsupportedSchemaVersion { .. })
    ));
    log(
        "canonical-adversaries",
        "pass",
        "trailing bytes and stale/future schema versions fail closed",
    );
}

#[test]
fn identifiability_identity_versions_and_transports_fail_closed() {
    const PROBLEM_MAGIC: &[u8] = b"fs-material-identifiability-problem\0";
    const EXECUTION_MAGIC: &[u8] = b"fs-material-identifiability-execution\0";
    const ASSESSMENT_MAGIC: &[u8] = b"fs-material-identifiability-assessment\0";

    let unresolved = problem_fixture(ProblemOptions::default())
        .document
        .expect("problem document");
    let mut problem_bytes = unresolved.canonical_bytes().expect("problem bytes");
    let mut bad_problem_magic = problem_bytes.clone();
    bad_problem_magic[0] ^= 0x01;
    assert!(IdentifiabilityProblemDocument::from_canonical_bytes(&bad_problem_magic).is_err());
    problem_bytes[PROBLEM_MAGIC.len()..PROBLEM_MAGIC.len() + 4]
        .copy_from_slice(&2_u32.to_le_bytes());
    assert!(matches!(
        IdentifiabilityProblemDocument::from_canonical_bytes(&problem_bytes),
        Err(IdentifiabilityError::UnsupportedSchemaVersion { .. })
    ));

    let problem = admit_fixture(problem_fixture(ProblemOptions::default()));
    let execution = execution(&problem, false, 17, 1.0e-10, false).expect("execution");
    let mut execution_bytes = execution.canonical_bytes().expect("execution bytes");
    let mut bad_execution_magic = execution_bytes.clone();
    bad_execution_magic[0] ^= 0x01;
    assert!(
        IdentifiabilityExecutionPlan::from_canonical_bytes(
            &bad_execution_magic,
            &problem,
            execution.source_authority(),
        )
        .is_err()
    );
    execution_bytes[EXECUTION_MAGIC.len()..EXECUTION_MAGIC.len() + 4]
        .copy_from_slice(&2_u32.to_le_bytes());
    assert!(matches!(
        IdentifiabilityExecutionPlan::from_canonical_bytes(
            &execution_bytes,
            &problem,
            execution.source_authority(),
        ),
        Err(IdentifiabilityError::UnsupportedSchemaVersion { .. })
    ));

    let assessment = assessment(&problem, &execution, "version-guard-receipt");
    let mut assessment_bytes = assessment.canonical_bytes().expect("assessment bytes");
    let mut bad_assessment_magic = assessment_bytes.clone();
    bad_assessment_magic[0] ^= 0x01;
    assert!(
        IdentifiabilityAssessment::from_canonical_bytes(
            &bad_assessment_magic,
            &problem,
            &execution,
            assessment.source_authority(),
        )
        .is_err()
    );
    assessment_bytes[ASSESSMENT_MAGIC.len()..ASSESSMENT_MAGIC.len() + 4]
        .copy_from_slice(&2_u32.to_le_bytes());
    assert!(matches!(
        IdentifiabilityAssessment::from_canonical_bytes(
            &assessment_bytes,
            &problem,
            &execution,
            assessment.source_authority(),
        ),
        Err(IdentifiabilityError::UnsupportedSchemaVersion { .. })
    ));
    assert!(check_source_admission_identity_version(2).is_err());
    log(
        "identity-stage-version-transports",
        "pass",
        "problem, source-admission, execution, and assessment versions fail closed independently",
    );
}

#[test]
fn source_ref_semantics_version_and_hash_are_mandatory() {
    assert!(
        SourceRef::try_new(
            source_key("zero"),
            SourceKind::Assumption,
            ContentHash([0; 32]),
            "fixture",
            1,
        )
        .is_err()
    );
    assert!(
        SourceRef::try_new(
            source_key("version-zero"),
            SourceKind::Assumption,
            hash("nonzero"),
            "fixture",
            0,
        )
        .is_err()
    );
    log(
        "source-ref-bounds",
        "pass",
        "zero identity and unpublished source semantics refused",
    );
}

#[test]
fn gauge_kinds_retain_ambitious_continuous_discrete_and_stratified_space() {
    let members = BTreeSet::from([role("yield_stress"), role("hardening_modulus")]);
    for (index, kind) in [
        GaugeKind::Continuous { dimension: 1 },
        GaugeKind::Discrete { group_order: 2 },
        GaugeKind::Mixed {
            continuous_dimension: 1,
            discrete_order: 2,
        },
        GaugeKind::Stratified {
            strata: source_key("strata"),
        },
    ]
    .into_iter()
    .enumerate()
    {
        assert!(
            GaugeDeclaration::try_new(
                GaugeClassId::try_new(format!("kind-{index}")).expect("gauge id"),
                members.clone(),
                source_key("action"),
                kind,
                GaugeHandling::Unresolved {
                    reason: "the theorem search has not resolved this gauge".to_string(),
                },
            )
            .is_ok()
        );
    }
    log(
        "gauge-kind-space",
        "pass",
        "schema preserves theorem-ready continuous/discrete/mixed/stratified gauges",
    );
}

#[test]
fn identity_version_guard_is_exact() {
    for checker in [
        check_authority_schema_version as fn(u32) -> Result<(), IdentifiabilityError>,
        check_problem_identity_version,
        check_source_admission_identity_version,
        check_execution_identity_version,
        check_assessment_identity_version,
    ] {
        assert!(checker(1).is_ok());
        assert!(checker(0).is_err());
        assert!(checker(2).is_err());
    }
    log(
        "authority-version-guard",
        "pass",
        "umbrella and all four identity-stage versions fail closed independently",
    );
}
