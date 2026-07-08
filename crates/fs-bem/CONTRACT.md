# fs-bem CONTRACT

## Purpose and layer

Layer: L3 (FLUX). Laplace BEM panel methods (plan §8.3 [F], bead
tfz.20): potential-flow screening for exterior aerodynamics — the
ornithoid flagship's wide-search stage. INVISCID HONESTY LABELS apply
everywhere: this is screening, not a viscous truth source.

## Public types and semantics

- `panel3d`: `SpherePanels` (centroid/normal/area panelization of
  fs-rep-mesh icospheres); `dense_matrix` — the collocation Neumann
  operator with the outside-limit jump −σ/2 on the diagonal and
  centroid-monopole off-diagonal rows (screening-grade; measured
  convergence is the gate); `fmm_matvec` — the SAME operator through
  three fs-fmm gradient-kernel passes dotted with target normals;
  `fmm_transpose_matvec` — the adjoint operator through the same FMM
  kernels, with panel-area placement and gradient antisymmetry tested
  against the dense transpose;
  `solve_exterior` — GMRES over the FMM matvec; `surface_velocity`.
- `panel2d`: `Airfoil2d` + `naca4_symmetric`; Hess–Smith `solve` —
  constant sources per panel plus one shared vortex density, the KUTTA
  row closing the system (equal tangential speeds leaving the two
  trailing-edge panels; circulation DETERMINED, not assumed); lift by
  PRESSURE INTEGRATION of the enforced surface field (the Γ-accounting
  shortcut was measurably wrong bookkeeping and is gone);
  `dcl_dalpha_adjoint` — one transposed solve for the solution
  sensitivity plus solve-free output partials, FD-gated. The
  constant-panel integrals carry a battery-pinned lesson: the normal
  component is (θ₂−θ₁)/2π — the reversed order self-cancels a closed
  sheet's field (caught by the single-panel-vs-quadrature and
  uniform-sheet probes).
- `wake2d`: `WakeSim` — impulsive-start free wake; Kelvin-conserving
  trailing-edge shedding, regularized point-vortex convection, the
  quasi-steady bound circulation relaxing against wake downwash;
  ledgered traces.

## Invariants

1. G0 Gauss identity: the assembled Neumann operator applied to ones
   gives −1 at every centroid within discretization tolerance
   (bem-001) — sign conventions cannot drift silently.
2. Sphere analytic (G2): mean surface-speed error vs 1.5·U·sinθ
   < 0.03 at 1280 panels and decreasing under refinement (bem-002).
3. The FMM path IS the dense operator: matvec and transpose relative
   deviations are < 1e-4 at order 6; GMRES(FMM) reproduces the
   dense-LU solution to < 1e-3 with iterations ledgered (bem-003).
4. Hess–Smith: lift slope within 5% of the thickness-corrected
   2π(1+0.77t) and above thin-airfoil 2π; stagnation Cp = 1 within 5%;
   Kutta row satisfied to roundoff; adjoint dCl/dα matches central FD
   to 1e-6 (bem-004).
5. Free wake: Wagner-like start (first/steady in [0.3, 0.7]),
   asymptote within [0.9, 1.05] of the steady Kutta circulation,
   coarse-grained monotone growth (early lumped-starting-vortex dips
   are ledgered, not hidden), bounded stable roll-up, bitwise
   determinism (bem-005).

## Error model

Structured panics on programmer contracts (degenerate panels,
singular systems name themselves via the LU refusal). Physical
honesty: inviscid screening labels in every battery row; no viscous
claims anywhere.

## Determinism class

Bit-deterministic across runs on a platform (dense LU, deterministic
FMM underneath, fixed shedding/convection order).

## Cancellation behavior

Bounded synchronous solves and stepped simulations with plain
cloneable state; chunked Cx polling is the fs-exec driver's.

## Unsafe boundary

`#![deny(unsafe_code)]` via workspace lints; no capsules.

## Feature flags

None.

## Conformance tests

`src/panel3d.rs` unit test: the private `LinearOp::apply_transpose`
wrapper matches the dense transpose. `tests/battery.rs`: bem-001 Gauss
identity; bem-002 sphere analytic; bem-003 FMM-vs-dense matvec,
transpose + GMRES; bem-004 Hess–Smith slope band, Cp sanity, Kutta,
adjoint gate; bem-005 impulsive-start free wake.

## No-claim boundaries

- 3D LIFTING surfaces (Kutta strips, wake SHEETS) and the fs-vpm
  pairing for flapping gaits — the 2D shedding loop ships; 3D is the
  flagship successor.
- Exact panel-integral far fields (centroid monopoles ship for
  off-diagonal rows; analytic quadrilateral/triangle integrals are
  follow-up under the same operator surface).
- Induced-drag decomposition and force/moment beyond lift (Cp
  machinery exists; the Trefftz-plane analysis is successor scope).
- Elastostatic BEM (staged later per the bead, noted not promised).
- XFOIL-class viscous corrections (never claimed — screening only).
