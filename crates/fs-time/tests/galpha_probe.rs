//! Generalized-α convergence regression across the ρ∞ range (kept from
//! the tfz.12 probe that diagnosed the battery's period-point metric
//! blindness: at t = 2π, cos′ = 0, so a q-only error measures phase
//! error QUADRATICALLY and fakes order ≈ 4; max(q, v) error is the
//! honest metric and shows clean order 2 at every ρ∞).

use fs_time::{GeneralizedAlpha, galpha_step};

fn err_at(rho: f64, h_nom: f64, t_end: f64) -> f64 {
    let steps = (t_end / h_nom).round() as usize;
    let hh = t_end / steps as f64;
    let ga = GeneralizedAlpha::new(&[1.0], &[0.0], &[1.0], 1, hh, rho);
    let (mut q, mut v, mut a) = (vec![1.0f64], vec![0.0f64], vec![-1.0f64]);
    let f = vec![0.0f64];
    for _ in 0..steps {
        galpha_step(&ga, &mut q, &mut v, &mut a, &f);
    }
    let (qe, ve) = (t_end.cos(), -t_end.sin());
    (q[0] - qe).abs().max((v[0] - ve).abs())
}

#[test]
fn galpha_order_two_across_rho_range() {
    for &rho in &[0.0f64, 0.3, 0.5, 0.8, 1.0] {
        for &t in &[1.7f64, std::f64::consts::TAU] {
            let (e1, e2, e3) = (
                err_at(rho, 0.04, t),
                err_at(rho, 0.02, t),
                err_at(rho, 0.01, t),
            );
            let (o1, o2) = ((e1 / e2).log2(), (e2 / e3).log2());
            println!(
                "{{\"suite\":\"fs-time\",\"case\":\"galpha-order-sweep\",\"verdict\":\"info\",\
                 \"detail\":\"rho={rho} T={t:.3} errs={e1:.3e}/{e2:.3e}/{e3:.3e} \
                 orders={o1:.2},{o2:.2}\"}}"
            );
            assert!(
                (o1 - 2.0).abs() < 0.15 && (o2 - 2.0).abs() < 0.15,
                "generalized-alpha order off at rho={rho}, T={t}: {o1:.2}, {o2:.2}"
            );
        }
    }
}

#[test]
fn galpha_reaches_the_static_response_under_a_constant_load() {
    // Regression: the external load enters the equilibrium RHS with COEFFICIENT
    // 1 (f_next is already the load at t+(1-af)h). A stray (1-af) scaling drove
    // the steady state to (1-af)*f/K instead of f/K — wrong for any nonzero load
    // with rho_inf > 0 (load effectively halved at rho_inf=1), and invisible to
    // the free-vibration (f=0) tests. 1-DOF damped SDOF settles to the static
    // K*q = f, i.e. q -> f/K for EVERY rho_inf.
    let (mm, cc, kk, load) = (1.0, 0.5, 2.0, 3.0);
    let target = load / kk;
    for &rho in &[0.0, 0.3, 0.5, 0.8, 1.0] {
        let ga = GeneralizedAlpha::new(&[mm], &[cc], &[kk], 1, 0.05, rho);
        let (mut q, mut v, mut a) = (vec![0.0], vec![0.0], vec![0.0]);
        let f = vec![load];
        for _ in 0..4000 {
            galpha_step(&ga, &mut q, &mut v, &mut a, &f);
        }
        assert!(
            (q[0] - target).abs() < 1e-6,
            "rho_inf={rho}: static response {} != f/K = {target}",
            q[0]
        );
    }
}
