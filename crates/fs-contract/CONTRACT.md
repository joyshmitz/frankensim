# CONTRACT: fs-contract

Assume-guarantee component contracts (plan addendum, Proposal E): certified
design motifs whose `(envelope ⇒ guarantee, certificate)` compose into system
claims via envelope containment.

## Purpose and layer

Layer L3. Depends on `fs-iface` (L3, `SpaceType` for typed interface
quantities) and `fs-evidence` (UTIL, the `Color` lattice). Pure, deterministic
composition logic — it does not run solves or produce certificates, it
composes existing ones.

## Public types and semantics

- `Interval { lo, hi }` — a closed, finite, ordered interval;
  `Interval::new` rejects non-finite/inverted; `contains(other)` is inclusive
  on the boundary.
- `Envelope` — an interval box over named, `SpaceType`-typed quantities
  (`with(quantity, space, interval)`); `OperatingConditions` — the system
  model's computed interval per quantity.
- `Contract { name, interface: SpaceType, linear, envelope, guarantee,
  certificate: Color, requires }` — an assume-guarantee motif. `color_ok()`:
  a NONLINEAR contract may not carry a verified-color certificate.
- `ContractLibrary` — the certified-motif catalog (`insert`, `get`).
- `compose(&lib, root, &ops) -> Result<SystemClaim, ContractError>` — resolves
  `root` + its transitive `requires`, checks each member's operating conditions
  land inside its envelope and its color discipline holds, and returns a
  `SystemClaim` whose certificate is the WEAKEST member's color.
- `ContractError` — `BadInterval` / `UnknownContract` / `MissingCondition` /
  `OutsideEnvelope` / `ColorDiscipline` / `CircularDependency`.

## Invariants

- SOUNDNESS: the composed certificate is never tighter than the weakest
  member's (its `ColorRank` is the minimum over members) — the Gauntlet
  contract-composition property.
- Envelope containment is inclusive on the boundary; a quantity with no
  operating condition, or one outside its envelope, blocks composition (fail
  closed — a guarantee is only asserted where provably inside).
- A nonlinear contract cannot be verified-color.
- The requires-graph is acyclic; a shared sub-contract (diamond) is resolved
  once, not flagged as a cycle.

## Error model

Structured `ContractError` values (refusals that teach), never panics.

## Determinism class

Fully deterministic: composition is a pure function of `(library, root, ops)`;
members are returned sorted; replay reproduces the claim.

## Cancellation behavior

None (synchronous pure functions).

## Unsafe boundary

None. `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/contract.rs` (Proposal E, 10 cases): interval validation + inclusive
boundary containment; composition when conditions land inside; the
weakest-member soundness invariant (verified + estimated → estimated);
outside-envelope and missing-condition rejection; nonlinear-cannot-be-verified
color discipline; circular-dependency rejection; the diamond shared
sub-contract; unknown-contract rejection; determinism.

## No-claim boundaries

- Composition rule v1 is deliberately primitive ENVELOPE CONTAINMENT over
  interval boxes — it does not model general assume-guarantee refinement,
  nonlinear superposition, or probabilistic envelopes.
- Contracts CARRY certificates (`Color`); this crate does not produce or verify
  the certificates themselves (the solvers + fs-evidence do). The composed
  color is the weakest member's, by rank.
- Operating conditions are supplied by the caller (the system model); this
  crate does not compute them.
- The `SpaceType` interface tag is carried on quantities; cross-contract
  coupling-type compatibility checking (via fs-iface's checker) is a later
  integration.
