//! I10.1 G0/G3 tests for retrospective identifiability admission.
//!
//! These tests deliberately cross the real `fs-evidence` artifact boundary:
//! every admitted case carries a canonical `ExperimentArtifact` and matching
//! `CalibrationSplit`.  Fixed JSON logs make the eventual batch-verification
//! evidence useful without treating structural admission as a theorem about
//! scientific identifiability or laboratory authenticity.

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
const TEST_HASH_DOMAIN: &str = "org.frankensim.fs-material.identifiability-retrospective-test.v1";

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

fn log(case: &str, verdict: &str, expected: &str, detail: &str) {
    println!(
        "{{\"suite\":\"fs-material/identifiability-retrospective\",\
         \"case\":\"{}\",\"verdict\":\"{}\",\"expected\":\"{}\",\
         \"detail\":\"{}\"}}",
        escape_json(case),
        escape_json(verdict),
        escape_json(expected),
        escape_json(detail),
    );
}

fn hash(label: &str) -> ContentHash {
    hash_domain(TEST_HASH_DOMAIN, label.as_bytes())
}

fn artifact(value: &str) -> ArtifactId {
    ArtifactId::try_new(value).expect("fixture artifact id")
}

fn qoi(value: &str) -> QoiId {
    QoiId::try_new(value).expect("fixture QoI id")
}

fn unit(value: &str) -> UnitId {
    UnitId::try_new(value).expect("fixture unit id")
}

fn axis(value: &str) -> AxisId {
    AxisId::try_new(value).expect("fixture axis id")
}

fn observation(value: &str) -> ObservationId {
    ObservationId::try_new(value).expect("fixture observation id")
}

fn case_id(value: &str) -> CaseId {
    CaseId::try_new(value).expect("fixture case id")
}

fn channel(value: &str) -> ObservationChannelId {
    ObservationChannelId::try_new(value).expect("fixture observation channel")
}

fn role(value: &str) -> ParameterRoleId {
    ParameterRoleId::try_new(value).expect("fixture parameter role")
}

fn source_key(value: &str) -> SourceKey {
    SourceKey::try_new(value).expect("fixture source key")
}

fn header(id: &str, units: &[&str], capability: &str) -> ArtifactHeader {
    ArtifactHeader::try_new(
        artifact(id),
        units.iter().copied().map(unit).collect(),
        SeedDeclaration::Fixed(0x171f_10_1),
        DeclaredBudget::Limit(1.0e-9),
        DeclaredBudget::Limit(30_000),
        DeclaredBudget::Limit(32 << 20),
        vec![(
            "fixture".to_string(),
            "identifiability-retrospective-v1".to_string(),
        )],
        vec![capability.to_string()],
    )
    .expect("Five Explicits fixture")
}

fn context() -> ContextOfUse {
    ContextOfUse::try_new(
        header("retrospective-context", &["Pa", "K"], "fixture.context"),
        "Calibrate a constitutive parameter without crossing preregistered evidence partitions.",
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
                    .expect("temperature applicability axis"),
            ],
            Vec::new(),
        )
        .expect("applicability domain"),
        ApplicabilityPolicy::Demote,
    )
    .expect("context fixture")
}

fn model_cards() -> (MaterialCard, ConstitutiveModelCard) {
    let model = ConstitutiveModelCard {
        law: LawId("retrospective-identifiability-fixture".to_string()),
        law_version: 1,
        parameters: BTreeMap::from([(
            "yield_stress".to_string(),
            LawParameter {
                value: 276.0e6,
                dims: STRESS,
            },
        )]),
        state_schema_version: 2,
        initial_state: InitialStatePolicy::ZeroInternalState,
        validity: ValidityDomain::unconstrained().with("temperature", 250.0, 450.0),
        sources: vec![hash("model-source")],
        provenance: Provenance {
            source: "retrospective admission fixture".to_string(),
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

fn physical_data(
    label: &str,
    experiment_qois: &[&str],
    rows: &[(&str, &str)],
    calibration: &[&str],
    validation: &[&str],
    blind: &[&str],
    source_bytes_label: &str,
) -> (ExperimentArtifact, CalibrationSplit) {
    let source_by_row = rows
        .iter()
        .map(|(id, source)| ((*id).to_string(), hash(source)))
        .collect::<BTreeMap<_, _>>();
    let manifest = ObservationManifest::try_new(
        rows.iter()
            .map(|(id, source)| (observation(id), hash(source)))
            .collect(),
    )
    .expect("injective experiment manifest");
    let experiment = ExperimentArtifact::try_new(
        header(
            &format!("experiment-{label}"),
            &["Pa"],
            "fixture.experiment",
        ),
        artifact(&format!("dataset-{label}")),
        ExperimentOrigin::Physical {
            apparatus_id: artifact(&format!("apparatus-{label}")),
            facility_id: artifact("facility-retrospective-fixture"),
        },
        experiment_qois.iter().copied().map(qoi).collect(),
        manifest,
        vec![InstrumentCalibration::new(
            artifact(&format!("instrument-{label}")),
            hash(&format!("sensor-{label}")),
            true,
        )],
        ClockSynchronization::SingleClock {
            clock_id: artifact(&format!("clock-{label}")),
        },
        RepeatabilitySummary::try_new(
            3,
            CovarianceMatrix::try_new(
                experiment_qois.len(),
                (0..experiment_qois.len())
                    .flat_map(|row| {
                        (0..=row).map(move |column| if row == column { 0.25 } else { 0.0 })
                    })
                    .collect(),
            )
            .expect("positive-semidefinite repeatability covariance"),
        )
        .expect("repeatability fixture"),
        DataAuthenticity::new(
            hash(source_bytes_label),
            hash(&format!("custody-{label}")),
            true,
        ),
    )
    .expect("experiment fixture");
    let experiment_hash = experiment.content_hash().expect("experiment hashes");
    let split = CalibrationSplit::try_new(
        header(&format!("split-{label}"), &["unitless"], "fixture.split"),
        ArtifactRef::new(
            ArtifactKind::ExperimentArtifact,
            experiment.id().clone(),
            experiment_hash,
        ),
        hash(&format!("preregistration-{label}")),
        calibration.iter().copied().map(observation).collect(),
        validation.iter().copied().map(observation).collect(),
        blind
            .iter()
            .map(|id| {
                (
                    observation(id),
                    *source_by_row
                        .get(*id)
                        .expect("blind row must exist in manifest"),
                )
            })
            .collect(),
    )
    .expect("calibration split fixture");
    (experiment, split)
}

fn blind_release_for(split: &CalibrationSplit, authority_label: &str) -> BlindReleaseReceipt {
    BlindReleaseReceipt::new(
        ArtifactRef::new(
            ArtifactKind::CalibrationSplit,
            split.id().clone(),
            split.content_hash().expect("split release hash"),
        ),
        split.blind_commitment(),
        hash(authority_label),
    )
    .expect("blind release fixture")
}

#[derive(Clone)]
struct RetrospectiveCaseFixture {
    id: &'static str,
    purpose: CasePurpose,
    observation_qoi: &'static str,
    observation_row: &'static str,
    experiment_key: &'static str,
    split_key: &'static str,
    experiment: ExperimentArtifact,
    split: CalibrationSplit,
    observation_instrument: Option<ArtifactId>,
    observation_clock: Option<ArtifactId>,
    observation_sensor: Option<SourceKey>,
    duplicate_row_channel: bool,
    blind_release: Option<BlindReleaseReceipt>,
}

impl RetrospectiveCaseFixture {
    fn with_observation_instrument(mut self, instrument: &str) -> Self {
        self.observation_instrument = Some(artifact(instrument));
        self
    }

    fn with_observation_clock(mut self, clock: &str) -> Self {
        self.observation_clock = Some(artifact(clock));
        self
    }

    fn with_observation_sensor(mut self, sensor: &str) -> Self {
        self.observation_sensor = Some(source_key(sensor));
        self
    }

    fn with_duplicate_row_channel(mut self) -> Self {
        self.duplicate_row_channel = true;
        self
    }

    fn without_blind_release(mut self) -> Self {
        self.blind_release = None;
        self
    }

    fn with_blind_release_authority(mut self, authority_label: &str) -> Self {
        self.blind_release = Some(blind_release_for(&self.split, authority_label));
        self
    }

    fn with_blind_release(mut self, release: BlindReleaseReceipt) -> Self {
        self.blind_release = Some(release);
        self
    }
}

fn case_fixture(
    id: &'static str,
    purpose: CasePurpose,
    observation_qoi: &'static str,
    observation_row: &'static str,
    experiment_key: &'static str,
    split_key: &'static str,
    data: (ExperimentArtifact, CalibrationSplit),
) -> RetrospectiveCaseFixture {
    let blind_release = matches!(&purpose, CasePurpose::BlindFalsification)
        .then(|| blind_release_for(&data.1, &format!("blind-release-authority-{id}")));
    RetrospectiveCaseFixture {
        id,
        purpose,
        observation_qoi,
        observation_row,
        experiment_key,
        split_key,
        experiment: data.0,
        split: data.1,
        observation_instrument: None,
        observation_clock: None,
        observation_sensor: None,
        duplicate_row_channel: false,
        blind_release,
    }
}

fn case_sensor_source(case: &RetrospectiveCaseFixture) -> SourceKey {
    if let Some(sensor) = &case.observation_sensor {
        return sensor.clone();
    }
    let instrument = case
        .experiment
        .instruments()
        .first()
        .expect("retrospective fixture instrument")
        .instrument_id()
        .as_str();
    let suffix = instrument
        .strip_prefix("instrument-")
        .expect("fixture instrument naming contract");
    source_key(&format!("sensor-{suffix}"))
}

fn study_case(case: &RetrospectiveCaseFixture) -> StudyCaseDocument {
    let frame = FrameBinding::try_new(
        artifact(&format!("frame-{}", case.id)),
        hash(&format!("frame-transform-{}", case.id)),
        "right-handed-cartesian",
    )
    .expect("frame fixture");
    let experiment_clock = match case.experiment.clocks() {
        ClockSynchronization::SingleClock { clock_id } => clock_id.clone(),
        ClockSynchronization::Synchronized { clock_ids, .. } => clock_ids
            .first()
            .expect("synchronized fixture clock")
            .clone(),
    };
    let instrument = case
        .experiment
        .instruments()
        .first()
        .expect("retrospective fixture instrument")
        .instrument_id()
        .clone();
    let observation_instrument = case
        .observation_instrument
        .clone()
        .unwrap_or_else(|| instrument.clone());
    let observation_clock = case
        .observation_clock
        .clone()
        .unwrap_or_else(|| experiment_clock.clone());
    let protocol = ProtocolBinding::try_new(
        artifact(&format!("protocol-{}", case.id)),
        7,
        2,
        3,
        hash(&format!("load-path-{}", case.id)),
        hash(&format!("environment-path-{}", case.id)),
        hash(&format!("time-grid-{}", case.id)),
        observation_clock.clone(),
    )
    .expect("protocol fixture");
    let observation_channel = channel(&format!("signal-{}", case.id));
    let observation = StudyObservation::try_new(
        observation_channel.clone(),
        qoi(case.observation_qoi),
        unit("Pa"),
        QuantitySpec::dimensional(STRESS),
        frame.clone(),
        format!("node-{}", case.id),
        "stress-output",
        source_key(&format!("operator-{}", case.id)),
        source_key(&format!("aggregation-{}", case.id)),
        case_sensor_source(case),
        observation_instrument,
        observation_clock,
        4,
        MarginalNoiseSpec::Gaussian {
            standard_deviation: 2.0e5,
        },
        MissingnessAssumption::Unknown {
            reason: "missingness has not yet been characterized".to_string(),
        },
        None,
        7,
        3,
        ObservationRows::Retrospective(BTreeSet::from([observation(case.observation_row)])),
    )
    .expect("retrospective observation fixture");
    let mut observations = vec![observation];
    if case.duplicate_row_channel {
        let original = &observations[0];
        observations.push(
            StudyObservation::try_new(
                channel(&format!("signal-{}-duplicate", case.id)),
                original.qoi().clone(),
                original.unit().clone(),
                original.quantity(),
                original.frame().clone(),
                original.graph_node().to_string(),
                original.graph_port().to_string(),
                original.operator().clone(),
                original.aggregation().clone(),
                original.sensor().clone(),
                original.instrument().clone(),
                original.clock().clone(),
                original.operator_version(),
                original.noise().clone(),
                original.missingness().clone(),
                original.saturation(),
                original.protocol_version(),
                original.refinement_version(),
                original.rows().clone(),
            )
            .expect("duplicate-row observation fixture"),
        );
    }
    let discrepancies = observations
        .iter()
        .map(|observation| {
            (
                observation.id().clone(),
                StudyDiscrepancy::Uncharacterized {
                    reason: "no discrepancy model is admitted for this test channel".to_string(),
                },
            )
        })
        .collect();
    StudyCaseDocument::try_new(
        case_id(case.id),
        case.purpose.clone(),
        InitialStateBinding::Zero { schema_version: 2 },
        SpecimenBinding::try_new(
            artifact(&format!("specimen-{}", case.id)),
            hash(&format!("geometry-{}", case.id)),
            hash(&format!("process-{}", case.id)),
            hash(&format!("preparation-{}", case.id)),
            frame,
        )
        .expect("specimen fixture"),
        protocol,
        source_key(&format!("forward-{}", case.id)),
        CaseDataDeclaration::Retrospective {
            experiment: source_key(case.experiment_key),
            split: source_key(case.split_key),
            parser: source_key("parser"),
            preprocessing: source_key("preprocessing"),
            parser_version: 2,
            split_grouping: artifact("split-by-specimen"),
        },
        observations,
        discrepancies,
    )
    .expect("study case fixture")
}

struct ProblemFixture {
    context: ContextOfUse,
    material: MaterialCard,
    model: ConstitutiveModelCard,
    document: IdentifiabilityProblemDocument,
    cases: Vec<RetrospectiveCaseFixture>,
}

fn problem_fixture(
    cases: Vec<RetrospectiveCaseFixture>,
    data_reuse: DataReusePolicy,
) -> ProblemFixture {
    let context = context();
    let (material, model) = model_cards();
    let mut sources = vec![
        source(
            "context",
            SourceKind::ContextOfUse,
            context.content_hash().expect("context hashes"),
        ),
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
        source("graph", SourceKind::ConstitutiveGraph, hash("graph")),
        source("parser", SourceKind::Parser, hash("parser")),
        source(
            "preprocessing",
            SourceKind::Preprocessing,
            hash("preprocessing"),
        ),
    ];
    let mut registered_sensor_sources = BTreeSet::new();
    for case in &cases {
        sources.extend([
            source(
                &format!("forward-{}", case.id),
                SourceKind::ForwardModel,
                hash(&format!("forward-{}", case.id)),
            ),
            source(
                &format!("operator-{}", case.id),
                SourceKind::ObservationOperator,
                hash(&format!("operator-{}", case.id)),
            ),
            source(
                &format!("aggregation-{}", case.id),
                SourceKind::ObservationOperator,
                hash(&format!("aggregation-{}", case.id)),
            ),
        ]);
        for candidate in [
            source(
                case.experiment_key,
                SourceKind::ExperimentArtifact,
                case.experiment
                    .content_hash()
                    .expect("experiment source hashes"),
            ),
            source(
                case.split_key,
                SourceKind::CalibrationSplit,
                case.split.content_hash().expect("split source hashes"),
            ),
        ] {
            if let Some(existing) = sources
                .iter()
                .find(|existing| existing.key() == candidate.key())
            {
                assert_eq!(
                    existing, &candidate,
                    "a shared concrete source key must retain exactly one source reference"
                );
            } else {
                sources.push(candidate);
            }
        }
        let sensor = case_sensor_source(case);
        if registered_sensor_sources.insert(sensor.clone()) {
            let certificate_hash = case
                .experiment
                .instruments()
                .first()
                .expect("fixture experiment instrument")
                .certificate_hash();
            let expected_hash = if case.observation_sensor.is_some() {
                hash(sensor.as_str())
            } else {
                certificate_hash
            };
            sources.push(source(
                sensor.as_str(),
                SourceKind::Metrology,
                expected_hash,
            ));
        }
    }
    if matches!(&data_reuse, DataReusePolicy::Shared { .. }) {
        sources.push(source(
            "joint-likelihood",
            SourceKind::Likelihood,
            hash("joint-likelihood"),
        ));
    }
    let parameter_domain =
        ParameterDomain::try_new(1.0e6, 1.0e9).expect("yield-stress parameter domain");
    let parameters = vec![
        StudyParameter::try_new(
            role("yield_stress"),
            QuantitySpec::dimensional(STRESS),
            parameter_domain,
            ParameterPurpose::Estimand,
            ParameterTreatment::Estimated,
            ParameterOwnerBinding::ConstitutiveModel,
            ParameterScopeBinding::Global,
            PriorPolicy::Distribution(ParameterPrior::Uniform {
                version: 1,
                domain: parameter_domain,
            }),
            InfluenceCoverage::Declared,
        )
        .expect("study parameter fixture"),
    ];
    let influences = cases
        .iter()
        .map(|case| {
            InfluenceDeclaration::new(
                InfluenceId::try_new(format!("yield-to-observation-{}", case.id))
                    .expect("influence id"),
                role("yield_stress"),
                DistributionFunctional::Location {
                    observation: ObservationKey::new(
                        case_id(case.id),
                        channel(&format!("signal-{}", case.id)),
                    ),
                },
                InfluenceRepresentation::Direct,
            )
        })
        .collect();
    let joint_noise = if matches!(&data_reuse, DataReusePolicy::Shared { .. }) {
        // This fixture conservatively declines an independence assumption for
        // reused provenance. The sharing group's likelihood is also the
        // explicit cross-case noise kernel; provenance reuse alone is not a
        // theorem of stochastic dependence.
        JointNoiseModel::ExternalKernel {
            model: source_key("joint-likelihood"),
        }
    } else {
        JointNoiseModel::Independent
    };
    let document = IdentifiabilityProblemDocument::try_new(
        source_key("context"),
        source_key("material"),
        source_key("model"),
        source_key("graph"),
        sources,
        parameters,
        Vec::new(),
        cases.iter().map(study_case).collect(),
        influences,
        Vec::new(),
        joint_noise,
        data_reuse,
    )
    .expect("retrospective problem is structurally valid");
    ProblemFixture {
        context,
        material,
        model,
        document,
        cases,
    }
}

fn opaque_resolutions(document: &IdentifiabilityProblemDocument) -> SourceResolutionSet {
    SourceResolutionSet::try_new(
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
                    source.key().as_str().as_bytes(),
                    AuthorityDisposition::ContentVerified,
                )
                .expect("opaque source resolution")
            })
            .collect(),
    )
    .expect("closed opaque source resolution set")
}

#[derive(Debug, Clone, Copy)]
enum BundleMode {
    Exact,
    Missing,
    Extra,
}

fn admit(
    fixture: ProblemFixture,
    bundle_mode: BundleMode,
) -> Result<AdmittedIdentifiabilityProblem, IdentifiabilityError> {
    admit_with_concrete_authority(fixture, bundle_mode, Vec::new())
}

fn admit_with_concrete_authority(
    fixture: ProblemFixture,
    bundle_mode: BundleMode,
    concrete_authority: Vec<(SourceKey, AuthorityDisposition)>,
) -> Result<AdmittedIdentifiabilityProblem, IdentifiabilityError> {
    let ProblemFixture {
        context,
        material,
        model,
        document,
        cases,
    } = fixture;
    let opaque = opaque_resolutions(&document);
    let mut bundles = BTreeMap::new();
    if !matches!(bundle_mode, BundleMode::Missing) {
        for case in &cases {
            let mut bundle = CaseSourceBundle::new(&case.experiment, &case.split);
            if let Some(release) = &case.blind_release {
                bundle = bundle.with_blind_release(release);
            }
            bundles.insert(case_id(case.id), bundle);
        }
    }
    if matches!(bundle_mode, BundleMode::Extra) {
        let source = cases.first().expect("extra-bundle fixture needs one case");
        bundles.insert(
            case_id("unknown-case"),
            CaseSourceBundle::new(&source.experiment, &source.split),
        );
    }
    let bundle = ProblemSourceBundle::new(&context, &material, &model, bundles, opaque)
        .with_concrete_authority(concrete_authority)?;
    AdmittedIdentifiabilityProblem::resolve_and_admit(document, bundle)
}

fn ordinary_data(label: &str) -> (ExperimentArtifact, CalibrationSplit) {
    let calibration = format!("cal-{label}");
    let validation = format!("val-{label}");
    let blind = format!("blind-{label}");
    let calibration_source = format!("source-cal-{label}");
    let validation_source = format!("source-val-{label}");
    let blind_source = format!("source-blind-{label}");
    let source_bytes = format!("source-bytes-{label}");
    physical_data(
        label,
        &["stress"],
        &[
            (calibration.as_str(), calibration_source.as_str()),
            (validation.as_str(), validation_source.as_str()),
            (blind.as_str(), blind_source.as_str()),
        ],
        &[calibration.as_str()],
        &[validation.as_str()],
        &[blind.as_str()],
        &source_bytes,
    )
}

fn replacement_split(
    label: &str,
    experiment_reference: ArtifactRef,
    calibration: &[&str],
    validation: &[&str],
    blind: &[(&str, ContentHash)],
) -> CalibrationSplit {
    CalibrationSplit::try_new(
        header(
            &format!("replacement-split-{label}"),
            &["unitless"],
            "fixture.split",
        ),
        experiment_reference,
        hash(&format!("replacement-preregistration-{label}")),
        calibration.iter().copied().map(observation).collect(),
        validation.iter().copied().map(observation).collect(),
        blind
            .iter()
            .map(|(id, source)| (observation(id), *source))
            .collect(),
    )
    .expect("replacement split is structurally valid")
}

fn experiment_reference(experiment: &ExperimentArtifact) -> ArtifactRef {
    ArtifactRef::new(
        ArtifactKind::ExperimentArtifact,
        experiment.id().clone(),
        experiment.content_hash().expect("experiment content hash"),
    )
}

#[test]
fn calibration_case_admits_only_its_calibration_partition() {
    let fixture = problem_fixture(
        vec![case_fixture(
            "a",
            CasePurpose::Calibration,
            "stress",
            "cal-a",
            "experiment-a",
            "split-a",
            ordinary_data("a"),
        )],
        DataReusePolicy::Disjoint,
    );
    let admitted = admit(fixture, BundleMode::Exact).expect("calibration case admits");
    assert_eq!(admitted.data().len(), 1);
    log(
        "calibration-partition-admission",
        "pass",
        "one source-resolved calibration lineage",
        &format!(
            "admitted_cases={}; problem_id={:?}",
            admitted.data().len(),
            admitted.id().digest(),
        ),
    );
}

#[test]
fn validation_only_case_admits_only_its_validation_partition() {
    let fixture = problem_fixture(
        vec![case_fixture(
            "a",
            CasePurpose::ValidationOnly,
            "stress",
            "val-a",
            "experiment-a",
            "split-a",
            ordinary_data("a"),
        )],
        DataReusePolicy::Disjoint,
    );
    let admitted = admit(fixture, BundleMode::Exact).expect("validation-only case admits");
    assert_eq!(admitted.data().len(), 1);
    log(
        "validation-partition-admission",
        "pass",
        "one source-resolved validation lineage",
        &format!("admitted_cases={}", admitted.data().len()),
    );
}

#[test]
fn blind_falsification_case_admits_only_its_blind_partition() {
    let fixture = problem_fixture(
        vec![case_fixture(
            "a",
            CasePurpose::BlindFalsification,
            "stress",
            "blind-a",
            "experiment-a",
            "split-a",
            ordinary_data("a"),
        )],
        DataReusePolicy::Disjoint,
    );
    let admitted = admit(fixture, BundleMode::Exact).expect("blind-falsification case admits");
    assert_eq!(admitted.data().len(), 1);
    log(
        "blind-partition-admission",
        "pass",
        "one source-resolved blind lineage",
        &format!("admitted_cases={}", admitted.data().len()),
    );
}

#[test]
fn blind_falsification_without_release_refuses() {
    let case = case_fixture(
        "a",
        CasePurpose::BlindFalsification,
        "stress",
        "blind-a",
        "experiment-a",
        "split-a",
        ordinary_data("a"),
    )
    .without_blind_release();
    let error = admit(
        problem_fixture(vec![case], DataReusePolicy::Disjoint),
        BundleMode::Exact,
    )
    .expect_err("sealed blind rows require a release");
    assert!(matches!(
        &error,
        IdentifiabilityError::InvalidText {
            field: "blind release",
            ..
        }
    ));
    log(
        "blind-release-required",
        "pass",
        "blind holdout remains sealed without authority",
        &error.to_string(),
    );
}

#[test]
fn blind_release_bound_to_another_split_refuses() {
    let data = ordinary_data("a");
    let wrong_release = BlindReleaseReceipt::new(
        ArtifactRef::new(
            ArtifactKind::CalibrationSplit,
            artifact("another-split"),
            hash("another-split"),
        ),
        data.1.blind_commitment(),
        hash("wrong-split-release-authority"),
    )
    .expect("structurally valid wrong-split receipt");
    let case = case_fixture(
        "a",
        CasePurpose::BlindFalsification,
        "stress",
        "blind-a",
        "experiment-a",
        "split-a",
        data,
    )
    .with_blind_release(wrong_release);
    let error = admit(
        problem_fixture(vec![case], DataReusePolicy::Disjoint),
        BundleMode::Exact,
    )
    .expect_err("release for another split must refuse");
    assert!(matches!(&error, IdentifiabilityError::Vv { .. }));
    log(
        "blind-release-split-binding",
        "pass",
        "release must bind the exact split id and content hash",
        &error.to_string(),
    );
}

#[test]
fn blind_release_with_wrong_split_id_refuses() {
    let data = ordinary_data("a");
    let wrong_release = BlindReleaseReceipt::new(
        ArtifactRef::new(
            ArtifactKind::CalibrationSplit,
            artifact("another-split"),
            data.1.content_hash().expect("real split hash"),
        ),
        data.1.blind_commitment(),
        hash("wrong-split-id-release-authority"),
    )
    .expect("structurally valid wrong-id receipt");
    let case = case_fixture(
        "a",
        CasePurpose::BlindFalsification,
        "stress",
        "blind-a",
        "experiment-a",
        "split-a",
        data,
    )
    .with_blind_release(wrong_release);
    let error = admit(
        problem_fixture(vec![case], DataReusePolicy::Disjoint),
        BundleMode::Exact,
    )
    .expect_err("right hash under another split id must refuse");
    assert!(matches!(&error, IdentifiabilityError::Vv { .. }));
    log(
        "blind-release-split-id-binding",
        "pass",
        "release binds split id independently of the split hash",
        &error.to_string(),
    );
}

#[test]
fn blind_release_with_wrong_split_hash_refuses() {
    let data = ordinary_data("a");
    let wrong_release = BlindReleaseReceipt::new(
        ArtifactRef::new(
            ArtifactKind::CalibrationSplit,
            data.1.id().clone(),
            hash("another-split-hash"),
        ),
        data.1.blind_commitment(),
        hash("wrong-split-hash-release-authority"),
    )
    .expect("structurally valid wrong-hash receipt");
    let case = case_fixture(
        "a",
        CasePurpose::BlindFalsification,
        "stress",
        "blind-a",
        "experiment-a",
        "split-a",
        data,
    )
    .with_blind_release(wrong_release);
    let error = admit(
        problem_fixture(vec![case], DataReusePolicy::Disjoint),
        BundleMode::Exact,
    )
    .expect_err("right split id under another hash must refuse");
    assert!(matches!(&error, IdentifiabilityError::Vv { .. }));
    log(
        "blind-release-split-hash-binding",
        "pass",
        "release binds split content hash independently of the split id",
        &error.to_string(),
    );
}

#[test]
fn blind_release_with_wrong_commitment_refuses() {
    let data = ordinary_data("a");
    let wrong_release = BlindReleaseReceipt::new(
        ArtifactRef::new(
            ArtifactKind::CalibrationSplit,
            data.1.id().clone(),
            data.1.content_hash().expect("split hash"),
        ),
        hash("wrong-blind-commitment"),
        hash("wrong-commitment-release-authority"),
    )
    .expect("structurally valid wrong-commitment receipt");
    let case = case_fixture(
        "a",
        CasePurpose::BlindFalsification,
        "stress",
        "blind-a",
        "experiment-a",
        "split-a",
        data,
    )
    .with_blind_release(wrong_release);
    let error = admit(
        problem_fixture(vec![case], DataReusePolicy::Disjoint),
        BundleMode::Exact,
    )
    .expect_err("release for another commitment must refuse");
    assert!(matches!(&error, IdentifiabilityError::Vv { .. }));
    log(
        "blind-release-commitment-binding",
        "pass",
        "release cannot be replayed against a different sealed row commitment",
        &error.to_string(),
    );
}

#[test]
fn nonblind_case_rejects_surplus_blind_release() {
    let case = case_fixture(
        "a",
        CasePurpose::Calibration,
        "stress",
        "cal-a",
        "experiment-a",
        "split-a",
        ordinary_data("a"),
    )
    .with_blind_release_authority("surplus-release-authority");
    let error = admit(
        problem_fixture(vec![case], DataReusePolicy::Disjoint),
        BundleMode::Exact,
    )
    .expect_err("non-blind case must not consume blind authority");
    assert!(matches!(
        &error,
        IdentifiabilityError::InvalidText {
            field: "blind release",
            ..
        }
    ));
    log(
        "blind-release-surplus",
        "pass",
        "blind-release authority is purpose-scoped and cannot be laundered into calibration",
        &error.to_string(),
    );
}

#[test]
fn blind_release_authority_moves_admission_not_problem_identity() {
    let make_fixture = |authority: &'static str| {
        problem_fixture(
            vec![
                case_fixture(
                    "a",
                    CasePurpose::BlindFalsification,
                    "stress",
                    "blind-a",
                    "experiment-a",
                    "split-a",
                    ordinary_data("a"),
                )
                .with_blind_release_authority(authority),
            ],
            DataReusePolicy::Disjoint,
        )
    };
    let left = admit(make_fixture("blind-authority-left"), BundleMode::Exact)
        .expect("left release admits");
    let right = admit(make_fixture("blind-authority-right"), BundleMode::Exact)
        .expect("right release admits");
    assert_eq!(left.id(), right.id());
    assert_ne!(left.source_admission_id(), right.source_admission_id());
    assert_ne!(
        left.source_admission_canonical_bytes()
            .expect("left source admission"),
        right
            .source_admission_canonical_bytes()
            .expect("right source admission"),
    );
    log(
        "blind-release-authority-identity",
        "pass",
        "release authority moves SourceAdmissionId while leaving the physical question stable",
        "two exact authority receipts retained in distinct admission preimages",
    );
}

#[test]
fn blind_release_authority_must_agree_with_explicit_concrete_authority() {
    let make_fixture = || {
        problem_fixture(
            vec![case_fixture(
                "a",
                CasePurpose::BlindFalsification,
                "stress",
                "blind-a",
                "experiment-a",
                "split-a",
                ordinary_data("a"),
            )],
            DataReusePolicy::Disjoint,
        )
    };
    let matching = AuthorityDisposition::ExternalTrustReceipt {
        trust_receipt: hash("blind-release-authority-a"),
    };
    admit_with_concrete_authority(
        make_fixture(),
        BundleMode::Exact,
        vec![(source_key("split-a"), matching)],
    )
    .expect("matching explicit split authority admits");

    let conflicting = AuthorityDisposition::ExternalTrustReceipt {
        trust_receipt: hash("different-explicit-split-authority"),
    };
    let error = admit_with_concrete_authority(
        make_fixture(),
        BundleMode::Exact,
        vec![(source_key("split-a"), conflicting)],
    )
    .expect_err("conflicting explicit split authority must refuse");
    assert!(matches!(
        &error,
        IdentifiabilityError::SourceMismatch {
            field: "blind release/concrete source authority",
        }
    ));
    log(
        "blind-release-explicit-authority-agreement",
        "pass",
        "release-derived and caller-declared split authority must agree exactly",
        &error.to_string(),
    );
}

#[test]
fn shared_split_with_matching_blind_release_admits() {
    let shared = ordinary_data("shared-blind-positive");
    let release = blind_release_for(&shared.1, "shared-blind-authority");
    let cases = vec![
        case_fixture(
            "a",
            CasePurpose::BlindFalsification,
            "stress",
            "blind-shared-blind-positive",
            "experiment-shared",
            "split-shared",
            shared.clone(),
        )
        .with_blind_release(release.clone()),
        case_fixture(
            "b",
            CasePurpose::BlindFalsification,
            "stress",
            "blind-shared-blind-positive",
            "experiment-shared",
            "split-shared",
            shared,
        )
        .with_blind_release(release),
    ];
    let reuse = DataReusePolicy::Shared {
        groups: vec![
            DataSharingGroup::try_new(
                BTreeSet::from([case_id("a"), case_id("b")]),
                source_key("joint-likelihood"),
                "both cases intentionally consume one released sealed campaign",
            )
            .expect("sharing group"),
        ],
    };
    let admitted = admit(problem_fixture(cases, reuse), BundleMode::Exact)
        .expect("one exact shared release admits deterministically");
    assert_eq!(admitted.data().len(), 2);
    log(
        "shared-split-release-agreement",
        "pass",
        "one exact release authority can safely authorize a shared split",
        &format!("admitted_cases={}", admitted.data().len()),
    );
}

#[test]
fn shared_split_with_conflicting_blind_releases_refuses() {
    let shared = ordinary_data("shared-blind");
    let cases = vec![
        case_fixture(
            "a",
            CasePurpose::BlindFalsification,
            "stress",
            "blind-shared-blind",
            "experiment-shared",
            "split-shared",
            shared.clone(),
        )
        .with_blind_release_authority("shared-authority-left"),
        case_fixture(
            "b",
            CasePurpose::BlindFalsification,
            "stress",
            "blind-shared-blind",
            "experiment-shared",
            "split-shared",
            shared,
        )
        .with_blind_release_authority("shared-authority-right"),
    ];
    let reuse = DataReusePolicy::Shared {
        groups: vec![
            DataSharingGroup::try_new(
                BTreeSet::from([case_id("a"), case_id("b")]),
                source_key("joint-likelihood"),
                "both cases intentionally consume the same sealed campaign",
            )
            .expect("sharing group"),
        ],
    };
    let error = admit(problem_fixture(cases, reuse), BundleMode::Exact)
        .expect_err("one split key cannot carry contradictory release authority");
    assert!(matches!(
        &error,
        IdentifiabilityError::SourceMismatch {
            field: "shared split blind release",
        }
    ));
    log(
        "shared-split-release-conflict",
        "pass",
        "shared split authority must be exact and order-independent",
        &error.to_string(),
    );
}

#[test]
fn split_bound_to_another_experiment_refuses() {
    let (experiment, _) = ordinary_data("a");
    let split = replacement_split(
        "wrong-experiment",
        ArtifactRef::new(
            ArtifactKind::ExperimentArtifact,
            artifact("another-experiment"),
            hash("another-experiment"),
        ),
        &["cal-a"],
        &["val-a"],
        &[("blind-a", hash("source-blind-a"))],
    );
    let error = admit(
        problem_fixture(
            vec![case_fixture(
                "a",
                CasePurpose::Calibration,
                "stress",
                "cal-a",
                "experiment-a",
                "split-a",
                (experiment, split),
            )],
            DataReusePolicy::Disjoint,
        ),
        BundleMode::Exact,
    )
    .expect_err("split bound to another experiment must refuse");
    assert!(matches!(&error, IdentifiabilityError::Vv { .. }));
    log(
        "split-experiment-binding",
        "pass",
        "CalibrationSplit must bind the exact admitted ExperimentArtifact",
        &error.to_string(),
    );
}

#[test]
fn split_partition_union_different_from_manifest_refuses() {
    let (experiment, _) = ordinary_data("a");
    let split = replacement_split(
        "partition-union",
        experiment_reference(&experiment),
        &["cal-a"],
        &["val-not-in-manifest"],
        &[("blind-a", hash("source-blind-a"))],
    );
    let error = admit(
        problem_fixture(
            vec![case_fixture(
                "a",
                CasePurpose::Calibration,
                "stress",
                "cal-a",
                "experiment-a",
                "split-a",
                (experiment, split),
            )],
            DataReusePolicy::Disjoint,
        ),
        BundleMode::Exact,
    )
    .expect_err("split partition union different from the manifest must refuse");
    assert!(matches!(&error, IdentifiabilityError::Vv { .. }));
    log(
        "split-manifest-partition-union",
        "pass",
        "calibration, validation, and blind IDs must exactly cover the manifest",
        &error.to_string(),
    );
}

#[test]
fn split_blind_source_different_from_manifest_refuses() {
    let (experiment, _) = ordinary_data("a");
    let split = replacement_split(
        "blind-source",
        experiment_reference(&experiment),
        &["cal-a"],
        &["val-a"],
        &[("blind-a", hash("wrong-blind-source"))],
    );
    let error = admit(
        problem_fixture(
            vec![case_fixture(
                "a",
                CasePurpose::Calibration,
                "stress",
                "cal-a",
                "experiment-a",
                "split-a",
                (experiment, split),
            )],
            DataReusePolicy::Disjoint,
        ),
        BundleMode::Exact,
    )
    .expect_err("blind row rebound to another immutable source must refuse");
    assert!(matches!(&error, IdentifiabilityError::Vv { .. }));
    log(
        "blind-row-source-binding",
        "pass",
        "blind row identity and immutable source identity remain jointly sealed",
        &error.to_string(),
    );
}

#[test]
fn observation_row_absent_from_manifest_refuses() {
    let error = admit(
        problem_fixture(
            vec![case_fixture(
                "a",
                CasePurpose::Calibration,
                "stress",
                "cal-not-in-manifest",
                "experiment-a",
                "split-a",
                ordinary_data("a"),
            )],
            DataReusePolicy::Disjoint,
        ),
        BundleMode::Exact,
    )
    .expect_err("observation row outside the admitted manifest must refuse");
    assert!(matches!(
        &error,
        IdentifiabilityError::UnknownReference {
            field: "observation raw row",
            ..
        }
    ));
    log(
        "observation-manifest-row-closure",
        "pass",
        "each retrospective observation row must occur in the admitted manifest",
        &error.to_string(),
    );
}

fn assert_case_partition_refuses(
    name: &str,
    purpose: CasePurpose,
    row: &'static str,
    expected_partition: &str,
) {
    let fixture = problem_fixture(
        vec![case_fixture(
            "a",
            purpose,
            "stress",
            row,
            "experiment-a",
            "split-a",
            ordinary_data("a"),
        )],
        DataReusePolicy::Disjoint,
    );
    let error =
        admit(fixture, BundleMode::Exact).expect_err("case-purpose partition leakage must refuse");
    assert!(matches!(
        &error,
        IdentifiabilityError::InvalidText {
            field: "case-purpose data partition",
            ..
        }
    ));
    log(
        name,
        "pass",
        &format!("only {expected_partition} rows are authorized"),
        &error.to_string(),
    );
}

#[test]
fn calibration_case_refuses_validation_partition() {
    assert_case_partition_refuses(
        "calibration-consuming-validation",
        CasePurpose::Calibration,
        "val-a",
        "calibration",
    );
}

#[test]
fn validation_case_refuses_blind_partition() {
    assert_case_partition_refuses(
        "validation-consuming-blind",
        CasePurpose::ValidationOnly,
        "blind-a",
        "validation",
    );
}

#[test]
fn blind_case_refuses_calibration_partition() {
    assert_case_partition_refuses(
        "blind-consuming-calibration",
        CasePurpose::BlindFalsification,
        "cal-a",
        "blind holdout",
    );
}

#[test]
fn observation_qoi_absent_from_experiment_refuses() {
    let fixture = problem_fixture(
        vec![case_fixture(
            "a",
            CasePurpose::Calibration,
            "tangent",
            "cal-a",
            "experiment-a",
            "split-a",
            ordinary_data("a"),
        )],
        DataReusePolicy::Disjoint,
    );
    let error = admit(fixture, BundleMode::Exact)
        .expect_err("context QoI without experiment QoI must refuse");
    assert!(matches!(
        &error,
        IdentifiabilityError::UnknownReference {
            field: "experiment observation QoI",
            ..
        }
    ));
    log(
        "experiment-qoi-closure",
        "pass",
        "observation QoI must occur in the admitted experiment",
        &error.to_string(),
    );
}

#[test]
fn observation_instrument_absent_from_experiment_refuses() {
    let case = case_fixture(
        "a",
        CasePurpose::Calibration,
        "stress",
        "cal-a",
        "experiment-a",
        "split-a",
        ordinary_data("a"),
    )
    .with_observation_instrument("instrument-not-in-experiment");
    let error = admit(
        problem_fixture(vec![case], DataReusePolicy::Disjoint),
        BundleMode::Exact,
    )
    .expect_err("observation instrument outside the experiment roster must refuse");
    assert!(matches!(
        &error,
        IdentifiabilityError::UnknownReference {
            field: "experiment observation instrument",
            ..
        }
    ));
    log(
        "experiment-instrument-closure",
        "pass",
        "observation instrument must occur in the admitted experiment roster",
        &error.to_string(),
    );
}

#[test]
fn observation_sensor_not_bound_to_instrument_calibration_refuses() {
    let case = case_fixture(
        "a",
        CasePurpose::Calibration,
        "stress",
        "cal-a",
        "experiment-a",
        "split-a",
        ordinary_data("a"),
    )
    .with_observation_sensor("sensor-wrong-calibration");
    let error = admit(
        problem_fixture(vec![case], DataReusePolicy::Disjoint),
        BundleMode::Exact,
    )
    .expect_err("metrology source not bound to instrument certificate must refuse");
    assert!(matches!(
        &error,
        IdentifiabilityError::SourceMismatch {
            field: "observation sensor/instrument calibration",
        }
    ));
    log(
        "instrument-calibration-source-binding",
        "pass",
        "observation metrology source must equal the admitted instrument certificate",
        &error.to_string(),
    );
}

#[test]
fn observation_clock_absent_from_experiment_refuses() {
    let case = case_fixture(
        "a",
        CasePurpose::Calibration,
        "stress",
        "cal-a",
        "experiment-a",
        "split-a",
        ordinary_data("a"),
    )
    .with_observation_clock("clock-not-in-experiment");
    let error = admit(
        problem_fixture(vec![case], DataReusePolicy::Disjoint),
        BundleMode::Exact,
    )
    .expect_err("observation clock outside the experiment topology must refuse");
    assert!(matches!(
        &error,
        IdentifiabilityError::UnknownReference {
            field: "experiment observation clock",
            ..
        }
    ));
    log(
        "experiment-clock-closure",
        "pass",
        "observation and protocol clock must occur in the admitted experiment topology",
        &error.to_string(),
    );
}

#[test]
fn one_raw_row_reused_across_channels_refuses_independent_noise() {
    let case = case_fixture(
        "a",
        CasePurpose::Calibration,
        "stress",
        "cal-a",
        "experiment-a",
        "split-a",
        ordinary_data("a"),
    )
    .with_duplicate_row_channel();
    let error = admit(
        problem_fixture(vec![case], DataReusePolicy::Disjoint),
        BundleMode::Exact,
    )
    .expect_err("one raw row reused across channels needs explicit dependence");
    assert!(matches!(&error, IdentifiabilityError::Covariance { .. }));
    log(
        "within-case-row-reuse-needs-dependence",
        "pass",
        "Independent noise cannot consume one immutable row through two channels",
        &error.to_string(),
    );
}

#[test]
fn distinct_two_case_campaign_admits_under_disjoint_policy() {
    let data_a = ordinary_data("a");
    let data_b = ordinary_data("b");
    assert_ne!(
        data_a.0.content_hash().expect("experiment a hash"),
        data_b.0.content_hash().expect("experiment b hash"),
        "disjoint baseline must use distinct experiment content",
    );
    let fixture = problem_fixture(
        vec![
            case_fixture(
                "a",
                CasePurpose::Calibration,
                "stress",
                "cal-a",
                "experiment-a",
                "split-a",
                data_a,
            ),
            case_fixture(
                "b",
                CasePurpose::Complementary {
                    reason: "independent loading path complements case a".to_string(),
                },
                "stress",
                "cal-b",
                "experiment-b",
                "split-b",
                data_b,
            ),
        ],
        DataReusePolicy::Disjoint,
    );
    let admitted = admit(fixture, BundleMode::Exact).expect("disjoint two-case campaign admits");
    assert_eq!(admitted.data().len(), 2);
    log(
        "disjoint-two-case-baseline",
        "pass",
        "two distinct source-resolved experiments admit under Disjoint",
        &format!("admitted_cases={}", admitted.data().len()),
    );
}

#[test]
fn exact_source_bytes_reuse_refuses_under_disjoint_policy() {
    let data_a = physical_data(
        "a",
        &["stress"],
        &[
            ("cal-a", "source-cal-a"),
            ("val-a", "source-val-a"),
            ("blind-a", "source-blind-a"),
        ],
        &["cal-a"],
        &["val-a"],
        &["blind-a"],
        "shared-raw-source-bytes",
    );
    let data_b = physical_data(
        "b",
        &["stress"],
        &[
            ("cal-b", "source-cal-b"),
            ("val-b", "source-val-b"),
            ("blind-b", "source-blind-b"),
        ],
        &["cal-b"],
        &["val-b"],
        &["blind-b"],
        "shared-raw-source-bytes",
    );
    assert_eq!(
        data_a.0.authenticity().source_bytes_hash(),
        data_b.0.authenticity().source_bytes_hash(),
    );
    assert_ne!(
        data_a.0.manifest().canonical_hash(),
        data_b.0.manifest().canonical_hash(),
    );
    let error = admit(
        problem_fixture(
            vec![
                case_fixture(
                    "a",
                    CasePurpose::Calibration,
                    "stress",
                    "cal-a",
                    "experiment-a",
                    "split-a",
                    data_a,
                ),
                case_fixture(
                    "b",
                    CasePurpose::Calibration,
                    "stress",
                    "cal-b",
                    "experiment-b",
                    "split-b",
                    data_b,
                ),
            ],
            DataReusePolicy::Disjoint,
        ),
        BundleMode::Exact,
    )
    .expect_err("identical raw-source bytes under Disjoint must refuse");
    assert!(matches!(
        &error,
        IdentifiabilityError::InvalidText {
            field: "data reuse policy",
            ..
        }
    ));
    log(
        "disjoint-source-bytes-alias",
        "pass",
        "distinct manifests cannot hide one exact raw-source byte stream",
        &error.to_string(),
    );
}

#[test]
fn exact_manifest_reuse_refuses_under_disjoint_policy() {
    let rows = [
        ("cal-shared", "source-cal-shared"),
        ("val-shared", "source-val-shared"),
        ("blind-shared", "source-blind-shared"),
    ];
    let data_a = physical_data(
        "a",
        &["stress"],
        &rows,
        &["cal-shared"],
        &["val-shared"],
        &["blind-shared"],
        "source-bytes-a",
    );
    let data_b = physical_data(
        "b",
        &["stress"],
        &rows,
        &["cal-shared"],
        &["val-shared"],
        &["blind-shared"],
        "source-bytes-b",
    );
    assert_eq!(
        data_a.0.manifest().canonical_hash(),
        data_b.0.manifest().canonical_hash(),
    );
    assert_ne!(
        data_a.0.authenticity().source_bytes_hash(),
        data_b.0.authenticity().source_bytes_hash(),
    );
    let error = admit(
        problem_fixture(
            vec![
                case_fixture(
                    "a",
                    CasePurpose::Calibration,
                    "stress",
                    "cal-shared",
                    "experiment-a",
                    "split-a",
                    data_a,
                ),
                case_fixture(
                    "b",
                    CasePurpose::Calibration,
                    "stress",
                    "cal-shared",
                    "experiment-b",
                    "split-b",
                    data_b,
                ),
            ],
            DataReusePolicy::Disjoint,
        ),
        BundleMode::Exact,
    )
    .expect_err("identical manifests under Disjoint must refuse");
    assert!(matches!(
        &error,
        IdentifiabilityError::InvalidText {
            field: "data reuse policy",
            ..
        }
    ));
    log(
        "disjoint-manifest-alias",
        "pass",
        "distinct authenticity wrappers cannot hide one exact observation manifest",
        &error.to_string(),
    );
}

#[test]
fn exact_row_source_reuse_refuses_under_disjoint_policy() {
    let data_a = physical_data(
        "a",
        &["stress"],
        &[
            ("cal-a", "shared-immutable-row-source"),
            ("val-a", "source-val-a"),
            ("blind-a", "source-blind-a"),
        ],
        &["cal-a"],
        &["val-a"],
        &["blind-a"],
        "source-bytes-a",
    );
    let data_b = physical_data(
        "b",
        &["stress"],
        &[
            ("cal-b", "shared-immutable-row-source"),
            ("val-b", "source-val-b"),
            ("blind-b", "source-blind-b"),
        ],
        &["cal-b"],
        &["val-b"],
        &["blind-b"],
        "source-bytes-b",
    );
    assert_ne!(
        data_a.0.authenticity().source_bytes_hash(),
        data_b.0.authenticity().source_bytes_hash(),
        "fixture must isolate row-source aliasing from source-byte identity",
    );
    assert_ne!(
        data_a.0.manifest().canonical_hash(),
        data_b.0.manifest().canonical_hash(),
        "fixture must isolate row-source aliasing from manifest identity",
    );
    let fixture = problem_fixture(
        vec![
            case_fixture(
                "a",
                CasePurpose::Calibration,
                "stress",
                "cal-a",
                "experiment-a",
                "split-a",
                data_a,
            ),
            case_fixture(
                "b",
                CasePurpose::Calibration,
                "stress",
                "cal-b",
                "experiment-b",
                "split-b",
                data_b,
            ),
        ],
        DataReusePolicy::Disjoint,
    );
    let error = admit(fixture, BundleMode::Exact)
        .expect_err("immutable row-source reuse under Disjoint must refuse");
    assert!(matches!(
        &error,
        IdentifiabilityError::InvalidText {
            field: "data reuse policy",
            ..
        }
    ));
    log(
        "disjoint-row-source-alias",
        "pass",
        "distinct experiments may not alias one immutable row source",
        &error.to_string(),
    );
}

#[test]
fn declared_sharing_group_admits_one_joint_raw_campaign() {
    let shared = physical_data(
        "shared",
        &["stress"],
        &[
            ("cal-a", "source-cal-a"),
            ("cal-b", "source-cal-b"),
            ("val-shared", "source-val-shared"),
            ("blind-shared", "source-blind-shared"),
        ],
        &["cal-a", "cal-b"],
        &["val-shared"],
        &["blind-shared"],
        "source-bytes-shared",
    );
    let fixture = problem_fixture(
        vec![
            case_fixture(
                "a",
                CasePurpose::Calibration,
                "stress",
                "cal-a",
                "experiment-a",
                "split-a",
                shared.clone(),
            ),
            case_fixture(
                "b",
                CasePurpose::SymmetryBreaking,
                "stress",
                "cal-b",
                "experiment-b",
                "split-b",
                shared,
            ),
        ],
        DataReusePolicy::Shared {
            groups: vec![
                DataSharingGroup::try_new(
                    BTreeSet::from([case_id("a"), case_id("b")]),
                    source_key("joint-likelihood"),
                    "both cases intentionally consume complementary channels from one raw campaign",
                )
                .expect("sharing group fixture"),
            ],
        },
    );
    let admitted = admit(fixture, BundleMode::Exact).expect("declared sharing admits");
    assert_eq!(admitted.data().len(), 2);
    log(
        "declared-joint-sharing",
        "pass",
        "one shared experiment plus an explicit joint likelihood",
        &format!("admitted_cases={}", admitted.data().len()),
    );
}

#[test]
fn declared_sharing_group_without_actual_overlap_refuses() {
    let fixture = problem_fixture(
        vec![
            case_fixture(
                "a",
                CasePurpose::Calibration,
                "stress",
                "cal-a",
                "experiment-a",
                "split-a",
                ordinary_data("a"),
            ),
            case_fixture(
                "b",
                CasePurpose::Complementary {
                    reason: "independent campaign used as a false sharing declaration".to_string(),
                },
                "stress",
                "cal-b",
                "experiment-b",
                "split-b",
                ordinary_data("b"),
            ),
        ],
        DataReusePolicy::Shared {
            groups: vec![
                DataSharingGroup::try_new(
                    BTreeSet::from([case_id("a"), case_id("b")]),
                    source_key("joint-likelihood"),
                    "this deliberately false declaration must not create sharing authority",
                )
                .expect("sharing group fixture"),
            ],
        },
    );
    let error = admit(fixture, BundleMode::Exact)
        .expect_err("a sharing declaration without admitted overlap must refuse");
    assert!(matches!(
        &error,
        IdentifiabilityError::InvalidText {
            field: "data sharing group",
            ..
        }
    ));
    log(
        "sharing-declaration-needs-overlap",
        "pass",
        "declaration alone cannot manufacture raw-data sharing authority",
        &error.to_string(),
    );
}

#[test]
fn missing_retrospective_case_bundle_refuses() {
    let missing_fixture = problem_fixture(
        vec![case_fixture(
            "a",
            CasePurpose::Calibration,
            "stress",
            "cal-a",
            "experiment-a",
            "split-a",
            ordinary_data("a"),
        )],
        DataReusePolicy::Disjoint,
    );
    let missing = admit(missing_fixture, BundleMode::Missing)
        .expect_err("missing retrospective case bundle must refuse");
    assert!(matches!(
        &missing,
        IdentifiabilityError::UnknownReference {
            field: "retrospective case source bundle",
            ..
        }
    ));
    log(
        "missing-case-bundle",
        "pass",
        "every retrospective case has one concrete bundle",
        &missing.to_string(),
    );
}

#[test]
fn unknown_retrospective_case_bundle_refuses() {
    let extra_fixture = problem_fixture(
        vec![case_fixture(
            "a",
            CasePurpose::Calibration,
            "stress",
            "cal-a",
            "experiment-a",
            "split-a",
            ordinary_data("a"),
        )],
        DataReusePolicy::Disjoint,
    );
    let extra = admit(extra_fixture, BundleMode::Extra)
        .expect_err("extra retrospective case bundle must refuse");
    assert!(matches!(
        &extra,
        IdentifiabilityError::Cardinality {
            field: "case source bundles",
            ..
        }
    ));
    log(
        "extra-case-bundle",
        "pass",
        "no unknown case bundle is accepted",
        &extra.to_string(),
    );
}
