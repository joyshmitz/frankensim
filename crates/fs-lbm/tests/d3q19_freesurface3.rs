//! Free-surface 3-D battery (bead sxnm, slice 1): construction closure,
//! mass-ledger conservation (the bead's 1e-9 bar with worst-per-step
//! logging), conversion cascade, tile activation as fluid advances,
//! deterministic replay, and gas-tile retirement. Gauntlet tiers: G0
//! (ledger algebra), G5 (replay determinism).

use fs_lbm::d3q19::freesurface3::{Cell3, ContactModel3, FreeSurface3, FreeSurfaceError3};
use fs_lbm::d3q19::sparse::morton3;

/// A fluid column occupying `[0..cx) × [0..cy) × [0..cz)` cells.
fn column(cx: usize, cy: usize, cz: usize) -> impl Fn(usize, usize, usize) -> bool {
    move |x, y, z| x < cx && y < cy && z < cz
}

#[test]
fn construction_inserts_interface_and_holds_closure() {
    let fs = FreeSurface3::new(
        16,
        16,
        16,
        0.8,
        [0.0; 3],
        0.0,
        ContactModel3::Neutral,
        column(8, 8, 8),
    )
    .expect("fixture admissible");
    // The column's exposed faces must be interface, its interior fluid,
    // and closure holds (asserted internally). Spot-check one interior
    // cell and one face cell.
    let interior_key = morton3(0, 0, 0);
    assert_eq!(fs.cell(interior_key, 0), Cell3::Fluid, "corner interior");
    // Cell (7,0,0) is the +x face of the column: tile (1,0,0) lane x=3.
    let face_key = morton3(1, 0, 0);
    let face_lane = 3; // (lx=3, ly=0, lz=0)
    assert_eq!(fs.cell(face_key, face_lane), Cell3::Interface);
    // Ledger: 8^3 fluid-ish cells at rho 1 minus the half-mass interface
    // shell — exact value checked by conservation tests; here just sanity.
    let ledger = fs.ledger_mass();
    assert!(
        ledger > 0.0 && ledger < 513.0,
        "ledger {ledger} implausible"
    );
}

#[test]
fn all_gas_fixture_refuses() {
    let refusal = FreeSurface3::new(
        16,
        16,
        16,
        0.8,
        [0.0; 3],
        0.0,
        ContactModel3::Neutral,
        |_, _, _| false,
    )
    .expect_err("no fluid must refuse");
    assert_eq!(refusal, FreeSurfaceError3::NoFluid);
}

#[test]
fn resting_puddle_conserves_the_ledger() {
    // A flat puddle with zero gravity: nothing should move, and the
    // ledger must hold to strict tolerance over 40 steps.
    let mut fs = FreeSurface3::new(
        16,
        16,
        16,
        0.9,
        [0.0; 3],
        0.0,
        ContactModel3::Neutral,
        column(16, 16, 4),
    )
    .expect("fixture admissible");
    let initial = fs.ledger_mass();
    for _ in 0..40 {
        fs.step().expect("step admissible");
    }
    let drift = ((fs.ledger_mass() - initial) / initial).abs();
    assert!(
        drift < 1e-9,
        "resting puddle ledger drifted {drift:e} relative over 40 steps \
         (worst step {:e})",
        fs.worst_step_violation()
    );
}

#[test]
fn dam_break_advances_converts_and_conserves() {
    // A 4×16×12 column against the x=0 wall of a 32×16×16 box under
    // -z gravity: the classic collapse. The bead's global bar is 1e-9
    // over the collapse with the worst per-step violation logged.
    let mut fs = FreeSurface3::new(
        32,
        16,
        16,
        0.55,
        [0.0, 0.0, -1e-4],
        0.0,
        ContactModel3::Neutral,
        column(4, 16, 12),
    )
    .expect("fixture admissible");
    let initial = fs.ledger_mass();
    let front_before = fs.wet_extent().expect("wet cells exist").0;
    for step in 0..400 {
        fs.step()
            .unwrap_or_else(|e| panic!("step {step} refused: {e}"));
    }
    let drift = ((fs.ledger_mass() - initial) / initial).abs();
    println!(
        "dam-break ledger drift {drift:e} relative; worst step {:e}; conversions {:?}",
        fs.worst_step_violation(),
        fs.conversions()
    );
    assert!(
        drift < 1e-9,
        "dam-break ledger drifted {drift:e} relative (bar 1e-9); worst step {:e}",
        fs.worst_step_violation()
    );
    let stats = fs.conversions();
    assert!(
        stats.to_fluid + stats.to_gas + stats.gas_to_interface + stats.fluid_to_interface > 0,
        "a collapsing column must convert cells: {stats:?}"
    );
    let front_after = fs.wet_extent().expect("wet cells exist").0;
    assert!(
        front_after > front_before,
        "the front must advance along +x: {front_before} -> {front_after}"
    );
}

#[test]
fn fluid_advance_activates_tiles() {
    // The construction margin is one tile; a hard collapse pushes the
    // front further, forcing activation of tiles beyond the margin.
    let mut fs = FreeSurface3::new(
        48,
        8,
        12,
        0.55,
        [0.0, 0.0, -5e-4],
        0.0,
        ContactModel3::Neutral,
        column(4, 8, 8),
    )
    .expect("fixture admissible");
    let tiles_before = fs.grid().active_tiles();
    for _ in 0..800 {
        fs.step().expect("step admissible");
    }
    let stats = fs.conversions();
    assert!(
        stats.tiles_activated > 0,
        "a long collapse must activate tiles beyond the margin: {stats:?} \
         (active {} -> {})",
        tiles_before,
        fs.grid().active_tiles()
    );
}

#[test]
fn replay_is_deterministic_bitwise() {
    let build = || {
        FreeSurface3::new(
            32,
            16,
            16,
            0.55,
            [0.0, 0.0, -1e-4],
            0.0,
            ContactModel3::Neutral,
            column(4, 16, 12),
        )
        .expect("fixture admissible")
    };
    let mut a = build();
    let mut b = build();
    for _ in 0..60 {
        a.step().expect("step admissible");
        b.step().expect("step admissible");
    }
    assert_eq!(
        a.grid().state_bits(),
        b.grid().state_bits(),
        "population replay diverged"
    );
    assert_eq!(
        a.ledger_mass().to_bits(),
        b.ledger_mass().to_bits(),
        "ledger replay diverged"
    );
    assert_eq!(
        a.conversions(),
        b.conversions(),
        "conversion replay diverged"
    );
}

#[test]
fn gas_tile_retirement_is_ledger_neutral() {
    // The construction margin includes all-gas tiles; retiring them must
    // not move the free-surface ledger and must shrink the active set.
    let mut fs = FreeSurface3::new(
        32,
        16,
        16,
        0.8,
        [0.0; 3],
        0.0,
        ContactModel3::Neutral,
        column(4, 16, 8),
    )
    .expect("fixture admissible");
    let ledger_before = fs.ledger_mass();
    let active_before = fs.grid().active_tiles();
    let retired = fs.retire_gas_tiles().expect("retirement admissible");
    assert!(retired > 0, "the margin must contain all-gas tiles");
    assert_eq!(fs.grid().active_tiles(), active_before - retired);
    let drift = ((fs.ledger_mass() - ledger_before) / ledger_before).abs();
    assert!(
        drift < 1e-15,
        "gas-tile retirement moved the ledger by {drift:e}"
    );
    // And the surface still steps after retirement.
    fs.step().expect("step admissible after retirement");
}

#[test]
fn wetting_contact_and_surface_tension_stay_bounded() {
    // σ > 0 with the wetting ghost: a smoke gate that the curvature port
    // keeps reference densities finite and the ledger conserved over a
    // short run (no physics claim beyond boundedness — 2-D parity).
    let mut fs = FreeSurface3::new(
        16,
        16,
        16,
        0.9,
        [0.0, 0.0, -5e-5],
        0.01,
        ContactModel3::Wetting,
        column(16, 16, 5),
    )
    .expect("fixture admissible");
    let initial = fs.ledger_mass();
    for _ in 0..30 {
        fs.step().expect("step admissible");
    }
    let drift = ((fs.ledger_mass() - initial) / initial).abs();
    assert!(
        drift < 1e-9,
        "σ>0 wetting run drifted {drift:e} relative (worst step {:e})",
        fs.worst_step_violation()
    );
}
