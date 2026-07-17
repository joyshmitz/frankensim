# CONTRACT: fs-thrust-e2e

CertQD-Thrust — a certified quality-diversity discovery campaign for
self-propelling point-vortex thrusters. An L6 (HELM) capstone that orchestrates
physics × illumination × fidelity-management × certificates × provenance.

## Purpose and layer

Layer L6 (HELM orchestration). Depends downward only: `fs-vpm` (L3 physics),
`fs-archive` (L4 QD), `fs-surrogate` (L4 certify-or-escalate), `fs-evidence`
(L1 colors), `fs-report` (L6 notebook). Ambition tag `[F]` (frontier synthesis;
the physics is a 2-D inviscid smoke tier).

## Public types and semantics

- `Design { gamma, d, l, ratio }` — a four-vortex quadrupole thruster; `vortices`
  (leading `±Γ` dipole + trailing `±Γ·ratio` dipole, total circulation 0),
  `budget` (`Σ|Γᵢ|`), `descriptor` (`[budget, length]`).
- `SimResult { drift, impulse_error }`; `simulate_thrust(design, steps, dt,
  core)` — RK4 vortex sim → net `x`-drift of the vortex mean + the
  linear-impulse-conservation error.
- `CampaignBudget` — full/short horizons, `dt`, core, bins, conformal `alpha`,
  `decision_tol`, `conserve_tol`, seed (the Five Explicits).
- `design_grid()` — the deterministic design sweep.
- `run_campaign(&CampaignBudget) -> CampaignReport` — the whole campaign;
  `CampaignReport` carries coverage, QD-score, best design + drift, verified vs
  estimated elite tallies, full/short sim counts, steps spent vs all-full, the
  conformal band, the certified drift envelope, the campaign color rank, and the
  content-addressed lab-notebook Markdown.

## Invariants

- PHYSICS: a four-vortex thruster with a leading dipole self-propels in `+x`
  (`drift > 0`); a converged inviscid sim conserves the exact linear impulse
  `I = (Σ Γᵢ yᵢ, −Σ Γᵢ xᵢ)` to a small error.
- CERTIFICATES: an escalated full sim earns a `Verified` drift band iff drift,
  impulse error, impulse scale, and `conserve_tol` are finite; the error, scale,
  and tolerance satisfy error ≥ 0, scale > 0, and tolerance ≥ 0; the relative
  error is within the inclusive tolerance; and the resulting interval is finite
  and ordered. Malformed state fails closed to an infinite-dispersion
  `Estimated` no-claim. A surrogate estimate is always `Estimated`.
- CERTIFY-OR-ESCALATE: the short surrogate is used only for designs inside the
  calibration validity hull when the conformal band is within `decision_tol`;
  everything else escalates — so the campaign spends strictly fewer integration
  steps than a naive all-full sweep whenever any design is served by the
  surrogate, at equal answer quality. The default eight-residual calibration
  uses binary-exact `alpha = 0.125`, safely above the `1/(n+1)` minimum required
  for the retained eighth order statistic; it never relies on rank clamping.
- NO LAUNDERING: the campaign-level color rank is the weakest elite color
  (`min` over `ColorRank`); the certified envelope is a `Hull` `compose` of the
  Verified bands and can never outrank `Verified`.
- DETERMINISM: no RNG; a fixed design grid + fixed physics ⇒ the notebook content
  hash and all metrics are bit-stable across runs.

## Error model

Total functions; `run_campaign` never panics on the default/grid inputs
(`conformal_band` is fed a non-empty calibration residual set by construction).

## Determinism class

Fully deterministic (G5): the sweep, sims, archive, colors, and notebook are pure
functions of `CampaignBudget`.

## Cancellation behavior

None here (a synchronous batch campaign); the production path would poll `Cx` at
sim-tile boundaries.

## Unsafe boundary

None. `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/thrust.rs` (4 cases): a four-vortex thruster self-propels and conserves
impulse; the campaign illuminates a certified diverse family (coverage/QD-score,
best drift > 0, both fidelities used, step savings, verified+estimated tally,
certified envelope, no-laundering rank, content-addressed notebook); the campaign
is deterministic (identical content hash + metrics across runs); and a public
`+∞` conservation tolerance cannot mint any Verified elite. The in-source
certificate battery additionally pins malformed public `SimResult` fields,
invalid scales/tolerances, interval overflow refusal, and valid inclusive
boundary behavior, plus admissibility of the default conformal order statistic.

Representative run (default budget): 28 niches, coverage 0.44, best drift ≈ 9.0,
19 Verified + 9 Estimated elites, 108 full / 84 surrogate sims, ≈32% integration-
step savings vs all-full, certified drift envelope ≈ [1.3, 9.0].

## No-claim boundaries

- The physics is `fs-vpm`'s 2-D INVISCID point-vortex core (no viscosity, no
  free surface, no body); "self-propulsion" is the vortex mean-position drift of
  a zero-total-circulation cluster, not a solid swimmer. A hybrid BEM+VPM body,
  viscous PSE, and 3-D filaments are the fuller physics, staged in `fs-vpm`.
- The surrogate is a linear time-extrapolation of a short-horizon sim; a POD/
  neural operator surrogate is `fs-surrogate`'s fuller deliverable.
- The design sweep is a regular grid; a QD variation/emitter loop (CMA-ME) over
  the archive is the fuller illumination.
- The lab notebook is `fs-report`'s v0 (deterministic Markdown + reproducing IR);
  ledger persistence and semantic design diffs are downstream integrations.
