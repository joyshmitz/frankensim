//! End-to-end battery: a PDHG-certified minimum-weight cantilever with a
//! tropical critical load path from load to support.

use fs_evidence::Color;
use fs_truss_e2e::run_campaign;

#[test]
fn the_optimal_truss_has_a_certified_load_path() {
    let report = run_campaign(4, 3, 4.0, 2.0, 1e-4);
    // a real ground structure was optimized down to a sparse active set.
    assert!(report.num_members > report.num_active, "nothing was pruned");
    assert!(report.num_active > 0, "no active bars");
    assert!(report.total_volume > 0.0, "zero volume");
    // OPTIMALITY, CERTIFIED: PDHG closed the duality gap and equilibrium holds.
    assert!(
        report.certified_optimal,
        "gap {} eq_res {}",
        report.gap, report.eq_residual
    );
    assert!(matches!(report.optimality_color, Color::Verified { .. }));
    // LOAD PATH, CERTIFIED: a non-trivial critical chain carrying real volume,
    // with a named bottleneck bar.
    assert!(
        report.critical_path.len() >= 2,
        "path too short: {:?}",
        report.critical_path
    );
    assert!(report.critical_path_volume > 0.0);
    assert!(report.bottleneck_member.is_some());
    assert!(
        report
            .critical_path
            .contains(&report.bottleneck_member.unwrap())
    );
    assert!(matches!(report.load_path_color, Color::Verified { .. }));
    // the critical path carries no more than the whole structure.
    assert!(report.critical_path_volume <= report.total_volume + 1e-6);
    println!(
        "{{\"campaign\":\"trusspath\",\"members\":{},\"active\":{},\"volume\":{:.4},\"gap\":{:.2e},\
         \"eq_res\":{:.2e},\"iters\":{},\"path_len\":{},\"path_volume\":{:.4},\"bottleneck\":{:?}}}",
        report.num_members,
        report.num_active,
        report.total_volume,
        report.gap,
        report.eq_residual,
        report.iters,
        report.critical_path.len(),
        report.critical_path_volume,
        report.bottleneck_member,
    );
}

#[test]
fn the_campaign_is_deterministic() {
    let a = run_campaign(4, 3, 4.0, 2.0, 1e-4);
    let b = run_campaign(4, 3, 4.0, 2.0, 1e-4);
    assert_eq!(a.total_volume.to_bits(), b.total_volume.to_bits());
    assert_eq!(a.critical_path, b.critical_path);
    assert_eq!(a.bottleneck_member, b.bottleneck_member);
}
