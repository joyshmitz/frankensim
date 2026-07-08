# fs-bo CONTRACT

## Purpose and layer

Layer: **L4 ASCENT** (deps: fs-ascent/fs-dfo L4, fs-la/fs-rand L1,
fs-math L0). Bayesian optimization (plan §9.4 [F]): in-house
Gaussian processes with exact Cholesky inference, deterministic
acquisitions, and BO loops that beat baselines with ledgered
evidence and replay bitwise. Inner optimizers come from the landed
stack: fs-ascent L-BFGS for hyperparameters, fs-dfo CMA-ES for
acquisition surfaces.

## Public types and semantics

- `Kernel`/`Matern` — Matérn ½, 3⁄2, 5⁄2 with ARD lengthscales and
  signal variance; the r → ∞ limit is guarded (degenerate
  lengthscales during hyperparameter search hit inf·0 = NaN in the
  polynomial×exp forms — the guard returns the true limit 0; the
  first draft crashed as a fake NotSpd mid-L-BFGS).
- `Gp` — exact zero-mean GP: `fit`/`try_fit` (Cholesky of
  K + σ_n²I; try_fit makes NotSpd a REJECTABLE hyperparameter
  candidate, not a crash — near-duplicate exploitation points with
  tiny noise are a normal BO condition), `predict` (mean/variance
  via triangular solves), `predict_joint` (posterior mean + jittered
  covariance Cholesky — the q-EI reparameterization substrate),
  `lml` (log marginal likelihood).
- `fit_hyperparams` — maximize LML over log-parameters by fs-ascent
  L-BFGS with FD gradients (≤ D+2 params — cost trivial at fixture
  scale; analytic LML gradients recorded follow-up), QMC-multistarted
  from scrambled-Sobol points (the plan's named detail); clamped
  exponentiation with a 1e−8 noise floor.
- `acq` — closed-form EI; Φ via Abramowitz–Stegun 7.1.26 (~1.5e−7
  absolute) and Φ⁻¹ via Acklam (~1e−9 relative), both DETERMINISTIC
  polynomials with documented accuracy — no platform libm;
  `normal_bank` (scrambled Sobol through Φ⁻¹ — fixed common random
  numbers); `q_expected_improvement` through the Cholesky
  reparameterization f = μ + L·z over the fixed bank.
- `bo::minimize` — Sobol initialization, per-iteration y
  STANDARDIZATION (EI is affine-invariant when applied consistently;
  without it the signal box cannot span arbitrary objective scales —
  measured: raw-y BO LOST to random on Branin), hyperparameter refit,
  acquisition argmax by CMA-ES restarts, greedy q-EI batch growth
  under common random numbers. Deterministic per seed.

## Invariants

- Kernel matrices are PSD on distinct points (G0-gated across all
  three families); posterior consistency: fitting the union of
  datasets in any order yields the same posterior.
- Acquisition surfaces are deterministic functions (fixed z-banks);
  whole BO runs replay bitwise.
- q-EI is monotone under batch growth (fixed bank) and matches
  closed-form EI at q = 1 to MC tolerance.

## Error model

Structured panics on dimension mismatches and direct `fit` of
non-SPD systems; the hyperparameter search path uses `try_fit`
(rejection, not crash). `phi_inv` asserts p ∈ (0,1).

## Determinism class

Bit-deterministic per seed; golden FNV-64 over GP posteriors,
acquisitions, and a short BO run: `0x4f5a_0601_3cd1_6f46`, recorded
on Apple M4 Pro, verified on Threadripper (x86_64).

## Cancellation behavior

Iteration-granular: the BO loop is resumable between batches (all
state is the (X, y) history); inner optimizers are the landed
resumable/deterministic engines. Cx wiring is driver scope.

## Unsafe boundary

None. `unsafe_code = "deny"`.

## Feature flags

None.

## Conformance tests

`tests/bo_battery.rs` (7 cases): kernel PSD across the family × 40
random points; posterior union-order consistency ≤ 1e−8; known-answer
posteriors (noiseless interpolation mean == y and var → 0 at data;
one-point closed form to 1e−12); Φ table values + Φ⁻¹ round-trip ≤
1e−6; EI ≥ 0, EI ≈ 0 at data, q-EI batch dominance + q-EI(1) vs
closed-form EI within 5% MC tolerance; EI-BO beats scrambled-Sobol
random search on Branin at matched budget over a fixed seed set
(median 0.80 vs 1.44, optimum 0.398 — the ledgered comparison) with
whole-run bitwise replay; cross-ISA golden hash.

## No-claim boundaries

- TuRBO trust-region BO (30–300d), multi-fidelity cost-aware
  acquisition with discrepancy models, inducing-point sparse GPs
  beyond ~10⁴ points, heteroscedastic likelihoods, and e-process
  stopping are the bead's recorded follow-up lanes.
- Acquisition gradients are derivative-free (CMA-ES); the
  FrankenTorch reparameterized-gradient tape through q-EI is the
  named follow-up (the fixed-bank surfaces are already
  differentiable-by-construction when the tape lands).
- q-NEI (noisy EI) is not implemented — EI with the standardized
  noise floor covers the deterministic-objective regime shipped
  here.
- No hyperparameter marginalization (point estimates by LML); no
  input warping.
