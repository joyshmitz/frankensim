//! Battery for scientific visualization (fs-viz). Each test checks a primitive
//! against ANALYTIC ground truth: rotation streamlines are circles, saddle
//! streamlines conserve xy, Hessian classification recovers the known Morse
//! type, and a circle-SDF isocontour lies on the circle.

use fs_viz::{CriticalKind, Grid2, classify_hessian, streamline};

fn radius(p: [f64; 2]) -> f64 {
    (p[0] * p[0] + p[1] * p[1]).sqrt()
}

#[test]
fn a_rotation_field_streams_along_a_circle() {
    // u = (-y, x): rigid rotation, so the radius is conserved.
    let line = streamline(|p| [-p[1], p[0]], [1.0, 0.0], 0.01, 400);
    for p in &line {
        assert!(
            (radius(*p) - 1.0).abs() < 1e-3,
            "radius {} drifted",
            radius(*p)
        );
    }
    // it actually goes somewhere (not a fixed point).
    assert!((line.last().unwrap()[1]).abs() > 0.1);
}

#[test]
fn a_saddle_field_conserves_the_hyperbola_invariant() {
    // u = (x, -y): flow x·y is invariant along a streamline.
    let line = streamline(|p| [p[0], -p[1]], [1.0, 1.0], 0.01, 50);
    for p in &line {
        assert!(
            (p[0] * p[1] - 1.0).abs() < 1e-4,
            "xy = {} drifted",
            p[0] * p[1]
        );
    }
    // x grows, y shrinks (the saddle's unstable/stable manifolds).
    assert!(line.last().unwrap()[0] > 1.4 && line.last().unwrap()[1] < 0.7);
}

#[test]
fn hessian_classification_recovers_the_morse_type() {
    let t = 1e-9;
    // f = x² + y²  -> minimum, index 0.
    assert_eq!(
        classify_hessian([[2.0, 0.0], [0.0, 2.0]], t).kind,
        CriticalKind::Minimum
    );
    // f = x² - y²  -> saddle, index 1.
    let s = classify_hessian([[2.0, 0.0], [0.0, -2.0]], t);
    assert_eq!(s.kind, CriticalKind::Saddle);
    assert_eq!(s.morse_index, 1);
    // f = -(x² + y²) -> maximum, index 2.
    assert_eq!(
        classify_hessian([[-2.0, 0.0], [0.0, -2.0]], t).morse_index,
        2
    );
    // f = xy -> saddle (off-diagonal Hessian, eigenvalues ±1).
    assert_eq!(
        classify_hessian([[0.0, 1.0], [1.0, 0.0]], t).kind,
        CriticalKind::Saddle
    );
    // a zero eigenvalue is degenerate.
    assert_eq!(
        classify_hessian([[2.0, 0.0], [0.0, 0.0]], t).kind,
        CriticalKind::Degenerate
    );
}

#[test]
fn a_circle_sdf_isocontour_lies_on_the_circle() {
    // f(x,y) = sqrt(x²+y²) - 1, zero level set is the unit circle.
    let grid = Grid2::from_fn(41, 41, [-2.0, -2.0], [2.0, 2.0], |p| radius(p) - 1.0);
    let crossings = grid.isocontour_crossings(0.0);
    assert!(!crossings.is_empty());
    for c in &crossings {
        assert!(
            (radius(*c) - 1.0).abs() < 0.02,
            "crossing radius {}",
            radius(*c)
        );
    }
    // a level set outside the field's range has no crossings.
    assert!(grid.isocontour_crossings(100.0).is_empty());
}

#[test]
fn the_grid_samples_and_addresses_correctly() {
    let grid = Grid2::from_fn(3, 3, [0.0, 0.0], [2.0, 2.0], |p| p[0] + p[1]);
    let (p00, p22) = (grid.point(0, 0), grid.point(2, 2));
    assert!(p00[0].abs() < 1e-12 && p00[1].abs() < 1e-12);
    assert!((p22[0] - 2.0).abs() < 1e-12 && (p22[1] - 2.0).abs() < 1e-12);
    assert!((grid.at(1, 1) - 2.0).abs() < 1e-12); // (1,1) -> value 1+1
}

#[test]
fn visualization_is_deterministic() {
    let a = streamline(|p| [-p[1], p[0]], [1.0, 0.0], 0.01, 100);
    let b = streamline(|p| [-p[1], p[0]], [1.0, 0.0], 0.01, 100);
    assert_eq!(a.len(), b.len());
    assert_eq!(
        a.last().unwrap()[0].to_bits(),
        b.last().unwrap()[0].to_bits()
    );
}
