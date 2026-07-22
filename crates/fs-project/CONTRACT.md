# CONTRACT: fs-project

The versioned `.fsim` project schema (bead f85xj.6.1): the user-facing
contract for the ratified thermal-design-assurance vertical
(`frankensim-vertical-ratification-v1`). One semantic model, two spellings,
canonical bytes, typed violations, receipted defaults, and explicit migration
receipts.

## Purpose and layer

Layer L6 (HELM). Depends on `fs-ir` (AST + both concrete syntaxes),
`fs-scenario` (entity identities, `Violation`), `fs-qty`, `fs-blake3`. This
crate persists project intent; it runs no solves and admits no scenarios
itself.

## Public types and semantics

- `ProjectSpec` carries every section of the cooling vertical's contract:
  `Metadata` (name, created, context of use, intended decision), the Five
  Explicits (`Versions`, `Seeds`, `Budgets`, capabilities list,
  `UnitsDoctrine`), `GeometryArtifact` references (imported quarantine-receipt
  ids — geometry never lives inline), `EntityDecl`
  assembly/part/region/interface declarations with persistent identities,
  `MaterialBinding` matdb card refs (state, admitted temperature range,
  source), `InterfaceCardBinding` TIM/contact card refs, `PowerDissipation`
  rows, `Cooling` (fans/vents/leakage; declared-empty lists are facts,
  omission is a violation), `Envelope`, `ThermalLimit` requirements with
  margins, `SolverSettings`, and `OutputRequest`s. Sections are `Option`s so
  recognition stays lenient and validation can name every omission at once.
- `ProjectSpec::validate() -> Vec<Violation>` (the fs-scenario
  code/what/fix triple): empty output is the definition of admissible. Every
  mandatory section omission has a stable `project-*-missing` code; empty
  load-bearing collections, dimension mismatches (checked against the six-base
  SI vectors in `spec::dims`), inverted ranges, malformed card hashes, duty
  range, unknown entity references, duplicate names, and out-of-order parents
  all carry named codes with fix hints.
- Entity identities: `resolve_entities` recomputes `fs_scenario::EntityId`s
  from the declarations (parents before children; interfaces reference two
  regions under an assembly). An optional `:id` pin is verified against the
  recomputation and drift is `project-entity-id-mismatch` — a pin proves
  byte-equal derivation inputs, never physical sameness. Display names are
  outside identity, exactly as in fs-scenario.
- Wire: `lower`/`recognize` map `ProjectSpec` to and from the `fs_ir::Node`
  envelope `(fsim-project :version 1 ...)`. `print_sexpr`/`parse_sexpr` and
  `print_json`/`parse_json` are the two spellings; `parse_sexpr_lenient`
  accepts noncanonical bytes and omitted defaultable fields, issuing a
  `CanonicalizationReceipt` (both hashes, `verifies()`) and `DefaultReceipt`s.
  The strict parsers refuse noncanonical input (`fsim-non-canonical`, first
  difference position) and applied defaults (`fsim-default-in-strict-mode`).
- Canonical bytes are the checked s-expression render; `canonical_hash`
  hashes them under `org.frankensim.fs-project.canonical.v1`. The JSON
  spelling parses to the same AST, so both spellings reach one hash.
- `FSIM_VERSION = 1`. Readers refuse other versions
  (`fsim-unsupported-version`); `migrate_envelope` is the only path from an
  older envelope, applies a registered `MigrationRule`, and returns a
  `ProjectMigrationReceipt` (old/new hashes + rule, `verifies()`). The only
  registered rule is the synthetic version-0 proof rule, named as such.
- The ONE defaultable field is power-row `duty` (`1.0`, continuous
  dissipation is the conservative assumption); everything else is mandatory,
  and the default is always receipted.

## Invariants

- Parse-render-parse is byte-idempotent for canonical documents in both
  spellings, and `print_sexpr` is deterministic.
- The strict parsers accept exactly the canonical bytes; the lenient parser
  never accepts silently (receipt or violation for every relaxation).
- Unknown sections and unknown keywords are refused by name
  (`project-unknown-field`), so typos cannot silently drop intent.
- Recognition never panics on malformed clause shapes; it degrades to named
  violations with placeholder values that validation then rejects.
- `Violation.fix` is non-empty for every emitted code (tested via the broken
  corpus).

## Error model

Two tiers, exactly as in fs-scenario/fs-ir: wire-layer refusals are
`ProjectError { code, detail, hint }` (syntax, non-project shape, unsupported
version, non-canonical bytes, strict-mode defaults, quantity spelling,
migration refusals); semantic findings are `Violation` triples collected by
`validate()`/`DecodedProject::findings()`. Internal `expect` is limited to
infallible writes to `String`.

## Determinism class

Fully deterministic: pure functions of the input bytes/spec; canonical
rendering has one spelling; hashing is domain-separated BLAKE3 over exact
bytes. No clocks, no RNG, no environment reads.

## Cancellation behavior

None. Documents are bounded project descriptions; a validation budget/plan
triad (fs-scenario style) is deliberately deferred until real projects show
the loader can be large, and is named future work rather than pretended.

## Unsafe boundary

None. Workspace `deny(unsafe_code)` lint applies.

## Feature flags

None.

## Conformance tests

`tests/project.rs`: the reference cooling project renders, parses strictly,
is admissible, and hash-stable; canonical bytes are identical across both
spellings; parse-render-parse idempotence across variants (empty cooling
lists, identity pins); all sixteen mandatory-section omissions surface their
named violations; unknown section and keyword refusal; the receipted duty
default with canonicalization receipt (and strict-mode refusal of the same
bytes); noncanonical whitespace refusal/receipt; the synthetic v0 migration
round trip with verifying receipt plus refusal of no-op and unknown-version
migrations; entity identity pins accepting the recomputed token and refusing
a stale one; and the broken-project corpus (14 rows) logging every violation
with its fix as the error-message quality bar.

## No-claim boundaries

- An admissible `.fsim` document proves the project is well-formed and
  internally consistent. It does NOT prove the referenced geometry artifacts,
  matdb cards, or capabilities exist, are compatible, or are validated —
  existence checks belong to the import/admission path (bead .6.3) and the
  capability registry, not this schema.
- Entity identity pins prove byte-equal derivation inputs at declaration
  time; unequal pins prove nothing about physical sameness (re-exports change
  bytes), which is why drift is a violation to adjudicate, not an automatic
  rebind.
- The schema does not persist fs-scenario's full `EntityCatalog`/
  `BindingTable` receipts (rebind chains, correspondences, evidence tiers);
  the fs-scenario CONTRACT's delegation of catalog+binding persistence to
  this crate is met for declarations and will extend to receipt chains when
  the import path (.6.3) produces them.
- `source_hash` on geometry artifacts reproduces `fs_io::ImportReceipt`'s
  current 64-bit FNV field: deterministic correlation, not
  collision-resistant authentication; the HELM-side BLAKE3-class upgrade is
  tracked upstream.
- The schema freeze (e16, bead .16.5) has NOT happened: version 1 is the
  first implemented version, not yet a frozen public promise. The migration
  machinery exists so the freeze can be honest when it lands.
- Validation is structural and dimensional; it makes no physics claim (a
  well-formed project can still describe an unsolvable or nonsensical study).
