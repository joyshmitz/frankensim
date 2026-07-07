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
