//! CCD-locality A/B harness (bead fz2.2): measured L3 topology, pin
//! verification, first-touch page-to-node audit, and the L3-island
//! effect quantified by timing proxy — spread (one worker per CCD,
//! working set inside its own 32 MiB island) vs packed (every worker
//! contending for ONE island) vs unpinned (OS placement).
//!
//! Report-only rows by design: locality changes TIMING, never bits
//! (P2), so the perf gates live in the roofline lanes; this harness
//! proves the MECHANISMS work and ledgers their measured effect.
//! Run: `cargo test -p fs-substrate --release --test ccd_ab -- --ignored --nocapture`

use fs_substrate::affinity::{CcdTopology, measured_l3_groups};
use fs_substrate::os_affinity::{OsAffinityError, current_cpu, page_nodes, pin_current_thread};
use std::time::Instant;

/// Stream-sum over a buffer; returns (checksum, GB/s aggregate for one pass).
fn stream_pass(buf: &[u64]) -> u64 {
    let mut acc = 0u64;
    for &v in buf {
        acc = acc.wrapping_add(v);
    }
    acc
}

/// Aggregate GB/s for `threads` workers each streaming its own buffer
/// `reps` times, with a per-worker pin list (empty = unpinned).
fn measure(buffers: &mut [Vec<u64>], pins: &[Vec<u32>], reps: usize) -> f64 {
    let bytes_total = buffers.iter().map(|b| b.len() * 8 * reps).sum::<usize>();
    let t0 = Instant::now();
    std::thread::scope(|s| {
        for (buf, pin) in buffers.iter().zip(pins) {
            let pin = pin.clone();
            s.spawn(move || {
                if !pin.is_empty() {
                    let _ = pin_current_thread(&pin);
                }
                let mut acc = 0u64;
                for _ in 0..reps {
                    acc = acc.wrapping_add(stream_pass(buf));
                }
                std::hint::black_box(acc);
            });
        }
    });
    bytes_total as f64 / t0.elapsed().as_secs_f64() / 1e9
}

#[test]
#[ignore = "perf harness: run explicitly in release with --ignored"]
fn ccd_locality_ab() {
    // --- Topology: measured where possible, heuristic otherwise. ---
    let groups = measured_l3_groups();
    let (topo, source) = match CcdTopology::from_l3_groups(&groups) {
        Some(t) => (t, "measured-sysfs"),
        None => (
            CcdTopology::from_probe(&fs_substrate::CapabilityProbe::topology_only()),
            "heuristic-probe",
        ),
    };
    println!(
        "{{\"metric\":\"ccd-topology\",\"source\":\"{source}\",\"groups\":{},\"cores_per_group\":{}}}",
        topo.ccds, topo.cores_per_ccd
    );

    // --- Pin verification (structural: refusal or proof, never a no-op). ---
    match current_cpu() {
        Ok(_) => {
            let target = groups.first().map_or(0, |g| g[0]);
            pin_current_thread(&[target]).expect("pin to first core of group 0");
            let now = current_cpu().expect("getcpu");
            assert_eq!(now, target, "pinned thread must run on its target CPU");
            println!("{{\"metric\":\"pin-verify\",\"verdict\":\"pass\",\"cpu\":{now}}}");
        }
        Err(OsAffinityError::Unsupported(why)) => {
            println!("{{\"metric\":\"pin-verify\",\"verdict\":\"skip\",\"why\":{why:?}}}");
        }
        Err(e) => panic!("getcpu failed structurally: {e}"),
    }

    // --- First-touch page-to-node audit (documents NPS mode as configured). ---
    let mut audit_buf = vec![0u8; 64 << 20];
    for (i, b) in audit_buf.iter_mut().enumerate().step_by(4096) {
        *b = (i % 251) as u8; // fault every page in
    }
    match page_nodes(&audit_buf, 4096) {
        Ok(nodes) => {
            let mut per_node = std::collections::BTreeMap::<i32, usize>::new();
            for &n in &nodes {
                *per_node.entry(n).or_default() += 1;
            }
            println!(
                "{{\"metric\":\"first-touch-audit\",\"pages\":{},\"per_node\":{per_node:?}}}",
                nodes.len()
            );
            assert!(
                per_node.keys().all(|&n| n >= 0),
                "every touched page reports a real node"
            );
        }
        Err(OsAffinityError::Unsupported(why)) => {
            println!("{{\"metric\":\"first-touch-audit\",\"verdict\":\"skip\",\"why\":{why:?}}}");
        }
        Err(e) => panic!("move_pages query failed structurally: {e}"),
    }
    drop(audit_buf);

    // --- L3-island A/B (needs >= 2 measured groups + working pinning). ---
    if groups.len() < 2 || current_cpu().is_err() {
        println!(
            "{{\"metric\":\"l3-island-ab\",\"verdict\":\"skip\",\"why\":\"needs >=2 measured L3 groups + pinning\"}}"
        );
        return;
    }
    // One worker per group; per-worker working set ~24 MiB (inside one
    // 32 MiB island), streamed repeatedly.
    let g = groups.len();
    let words = (24 << 20) / 8;
    let mut buffers: Vec<Vec<u64>> = (0..g)
        .map(|k| (0..words).map(|i| (i as u64) ^ (k as u64)).collect())
        .collect();
    let reps = 8;
    // SPREAD: worker k owns group k's cores.
    let spread_pins: Vec<Vec<u32>> = groups.clone();
    // PACKED: every worker squeezed onto group 0's cores.
    let packed_pins: Vec<Vec<u32>> = (0..g).map(|_| groups[0].clone()).collect();
    // UNPINNED: OS placement.
    let free_pins: Vec<Vec<u32>> = (0..g).map(|_| Vec::new()).collect();
    // Warm + best-of-3 per configuration.
    let best = |buffers: &mut [Vec<u64>], pins: &[Vec<u32>]| -> f64 {
        let mut best = 0.0f64;
        for _ in 0..3 {
            best = best.max(measure(buffers, pins, reps));
        }
        best
    };
    let spread = best(&mut buffers, &spread_pins);
    let packed = best(&mut buffers, &packed_pins);
    let free = best(&mut buffers, &free_pins);
    println!(
        "{{\"metric\":\"l3-island-ab\",\"groups\":{g},\"ws_mib_per_worker\":24,\
         \"spread_gbs\":{spread:.1},\"packed_gbs\":{packed:.1},\"unpinned_gbs\":{free:.1},\
         \"spread_over_packed\":{:.2},\"spread_over_unpinned\":{:.2}}}",
        spread / packed.max(1e-9),
        spread / free.max(1e-9),
    );
    assert!(
        spread > packed,
        "one working set per L3 island must beat {g} sets contending for one island \
         (spread {spread:.1} vs packed {packed:.1} GB/s)"
    );
}
