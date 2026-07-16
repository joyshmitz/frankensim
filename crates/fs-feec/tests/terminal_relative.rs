//! G0/G3 battery for I13.2a terminal-relative physical schema identities.
//!
//! The battery covers exact incidence, relative subcomplex admission,
//! canonical replay, port/Machine binding, nominal coefficient sectors, and
//! declared conversion boundaries.  It does not claim homology computation,
//! field transfer, or coil manufacturability.

#![cfg(feature = "terminal-relative")]

use fs_couple::{
    CoordinateBinding, PortKind, PortOrientation, PortSchema, PortTimestamp, StableId,
};
use fs_feec::terminal_relative::{
    BoundaryIncidence, CellRef, CellularSubcomplex, ConductorComponent, ConductorComponentId,
    ConversionMapId, DeclaredPhysicalMap, DeclaredPhysicalMapKind, DistributedCurrent,
    FiniteCellComplex, GeometricCoil, IncidenceSign, IntegralRelativeChain,
    IntegralWindingRepresentative, MachineBindingStatus, OrientationMapSign, PhaseId,
    PhysicalObjectId, PhysicalTerminal, PhysicalTerminalId, PresentedMachinePortRef,
    RealCurrentAmplitude, RealRelativeCochain, TerminalOrientation, TerminalPortCoordinate,
    TerminalPortTrivialization, TerminalRelativeCoefficientDomain, TerminalRelativeError,
    TerminalRelativePair, TerminalRole, TrivializationId,
};
use fs_qty::{Current, Dims};

fn stable(value: &str) -> StableId {
    StableId::new(value).expect("fixture stable id")
}

fn interval_complex() -> FiniteCellComplex {
    FiniteCellComplex::try_new(
        1,
        vec![2, 1],
        vec![
            BoundaryIncidence::new(
                CellRef::new(0, 0),
                CellRef::new(1, 0),
                IncidenceSign::Negative,
            ),
            BoundaryIncidence::new(
                CellRef::new(0, 1),
                CellRef::new(1, 0),
                IncidenceSign::Positive,
            ),
        ],
    )
    .expect("oriented interval")
}

fn subcomplex(
    ambient: &FiniteCellComplex,
    id: &str,
    cells: impl IntoIterator<Item = CellRef>,
) -> CellularSubcomplex {
    CellularSubcomplex::try_new(stable(id), cells, ambient).expect("fixture subcomplex")
}

fn electrical_port(id: &str, tick: u64) -> PortSchema {
    PortKind::ElectricalVoltageCurrent
        .scalar_seed_schema(
            stable(id),
            CoordinateBinding::new(
                stable("basis/winding-terminal"),
                stable("frame/winding-terminal"),
                PortOrientation::OutwardFromOwner,
            ),
            PortTimestamp::new(stable("clock/electrical"), tick),
        )
        .expect("electrical port")
}

fn terminal(
    ambient: &FiniteCellComplex,
    ordinal: u32,
    id: &str,
    role: TerminalRole,
    orientation: TerminalOrientation,
    sign: OrientationMapSign,
    tick: u64,
) -> PhysicalTerminal {
    let port = electrical_port(&format!("port/{id}"), tick);
    PhysicalTerminal::new(
        PhysicalTerminalId::new(format!("terminal/{id}")).expect("terminal id"),
        subcomplex(
            ambient,
            &format!("support/{id}"),
            [CellRef::new(0, ordinal)],
        ),
        ConductorComponentId::new("component/winding").expect("component id"),
        PhaseId::new("phase/a").expect("phase id"),
        role,
        orientation,
        TerminalPortCoordinate::Flow,
        port.clone(),
        PresentedMachinePortRef::try_new(
            stable("org.frankensim.fs-ir.machine.graph.v1"),
            1,
            [0x42; 32],
            stable("machine-owner/stator-winding"),
            stable(&format!("port/{id}")),
            stable(&format!("machine-terminal/{id}-voltage")),
            stable(&format!("machine-terminal/{id}-current")),
        )
        .expect("presented Machine-IR port"),
        TerminalPortTrivialization::new(
            TrivializationId::new(format!("trivialization/{id}")).expect("trivialization id"),
            port.id().clone(),
            sign,
            stable("voltage-reference/dc-link-negative"),
            stable(&format!("current-reference/{id}")),
        ),
    )
    .expect("physical terminal")
}

fn pair(tick: u64, reverse_declarations: bool) -> TerminalRelativePair {
    let complex = interval_complex();
    let conductor = subcomplex(
        &complex,
        "support/conductor",
        [CellRef::new(0, 0), CellRef::new(0, 1), CellRef::new(1, 0)],
    );
    let insulation = subcomplex(&complex, "support/insulation-empty", []);
    let relative = subcomplex(
        &complex,
        "support/terminal-relative",
        [CellRef::new(0, 0), CellRef::new(0, 1)],
    );
    let component = ConductorComponent::new(
        ConductorComponentId::new("component/winding").expect("component id"),
        subcomplex(
            &complex,
            "support/component-winding",
            [CellRef::new(0, 0), CellRef::new(0, 1), CellRef::new(1, 0)],
        ),
    )
    .expect("component");
    let driven = terminal(
        &complex,
        0,
        "a-positive",
        TerminalRole::Driven,
        TerminalOrientation::OutOfConductor,
        OrientationMapSign::Preserve,
        tick,
    );
    let return_reference = terminal(
        &complex,
        1,
        "a-return",
        TerminalRole::ReturnReference,
        TerminalOrientation::IntoConductor,
        OrientationMapSign::Reverse,
        tick,
    );
    let terminals = if reverse_declarations {
        vec![return_reference, driven]
    } else {
        vec![driven, return_reference]
    };
    TerminalRelativePair::try_new(
        complex,
        conductor,
        relative,
        insulation,
        vec![component],
        terminals,
    )
    .expect("terminal-relative pair")
}

#[test]
fn i13_2a_001_exact_incidence_accepts_a_triangle_and_rejects_d_squared_defect() {
    let vertices_and_edges = vec![
        BoundaryIncidence::new(
            CellRef::new(0, 0),
            CellRef::new(1, 0),
            IncidenceSign::Negative,
        ),
        BoundaryIncidence::new(
            CellRef::new(0, 1),
            CellRef::new(1, 0),
            IncidenceSign::Positive,
        ),
        BoundaryIncidence::new(
            CellRef::new(0, 1),
            CellRef::new(1, 1),
            IncidenceSign::Negative,
        ),
        BoundaryIncidence::new(
            CellRef::new(0, 2),
            CellRef::new(1, 1),
            IncidenceSign::Positive,
        ),
        BoundaryIncidence::new(
            CellRef::new(0, 0),
            CellRef::new(1, 2),
            IncidenceSign::Negative,
        ),
        BoundaryIncidence::new(
            CellRef::new(0, 2),
            CellRef::new(1, 2),
            IncidenceSign::Positive,
        ),
    ];
    let mut valid = vertices_and_edges.clone();
    valid.extend([
        BoundaryIncidence::new(
            CellRef::new(1, 0),
            CellRef::new(2, 0),
            IncidenceSign::Positive,
        ),
        BoundaryIncidence::new(
            CellRef::new(1, 1),
            CellRef::new(2, 0),
            IncidenceSign::Positive,
        ),
        BoundaryIncidence::new(
            CellRef::new(1, 2),
            CellRef::new(2, 0),
            IncidenceSign::Negative,
        ),
    ]);
    FiniteCellComplex::try_new(2, vec![3, 3, 1], valid).expect("triangle d squared is zero");

    let mut corrupt = vertices_and_edges;
    corrupt.extend([
        BoundaryIncidence::new(
            CellRef::new(1, 0),
            CellRef::new(2, 0),
            IncidenceSign::Positive,
        ),
        BoundaryIncidence::new(
            CellRef::new(1, 1),
            CellRef::new(2, 0),
            IncidenceSign::Positive,
        ),
        BoundaryIncidence::new(
            CellRef::new(1, 2),
            CellRef::new(2, 0),
            IncidenceSign::Positive,
        ),
    ]);
    assert!(matches!(
        FiniteCellComplex::try_new(2, vec![3, 3, 1], corrupt),
        Err(TerminalRelativeError::BoundarySquaredNonzero { .. })
    ));
}

#[test]
fn i13_2a_002_subcomplex_and_terminal_insulation_defects_fail_closed() {
    let complex = interval_complex();
    assert!(matches!(
        CellularSubcomplex::try_new(stable("support/not-closed"), [CellRef::new(1, 0)], &complex,),
        Err(TerminalRelativeError::NotASubcomplex { .. })
    ));

    let conductor = subcomplex(
        &complex,
        "support/conductor",
        [CellRef::new(0, 0), CellRef::new(0, 1), CellRef::new(1, 0)],
    );
    let insulation = subcomplex(&complex, "support/insulation-left", [CellRef::new(0, 0)]);
    let component = ConductorComponent::new(
        ConductorComponentId::new("component/winding").unwrap(),
        conductor.clone(),
    )
    .unwrap();
    let terminals = vec![
        terminal(
            &complex,
            0,
            "a-positive",
            TerminalRole::Driven,
            TerminalOrientation::OutOfConductor,
            OrientationMapSign::Preserve,
            7,
        ),
        terminal(
            &complex,
            1,
            "a-return",
            TerminalRole::ReturnReference,
            TerminalOrientation::IntoConductor,
            OrientationMapSign::Reverse,
            7,
        ),
    ];
    assert!(matches!(
        TerminalRelativePair::try_new(
            complex,
            conductor,
            subcomplex(
                &interval_complex(),
                "support/terminal-relative-overlap",
                [CellRef::new(0, 0), CellRef::new(0, 1)],
            ),
            insulation,
            vec![component],
            terminals,
        ),
        Err(TerminalRelativeError::TerminalInsulationOverlap { .. })
    ));
}

#[test]
fn i13_2a_003_phase_reference_and_orientation_semantics_are_mandatory() {
    let complex = interval_complex();
    let conductor = subcomplex(
        &complex,
        "support/conductor",
        [CellRef::new(0, 0), CellRef::new(0, 1), CellRef::new(1, 0)],
    );
    let component = ConductorComponent::new(
        ConductorComponentId::new("component/winding").unwrap(),
        conductor.clone(),
    )
    .unwrap();
    let both_driven = vec![
        terminal(
            &complex,
            0,
            "a-positive",
            TerminalRole::Driven,
            TerminalOrientation::OutOfConductor,
            OrientationMapSign::Preserve,
            7,
        ),
        terminal(
            &complex,
            1,
            "a-return",
            TerminalRole::Driven,
            TerminalOrientation::IntoConductor,
            OrientationMapSign::Reverse,
            7,
        ),
    ];
    assert!(matches!(
        TerminalRelativePair::try_new(
            complex.clone(),
            conductor.clone(),
            subcomplex(
                &complex,
                "support/terminal-relative-missing-role",
                [CellRef::new(0, 0), CellRef::new(0, 1)],
            ),
            subcomplex(&complex, "support/insulation-empty", []),
            vec![component.clone()],
            both_driven,
        ),
        Err(TerminalRelativeError::MissingPhaseRole {
            role: TerminalRole::ReturnReference,
            ..
        })
    ));

    let same_orientation = vec![
        terminal(
            &complex,
            0,
            "a-positive",
            TerminalRole::Driven,
            TerminalOrientation::OutOfConductor,
            OrientationMapSign::Preserve,
            7,
        ),
        terminal(
            &complex,
            1,
            "a-return",
            TerminalRole::ReturnReference,
            TerminalOrientation::OutOfConductor,
            OrientationMapSign::Preserve,
            7,
        ),
    ];
    assert!(matches!(
        TerminalRelativePair::try_new(
            complex.clone(),
            conductor,
            subcomplex(
                &complex,
                "support/terminal-relative-orientation",
                [CellRef::new(0, 0), CellRef::new(0, 1)],
            ),
            subcomplex(&complex, "support/insulation-empty-2", []),
            vec![component],
            same_orientation,
        ),
        Err(TerminalRelativeError::PhaseOrientationDoesNotClose { .. })
    ));
}

#[test]
fn i13_2a_004_port_and_machine_bindings_are_presented_and_identity_bearing() {
    let canonical = pair(17, false);
    let permuted = pair(17, true);
    let retimed = pair(18, false);
    assert_eq!(canonical.identity(), permuted.identity());
    assert_ne!(canonical.identity(), retimed.identity());
    assert!(canonical.canonical_bytes() > 0);
    let receipt = canonical.complex_receipt();
    assert_eq!(receipt.identity_receipt().id(), canonical.identity());
    assert_eq!(
        receipt.coefficient_domains(),
        &[
            TerminalRelativeCoefficientDomain::Integers,
            TerminalRelativeCoefficientDomain::FiniteReal,
        ]
    );
    assert_eq!(receipt.current_dimensions(), Current::DIMS);
    assert_eq!(receipt.terminal_bindings().len(), 2);
    assert_eq!(
        receipt.machine_binding(),
        MachineBindingStatus::PresentedOnly
    );

    let positive = canonical
        .terminals()
        .iter()
        .find(|terminal| terminal.role() == TerminalRole::Driven)
        .expect("driven terminal");
    let positive_receipt = receipt
        .terminal_bindings()
        .iter()
        .find(|binding| binding.terminal() == positive.id())
        .expect("driven terminal receipt");
    assert_eq!(positive_receipt.port_schema(), positive.port());
    assert_eq!(positive_receipt.machine(), positive.machine());
    assert_eq!(positive_receipt.trivialization(), positive.trivialization());
    assert_eq!(positive.phase().as_str(), "phase/a");
    assert_eq!(positive.port().kind(), PortKind::ElectricalVoltageCurrent);
    assert_eq!(positive.coordinate(), TerminalPortCoordinate::Flow);
    assert_eq!(positive.port().timestamp().tick(), 17);
    assert_eq!(
        positive.port().coordinates().orientation(),
        PortOrientation::OutwardFromOwner
    );
    assert_eq!(
        positive.machine().authority_domain().as_str(),
        "org.frankensim.fs-ir.machine.graph.v1"
    );
    assert_eq!(
        positive.machine().flow_terminal().as_str(),
        "machine-terminal/a-positive-current"
    );
    assert_eq!(
        positive.trivialization().sign(),
        OrientationMapSign::Preserve
    );
}

#[test]
fn i13_2a_005_integral_and_real_objects_remain_nominally_distinct() {
    let pair = pair(23, false);
    let phase = PhaseId::new("phase/a").unwrap();
    let chain = IntegralRelativeChain::try_new(&pair, phase.clone(), 1, vec![3]).unwrap();
    let boundary = pair.boundary(&chain).expect("relative boundary");
    assert_eq!(boundary.degree(), 0);
    assert!(boundary.coefficients().is_empty());

    let representative =
        IntegralWindingRepresentative::try_new(&pair, phase.clone(), vec![3]).unwrap();
    assert_eq!(representative.chain().coefficients(), &[3]);
    let scaled_representative =
        IntegralWindingRepresentative::try_new(&pair, phase.clone(), vec![4]).unwrap();
    assert_ne!(representative.identity(), scaled_representative.identity());
    assert_ne!(
        representative.object_ref().identity(),
        scaled_representative.object_ref().identity()
    );

    let amplitude = RealCurrentAmplitude::try_new(
        PhysicalObjectId::new("object/current-amplitude").unwrap(),
        &pair,
        phase.clone(),
        Current::new(2.5),
    )
    .unwrap();
    assert_eq!(amplitude.value().value().to_bits(), 2.5_f64.to_bits());

    let current_cochain =
        RealRelativeCochain::try_new(&pair, phase.clone(), 1, Current::DIMS, vec![2.5]).unwrap();
    let distributed = DistributedCurrent::new(
        PhysicalObjectId::new("object/distributed-current").unwrap(),
        current_cochain,
        stable("receipt/divergence-v1"),
        stable("receipt/terminal-constraint-v1"),
    )
    .unwrap();
    assert_eq!(distributed.cochain().values(), &[2.5]);

    let coil = GeometricCoil::try_new(
        PhysicalObjectId::new("object/geometric-coil").unwrap(),
        &pair,
        phase,
        ConductorComponentId::new("component/winding").unwrap(),
        stable("artifact/connectivity-v1"),
        stable("artifact/manufacturing-v1"),
    )
    .unwrap();

    let winding_realization = DeclaredPhysicalMap::try_new(
        ConversionMapId::new("map/winding-realization").unwrap(),
        DeclaredPhysicalMapKind::WindingRealization,
        representative.object_ref(),
        coil.object_ref(),
        stable("artifact/winding-realization-v1"),
    )
    .unwrap();
    assert_ne!(
        winding_realization.source().kind(),
        winding_realization.target().kind()
    );

    let current_realization = DeclaredPhysicalMap::try_new(
        ConversionMapId::new("map/current-realization").unwrap(),
        DeclaredPhysicalMapKind::CurrentRealization,
        amplitude.object_ref(),
        distributed.object_ref(),
        stable("artifact/current-realization-v1"),
    )
    .unwrap();
    assert_ne!(
        current_realization.source().kind(),
        current_realization.target().kind()
    );
}

#[test]
fn i13_2a_006_real_coboundary_is_typed_and_nonfinite_values_refuse() {
    let pair = pair(29, false);
    let phase = PhaseId::new("phase/a").unwrap();
    let zero_form =
        RealRelativeCochain::try_new(&pair, phase.clone(), 0, Dims::NONE, Vec::new()).unwrap();
    let one_form = pair.coboundary(&zero_form).unwrap();
    assert_eq!(one_form.degree(), 1);
    assert_eq!(one_form.values(), &[0.0]);

    assert_eq!(
        RealRelativeCochain::try_new(&pair, phase, 1, Current::DIMS, vec![f64::NAN]),
        Err(TerminalRelativeError::NonFiniteRealCoefficient { index: 0 })
    );
}

#[test]
fn i13_2a_007_duplicate_incidence_and_wrong_trivialization_refuse() {
    let duplicate = BoundaryIncidence::new(
        CellRef::new(0, 0),
        CellRef::new(1, 0),
        IncidenceSign::Negative,
    );
    assert!(matches!(
        FiniteCellComplex::try_new(1, vec![2, 1], vec![duplicate, duplicate]),
        Err(TerminalRelativeError::DuplicateIncidence { .. })
    ));

    let complex = interval_complex();
    let port = electrical_port("port/mismatch", 1);
    assert!(matches!(
        PhysicalTerminal::new(
            PhysicalTerminalId::new("terminal/mismatch").unwrap(),
            subcomplex(&complex, "support/mismatch", [CellRef::new(0, 0)]),
            ConductorComponentId::new("component/winding").unwrap(),
            PhaseId::new("phase/a").unwrap(),
            TerminalRole::Driven,
            TerminalOrientation::OutOfConductor,
            TerminalPortCoordinate::Flow,
            port,
            PresentedMachinePortRef::try_new(
                stable("org.frankensim.fs-ir.machine.graph.v1"),
                1,
                [0x24; 32],
                stable("machine-owner/stator-winding"),
                stable("port/mismatch"),
                stable("machine-terminal/mismatch-voltage"),
                stable("machine-terminal/mismatch-current"),
            )
            .unwrap(),
            TerminalPortTrivialization::new(
                TrivializationId::new("trivialization/mismatch").unwrap(),
                stable("port/foreign"),
                OrientationMapSign::Preserve,
                stable("voltage-reference/zero"),
                stable("current-reference/positive"),
            ),
        ),
        Err(TerminalRelativeError::TrivializationPortMismatch { .. })
    ));

    let port = electrical_port("port/effort-selected", 1);
    let port_id = port.id().clone();
    assert!(matches!(
        PhysicalTerminal::new(
            PhysicalTerminalId::new("terminal/effort-selected").unwrap(),
            subcomplex(&complex, "support/effort-selected", [CellRef::new(0, 0)],),
            ConductorComponentId::new("component/winding").unwrap(),
            PhaseId::new("phase/a").unwrap(),
            TerminalRole::Driven,
            TerminalOrientation::OutOfConductor,
            TerminalPortCoordinate::Effort,
            port,
            PresentedMachinePortRef::try_new(
                stable("org.frankensim.fs-ir.machine.graph.v1"),
                1,
                [0x25; 32],
                stable("machine-owner/stator-winding"),
                stable("port/effort-selected"),
                stable("machine-terminal/effort-selected-voltage"),
                stable("machine-terminal/effort-selected-current"),
            )
            .unwrap(),
            TerminalPortTrivialization::new(
                TrivializationId::new("trivialization/effort-selected").unwrap(),
                port_id,
                OrientationMapSign::Preserve,
                stable("voltage-reference/zero"),
                stable("current-reference/positive"),
            ),
        ),
        Err(TerminalRelativeError::TerminalRequiresFlowCoordinate { .. })
    ));

    let port = electrical_port("port/orientation-mismatch", 1);
    let port_id = port.id().clone();
    assert!(matches!(
        PhysicalTerminal::new(
            PhysicalTerminalId::new("terminal/orientation-mismatch").unwrap(),
            subcomplex(
                &complex,
                "support/orientation-mismatch",
                [CellRef::new(0, 0)],
            ),
            ConductorComponentId::new("component/winding").unwrap(),
            PhaseId::new("phase/a").unwrap(),
            TerminalRole::Driven,
            TerminalOrientation::IntoConductor,
            TerminalPortCoordinate::Flow,
            port,
            PresentedMachinePortRef::try_new(
                stable("org.frankensim.fs-ir.machine.graph.v1"),
                1,
                [0x27; 32],
                stable("machine-owner/stator-winding"),
                stable("port/orientation-mismatch"),
                stable("machine-terminal/orientation-mismatch-voltage"),
                stable("machine-terminal/orientation-mismatch-current"),
            )
            .unwrap(),
            TerminalPortTrivialization::new(
                TrivializationId::new("trivialization/orientation-mismatch").unwrap(),
                port_id,
                OrientationMapSign::Preserve,
                stable("voltage-reference/zero"),
                stable("current-reference/positive"),
            ),
        ),
        Err(TerminalRelativeError::TerminalOrientationMismatch { .. })
    ));

    assert!(matches!(
        PresentedMachinePortRef::try_new(
            stable("org.frankensim.fs-ir.machine-graph.v1"),
            1,
            [0x26; 32],
            stable("machine-owner/stator-winding"),
            stable("port/wrong-domain"),
            stable("machine-terminal/wrong-domain-voltage"),
            stable("machine-terminal/wrong-domain-current"),
        ),
        Err(TerminalRelativeError::MachineGraphSchemaMismatch { .. })
    ));
    assert!(matches!(
        PresentedMachinePortRef::try_new(
            stable("org.frankensim.fs-ir.machine.graph.v1"),
            1,
            [0; 32],
            stable("machine-owner/stator-winding"),
            stable("port/zero-graph"),
            stable("machine-terminal/zero-graph-voltage"),
            stable("machine-terminal/zero-graph-current"),
        ),
        Err(TerminalRelativeError::ZeroMachineGraphDigest)
    ));
}

#[test]
fn i13_2a_008_relative_subcomplex_is_explicit_and_contained() {
    let complex = interval_complex();
    let conductor = subcomplex(&complex, "support/conductor-left", [CellRef::new(0, 0)]);
    let outside = subcomplex(&complex, "support/relative-outside", [CellRef::new(0, 1)]);
    assert!(matches!(
        TerminalRelativePair::try_new(
            complex.clone(),
            conductor,
            outside,
            subcomplex(&complex, "support/insulation-empty", []),
            Vec::new(),
            Vec::new(),
        ),
        Err(TerminalRelativeError::RelativeOutsideConductor { .. })
    ));

    let conductor = subcomplex(
        &complex,
        "support/conductor",
        [CellRef::new(0, 0), CellRef::new(0, 1), CellRef::new(1, 0)],
    );
    let component = ConductorComponent::new(
        ConductorComponentId::new("component/winding").unwrap(),
        conductor.clone(),
    )
    .unwrap();
    let terminals = vec![
        terminal(
            &complex,
            0,
            "a-positive",
            TerminalRole::Driven,
            TerminalOrientation::OutOfConductor,
            OrientationMapSign::Preserve,
            7,
        ),
        terminal(
            &complex,
            1,
            "a-return",
            TerminalRole::ReturnReference,
            TerminalOrientation::IntoConductor,
            OrientationMapSign::Reverse,
            7,
        ),
    ];
    assert!(matches!(
        TerminalRelativePair::try_new(
            complex.clone(),
            conductor,
            subcomplex(&complex, "support/relative-only-left", [CellRef::new(0, 0)],),
            subcomplex(&complex, "support/insulation-empty-2", []),
            vec![component],
            terminals,
        ),
        Err(TerminalRelativeError::TerminalOutsideRelativeSubcomplex { .. })
    ));
}

#[test]
fn i13_2a_009_components_must_be_full_dimensional_closures() {
    let complex = interval_complex();
    let conductor = subcomplex(
        &complex,
        "support/conductor",
        [CellRef::new(0, 0), CellRef::new(0, 1), CellRef::new(1, 0)],
    );
    let winding = ConductorComponent::new(
        ConductorComponentId::new("component/winding").unwrap(),
        conductor.clone(),
    )
    .unwrap();
    let ghost = ConductorComponent::new(
        ConductorComponentId::new("component/ghost").unwrap(),
        subcomplex(&complex, "support/ghost", [CellRef::new(0, 0)]),
    )
    .unwrap();
    let terminals = vec![
        terminal(
            &complex,
            0,
            "a-positive",
            TerminalRole::Driven,
            TerminalOrientation::OutOfConductor,
            OrientationMapSign::Preserve,
            7,
        ),
        terminal(
            &complex,
            1,
            "a-return",
            TerminalRole::ReturnReference,
            TerminalOrientation::IntoConductor,
            OrientationMapSign::Reverse,
            7,
        ),
    ];
    assert!(matches!(
        TerminalRelativePair::try_new(
            complex.clone(),
            conductor,
            subcomplex(
                &complex,
                "support/terminal-relative",
                [CellRef::new(0, 0), CellRef::new(0, 1)],
            ),
            subcomplex(&complex, "support/insulation-empty", []),
            vec![winding, ghost],
            terminals,
        ),
        Err(TerminalRelativeError::ComponentHasNoTopCell { .. })
    ));
}
