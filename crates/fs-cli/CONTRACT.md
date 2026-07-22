# CONTRACT: fs-cli

The stable command-line membrane for the Cooling 0.1 product workflow (bead
`frankensim-extreal-program-f85xj.6.2`). The binary is named `frankensim`;
the package is `fs-cli` so the workspace retains its flat `fs-*` crate
convention.

## Purpose and layer

Layer L6 (HELM). `fs-cli` turns command-line arguments and project bytes into
deterministic result records and structured diagnostics. It owns presentation
and exit semantics, not project-schema, solver, report, or package authority.
Those remain with `fs-project`, `fs-session` and the cooling pipeline,
`fs-report`, and `fs-package` respectively.

## Public surface

The v0 grammar is intentionally small:

```text
frankensim [--json] validate <project.fsim|project.json>
frankensim [--json] solve <project.fsim|project.json>
frankensim [--json] solve --resume <run-id>
frankensim [--json] report <run-id>
frankensim [--json] package <run-id>
```

`--json` may appear once at any position. Unknown flags, missing operands, and
extra operands are refused. Project inputs are capped at 16 MiB before parsing.
`.fsim` selects the canonical s-expression spelling and `.json` the canonical
JSON spelling; unknown extensions are refused rather than guessed.

`validate` invokes the strict `fs-project` reader and all of its recognition
and semantic checks. A successful result reports the canonical project hash,
schema version, zero findings, and the exact authority class
`structural-project-admission`.

The remaining verbs are present in the parser but currently return the stable
`cli-stage-unavailable` refusal naming the producer Bead that must land before
the verb can execute:

- solve/resume: `frankensim-extreal-program-f85xj.6.5`;
- report: `frankensim-extreal-program-f85xj.6.9`;
- package: `frankensim-extreal-program-f85xj.6.10`.

This is a deliberate fail-closed integration seam. Reusing the photovoltaic
skeleton or emitting placeholder artifacts would turn a CLI-shaped mock into
a product claim.

## Output and exit contract

- stdout carries final result records only;
- stderr carries diagnostics (and will carry solve progress JSON-lines once
  solve orchestration exists);
- JSON mode emits one complete object per line in deterministic field order;
- text mode emits stable `key=value` result rows and `ERROR`/`FIX` diagnostic
  pairs;
- exit `0` is success, `2` usage, `3` input I/O/encoding/size, `4` project
  refusal, and `5` unavailable product stage.

Diagnostic codes and fix text are machine-facing compatibility surface.
Human prose may improve without changing a code or exit class.

## Invariants

- Argument order never changes semantic output except for the documented
  position-independent `--json` flag.
- A successful validation has exactly zero `DecodedProject::findings()` and
  no lenient default or canonicalization receipt, because the CLI uses strict
  readers.
- Every refusal has a non-empty code, message, and suggested fix.
- User-controlled strings are escaped before JSON emission; every JSON record
  is one line.
- No unavailable stage writes a run, report, package, checkpoint, or ledger
  artifact.

## Determinism and cancellation

Argument parsing, validation formatting, and unavailable-stage refusals are
pure functions of arguments and input bytes except for the explicit file read.
They read no clock, RNG, network, or machine state. Validation is bounded by
the 16 MiB CLI input cap but has no asynchronous cancellation surface.

Solve cancellation is not implemented by this checkpoint. It must use the
`fs-session` request -> drain -> finalize protocol, checkpoint on cancellation,
and prove resume equivalence before the solve verb stops returning
`cli-stage-unavailable`.

## Unsafe boundary and features

No unsafe code. No feature flags. Runtime dependencies remain Franken-only.

## Conformance tests

`tests/cli.rs` covers the grammar and all four v0 verbs, stable exit classes,
strict validation success, structural findings with fixes, noncanonical input
refusal, JSON escaping/line discipline, and the exact producer-Bead refusal
for each not-yet-integrated stage.

## No-claim boundaries

- `validate` proves only canonical structural and dimensional admissibility.
  It does not prove referenced artifacts or material cards exist, a requested
  capability is installed, the project is solvable, or any physical model is
  valid.
- The presence of solve/report/package in help and parsing is not an
  implementation claim. Until their named authorities land, execution fails
  before side effects.
- No cancellation, checkpoint/restart, run identity, ledger persistence,
  report rendering, evidence packaging, or end-to-end determinism claim is
  made by this checkpoint.
