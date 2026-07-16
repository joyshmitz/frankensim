//! FrankenScript Machine-graph codec conformance (Gauntlet G0/G3).

use fs_ir::machine::codec::{
    MAX_MACHINE_GRAPH_AST_NODES, MachineGraphAstAdmissionError, MachineGraphCodecRule,
    admit_machine_graph_ast_v1, parse_machine_graph_program_v1, parse_machine_graph_v1,
    write_machine_graph_program_v1, write_machine_graph_v1,
};
use fs_ir::machine::{
    MAX_MACHINE_GRAPH_CLOCKS, MAX_MACHINE_GRAPH_OWNED_ELEMENTS, MachineGraphRule,
};
use fs_ir::{Node, NodeKind, VersionedProgram, json, sexpr};

const SOURCE_MODEL_DIGEST_BYTE: u8 = 0xab;

fn digest(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn valid_source(version: &str) -> String {
    let source_model = digest(SOURCE_MODEL_DIGEST_BYTE);
    let load_model = digest(2);
    let source_material = digest(3);
    let load_material = digest(4);
    let interface = digest(5);
    format!(
        r#"(machine-graph-v1
  (clocks
    (clock "clock/mechanical" (periodic "1000000" "0")))
  (subsystems
    (subsystem "subsystem/source"
      (ref "models/source" "{version}" "{source_model}")
      (bodies "body/source")
      (surface-patches)
      (contact-features)
      (state-slots))
    (subsystem "subsystem/load"
      (ref "models/load" "{version}" "{load_model}")
      (bodies "body/load")
      (surface-patches)
      (contact-features)
      (state-slots)))
  (terminals
    (terminal "terminal/source-effort" "subsystem/source"
      (semantic (pressure) static)
      (scalar) output "clock/mechanical" (frame "world/mechanical" preserving))
    (terminal "terminal/source-flow" "subsystem/source"
      (dims 3 0 -1 0 0 0)
      (scalar) output "clock/mechanical" (frame "world/mechanical" preserving))
    (terminal "terminal/load-effort" "subsystem/load"
      (semantic (pressure) static)
      (scalar) input "clock/mechanical" (frame "world/mechanical" preserving))
    (terminal "terminal/load-flow" "subsystem/load"
      (dims 3 0 -1 0 0 0)
      (scalar) input "clock/mechanical" (frame "world/mechanical" preserving)))
  (ports
    (port "port/source" "subsystem/source"
      "terminal/source-effort" "terminal/source-flow" out-of-subsystem)
    (port "port/load" "subsystem/load"
      "terminal/load-effort" "terminal/load-flow" into-subsystem))
  (relations
    (relation "relation/effort" "terminal/source-effort" "terminal/load-effort"
      (algebraic))
    (relation "relation/flow" "terminal/source-flow" "terminal/load-flow"
      (algebraic)))
  (materials
    (material (body "body/source")
      (ref "materials/source" "{version}" "{source_material}"))
    (material (body "body/load")
      (ref "materials/load" "{version}" "{load_material}")))
  (interfaces
    (interface "interface/source-load" "port/source" "port/load"
      (ref "interfaces/hydraulic" "{version}" "{interface}") aligned)))"#
    )
}

fn parse_valid() -> Node {
    sexpr::parse(&valid_source("1")).expect("valid Machine graph literal")
}

fn root_items(node: &mut Node) -> &mut Vec<Node> {
    let NodeKind::List(items) = &mut node.kind else {
        panic!("fixture root is a list")
    };
    items
}

fn section_items(node: &mut Node) -> &mut Vec<Node> {
    let NodeKind::List(items) = &mut node.kind else {
        panic!("fixture section is a list")
    };
    items
}

#[test]
fn g0_literal_sexpr_and_json_publish_the_same_machine_identity() {
    let literal = parse_valid();
    let admitted = admit_machine_graph_ast_v1(&literal).expect("literal graph admits");

    let program = write_machine_graph_program_v1(&admitted).expect("admitted graph encodes");
    let sexpr_bytes = program
        .print_sexpr_checked()
        .expect("canonical s-expression");
    let json_bytes = program.print_json_checked().expect("canonical JSON");
    let from_sexpr = VersionedProgram::parse_sexpr(&sexpr_bytes).expect("s-expression reparses");
    let from_json = VersionedProgram::parse_json(&json_bytes).expect("JSON reparses");
    let admitted_sexpr = parse_machine_graph_program_v1(&from_sexpr)
        .expect("s-expression graph decodes")
        .admit()
        .expect("s-expression graph admits");
    let admitted_json = parse_machine_graph_program_v1(&from_json)
        .expect("JSON graph decodes")
        .admit()
        .expect("JSON graph admits");

    assert_eq!(admitted.identity(), admitted_sexpr.identity());
    assert_eq!(admitted.identity(), admitted_json.identity());
    let canonical_node = write_machine_graph_v1(&admitted).expect("graph writes");
    assert!(canonical_node.same_shape(from_sexpr.program()));
    assert!(canonical_node.same_shape(from_json.program()));
    assert_eq!(
        sexpr::print(&canonical_node).expect("canonical node prints"),
        sexpr::print(from_json.program()).expect("JSON-derived node prints")
    );
}

#[test]
fn g3_source_row_permutations_do_not_move_admitted_identity() {
    let baseline = admit_machine_graph_ast_v1(&parse_valid())
        .expect("baseline admits")
        .identity();
    let mut permuted = parse_valid();
    for section in &mut root_items(&mut permuted)[1..] {
        section_items(section)[1..].reverse();
    }
    let moved = admit_machine_graph_ast_v1(&permuted)
        .expect("permuted graph admits")
        .identity();
    assert_eq!(baseline, moved);
}

#[test]
fn g0_semantic_mutation_moves_identity_and_full_u64_versions_round_trip() {
    let baseline_source = valid_source("1");
    let baseline = admit_machine_graph_ast_v1(
        &sexpr::parse(&baseline_source).expect("baseline syntax parses"),
    )
    .expect("baseline admits");

    let changed_source = baseline_source.replacen(&digest(SOURCE_MODEL_DIGEST_BYTE), &digest(9), 1);
    let changed =
        admit_machine_graph_ast_v1(&sexpr::parse(&changed_source).expect("mutated syntax parses"))
            .expect("mutated graph admits");
    assert_ne!(baseline.identity(), changed.identity());

    let max_source = valid_source("18446744073709551615");
    let max = admit_machine_graph_ast_v1(&sexpr::parse(&max_source).expect("u64 syntax parses"))
        .expect("u64::MAX reference versions admit");
    let max_program = write_machine_graph_program_v1(&max).expect("max versions encode");
    assert!(max_program.print_sexpr().contains("18446744073709551615"));
    let reparsed =
        VersionedProgram::parse_json(&max_program.print_json()).expect("max-version JSON reparses");
    let readmitted = admit_machine_graph_ast_v1(reparsed.program()).expect("reparsed graph admits");
    assert_eq!(max.identity(), readmitted.identity());
}

#[test]
fn g0_codec_refusals_retain_rule_span_path_and_hint() {
    let source = valid_source("1");
    let uppercase = source.replacen(
        &digest(SOURCE_MODEL_DIGEST_BYTE),
        &digest(SOURCE_MODEL_DIGEST_BYTE).to_uppercase(),
        1,
    );
    let uppercase_node = sexpr::parse(&uppercase).expect("generic syntax remains valid");
    let error = parse_machine_graph_v1(&uppercase_node).expect_err("uppercase digest must refuse");
    assert_eq!(error.rule(), MachineGraphCodecRule::InvalidReference);
    assert_eq!(error.code(), "MachineGraphCodecInvalidReference");
    assert!(error.span().end > error.span().start);
    assert_eq!(error.path(), "$[2][1][2][3]");
    assert!(!error.detail().is_empty());
    assert!(!error.hint().is_empty());

    let leading_zero = source.replacen("\"1\"", "\"01\"", 1);
    let error =
        parse_machine_graph_v1(&sexpr::parse(&leading_zero).expect("generic syntax remains valid"))
            .expect_err("noncanonical u64 must refuse");
    assert_eq!(error.rule(), MachineGraphCodecRule::InvalidNumber);

    let zero_digest = source.replacen(&digest(SOURCE_MODEL_DIGEST_BYTE), &"0".repeat(64), 1);
    let error =
        parse_machine_graph_v1(&sexpr::parse(&zero_digest).expect("generic syntax remains valid"))
            .expect_err("zero semantic digest must refuse");
    assert_eq!(error.rule(), MachineGraphCodecRule::InvalidReference);
    assert_eq!(error.path(), "$[2][1][2][3]");

    let invalid_namespace = source.replacen("models/source", "Models Source", 1);
    let error = parse_machine_graph_v1(
        &sexpr::parse(&invalid_namespace).expect("generic syntax remains valid"),
    )
    .expect_err("invalid reference namespace must refuse");
    assert_eq!(error.rule(), MachineGraphCodecRule::InvalidReference);
    assert_eq!(error.path(), "$[2][1][2][1]");

    let wrong_order = source.replacen("(clocks", "(terminals", 1);
    let error =
        parse_machine_graph_v1(&sexpr::parse(&wrong_order).expect("generic syntax remains valid"))
            .expect_err("out-of-order section must refuse");
    assert_eq!(error.rule(), MachineGraphCodecRule::UnexpectedForm);
    assert_eq!(error.path(), "$[1]");

    let unknown_orientation = source.replacen("preserving", "sideways", 1);
    let error = parse_machine_graph_v1(
        &sexpr::parse(&unknown_orientation).expect("generic syntax remains valid"),
    )
    .expect_err("unknown nested orientation must refuse");
    assert_eq!(error.rule(), MachineGraphCodecRule::UnknownTag);
    assert_eq!(error.path(), "$[3][1][7][2]");
}

#[test]
fn g0_codec_resource_preflight_refuses_before_entry_decode() {
    assert!(MAX_MACHINE_GRAPH_AST_NODES > MAX_MACHINE_GRAPH_CLOCKS);
    let mut oversized = sexpr::parse(
        "(machine-graph-v1 (clocks) (subsystems) (terminals) (ports) (relations) (materials) (interfaces))",
    )
    .expect("empty graph syntax parses");
    let root = root_items(&mut oversized);
    let clocks = section_items(&mut root[1]);
    clocks.push(Node::synthetic(NodeKind::Float(f64::NAN)));
    clocks
        .extend((0..MAX_MACHINE_GRAPH_CLOCKS).map(|_| Node::synthetic(NodeKind::List(Vec::new()))));
    let error = parse_machine_graph_v1(&oversized).expect_err("clock cap must refuse");
    assert_eq!(error.rule(), MachineGraphCodecRule::ResourceLimit);
    assert_eq!(error.path(), "$[1]");

    let mut oversized_owned = parse_valid();
    let root = root_items(&mut oversized_owned);
    let subsystems = section_items(&mut root[2]);
    let subsystem = root_items(&mut subsystems[1]);
    let bodies = section_items(&mut subsystem[3]);
    bodies.push(Node::synthetic(NodeKind::Float(f64::NAN)));
    bodies.extend(
        (1..MAX_MACHINE_GRAPH_OWNED_ELEMENTS)
            .map(|_| Node::synthetic(NodeKind::Str("body/repeated".to_string()))),
    );
    let error = parse_machine_graph_v1(&oversized_owned)
        .expect_err("aggregate ownership cap must precede recursive AST validation");
    assert_eq!(error.rule(), MachineGraphCodecRule::ResourceLimit);
    assert_eq!(error.path(), "$[2][1][3]");
}

#[test]
fn g0_syntax_success_cannot_bypass_semantic_graph_refusal() {
    let mut unclosed = parse_valid();
    let root = root_items(&mut unclosed);
    let relations = section_items(&mut root[5]);
    relations.remove(1);

    let error = admit_machine_graph_ast_v1(&unclosed).expect_err("missing source must refuse");
    let MachineGraphAstAdmissionError::Graph(refusal) = error else {
        panic!("valid syntax must reach semantic graph refusal")
    };
    assert!(
        refusal
            .findings()
            .iter()
            .any(|finding| finding.rule() == MachineGraphRule::MissingSourceClosure)
    );
}

#[test]
fn g3_generic_json_shape_is_not_a_second_machine_grammar() {
    let node = parse_valid();
    let json_bytes = json::print(&node).expect("generic JSON prints");
    let from_json = json::parse(&json_bytes).expect("generic JSON reparses");
    let left = admit_machine_graph_ast_v1(&node).expect("s-expression AST admits");
    let right = admit_machine_graph_ast_v1(&from_json).expect("JSON AST admits");
    assert_eq!(left.identity(), right.identity());
}
