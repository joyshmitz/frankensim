# SAFETY — fs-simd NEON transpose capsule

Scope: `crates/fs-simd/src/neon/transpose/mod.rs` (`trn1c64`,
`gath8c64`, `scat8c64`), compiled
only on `target_arch = "aarch64"` where NEON is baseline — no runtime
feature gate is needed for `vld1q_f64`/`vst1q_f64`.

## Invariants the `unsafe` block relies on

1. **Bounds.** `checked_trn1c64_len` constructs `2·n1²` without wrapping,
   and both slice lengths are asserted equal to it before any pointer math.
   Loop structure guarantees `i < n1` and `j < n1`,
   so every element index `j·n1 + i` and `i·n1 + j` is `< n1²`, and the
   dereferenced f64 offsets `2·idx` and `2·idx + 1` (one q-register =
   two f64) are strictly inside both slices.
2. **Aliasing.** `src: &[f64]` and `dst: &mut [f64]` are simultaneous
   shared and exclusive borrows, so the slices cannot overlap; loads
   and stores never alias.
3. **Alignment.** `vld1q_f64`/`vst1q_f64` are unaligned-tolerant on
   AArch64; no alignment precondition is assumed.
4. **No arithmetic.** The capsule performs exact 16-byte moves only —
   bitwise equality with the scalar twin is move equality, gated in
   `tier_equivalence_battery` (square, non-multiple-of-8, and n1 = 1
   edges).

## Invariants: gath8c64 / scat8c64 (bead 27d3)

Same discipline as `trn1c64`: lengths asserted up front (`2·n1²` matrix,
`16·n1` buffer, `g + 8 <= n1`), loop bounds keep every complex index
inside both slices, borrows forbid aliasing, and the intrinsics are
unaligned-tolerant. Pure 16-byte moves — no arithmetic; bitwise equality
with the scalar twins gated in the tier battery (gather, scatter, and
scatter∘gather = identity on the touched columns).

## Twin

`scalar::trn1c64` implements the identical tiling and iteration order
with bounds-checked indexing; the battery asserts bitwise equality of
the full destination for every tested shape.
