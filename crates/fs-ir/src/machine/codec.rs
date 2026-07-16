//! Canonical FrankenScript AST transport for the admitted Machine graph.
//!
//! This module owns syntax only. It decodes one explicit, versioned AST into
//! an authority-free [`MachineGraphDraft`], then delegates every semantic
//! decision to the existing graph admission boundary. The codec never treats
//! collection position as identity and never publishes an admitted identity
//! on a syntax or graph refusal.

use core::fmt;
use core::num::NonZeroU64;

use fs_qty::Dims;
use fs_qty::semantic::{
    AngleDomain, CompositionBasis, QuantityKind, SemanticType, StrainBasis, StrainComponent,
    ValueForm,
};

use crate::VersionedProgram;
use crate::ast::{Node, NodeKind, Span};

use super::{
    AdmittedMachineGraph, BodyId, ClockId, ClockSpec, ContactFeatureId, FrameBinding,
    InterfaceBinding, InterfaceCardRef, InterfaceId, InterfaceOrientation,
    MAX_MACHINE_GRAPH_CLOCKS, MAX_MACHINE_GRAPH_INTERFACES, MAX_MACHINE_GRAPH_MATERIALS,
    MAX_MACHINE_GRAPH_OWNED_ELEMENTS, MAX_MACHINE_GRAPH_PORTS, MAX_MACHINE_GRAPH_RELATIONS,
    MAX_MACHINE_GRAPH_SUBSYSTEMS, MAX_MACHINE_GRAPH_TERMINALS, MachineClock, MachineGraphDraft,
    MachineGraphRefusal, MachineIdError, MachineReferenceError, MaterialBinding, MaterialCardRef,
    MaterialTarget, ModelRef, OrientationParity, PortEnergyRole, PortId, PortSpec, RelationId,
    RelationMode, RelationSpec, SolvePolicyRef, StateSlotId, SubsystemId, SubsystemSpec,
    SurfacePatchId, TerminalCausality, TerminalId, TerminalQuantitySpec, TerminalShape,
    TerminalSpec,
};

/// Version of the canonical Machine-graph FrankenScript form.
pub const MACHINE_GRAPH_AST_SCHEMA_VERSION_V1: u32 = 1;
/// Root symbol for the canonical Machine-graph FrankenScript form.
pub const MACHINE_GRAPH_AST_HEAD_V1: &str = "machine-graph-v1";
/// Maximum total generic AST nodes inspected by one Machine-graph decode.
pub const MAX_MACHINE_GRAPH_AST_NODES: usize = 262_144;
/// Maximum aggregate string/symbol/keyword/quantity-text bytes in one decode.
pub const MAX_MACHINE_GRAPH_AST_TEXT_BYTES: usize = 16 * 1_024 * 1_024;

/// Closed syntax/resource refusal vocabulary for the Machine-graph codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum MachineGraphCodecRule {
    /// The caller supplied a forged or otherwise invalid generic AST.
    InvalidAst = 1,
    /// A list had the wrong head, arity, or atom kind.
    UnexpectedForm = 2,
    /// A closed enum/tag token was unknown.
    UnknownTag = 3,
    /// A numeric atom was noncanonical, out of range, or zero when forbidden.
    InvalidNumber = 4,
    /// A durable Machine identifier was not a canonical role-specific key.
    InvalidIdentifier = 5,
    /// An opaque external reference was malformed or all-zero.
    InvalidReference = 6,
    /// A public collection or aggregate owned-element bound was exceeded.
    ResourceLimit = 7,
}

impl MachineGraphCodecRule {
    /// Stable structured diagnostic code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidAst => "MachineGraphCodecInvalidAst",
            Self::UnexpectedForm => "MachineGraphCodecUnexpectedForm",
            Self::UnknownTag => "MachineGraphCodecUnknownTag",
            Self::InvalidNumber => "MachineGraphCodecInvalidNumber",
            Self::InvalidIdentifier => "MachineGraphCodecInvalidIdentifier",
            Self::InvalidReference => "MachineGraphCodecInvalidReference",
            Self::ResourceLimit => "MachineGraphCodecResourceLimit",
        }
    }
}

/// One bounded, path-addressed Machine-graph syntax refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineGraphCodecError {
    rule: MachineGraphCodecRule,
    span: Span,
    path: Box<str>,
    detail: Box<str>,
    hint: Box<str>,
}

impl MachineGraphCodecError {
    /// Closed refusal rule.
    #[must_use]
    pub const fn rule(&self) -> MachineGraphCodecRule {
        self.rule
    }

    /// Stable structured diagnostic code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.rule.code()
    }

    /// Exact source span of the offending AST node.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    /// Deterministic structural path within the Machine form.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Human-readable refusal detail.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Actionable correction hint.
    #[must_use]
    pub fn hint(&self) -> &str {
        &self.hint
    }
}

impl fmt::Display for MachineGraphCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at {} (bytes {}..{}): {}; hint: {}",
            self.code(),
            self.path,
            self.span.start,
            self.span.end,
            self.detail,
            self.hint
        )
    }
}

impl std::error::Error for MachineGraphCodecError {}

/// Distinguishes syntax refusal from semantic Machine-graph refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineGraphAstAdmissionError {
    /// The generic AST could not be decoded as the v1 Machine graph grammar.
    Codec(MachineGraphCodecError),
    /// The decoded authority-free draft failed the existing graph admission.
    Graph(MachineGraphRefusal),
}

impl MachineGraphAstAdmissionError {
    /// Stable top-level refusal code without collapsing syntax and semantics.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Codec(error) => error.code(),
            Self::Graph(_) => "MachineGraphRefused",
        }
    }
}

impl fmt::Display for MachineGraphAstAdmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(error) => error.fmt(f),
            Self::Graph(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for MachineGraphAstAdmissionError {}

impl From<MachineGraphCodecError> for MachineGraphAstAdmissionError {
    fn from(value: MachineGraphCodecError) -> Self {
        Self::Codec(value)
    }
}

/// Decode one canonical v1 Machine-graph AST into an authority-free draft.
///
/// The root grammar is positional and closed:
///
/// ```text
/// (machine-graph-v1
///   (clocks ...)
///   (subsystems ...)
///   (terminals ...)
///   (ports ...)
///   (relations ...)
///   (materials ...)
///   (interfaces ...))
/// ```
///
/// Integer-sized semantic fields are encoded as canonical decimal strings so
/// the syntax can retain the complete `u64` domain instead of inheriting the
/// generic AST's signed-integer limit.
///
/// # Errors
/// Refuses malformed ASTs, unknown closed tags, noncanonical numbers or
/// digests, invalid role-specific IDs/references, and oversized collections.
pub fn parse_machine_graph_v1(node: &Node) -> Result<MachineGraphDraft, MachineGraphCodecError> {
    preflight_ast(node)?;
    preflight_graph_caps(node)?;
    node.validate().map_err(|error| MachineGraphCodecError {
        rule: MachineGraphCodecRule::InvalidAst,
        span: error.span,
        path: "$".into(),
        detail: error.to_string().into_boxed_str(),
        hint: "repair the generic FrankenScript AST before Machine decoding"
            .to_string()
            .into_boxed_str(),
    })?;

    let root = exact_form(node, MACHINE_GRAPH_AST_HEAD_V1, 7, "$")?;
    let clocks = section_items(&root[0], "clocks", MAX_MACHINE_GRAPH_CLOCKS, "$[1]")?;
    let subsystems = section_items(&root[1], "subsystems", MAX_MACHINE_GRAPH_SUBSYSTEMS, "$[2]")?;
    let terminals = section_items(&root[2], "terminals", MAX_MACHINE_GRAPH_TERMINALS, "$[3]")?;
    let ports = section_items(&root[3], "ports", MAX_MACHINE_GRAPH_PORTS, "$[4]")?;
    let relations = section_items(&root[4], "relations", MAX_MACHINE_GRAPH_RELATIONS, "$[5]")?;
    let materials = section_items(&root[5], "materials", MAX_MACHINE_GRAPH_MATERIALS, "$[6]")?;
    let interfaces = section_items(&root[6], "interfaces", MAX_MACHINE_GRAPH_INTERFACES, "$[7]")?;

    let mut decoded_clocks = reserved_vec(clocks.len(), &root[0], "$[1]")?;
    for (index, item) in clocks.iter().enumerate() {
        decoded_clocks.push(parse_clock(item, &format!("$[1][{}]", index + 1))?);
    }

    let mut owned_elements = 0_usize;
    let mut decoded_subsystems = reserved_vec(subsystems.len(), &root[1], "$[2]")?;
    for (index, item) in subsystems.iter().enumerate() {
        decoded_subsystems.push(parse_subsystem(
            item,
            &format!("$[2][{}]", index + 1),
            &mut owned_elements,
        )?);
    }

    let mut decoded_terminals = reserved_vec(terminals.len(), &root[2], "$[3]")?;
    for (index, item) in terminals.iter().enumerate() {
        decoded_terminals.push(parse_terminal(item, &format!("$[3][{}]", index + 1))?);
    }

    let mut decoded_ports = reserved_vec(ports.len(), &root[3], "$[4]")?;
    for (index, item) in ports.iter().enumerate() {
        decoded_ports.push(parse_port(item, &format!("$[4][{}]", index + 1))?);
    }

    let mut decoded_relations = reserved_vec(relations.len(), &root[4], "$[5]")?;
    for (index, item) in relations.iter().enumerate() {
        decoded_relations.push(parse_relation(item, &format!("$[5][{}]", index + 1))?);
    }

    let mut decoded_materials = reserved_vec(materials.len(), &root[5], "$[6]")?;
    for (index, item) in materials.iter().enumerate() {
        decoded_materials.push(parse_material(item, &format!("$[6][{}]", index + 1))?);
    }

    let mut decoded_interfaces = reserved_vec(interfaces.len(), &root[6], "$[7]")?;
    for (index, item) in interfaces.iter().enumerate() {
        decoded_interfaces.push(parse_interface(item, &format!("$[7][{}]", index + 1))?);
    }

    Ok(MachineGraphDraft {
        clocks: decoded_clocks,
        subsystems: decoded_subsystems,
        terminals: decoded_terminals,
        ports: decoded_ports,
        relations: decoded_relations,
        materials: decoded_materials,
        interfaces: decoded_interfaces,
    })
}

/// Decode the program body of a version-enforced FrankenScript artifact.
///
/// The outer [`VersionedProgram`] has already enforced the global IR version;
/// this boundary separately enforces the local Machine graph v1 grammar.
///
/// # Errors
/// Returns the same bounded syntax/resource refusals as
/// [`parse_machine_graph_v1`].
pub fn parse_machine_graph_program_v1(
    program: &VersionedProgram,
) -> Result<MachineGraphDraft, MachineGraphCodecError> {
    parse_machine_graph_v1(program.program())
}

/// Decode and semantically admit one v1 Machine-graph AST.
///
/// # Errors
/// [`MachineGraphAstAdmissionError::Codec`] names syntax/resource refusal;
/// [`MachineGraphAstAdmissionError::Graph`] retains the complete deterministic
/// finding set from the existing semantic admission boundary.
pub fn admit_machine_graph_ast_v1(
    node: &Node,
) -> Result<AdmittedMachineGraph, MachineGraphAstAdmissionError> {
    parse_machine_graph_v1(node)?
        .admit()
        .map_err(MachineGraphAstAdmissionError::Graph)
}

/// Encode one admitted graph as the canonical v1 Machine-graph AST.
///
/// # Errors
/// Returns a structured internal-boundary error if an admitted value cannot be
/// represented by the declared grammar or the synthesized AST fails its
/// generic invariants.
pub fn write_machine_graph_v1(
    graph: &AdmittedMachineGraph,
) -> Result<Node, MachineGraphCodecError> {
    let root = form(
        MACHINE_GRAPH_AST_HEAD_V1,
        vec![
            section("clocks", graph.clocks().iter().map(write_clock).collect()),
            section(
                "subsystems",
                graph.subsystems().iter().map(write_subsystem).collect(),
            ),
            section(
                "terminals",
                graph.terminals().iter().map(write_terminal).collect(),
            ),
            section("ports", graph.ports().iter().map(write_port).collect()),
            section(
                "relations",
                graph.relations().iter().map(write_relation).collect(),
            ),
            section(
                "materials",
                graph.materials().iter().map(write_material).collect(),
            ),
            section(
                "interfaces",
                graph.interfaces().iter().map(write_interface).collect(),
            ),
        ],
    );
    root.validate().map_err(|error| MachineGraphCodecError {
        rule: MachineGraphCodecRule::InvalidAst,
        span: error.span,
        path: "$".into(),
        detail: format!("canonical Machine graph writer produced an invalid AST: {error}")
            .into_boxed_str(),
        hint: "treat this as a codec implementation defect"
            .to_string()
            .into_boxed_str(),
    })?;
    Ok(root)
}

/// Encode one admitted graph into a current-version FrankenScript artifact.
///
/// # Errors
/// Refuses any internal representation defect rather than emitting an
/// unversioned or structurally invalid artifact.
pub fn write_machine_graph_program_v1(
    graph: &AdmittedMachineGraph,
) -> Result<VersionedProgram, MachineGraphCodecError> {
    let node = write_machine_graph_v1(graph)?;
    VersionedProgram::try_current(node).map_err(|error| MachineGraphCodecError {
        rule: MachineGraphCodecRule::InvalidAst,
        span: error.span,
        path: "$".into(),
        detail: format!("canonical Machine graph could not enter the IR envelope: {error}")
            .into_boxed_str(),
        hint: "treat this as a codec implementation defect"
            .to_string()
            .into_boxed_str(),
    })
}

fn preflight_ast(node: &Node) -> Result<(), MachineGraphCodecError> {
    let mut stack = Vec::new();
    stack
        .try_reserve_exact(256)
        .map_err(|_| resource_error(node, "$", "AST preflight stack allocation was refused"))?;
    stack.push(node);
    let mut visited = 0_usize;
    let mut text_bytes = 0_usize;
    while let Some(current) = stack.pop() {
        visited = visited
            .checked_add(1)
            .ok_or_else(|| resource_error(current, "$", "AST node count overflowed usize"))?;
        if visited > MAX_MACHINE_GRAPH_AST_NODES {
            return Err(resource_error(
                current,
                "$",
                &format!(
                    "AST node count exceeds cap {MAX_MACHINE_GRAPH_AST_NODES} before semantic decoding"
                ),
            ));
        }
        let added_text = match &current.kind {
            NodeKind::Qty { text, .. }
            | NodeKind::Str(text)
            | NodeKind::Symbol(text)
            | NodeKind::Keyword(text) => text.len(),
            NodeKind::Int(_)
            | NodeKind::Float(_)
            | NodeKind::Count { .. }
            | NodeKind::Seed(_)
            | NodeKind::List(_) => 0,
        };
        text_bytes = text_bytes
            .checked_add(added_text)
            .ok_or_else(|| resource_error(current, "$", "AST text byte count overflowed usize"))?;
        if text_bytes > MAX_MACHINE_GRAPH_AST_TEXT_BYTES {
            return Err(resource_error(
                current,
                "$",
                &format!(
                    "AST text bytes exceed cap {MAX_MACHINE_GRAPH_AST_TEXT_BYTES} before semantic decoding"
                ),
            ));
        }
        if let NodeKind::List(items) = &current.kind {
            let projected = visited
                .checked_add(stack.len())
                .and_then(|count| count.checked_add(items.len()))
                .ok_or_else(|| resource_error(current, "$", "AST node count overflowed usize"))?;
            if projected > MAX_MACHINE_GRAPH_AST_NODES {
                return Err(resource_error(
                    current,
                    "$",
                    &format!(
                        "AST node count exceeds cap {MAX_MACHINE_GRAPH_AST_NODES} before semantic decoding"
                    ),
                ));
            }
            stack.try_reserve(items.len()).map_err(|_| {
                resource_error(current, "$", "AST preflight stack growth was refused")
            })?;
            stack.extend(items);
        }
    }
    Ok(())
}

fn preflight_graph_caps(node: &Node) -> Result<(), MachineGraphCodecError> {
    let NodeKind::List(root) = &node.kind else {
        return Ok(());
    };
    if !matches!(root.first().map(|item| &item.kind), Some(NodeKind::Symbol(head)) if head == MACHINE_GRAPH_AST_HEAD_V1)
    {
        return Ok(());
    }

    let sections = [
        (1_usize, "clocks", MAX_MACHINE_GRAPH_CLOCKS),
        (2, "subsystems", MAX_MACHINE_GRAPH_SUBSYSTEMS),
        (3, "terminals", MAX_MACHINE_GRAPH_TERMINALS),
        (4, "ports", MAX_MACHINE_GRAPH_PORTS),
        (5, "relations", MAX_MACHINE_GRAPH_RELATIONS),
        (6, "materials", MAX_MACHINE_GRAPH_MATERIALS),
        (7, "interfaces", MAX_MACHINE_GRAPH_INTERFACES),
    ];
    for (index, head, cap) in sections {
        let Some(section) = root.get(index) else {
            continue;
        };
        let Some(items) = recognized_form_items(section, head) else {
            continue;
        };
        if items.len() > cap {
            return Err(resource_error(
                section,
                &format!("$[{index}]"),
                &format!(
                    "section {head} contains {} entries, above cap {cap}",
                    items.len()
                ),
            ));
        }
    }

    let Some(subsystems) = root
        .get(2)
        .and_then(|section| recognized_form_items(section, "subsystems"))
    else {
        return Ok(());
    };
    let mut owned = 0_usize;
    for (subsystem_index, subsystem) in subsystems.iter().enumerate() {
        let Some(fields) = recognized_form_items(subsystem, "subsystem") else {
            continue;
        };
        for (field_index, head) in [
            (2_usize, "bodies"),
            (3, "surface-patches"),
            (4, "contact-features"),
            (5, "state-slots"),
        ] {
            let Some(field) = fields.get(field_index) else {
                continue;
            };
            let Some(items) = recognized_form_items(field, head) else {
                continue;
            };
            owned = owned.checked_add(items.len()).ok_or_else(|| {
                resource_error(
                    field,
                    "$[2]",
                    "aggregate owned-element count overflowed usize",
                )
            })?;
            if owned > MAX_MACHINE_GRAPH_OWNED_ELEMENTS {
                return Err(resource_error(
                    field,
                    &format!("$[2][{}][{}]", subsystem_index + 1, field_index + 1),
                    &format!(
                        "aggregate owned-element count {owned} exceeds cap {MAX_MACHINE_GRAPH_OWNED_ELEMENTS}"
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn recognized_form_items<'a>(node: &'a Node, expected_head: &str) -> Option<&'a [Node]> {
    let NodeKind::List(items) = &node.kind else {
        return None;
    };
    if matches!(items.first().map(|item| &item.kind), Some(NodeKind::Symbol(head)) if head == expected_head)
    {
        Some(&items[1..])
    } else {
        None
    }
}

fn parse_clock(node: &Node, path: &str) -> Result<ClockSpec, MachineGraphCodecError> {
    let args = exact_form(node, "clock", 2, path)?;
    Ok(ClockSpec {
        id: parse_id(&args[0], "clock-id", &format!("{path}[1]"), |key| {
            ClockId::new(key)
        })?,
        clock: parse_clock_mode(&args[1], &format!("{path}[2]"))?,
    })
}

fn parse_clock_mode(node: &Node, path: &str) -> Result<MachineClock, MachineGraphCodecError> {
    let (head, args) = raw_form(node, path)?;
    match head {
        "continuous" => {
            require_arity(node, args, 0, path)?;
            Ok(MachineClock::Continuous)
        }
        "event-driven" => {
            require_arity(node, args, 0, path)?;
            Ok(MachineClock::EventDriven)
        }
        "periodic" => {
            require_arity(node, args, 2, path)?;
            Ok(MachineClock::Periodic {
                period_ns: parse_nonzero_u64(&args[0], &format!("{path}[1]"))?,
                phase_ns: parse_u64(&args[1], &format!("{path}[2]"))?,
            })
        }
        _ => Err(unknown_tag(node, path, "clock mode", head)),
    }
}

fn parse_subsystem(
    node: &Node,
    path: &str,
    owned_elements: &mut usize,
) -> Result<SubsystemSpec, MachineGraphCodecError> {
    let args = exact_form(node, "subsystem", 6, path)?;
    Ok(SubsystemSpec {
        id: parse_id(&args[0], "subsystem-id", &format!("{path}[1]"), |key| {
            SubsystemId::new(key)
        })?,
        model: parse_reference(
            &args[1],
            "model-ref",
            &format!("{path}[2]"),
            |namespace, version, digest| ModelRef::new(namespace, version, digest),
        )?,
        bodies: parse_owned_ids(
            &args[2],
            "bodies",
            "body-id",
            &format!("{path}[3]"),
            owned_elements,
            |key| BodyId::new(key),
        )?,
        surface_patches: parse_owned_ids(
            &args[3],
            "surface-patches",
            "surface-patch-id",
            &format!("{path}[4]"),
            owned_elements,
            |key| SurfacePatchId::new(key),
        )?,
        contact_features: parse_owned_ids(
            &args[4],
            "contact-features",
            "contact-feature-id",
            &format!("{path}[5]"),
            owned_elements,
            |key| ContactFeatureId::new(key),
        )?,
        state_slots: parse_owned_ids(
            &args[5],
            "state-slots",
            "state-slot-id",
            &format!("{path}[6]"),
            owned_elements,
            |key| StateSlotId::new(key),
        )?,
    })
}

fn parse_terminal(node: &Node, path: &str) -> Result<TerminalSpec, MachineGraphCodecError> {
    let args = exact_form(node, "terminal", 7, path)?;
    Ok(TerminalSpec {
        id: parse_id(&args[0], "terminal-id", &format!("{path}[1]"), |key| {
            TerminalId::new(key)
        })?,
        owner: parse_id(&args[1], "subsystem-id", &format!("{path}[2]"), |key| {
            SubsystemId::new(key)
        })?,
        quantity: parse_quantity(&args[2], &format!("{path}[3]"))?,
        shape: parse_shape(&args[3], &format!("{path}[4]"))?,
        causality: parse_causality(&args[4], &format!("{path}[5]"))?,
        clock: parse_id(&args[5], "clock-id", &format!("{path}[6]"), |key| {
            ClockId::new(key)
        })?,
        frame: parse_frame(&args[6], &format!("{path}[7]"))?,
    })
}

fn parse_quantity(node: &Node, path: &str) -> Result<TerminalQuantitySpec, MachineGraphCodecError> {
    let (head, args) = raw_form(node, path)?;
    match head {
        "dims" => {
            require_arity(node, args, 6, path)?;
            let mut dims = [0_i8; 6];
            for (index, value) in args.iter().enumerate() {
                let NodeKind::Int(exponent) = &value.kind else {
                    return Err(unexpected(
                        value,
                        &format!("{path}[{}]", index + 1),
                        "dimension exponent must be an integer atom",
                        "use six signed i8 exponents in [m, kg, s, K, A, mol] order",
                    ));
                };
                dims[index] = i8::try_from(*exponent).map_err(|_| {
                    number_error(
                        &args[index],
                        &format!("{path}[{}]", index + 1),
                        "dimension exponent is outside i8",
                    )
                })?;
            }
            Ok(TerminalQuantitySpec::Dimensional(Dims(dims)))
        }
        "semantic" => {
            require_arity(node, args, 2, path)?;
            Ok(TerminalQuantitySpec::Semantic(SemanticType::new(
                parse_quantity_kind(&args[0], &format!("{path}[1]"))?,
                parse_value_form(&args[1], &format!("{path}[2]"))?,
            )))
        }
        _ => Err(unknown_tag(node, path, "terminal quantity", head)),
    }
}

fn parse_quantity_kind(node: &Node, path: &str) -> Result<QuantityKind, MachineGraphCodecError> {
    let (head, args) = raw_form(node, path)?;
    let simple = match head {
        "absolute-temperature" => Some(QuantityKind::AbsoluteTemperature),
        "temperature-difference" => Some(QuantityKind::TemperatureDifference),
        "torque" => Some(QuantityKind::Torque),
        "energy" => Some(QuantityKind::Energy),
        "pressure" => Some(QuantityKind::Pressure),
        "stress" => Some(QuantityKind::Stress),
        "mass" => Some(QuantityKind::Mass),
        "amount" => Some(QuantityKind::Amount),
        "molar-mass" => Some(QuantityKind::MolarMass),
        "mass-concentration" => Some(QuantityKind::MassConcentration),
        "amount-concentration" => Some(QuantityKind::AmountConcentration),
        "entropy" => Some(QuantityKind::Entropy),
        "heat-capacity" => Some(QuantityKind::HeatCapacity),
        "acoustic-pressure" => Some(QuantityKind::AcousticPressure),
        "acoustic-power" => Some(QuantityKind::AcousticPower),
        _ => None,
    };
    if let Some(kind) = simple {
        require_arity(node, args, 0, path)?;
        return Ok(kind);
    }
    match head {
        "angle" => {
            require_arity(node, args, 1, path)?;
            Ok(QuantityKind::Angle(parse_angle_domain(
                &args[0],
                &format!("{path}[1]"),
            )?))
        }
        "angular-velocity" => {
            require_arity(node, args, 1, path)?;
            Ok(QuantityKind::AngularVelocity(parse_angle_domain(
                &args[0],
                &format!("{path}[1]"),
            )?))
        }
        "strain" => {
            require_arity(node, args, 2, path)?;
            Ok(QuantityKind::Strain {
                basis: match symbol(&args[0], &format!("{path}[1]"))? {
                    "tensor" => StrainBasis::Tensor,
                    "engineering" => StrainBasis::Engineering,
                    value => {
                        return Err(unknown_tag(
                            &args[0],
                            &format!("{path}[1]"),
                            "strain basis",
                            value,
                        ));
                    }
                },
                component: match symbol(&args[1], &format!("{path}[2]"))? {
                    "normal" => StrainComponent::Normal,
                    "shear" => StrainComponent::Shear,
                    value => {
                        return Err(unknown_tag(
                            &args[1],
                            &format!("{path}[2]"),
                            "strain component",
                            value,
                        ));
                    }
                },
            })
        }
        "composition" => {
            require_arity(node, args, 1, path)?;
            let basis = match symbol(&args[0], &format!("{path}[1]"))? {
                "mass-fraction" => CompositionBasis::MassFraction,
                "mole-fraction" => CompositionBasis::MoleFraction,
                "volume-fraction" => CompositionBasis::VolumeFraction,
                value => {
                    return Err(unknown_tag(
                        &args[0],
                        &format!("{path}[1]"),
                        "composition basis",
                        value,
                    ));
                }
            };
            Ok(QuantityKind::Composition(basis))
        }
        _ => Err(unknown_tag(node, path, "semantic quantity kind", head)),
    }
}

fn parse_angle_domain(node: &Node, path: &str) -> Result<AngleDomain, MachineGraphCodecError> {
    match symbol(node, path)? {
        "mechanical" => Ok(AngleDomain::Mechanical),
        "electrical" => Ok(AngleDomain::Electrical),
        value => Err(unknown_tag(node, path, "angle domain", value)),
    }
}

fn parse_value_form(node: &Node, path: &str) -> Result<ValueForm, MachineGraphCodecError> {
    match symbol(node, path)? {
        "static" => Ok(ValueForm::Static),
        "instantaneous" => Ok(ValueForm::Instantaneous),
        "peak" => Ok(ValueForm::Peak),
        "rms" => Ok(ValueForm::Rms),
        value => Err(unknown_tag(node, path, "semantic value form", value)),
    }
}

fn parse_shape(node: &Node, path: &str) -> Result<TerminalShape, MachineGraphCodecError> {
    let (head, args) = raw_form(node, path)?;
    match head {
        "scalar" => {
            require_arity(node, args, 0, path)?;
            Ok(TerminalShape::Scalar)
        }
        "vector" => {
            require_arity(node, args, 1, path)?;
            Ok(TerminalShape::Vector {
                components: parse_nonzero_u64(&args[0], &format!("{path}[1]"))?,
            })
        }
        "tensor" => {
            require_arity(node, args, 2, path)?;
            Ok(TerminalShape::Tensor {
                rows: parse_nonzero_u64(&args[0], &format!("{path}[1]"))?,
                columns: parse_nonzero_u64(&args[1], &format!("{path}[2]"))?,
            })
        }
        "field-trace" => {
            require_arity(node, args, 1, path)?;
            Ok(TerminalShape::FieldTrace {
                components: parse_nonzero_u64(&args[0], &format!("{path}[1]"))?,
            })
        }
        _ => Err(unknown_tag(node, path, "terminal shape", head)),
    }
}

fn parse_causality(node: &Node, path: &str) -> Result<TerminalCausality, MachineGraphCodecError> {
    match symbol(node, path)? {
        "input" => Ok(TerminalCausality::Input),
        "output" => Ok(TerminalCausality::Output),
        "external-input" => Ok(TerminalCausality::ExternalInput),
        value => Err(unknown_tag(node, path, "terminal causality", value)),
    }
}

fn parse_frame(node: &Node, path: &str) -> Result<FrameBinding, MachineGraphCodecError> {
    let args = exact_form(node, "frame", 2, path)?;
    let orientation = match symbol(&args[1], &format!("{path}[2]"))? {
        "preserving" => OrientationParity::Preserving,
        "reversing" => OrientationParity::Reversing,
        value => {
            return Err(unknown_tag(
                &args[1],
                &format!("{path}[2]"),
                "frame orientation",
                value,
            ));
        }
    };
    FrameBinding::new(string(&args[0], &format!("{path}[1]"))?, orientation)
        .map_err(|error| id_error(&args[0], &format!("{path}[1]"), "frame-binding", &error))
}

fn parse_port(node: &Node, path: &str) -> Result<PortSpec, MachineGraphCodecError> {
    let args = exact_form(node, "port", 5, path)?;
    let energy_role = match symbol(&args[4], &format!("{path}[5]"))? {
        "into-subsystem" => PortEnergyRole::IntoSubsystem,
        "out-of-subsystem" => PortEnergyRole::OutOfSubsystem,
        value => {
            return Err(unknown_tag(
                &args[4],
                &format!("{path}[5]"),
                "port energy role",
                value,
            ));
        }
    };
    Ok(PortSpec {
        id: parse_id(&args[0], "port-id", &format!("{path}[1]"), |key| {
            PortId::new(key)
        })?,
        owner: parse_id(&args[1], "subsystem-id", &format!("{path}[2]"), |key| {
            SubsystemId::new(key)
        })?,
        effort: parse_id(&args[2], "terminal-id", &format!("{path}[3]"), |key| {
            TerminalId::new(key)
        })?,
        flow: parse_id(&args[3], "terminal-id", &format!("{path}[4]"), |key| {
            TerminalId::new(key)
        })?,
        energy_role,
    })
}

fn parse_relation(node: &Node, path: &str) -> Result<RelationSpec, MachineGraphCodecError> {
    let args = exact_form(node, "relation", 4, path)?;
    Ok(RelationSpec {
        id: parse_id(&args[0], "relation-id", &format!("{path}[1]"), |key| {
            RelationId::new(key)
        })?,
        source: parse_id(&args[1], "terminal-id", &format!("{path}[2]"), |key| {
            TerminalId::new(key)
        })?,
        target: parse_id(&args[2], "terminal-id", &format!("{path}[3]"), |key| {
            TerminalId::new(key)
        })?,
        mode: parse_relation_mode(&args[3], &format!("{path}[4]"))?,
    })
}

fn parse_relation_mode(node: &Node, path: &str) -> Result<RelationMode, MachineGraphCodecError> {
    let (head, args) = raw_form(node, path)?;
    match head {
        "algebraic" if args.is_empty() => Ok(RelationMode::Algebraic { solve_policy: None }),
        "algebraic" if args.len() == 1 => Ok(RelationMode::Algebraic {
            solve_policy: Some(parse_reference(
                &args[0],
                "solve-policy-ref",
                &format!("{path}[1]"),
                |namespace, version, digest| SolvePolicyRef::new(namespace, version, digest),
            )?),
        }),
        "algebraic" => Err(arity_error(node, path, "algebraic", "0 or 1", args.len())),
        "stateful" => {
            require_arity(node, args, 1, path)?;
            Ok(RelationMode::Stateful {
                state_slot: parse_id(&args[0], "state-slot-id", &format!("{path}[1]"), |key| {
                    StateSlotId::new(key)
                })?,
            })
        }
        _ => Err(unknown_tag(node, path, "relation mode", head)),
    }
}

fn parse_material(node: &Node, path: &str) -> Result<MaterialBinding, MachineGraphCodecError> {
    let args = exact_form(node, "material", 2, path)?;
    let (head, target_args) = raw_form(&args[0], &format!("{path}[1]"))?;
    require_arity(&args[0], target_args, 1, &format!("{path}[1]"))?;
    let target = match head {
        "body" => MaterialTarget::Body(parse_id(
            &target_args[0],
            "body-id",
            &format!("{path}[1][1]"),
            |key| BodyId::new(key),
        )?),
        "surface-patch" => MaterialTarget::SurfacePatch(parse_id(
            &target_args[0],
            "surface-patch-id",
            &format!("{path}[1][1]"),
            |key| SurfacePatchId::new(key),
        )?),
        _ => {
            return Err(unknown_tag(
                &args[0],
                &format!("{path}[1]"),
                "material target",
                head,
            ));
        }
    };
    Ok(MaterialBinding {
        target,
        material: parse_reference(
            &args[1],
            "material-card-ref",
            &format!("{path}[2]"),
            |namespace, version, digest| MaterialCardRef::new(namespace, version, digest),
        )?,
    })
}

fn parse_interface(node: &Node, path: &str) -> Result<InterfaceBinding, MachineGraphCodecError> {
    let args = exact_form(node, "interface", 5, path)?;
    let orientation = match symbol(&args[4], &format!("{path}[5]"))? {
        "aligned" => InterfaceOrientation::Aligned,
        "opposed" => InterfaceOrientation::Opposed,
        value => {
            return Err(unknown_tag(
                &args[4],
                &format!("{path}[5]"),
                "interface orientation",
                value,
            ));
        }
    };
    Ok(InterfaceBinding {
        id: parse_id(&args[0], "interface-id", &format!("{path}[1]"), |key| {
            InterfaceId::new(key)
        })?,
        negative: parse_id(&args[1], "port-id", &format!("{path}[2]"), |key| {
            PortId::new(key)
        })?,
        positive: parse_id(&args[2], "port-id", &format!("{path}[3]"), |key| {
            PortId::new(key)
        })?,
        interface: parse_reference(
            &args[3],
            "interface-card-ref",
            &format!("{path}[4]"),
            |namespace, version, digest| InterfaceCardRef::new(namespace, version, digest),
        )?,
        orientation,
    })
}

fn parse_owned_ids<T, F>(
    node: &Node,
    head: &str,
    role: &str,
    path: &str,
    owned_elements: &mut usize,
    mut constructor: F,
) -> Result<Vec<T>, MachineGraphCodecError>
where
    F: FnMut(&str) -> Result<T, MachineIdError>,
{
    let items = section_items(node, head, MAX_MACHINE_GRAPH_OWNED_ELEMENTS, path)?;
    let next = owned_elements.checked_add(items.len()).ok_or_else(|| {
        resource_error(node, path, "aggregate owned-element count overflowed usize")
    })?;
    if next > MAX_MACHINE_GRAPH_OWNED_ELEMENTS {
        return Err(resource_error(
            node,
            path,
            &format!(
                "aggregate owned-element count {next} exceeds cap {MAX_MACHINE_GRAPH_OWNED_ELEMENTS}"
            ),
        ));
    }
    let mut decoded = reserved_vec(items.len(), node, path)?;
    for (index, item) in items.iter().enumerate() {
        decoded.push(parse_id(
            item,
            role,
            &format!("{path}[{}]", index + 1),
            &mut constructor,
        )?);
    }
    *owned_elements = next;
    Ok(decoded)
}

fn parse_id<T, F>(
    node: &Node,
    role: &str,
    path: &str,
    constructor: F,
) -> Result<T, MachineGraphCodecError>
where
    F: FnOnce(&str) -> Result<T, MachineIdError>,
{
    let key = string(node, path)?;
    constructor(key).map_err(|error| id_error(node, path, role, &error))
}

fn parse_reference<T, F>(
    node: &Node,
    role: &str,
    path: &str,
    constructor: F,
) -> Result<T, MachineGraphCodecError>
where
    F: FnOnce(&str, NonZeroU64, [u8; 32]) -> Result<T, MachineReferenceError>,
{
    let args = exact_form(node, "ref", 3, path)?;
    let namespace = string(&args[0], &format!("{path}[1]"))?;
    let version = parse_nonzero_u64(&args[1], &format!("{path}[2]"))?;
    let digest = parse_digest(&args[2], &format!("{path}[3]"))?;
    constructor(namespace, version, digest).map_err(|error| match error {
        MachineReferenceError::Namespace(source) => MachineGraphCodecError {
            rule: MachineGraphCodecRule::InvalidReference,
            span: args[0].span,
            path: format!("{path}[1]").into_boxed_str(),
            detail: format!("invalid {role} namespace: {source}").into_boxed_str(),
            hint: "use a bounded canonical external-reference namespace"
                .to_string()
                .into_boxed_str(),
        },
        MachineReferenceError::ZeroDigest {
            role: reference_role,
        } => MachineGraphCodecError {
            rule: MachineGraphCodecRule::InvalidReference,
            span: args[2].span,
            path: format!("{path}[3]").into_boxed_str(),
            detail: format!("{reference_role} semantic digest must not be all zero")
                .into_boxed_str(),
            hint: "supply the exact nonzero 32-byte semantic digest as lowercase hex"
                .to_string()
                .into_boxed_str(),
        },
    })
}

fn parse_u64(node: &Node, path: &str) -> Result<u64, MachineGraphCodecError> {
    let text = string(node, path)?;
    if text.is_empty()
        || (text.len() > 1 && text.starts_with('0'))
        || !text.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(number_error(
            node,
            path,
            "expected canonical unsigned decimal string without sign or leading zero",
        ));
    }
    text.parse::<u64>()
        .map_err(|_| number_error(node, path, "unsigned decimal is outside u64"))
}

fn parse_nonzero_u64(node: &Node, path: &str) -> Result<NonZeroU64, MachineGraphCodecError> {
    let value = parse_u64(node, path)?;
    NonZeroU64::new(value).ok_or_else(|| number_error(node, path, "value must be nonzero"))
}

fn parse_digest(node: &Node, path: &str) -> Result<[u8; 32], MachineGraphCodecError> {
    let text = string(node, path)?;
    if text.len() != 64
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(MachineGraphCodecError {
            rule: MachineGraphCodecRule::InvalidReference,
            span: node.span,
            path: path.to_string().into_boxed_str(),
            detail: "semantic digest must be exactly 64 lowercase hexadecimal characters"
                .to_string()
                .into_boxed_str(),
            hint: "encode the exact 32-byte digest as lowercase hex"
                .to_string()
                .into_boxed_str(),
        });
    }
    let mut digest = [0_u8; 32];
    let bytes = text.as_bytes();
    for (index, slot) in digest.iter_mut().enumerate() {
        let high = hex_nibble(bytes[index * 2]).ok_or_else(|| {
            invalid_digest(
                node,
                path,
                "semantic digest contains a non-hexadecimal byte",
            )
        })?;
        let low = hex_nibble(bytes[index * 2 + 1]).ok_or_else(|| {
            invalid_digest(
                node,
                path,
                "semantic digest contains a non-hexadecimal byte",
            )
        })?;
        *slot = (high << 4) | low;
    }
    Ok(digest)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn invalid_digest(node: &Node, path: &str, detail: &str) -> MachineGraphCodecError {
    MachineGraphCodecError {
        rule: MachineGraphCodecRule::InvalidReference,
        span: node.span,
        path: path.to_string().into_boxed_str(),
        detail: detail.to_string().into_boxed_str(),
        hint: "encode the exact 32-byte digest as lowercase hex"
            .to_string()
            .into_boxed_str(),
    }
}

fn exact_form<'a>(
    node: &'a Node,
    expected_head: &str,
    expected_args: usize,
    path: &str,
) -> Result<&'a [Node], MachineGraphCodecError> {
    let (head, args) = raw_form(node, path)?;
    if head != expected_head {
        return Err(unexpected(
            node,
            path,
            &format!("expected ({expected_head} ...), found ({head} ...)"),
            &format!("use the exact {expected_head} form in canonical field order"),
        ));
    }
    require_arity(node, args, expected_args, path)?;
    Ok(args)
}

fn raw_form<'a>(
    node: &'a Node,
    path: &str,
) -> Result<(&'a str, &'a [Node]), MachineGraphCodecError> {
    let NodeKind::List(items) = &node.kind else {
        return Err(unexpected(
            node,
            path,
            "expected a list form",
            "wrap the value in the documented headed form",
        ));
    };
    let Some(first) = items.first() else {
        return Err(unexpected(
            node,
            path,
            "empty lists are not valid Machine forms",
            "start the list with its documented symbol",
        ));
    };
    let NodeKind::Symbol(head) = &first.kind else {
        return Err(unexpected(
            first,
            &format!("{path}[0]"),
            "Machine form head must be a symbol",
            "use the documented literal head symbol",
        ));
    };
    Ok((head, &items[1..]))
}

fn section_items<'a>(
    node: &'a Node,
    expected_head: &str,
    max_items: usize,
    path: &str,
) -> Result<&'a [Node], MachineGraphCodecError> {
    let (head, items) = raw_form(node, path)?;
    if head != expected_head {
        return Err(unexpected(
            node,
            path,
            &format!("expected section {expected_head}, found {head}"),
            "retain all seven sections in canonical graph order, including empty sections",
        ));
    }
    if items.len() > max_items {
        return Err(resource_error(
            node,
            path,
            &format!(
                "section {expected_head} contains {} entries, above cap {max_items}",
                items.len()
            ),
        ));
    }
    Ok(items)
}

fn require_arity(
    node: &Node,
    args: &[Node],
    expected: usize,
    path: &str,
) -> Result<(), MachineGraphCodecError> {
    if args.len() == expected {
        Ok(())
    } else {
        let head = node.head().unwrap_or("<non-form>");
        Err(arity_error(
            node,
            path,
            head,
            &expected.to_string(),
            args.len(),
        ))
    }
}

fn arity_error(
    node: &Node,
    path: &str,
    head: &str,
    expected: &str,
    actual: usize,
) -> MachineGraphCodecError {
    unexpected(
        node,
        path,
        &format!("form {head} expects {expected} arguments, found {actual}"),
        "use the exact positional v1 grammar; omitted values require explicit empty sections/forms",
    )
}

fn string<'a>(node: &'a Node, path: &str) -> Result<&'a str, MachineGraphCodecError> {
    match &node.kind {
        NodeKind::Str(value) => Ok(value),
        _ => Err(unexpected(
            node,
            path,
            "expected a string atom",
            "quote identifiers, canonical keys, decimal u64 values, and digests",
        )),
    }
}

fn symbol<'a>(node: &'a Node, path: &str) -> Result<&'a str, MachineGraphCodecError> {
    match &node.kind {
        NodeKind::Symbol(value) => Ok(value),
        _ => Err(unexpected(
            node,
            path,
            "expected a closed-tag symbol",
            "use one documented lowercase tag without quotes",
        )),
    }
}

fn reserved_vec<T>(
    count: usize,
    node: &Node,
    path: &str,
) -> Result<Vec<T>, MachineGraphCodecError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| resource_error(node, path, "allocation for decoded entries was refused"))?;
    Ok(values)
}

fn unexpected(node: &Node, path: &str, detail: &str, hint: &str) -> MachineGraphCodecError {
    MachineGraphCodecError {
        rule: MachineGraphCodecRule::UnexpectedForm,
        span: node.span,
        path: path.to_string().into_boxed_str(),
        detail: detail.to_string().into_boxed_str(),
        hint: hint.to_string().into_boxed_str(),
    }
}

fn unknown_tag(node: &Node, path: &str, family: &str, value: &str) -> MachineGraphCodecError {
    MachineGraphCodecError {
        rule: MachineGraphCodecRule::UnknownTag,
        span: node.span,
        path: path.to_string().into_boxed_str(),
        detail: format!("unknown {family} tag {value:?}").into_boxed_str(),
        hint: "use one tag from the closed v1 Machine grammar"
            .to_string()
            .into_boxed_str(),
    }
}

fn number_error(node: &Node, path: &str, detail: &str) -> MachineGraphCodecError {
    MachineGraphCodecError {
        rule: MachineGraphCodecRule::InvalidNumber,
        span: node.span,
        path: path.to_string().into_boxed_str(),
        detail: detail.to_string().into_boxed_str(),
        hint: "use the exact bounded numeric spelling required by the v1 grammar"
            .to_string()
            .into_boxed_str(),
    }
}

fn id_error(node: &Node, path: &str, role: &str, error: &MachineIdError) -> MachineGraphCodecError {
    MachineGraphCodecError {
        rule: MachineGraphCodecRule::InvalidIdentifier,
        span: node.span,
        path: path.to_string().into_boxed_str(),
        detail: format!("invalid {role}: {error}").into_boxed_str(),
        hint: "use a bounded canonical role-specific Machine key"
            .to_string()
            .into_boxed_str(),
    }
}

fn resource_error(node: &Node, path: &str, detail: &str) -> MachineGraphCodecError {
    MachineGraphCodecError {
        rule: MachineGraphCodecRule::ResourceLimit,
        span: node.span,
        path: path.to_string().into_boxed_str(),
        detail: detail.to_string().into_boxed_str(),
        hint: "split the machine or reduce the declared collection before admission"
            .to_string()
            .into_boxed_str(),
    }
}

fn sym(value: &str) -> Node {
    Node::synthetic(NodeKind::Symbol(value.to_string()))
}

fn string_node(value: &str) -> Node {
    Node::synthetic(NodeKind::Str(value.to_string()))
}

fn int_node(value: i64) -> Node {
    Node::synthetic(NodeKind::Int(value))
}

fn form(head: &str, args: Vec<Node>) -> Node {
    let mut items = Vec::with_capacity(args.len() + 1);
    items.push(sym(head));
    items.extend(args);
    Node::synthetic(NodeKind::List(items))
}

fn section(head: &str, entries: Vec<Node>) -> Node {
    form(head, entries)
}

fn u64_node(value: u64) -> Node {
    string_node(&value.to_string())
}

fn digest_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(64);
    for byte in digest {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
}

fn reference_node(namespace: &str, version: NonZeroU64, digest: [u8; 32]) -> Node {
    form(
        "ref",
        vec![
            string_node(namespace),
            u64_node(version.get()),
            string_node(&digest_hex(digest)),
        ],
    )
}

fn write_clock(clock: &ClockSpec) -> Node {
    let mode = match clock.clock {
        MachineClock::Continuous => form("continuous", Vec::new()),
        MachineClock::Periodic {
            period_ns,
            phase_ns,
        } => form(
            "periodic",
            vec![u64_node(period_ns.get()), u64_node(phase_ns)],
        ),
        MachineClock::EventDriven => form("event-driven", Vec::new()),
    };
    form("clock", vec![string_node(clock.id.canonical_key()), mode])
}

fn write_subsystem(subsystem: &SubsystemSpec) -> Node {
    form(
        "subsystem",
        vec![
            string_node(subsystem.id.canonical_key()),
            reference_node(
                subsystem.model.namespace(),
                subsystem.model.schema_version(),
                subsystem.model.semantic_digest(),
            ),
            section(
                "bodies",
                subsystem
                    .bodies
                    .iter()
                    .map(|id| string_node(id.canonical_key()))
                    .collect(),
            ),
            section(
                "surface-patches",
                subsystem
                    .surface_patches
                    .iter()
                    .map(|id| string_node(id.canonical_key()))
                    .collect(),
            ),
            section(
                "contact-features",
                subsystem
                    .contact_features
                    .iter()
                    .map(|id| string_node(id.canonical_key()))
                    .collect(),
            ),
            section(
                "state-slots",
                subsystem
                    .state_slots
                    .iter()
                    .map(|id| string_node(id.canonical_key()))
                    .collect(),
            ),
        ],
    )
}

fn write_terminal(terminal: &TerminalSpec) -> Node {
    let causality = match terminal.causality {
        TerminalCausality::Input => "input",
        TerminalCausality::Output => "output",
        TerminalCausality::ExternalInput => "external-input",
    };
    form(
        "terminal",
        vec![
            string_node(terminal.id.canonical_key()),
            string_node(terminal.owner.canonical_key()),
            write_quantity(terminal.quantity),
            write_shape(terminal.shape),
            sym(causality),
            string_node(terminal.clock.canonical_key()),
            form(
                "frame",
                vec![
                    string_node(terminal.frame.canonical_key()),
                    sym(match terminal.frame.orientation() {
                        OrientationParity::Preserving => "preserving",
                        OrientationParity::Reversing => "reversing",
                    }),
                ],
            ),
        ],
    )
}

fn write_quantity(quantity: TerminalQuantitySpec) -> Node {
    match quantity {
        TerminalQuantitySpec::Dimensional(Dims(dims)) => form(
            "dims",
            dims.into_iter()
                .map(|value| int_node(i64::from(value)))
                .collect(),
        ),
        TerminalQuantitySpec::Semantic(semantic) => form(
            "semantic",
            vec![
                write_quantity_kind(semantic.kind()),
                sym(match semantic.form() {
                    ValueForm::Static => "static",
                    ValueForm::Instantaneous => "instantaneous",
                    ValueForm::Peak => "peak",
                    ValueForm::Rms => "rms",
                }),
            ],
        ),
    }
}

fn write_quantity_kind(kind: QuantityKind) -> Node {
    match kind {
        QuantityKind::AbsoluteTemperature => form("absolute-temperature", Vec::new()),
        QuantityKind::TemperatureDifference => form("temperature-difference", Vec::new()),
        QuantityKind::Angle(domain) => form("angle", vec![write_angle_domain(domain)]),
        QuantityKind::AngularVelocity(domain) => {
            form("angular-velocity", vec![write_angle_domain(domain)])
        }
        QuantityKind::Torque => form("torque", Vec::new()),
        QuantityKind::Energy => form("energy", Vec::new()),
        QuantityKind::Pressure => form("pressure", Vec::new()),
        QuantityKind::Stress => form("stress", Vec::new()),
        QuantityKind::Strain { basis, component } => form(
            "strain",
            vec![
                sym(match basis {
                    StrainBasis::Tensor => "tensor",
                    StrainBasis::Engineering => "engineering",
                }),
                sym(match component {
                    StrainComponent::Normal => "normal",
                    StrainComponent::Shear => "shear",
                }),
            ],
        ),
        QuantityKind::Composition(basis) => form(
            "composition",
            vec![sym(match basis {
                CompositionBasis::MassFraction => "mass-fraction",
                CompositionBasis::MoleFraction => "mole-fraction",
                CompositionBasis::VolumeFraction => "volume-fraction",
            })],
        ),
        QuantityKind::Mass => form("mass", Vec::new()),
        QuantityKind::Amount => form("amount", Vec::new()),
        QuantityKind::MolarMass => form("molar-mass", Vec::new()),
        QuantityKind::MassConcentration => form("mass-concentration", Vec::new()),
        QuantityKind::AmountConcentration => form("amount-concentration", Vec::new()),
        QuantityKind::Entropy => form("entropy", Vec::new()),
        QuantityKind::HeatCapacity => form("heat-capacity", Vec::new()),
        QuantityKind::AcousticPressure => form("acoustic-pressure", Vec::new()),
        QuantityKind::AcousticPower => form("acoustic-power", Vec::new()),
    }
}

fn write_angle_domain(domain: AngleDomain) -> Node {
    sym(match domain {
        AngleDomain::Mechanical => "mechanical",
        AngleDomain::Electrical => "electrical",
    })
}

fn write_shape(shape: TerminalShape) -> Node {
    match shape {
        TerminalShape::Scalar => form("scalar", Vec::new()),
        TerminalShape::Vector { components } => form("vector", vec![u64_node(components.get())]),
        TerminalShape::Tensor { rows, columns } => form(
            "tensor",
            vec![u64_node(rows.get()), u64_node(columns.get())],
        ),
        TerminalShape::FieldTrace { components } => {
            form("field-trace", vec![u64_node(components.get())])
        }
    }
}

fn write_port(port: &PortSpec) -> Node {
    form(
        "port",
        vec![
            string_node(port.id.canonical_key()),
            string_node(port.owner.canonical_key()),
            string_node(port.effort.canonical_key()),
            string_node(port.flow.canonical_key()),
            sym(match port.energy_role {
                PortEnergyRole::IntoSubsystem => "into-subsystem",
                PortEnergyRole::OutOfSubsystem => "out-of-subsystem",
            }),
        ],
    )
}

fn write_relation(relation: &RelationSpec) -> Node {
    let mode = match &relation.mode {
        RelationMode::Algebraic { solve_policy } => form(
            "algebraic",
            solve_policy
                .iter()
                .map(|policy| {
                    reference_node(
                        policy.namespace(),
                        policy.schema_version(),
                        policy.semantic_digest(),
                    )
                })
                .collect(),
        ),
        RelationMode::Stateful { state_slot } => {
            form("stateful", vec![string_node(state_slot.canonical_key())])
        }
    };
    form(
        "relation",
        vec![
            string_node(relation.id.canonical_key()),
            string_node(relation.source.canonical_key()),
            string_node(relation.target.canonical_key()),
            mode,
        ],
    )
}

fn write_material(material: &MaterialBinding) -> Node {
    let target = match &material.target {
        MaterialTarget::Body(id) => form("body", vec![string_node(id.canonical_key())]),
        MaterialTarget::SurfacePatch(id) => {
            form("surface-patch", vec![string_node(id.canonical_key())])
        }
    };
    form(
        "material",
        vec![
            target,
            reference_node(
                material.material.namespace(),
                material.material.schema_version(),
                material.material.semantic_digest(),
            ),
        ],
    )
}

fn write_interface(interface: &InterfaceBinding) -> Node {
    form(
        "interface",
        vec![
            string_node(interface.id.canonical_key()),
            string_node(interface.negative.canonical_key()),
            string_node(interface.positive.canonical_key()),
            reference_node(
                interface.interface.namespace(),
                interface.interface.schema_version(),
                interface.interface.semantic_digest(),
            ),
            sym(match interface.orientation {
                InterfaceOrientation::Aligned => "aligned",
                InterfaceOrientation::Opposed => "opposed",
            }),
        ],
    )
}
