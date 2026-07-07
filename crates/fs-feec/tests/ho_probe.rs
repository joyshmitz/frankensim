//! Per-mode convergence regression (kept from the tfz.6 probe that
//! separated pre-asymptotics from bugs): both the (1,1,1) and the
//! mixed (2,1,3) Laplacian eigenmodes must converge at order → r+1
//! on m ≥ 2 ladders at r = 2 — the diagnosis that pinned the MMS
//! battery's ladder policy (single-cell parity superconvergence and
//! coarse-mesh pre-asymptotics are metric traps, not method bugs).
use fs_feec::highorder::hex::{TensorSpace, pcg_matfree};

fn solve_err(
    m: usize,
    r: usize,
    u_exact: &dyn Fn([f64; 3]) -> f64,
    f_exact: &dyn Fn([f64; 3]) -> f64,
) -> f64 {
    let sp = TensorSpace::new(m, r);
    let b = sp.load(&|p| f_exact(p));
    let mask = sp.interior_mask();
    let diag = sp.stiffness_diagonal();
    let mut bm = b;
    for (bi, &mk) in bm.iter_mut().zip(&mask) {
        if !mk {
            *bi = 0.0;
        }
    }
    let mut x = vec![0.0f64; sp.ndof()];
    let (it, conv) = pcg_matfree(
        &|v| sp.apply_stiffness(v),
        &bm,
        &mut x,
        &mask,
        &diag,
        1e-13,
        40_000,
    );
    assert!(conv, "pcg failed m={m} r={r} it={it}");
    sp.l2_error(&x, &|p| u_exact(p))
}

#[test]
fn per_mode_orders_reach_asymptotics() {
    let pi = std::f64::consts::PI;
    // Mode A: (1,1,1); Mode B: (2,1,3).
    let ua = move |p: [f64; 3]| (pi * p[0]).sin() * (pi * p[1]).sin() * (pi * p[2]).sin();
    let fa = move |p: [f64; 3]| 3.0 * pi * pi * ua(p);
    let ub =
        move |p: [f64; 3]| (2.0 * pi * p[0]).sin() * (pi * p[1]).sin() * (3.0 * pi * p[2]).sin();
    let fb = move |p: [f64; 3]| 14.0 * pi * pi * ub(p);
    for (name, u, f) in [
        (
            "A(1,1,1)",
            &ua as &dyn Fn([f64; 3]) -> f64,
            &fa as &dyn Fn([f64; 3]) -> f64,
        ),
        ("B(2,1,3)", &ub, &fb),
    ] {
        let mut prev: Option<f64> = None;
        for m in [2usize, 4, 8] {
            let e = solve_err(m, 2, u, f);
            let slope = prev.map(|p: f64| (p / e).ln() / (2.0f64).ln());
            println!(
                "{{\"suite\":\"fs-feec-ho\",\"case\":\"mode-sweep\",\"verdict\":\"info\",\"detail\":\"{name} r=2 m={m} L2={e:.4e} slope={slope:?}\"}}"
            );
            if m == 8 {
                let s = slope.expect("ladder ran");
                assert!(s > 2.6, "mode {name}: asymptotic slope {s:.2} below gate");
            }
            prev = Some(e);
        }
    }
}
