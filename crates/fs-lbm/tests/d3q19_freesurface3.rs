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

/// JSONL verdict record, matching the 2-D extension battery's format.
fn verdict(name: &str, pass: bool, details: &str) {
    println!("{{\"test\":\"{name}\",\"pass\":{pass},\"details\":\"{details}\"}}");
    assert!(pass, "{name}: {details}");
}

/// Linear interpolation of the in-repo Martin-Moyce reference curve.
fn martin_moyce_reference(t_star: f64) -> Option<f64> {
    let raw = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/reference/martin-moyce-1952.jsonl"),
    )
    .expect("in-repo Martin-Moyce reference data present");
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for line in raw.lines() {
        // Minimal field scrape (no JSON dep in this battery): data rows
        // look like {"t_star": 0.41, "z": 1.11}.
        let Some(t_idx) = line.find("\"t_star\":") else {
            continue;
        };
        let Some(z_idx) = line.find("\"z\":") else {
            continue;
        };
        let t: f64 = line[t_idx + 9..]
            .split(&[',', '}'][..])
            .next()
            .expect("t field")
            .trim()
            .parse()
            .expect("t value parses");
        let z: f64 = line[z_idx + 4..]
            .split(&[',', '}'][..])
            .next()
            .expect("z field")
            .trim()
            .parse()
            .expect("z value parses");
        pts.push((t, z));
    }
    assert!(pts.len() >= 10, "reference data unexpectedly short");
    if t_star < pts[0].0 || t_star > pts[pts.len() - 1].0 {
        return None;
    }
    let after = pts.iter().position(|&(t, _)| t >= t_star)?;
    if after == 0 {
        return Some(pts[0].1);
    }
    let (t0, z0) = pts[after - 1];
    let (t1, z1) = pts[after];
    Some(z0 + (z1 - z0) * (t_star - t0) / (t1 - t0))
}

/// lbm3-105: the 3-D Martin-Moyce dam-break battery. HARD GATE = the
/// exact band the 2-D battery (lbm-105) uses at coarse lattice: the
/// nondimensional front z = x/a advances monotonically after the initial
/// transient and stays under the broad upper envelope 2.2*t*+1 for
/// 0.5 < t* < 2. The in-repo digitized Martin-Moyce reference curve is
/// compared REPORT-ONLY (max relative deviation in the verdict detail):
/// a quantitative central band is fine-lattice validation scope, exactly
/// as the 2-D battery states. Gauntlet tier: G2 (canonical benchmark,
/// coarse-lattice honesty).
#[test]
fn lbm3_105_martin_moyce_front() {
    let a = 8usize; // column base (cells)
    let g = 5e-5;
    let mut fs = FreeSurface3::new(
        56,
        8,
        24,
        0.55,
        [0.0, 0.0, -g],
        0.0,
        ContactModel3::Neutral,
        column(8, 8, 16), // base a, height 2a: the n^2 = 2 geometry
    )
    .expect("fixture admissible");
    let m0 = fs.ledger_mass();
    #[allow(clippy::cast_precision_loss)]
    let tstar = |t: usize| (t as f64) * (2.0 * g / a as f64).sqrt();

    let mut ok = true;
    let mut detail = String::new();
    let mut checked = 0u32;
    let mut last_z = 1.0f64;
    let mut worst_ref_dev = 0.0f64;
    for t in 1..=600 {
        fs.step()
            .unwrap_or_else(|e| panic!("step {t} refused: {e}"));
        let ts = tstar(t);
        if ts > 0.5 && ts < 2.0 && t % 75 == 0 {
            #[allow(clippy::cast_precision_loss)]
            let z = fs.surge_front_x(4).expect("wet bottom slab") as f64 / a as f64;
            let hi = 2.2f64.mul_add(ts, 1.0);
            use std::fmt::Write as _;
            let _ = write!(detail, "t*={ts:.2}: z={z:.2} <= {hi:.2}; ");
            if z + 1e-12 < last_z || z > hi {
                ok = false;
            }
            if let Some(zref) = martin_moyce_reference(ts) {
                worst_ref_dev = worst_ref_dev.max(((z - zref) / zref).abs());
            }
            last_z = z;
            checked += 1;
        }
    }
    let drift = ((fs.ledger_mass() - m0) / m0).abs();
    use std::fmt::Write as _;
    let _ = write!(
        detail,
        "checked={checked}; worst MM ref deviation {worst_ref_dev:.2} (report-only); \
         ledger drift {drift:.2e} (worst step {:.2e})",
        fs.worst_step_violation()
    );
    verdict(
        "lbm3-105-martin-moyce-front",
        ok && checked >= 3 && drift < 1e-9,
        &detail,
    );
}
