# CONTRACT: fs-neuroshape-e2e

NeuroShapeCert — certified facts about a neural implicit shape. Layer L5
(LUMEN).

## Purpose and layer

Composes `fs-rep-neural` (Lipschitz + IBP + interval sign-margin safe step) and
`fs-viz` (isocontour + Hessian classification). Deps point downward. The campaign
no longer depends on `fs-evidence`: the only enclosed-component state it
publishes is the typed `ComponentCountEvidence`, so there is no color left to
mint here.

## Public types and semantics

- `blob_sdf_net() -> MlpSdf` — the spectral-normalized `tanh`-MLP whose
  effective field is approximately `2.12·Σ tanh(3(±coord−0.7)) + 6.5`.
- `run_campaign(&MlpSdf, ring_r, inner) -> NeuroShapeReport` — certifies the
  Lipschitz bound, a no-tunnel sphere-trace radius, an interval topology
  certificate (inside box + a closed boundary frame), an origin-Hessian
  curvature cross-check, and localizes the sampled zero set. Panics on
  inadmissible input.
- `try_run_campaign(...) -> Result<NeuroShapeReport, CampaignError>` — the same
  campaign with a structured, non-trapping refusal for untrusted boundaries:
  a non-2-D network, or a non-finite / non-positive `ring_r` / negative `inner`.
- `NeuroShapeReport::safe_step: SafeStepDerivation` — the replayable no-tunnel
  step derivation from `fs_rep_neural::derive_safe_step`, carrying the origin
  enclosure, the certified `magnitude_lower_bound` on `|f(0)|`, the Lipschitz
  upper bound, the downward-rounded `radius`, and a `SafeStepStatus`.
  `origin_value` remains alongside it as a nominal visualization value with no
  certificate authority.
- `NeuroShapeReport::field_identity` plus the activation/safe-step semantics
  versions, names, and ULP budget — the arithmetic a replay must reproduce.
- `CertifiedEnclosedComponentExists` — a private-field witness constructed only
  when the central box is interval-negative, all four strips of the closed frame
  are interval-positive, the intervals are finite and ordered, and the central
  box lies strictly inside the frame. It proves one negative component exists
  and is enclosed by the frame.
- `ComponentCountEvidence` — non-exhaustive typed state: `Unknown` has lower
  bound zero; `LowerBound(CertifiedEnclosedComponentExists)` has lower bound one.
  `exact_count()` is always `None` in this tranche.
- `NEUROSHAPE_COMPONENT_EVIDENCE_SCHEMA_VERSION = 1` — the public semantic
  version for component evidence. Version 1 means that an enclosed-component
  witness supplies a global lower bound only and never an exact count. Any
  serialized adapter must carry this value and version-aware consumers must
  reject versions they do not implement.

## Invariants

- SOUND SPHERE TRACING: `safe_step.radius()` under-estimates the distance to the
  NEAREST surface point (no tunneling). Its authority is the INTERVAL sign margin
  at the origin — `magnitude_lower_bound` is an endpoint of the degenerate IBP
  enclosure `eval_interval([0,0], [0,0])`, hence a certified lower bound on
  `|f(0)|` — divided by the certified Lipschitz upper bound `L` and rounded DOWN.
  The nominal `origin_value` is NOT the certificate: `|origin_value|/L` is an
  ordinary round-to-nearest forward pass whose own evaluation error is
  unaccounted for, and it can exceed the true `|f(0)|/L`. An enclosure that does
  not exclude zero yields `radius = 0` with a `SafeStepStatus` that says so; no
  step is ever published without a certified sign margin.
- TOPOLOGY: a certified-inside central box (`hi < 0`) enclosed by FOUR edge
  strips (`lo > 0`) that tile the box boundary into a CLOSED frame proves that
  the connected component meeting the central box exists and cannot cross the
  frame. `MlpSdf` is continuous (affine maps composed with `tanh`), so the
  connected negative central square lies in one negative component and every
  path from it to the exterior crosses the positive frame. Therefore the global
  component count is at least one. This does not bound the whole negative set
  and does not exclude disconnected components either inside or outside the
  frame.
- `component_count_evidence` is `LowerBound(witness)` only when the typed
  interval-frame witness exists, and it never carries an exact count. A
  too-small/open/invalid frame yields typed `Unknown` with lower bound zero.
- `boundary_frame_certified` says exactly that all four frame strips are
  certified positive. It replaces the ambiguous former field `bounded`.
- A positive-definite finite-difference Hessian at the origin is curvature
  evidence only. Without a certified zero gradient it does not establish a
  critical point or minimum, much less uniqueness or a component count.
- Deterministic (fixed net + grid; no RNG).

## Error model

Total on the demo net; `eval_interval`/`classify_hessian` are total.
`try_run_campaign` returns a typed `CampaignError` for a wrong input dimension or
a non-finite/out-of-range geometric parameter; `run_campaign` panics on the same
inputs. Untrusted boundaries (the WASM export) must call the fallible form.

## Determinism class

Fully deterministic (G5).

## Cancellation behavior

None (a synchronous batch).

## Unsafe boundary

None; `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/neuroshape.rs` (5): G0 pins component-evidence schema version 1, typed
lower-bound state, and the private witness payload for the certified frame,
including explicit refusal to return an exact count; Lipschitz / interval
sign-margin safe-step / enclosure checks, including that the published step
derivation is the one derived from the origin enclosure and that the report's
field identity is the net's; an open frame yields typed `Unknown`; admission
refuses a non-2-D net and non-finite/out-of-range geometry; G5 determinism
includes the field identity, the safe-step status/radius/margin bits, and the
typed topology evidence.

## No-claim boundaries

2-D demo net; the Lipschitz bound is the (loose) product-of-spectral-norms. The
interval-frame certificate proves that at least one enclosed negative component
exists. It does NOT prove the full negative set is bounded, exclude exterior or
additional interior components, establish a finite upper component-count bound,
or certify any exact component count. The sampled contour is localization only;
the finite-difference Hessian is not a critical-point or global Morse/Conley
certificate. There is no
complete admitted domain cover, exterior sign certificate, unresolved-cell
accounting, cubical homology witness, refinement-stability witness, sheaf-glued
coverage proof, cancellation protocol, durable replay receipt, or source-bound
exact-topology identity in this tranche. The constructor-sealed witness itself
has no source/field identity, units, budget, schema, or authenticated issuer and
is therefore campaign-local candidate data, not a portable authority receipt.
Those are required before an
`ExactComponentCount` state may exist.
