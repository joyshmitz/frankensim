//! G0/G3 fixtures for feature-gated I01.3 port equation lowering.

use fs_couple::{
    ConservationRole, CoordinateBinding, FieldMeasureSide, PortKind, PortOrientation, PortSchema,
    PortTimestamp, PortValueShape, PowerPairing, StableId,
};
use fs_iface::SpaceType;
use fs_opdsl::{
    AccountingTermKind, LossOwnershipId, OwnershipDisposition, PortDiscretization,
    PortEquationError, PortEquationSense, PortEquationSpec, SpatialSupport, SystemExpr,
    compile_port_equation, compile_port_equations,
};
use fs_qty::Dims;

const POWER: Dims = Dims([2, 1, -3, 0, 0, 0]);

fn stable(value: &str) -> StableId {
    StableId::new(value).expect("fixture stable id")
}

fn coordinates(orientation: PortOrientation) -> CoordinateBinding {
    CoordinateBinding::new(stable("basis-main"), stable("frame-lab"), orientation)
}

fn scalar_schema(port: &str, orientation: PortOrientation) -> PortSchema {
    PortKind::MechanicalForceVelocity
        .scalar_seed_schema(
            stable(port),
            coordinates(orientation),
            PortTimestamp::new(stable("clock-main"), 17),
        )
        .expect("admitted scalar port")
}

fn spec(
    port: &str,
    sense: PortEquationSense,
    term_kind: AccountingTermKind,
    ownership: OwnershipDisposition,
) -> PortEquationSpec {
    PortEquationSpec::new(
        scalar_schema(port, PortOrientation::OutwardFromOwner),
        PortDiscretization::lumped(),
        sense,
        term_kind,
        ownership,
    )
}

#[test]
fn g0_scalar_port_lowers_to_typed_power_equation_and_receipt() {
    let compiled = compile_port_equation(spec(
        "port-mechanical-a",
        PortEquationSense::AsDeclared,
        AccountingTermKind::Reversible,
        OwnershipDisposition::NotApplicable,
    ))
    .expect("neutral scalar schema lowers");

    assert_eq!(compiled.system().fields().len(), 3);
    assert_eq!(compiled.system().equations().len(), 1);
    assert!(matches!(
        &compiled.system().equations()[0].rhs,
        SystemExpr::PortPair {
            kind: PortKind::MechanicalForceVelocity,
            measure_dims,
            ..
        } if *measure_dims == Dims::NONE
    ));
    assert_eq!(compiled.receipt().port_id(), "port-mechanical-a");
    assert_eq!(compiled.receipt().product_dims(), POWER);
    assert_eq!(compiled.receipt().sign(), 1);
    assert_eq!(
        compiled.receipt().term_kind(),
        AccountingTermKind::Reversible
    );
    assert!(!compiled.system().extension().is_empty());

    let json = compiled.receipt().to_json();
    assert!(json.contains("\"schema\":\"fs-opdsl-port-equation-receipt-v1\""));
    assert!(json.contains("\"authority\":\"structural-generated\""));
    assert!(json.contains("\"product_dims\":[2,1,-3,0,0,0]"));
    assert!(json.contains("closed-window conservation remain external"));
}

#[test]
fn g3_orientation_reversal_is_an_explicit_identity_bearing_negative_sign() {
    let declared = compile_port_equation(spec(
        "port-orientation",
        PortEquationSense::AsDeclared,
        AccountingTermKind::Reversible,
        OwnershipDisposition::NotApplicable,
    ))
    .expect("declared orientation");
    let reversed = compile_port_equation(spec(
        "port-orientation",
        PortEquationSense::Reversed,
        AccountingTermKind::Reversible,
        OwnershipDisposition::NotApplicable,
    ))
    .expect("reversed orientation");

    assert_ne!(
        declared.receipt().system_identity(),
        reversed.receipt().system_identity(),
        "orientation sense is semantic, not display metadata"
    );
    assert_eq!(reversed.receipt().sign(), -1);
    assert!(matches!(
        &reversed.system().equations()[0].rhs,
        SystemExpr::Scale(value, inner)
            if value.to_bits() == (-1.0f64).to_bits()
                && matches!(inner.as_ref(), SystemExpr::PortPair { .. })
    ));
}

#[test]
fn g0_field_duality_retains_space_roles_measure_and_component_shape() {
    let area = Dims([2, 0, 0, 0, 0, 0]);
    let kind = PortKind::MechanicalForceVelocity;
    let pointwise_effort = kind
        .canonical_effort_dimensions()
        .checked_minus(area)
        .expect("traction dimensions");
    let shape =
        PortValueShape::field(3, SpaceType::HGrad, SpaceType::HDiv).expect("nonempty field shape");
    let schema = PortSchema::try_new(
        stable("port-field"),
        kind,
        pointwise_effort,
        kind.canonical_flow_dimensions(),
        shape,
        coordinates(PortOrientation::OutwardFromOwner),
        PowerPairing::FieldDuality {
            measure_dimensions: area,
            measure_side: FieldMeasureSide::Effort,
        },
        PortTimestamp::new(stable("clock-main"), 23),
        [ConservationRole::Energy],
    )
    .expect("field duality schema");
    let compiled = compile_port_equation(PortEquationSpec::new(
        schema.clone(),
        PortDiscretization::field(12, 18).expect("nonempty dofs"),
        PortEquationSense::AsDeclared,
        AccountingTermKind::Storage,
        OwnershipDisposition::Owned(stable("storage-owner")),
    ))
    .expect("field schema lowers");

    assert_eq!(compiled.system().fields()[0].space.degree, 0);
    assert_eq!(compiled.system().fields()[0].space.n, 12);
    assert_eq!(compiled.system().fields()[1].space.degree, 2);
    assert_eq!(compiled.system().fields()[1].space.n, 18);
    assert!(
        compiled
            .system()
            .fields()
            .iter()
            .all(|field| field.support == SpatialSupport::BoundaryTrace)
    );
    assert!(matches!(
        &compiled.system().equations()[0].rhs,
        SystemExpr::PortPair {
            measure_dims,
            ..
        } if *measure_dims == area
    ));

    assert!(matches!(
        compile_port_equation(PortEquationSpec::new(
            schema.clone(),
            PortDiscretization::lumped(),
            PortEquationSense::AsDeclared,
            AccountingTermKind::Storage,
            OwnershipDisposition::Owned(stable("owner-two")),
        )),
        Err(PortEquationError::DiscretizationMismatch {
            expected: "field",
            actual: "lumped",
        })
    ));
    assert!(matches!(
        compile_port_equation(PortEquationSpec::new(
            schema,
            PortDiscretization::field(10, 18).expect("nonempty dofs"),
            PortEquationSense::AsDeclared,
            AccountingTermKind::Storage,
            OwnershipDisposition::Owned(stable("owner-three")),
        )),
        Err(PortEquationError::FieldComponentMismatch {
            variable: "effort",
            dofs: 10,
            components: 3,
        })
    ));
}

#[test]
fn g0_ownership_is_role_checked_and_unique_across_a_batch() {
    let duplicate_port = spec(
        "port-duplicate",
        PortEquationSense::AsDeclared,
        AccountingTermKind::Reversible,
        OwnershipDisposition::NotApplicable,
    );
    assert_eq!(
        compile_port_equations(vec![duplicate_port.clone(), duplicate_port])
            .expect_err("duplicate source identity must refuse"),
        PortEquationError::DuplicatePortId {
            port: "port-duplicate".to_string(),
        }
    );

    assert!(matches!(
        compile_port_equation(spec(
            "port-bad-reversible",
            PortEquationSense::AsDeclared,
            AccountingTermKind::Reversible,
            OwnershipDisposition::Owned(stable("impossible-owner")),
        )),
        Err(PortEquationError::OwnershipMismatch {
            term_kind: AccountingTermKind::Reversible,
            ..
        })
    ));
    assert!(matches!(
        compile_port_equation(spec(
            "port-bad-storage",
            PortEquationSense::AsDeclared,
            AccountingTermKind::Storage,
            OwnershipDisposition::ExplicitlyUnowned {
                rationale: stable("missing-storage-model"),
            },
        )),
        Err(PortEquationError::OwnershipMismatch {
            term_kind: AccountingTermKind::Storage,
            ..
        })
    ));

    let owned_loss = compile_port_equation(spec(
        "port-owned-loss",
        PortEquationSense::AsDeclared,
        AccountingTermKind::Dissipation,
        OwnershipDisposition::Owned(stable("loss-owner")),
    ))
    .expect("concrete loss ownership");
    let reversed_loss = compile_port_equation(spec(
        "port-owned-loss",
        PortEquationSense::Reversed,
        AccountingTermKind::Dissipation,
        OwnershipDisposition::Owned(stable("loss-owner")),
    ))
    .expect("orientation does not change physical loss ownership");
    let loss_id = owned_loss
        .receipt()
        .loss_ownership_id()
        .expect("owned loss mints a nominal identity");
    assert_eq!(Some(loss_id), reversed_loss.receipt().loss_ownership_id());
    assert_eq!(loss_id.to_hex().len(), 64);
    assert_eq!(LossOwnershipId::parse_hex(&loss_id.to_hex()), Some(loss_id));
    assert!(owned_loss.receipt().to_json().contains(&loss_id.to_hex()));

    let duplicate_owner = stable("shared-loss-owner");
    let error = compile_port_equations(vec![
        spec(
            "port-source",
            PortEquationSense::AsDeclared,
            AccountingTermKind::Source,
            OwnershipDisposition::Owned(duplicate_owner.clone()),
        ),
        spec(
            "port-dissipation",
            PortEquationSense::AsDeclared,
            AccountingTermKind::Dissipation,
            OwnershipDisposition::Owned(duplicate_owner),
        ),
    ])
    .expect_err("one owner cannot own two generated terms");
    assert!(matches!(
        error,
        PortEquationError::DuplicateOwnership {
            ref owner,
            ref first_port,
            ref second_port,
        } if owner == "shared-loss-owner"
            && first_port == "port-dissipation"
            && second_port == "port-source"
    ));
}

#[test]
fn g3_batch_order_is_canonical_and_explicit_unowned_loss_is_retained() {
    let source = spec(
        "port-z-source",
        PortEquationSense::AsDeclared,
        AccountingTermKind::Source,
        OwnershipDisposition::Owned(stable("source-owner")),
    );
    let loss = spec(
        "port-a-loss",
        PortEquationSense::Reversed,
        AccountingTermKind::Dissipation,
        OwnershipDisposition::ExplicitlyUnowned {
            rationale: stable("outside-model-scope"),
        },
    );
    let forward = compile_port_equations(vec![source.clone(), loss.clone()])
        .expect("forward declaration order");
    let reverse = compile_port_equations(vec![loss, source]).expect("reverse declaration order");

    let forward_ids: Vec<_> = forward
        .equations()
        .iter()
        .map(|equation| equation.receipt().system_identity())
        .collect();
    let reverse_ids: Vec<_> = reverse
        .equations()
        .iter()
        .map(|equation| equation.receipt().system_identity())
        .collect();
    assert_eq!(forward_ids, reverse_ids);
    assert_eq!(forward.equations()[0].receipt().port_id(), "port-a-loss");
    assert!(
        forward.equations()[0]
            .receipt()
            .to_json()
            .contains("explicitly-unowned:outside-model-scope")
    );
    assert_eq!(forward.equations()[0].receipt().loss_ownership_id(), None);
}

#[test]
fn g0_empty_shape_and_metadata_resource_bombs_refuse_before_generation() {
    assert_eq!(
        PortDiscretization::field(0, 1),
        Err(PortEquationError::ZeroFieldDofs { variable: "effort" })
    );
    assert!(matches!(
        compile_port_equations(Vec::new()),
        Err(PortEquationError::EmptyBatch)
    ));

    let oversized_id = "a".repeat(5_000);
    let oversized = PortKind::MechanicalForceVelocity
        .scalar_seed_schema(
            StableId::new(oversized_id).expect("upstream stable ids are not length capped"),
            coordinates(PortOrientation::OutwardFromOwner),
            PortTimestamp::new(stable("clock-main"), 29),
        )
        .expect("upstream schema admits the identifier");
    assert!(matches!(
        compile_port_equation(PortEquationSpec::new(
            oversized,
            PortDiscretization::lumped(),
            PortEquationSense::AsDeclared,
            AccountingTermKind::Reversible,
            OwnershipDisposition::NotApplicable,
        )),
        Err(PortEquationError::CompilerMetadataTooLarge { .. })
    ));
}
