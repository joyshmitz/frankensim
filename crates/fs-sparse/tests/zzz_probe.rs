//! TEMPORARY probe — delete after review.
use fs_sparse::{Coo, CsrCompact};

fn lcg(seed: &mut u64) -> f64 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*seed >> 11) as f64) / (1u64 << 53) as f64 - 0.5
}

fn bits_eq(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

#[test]
fn probe_numa_localized_identity() {
    // numa_localized must be bitwise identical to the source for many shapes/threads.
    for (nr, nc, perrow) in [
        (1usize, 1usize, 1usize),
        (5, 4, 2),
        (257, 199, 8),
        (2, 50, 20),
        (100, 3, 1),
    ] {
        let mut s = 0xABCDu64 + nr as u64;
        let mut coo = Coo::new(nr, nc);
        for r in 0..nr {
            for _ in 0..perrow {
                let c = ((lcg(&mut s) + 0.5) * nc as f64) as usize % nc;
                coo.push(r, c, lcg(&mut s));
            }
        }
        let a = coo.assemble();
        let compact = CsrCompact::from_csr(&a);
        for t in [1usize, 2, 3, 7, 16, 64, nr.max(1) + 5] {
            let loc = compact.numa_localized(t);
            assert_eq!(loc, compact, "numa_localized({t}) differs for {nr}x{nc}");
            // and spmv on the localized copy equals serial
            let x: Vec<f64> = (0..nc).map(|i| 0.3 + (i % 11) as f64).collect();
            let mut y_ref = vec![0.0; nr];
            let mut y_loc = vec![0.0; nr];
            compact.spmv(&x, &mut y_ref);
            loc.spmv(&x, &mut y_loc);
            assert!(bits_eq(&y_ref, &y_loc), "loc spmv mismatch {nr}x{nc} t={t}");
            let mut y_sh = vec![0.0; nr];
            loc.spmv_sharded(&x, &mut y_sh, t);
            assert!(
                bits_eq(&y_ref, &y_sh),
                "loc sharded mismatch {nr}x{nc} t={t}"
            );
        }
    }
}

#[test]
fn probe_assemble_parallel_edges() {
    // t > nrows, single element, empty, non-square, all-in-one-row, duplicates.
    // empty matrix
    let e = Coo::new(3, 3);
    for t in [1usize, 4, 10] {
        assert_eq!(e.assemble(), e.assemble_parallel(t));
    }
    // single element
    let mut one = Coo::new(1, 1);
    one.push(0, 0, 3.5);
    for t in [1usize, 2, 8] {
        assert_eq!(one.assemble(), one.assemble_parallel(t));
    }
    // all triplets in the last row, t huge
    let mut last = Coo::new(6, 6);
    for c in 0..6 {
        last.push(5, c, c as f64 + 1.0);
        last.push(5, c, 0.25); // duplicate
    }
    for t in [1usize, 2, 3, 4, 8, 20] {
        assert_eq!(last.assemble(), last.assemble_parallel(t), "last-row t={t}");
    }
    // non-square with empty rows and duplicates, t > nrows
    let mut s = 0x99u64;
    let (nr, nc) = (13usize, 41usize);
    let mut coo = Coo::new(nr, nc);
    for _ in 0..500 {
        let r = (s.wrapping_mul(6364136223846793005).wrapping_add(1) >> 33) as usize % nr;
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let c = (s >> 33) as usize % nc;
        coo.push(r, c, lcg(&mut s));
    }
    for t in [1usize, 2, 5, 13, 14, 40, 100] {
        assert_eq!(coo.assemble(), coo.assemble_parallel(t), "nonsq t={t}");
    }
    // heavy skew: row 0 huge, rest empty
    let mut skew = Coo::new(50, 50);
    for c in 0..50 {
        skew.push(0, c, c as f64);
    }
    for t in [1usize, 2, 3, 7, 16, 100] {
        assert_eq!(skew.assemble(), skew.assemble_parallel(t), "skew t={t}");
    }
}

#[test]
fn probe_bitwise_vals_not_just_partial_eq() {
    // Csr PartialEq on f64 is value-eq (±0, NaN). Check TRUE bit equality of vals.
    let mut s = 0x1234u64;
    let (nr, nc) = (129usize, 77usize);
    let mut coo = Coo::new(nr, nc);
    for _ in 0..4000 {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let r = (s >> 33) as usize % nr;
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let c = (s >> 33) as usize % nc;
        coo.push(r, c, lcg(&mut s));
    }
    let serial = coo.assemble();
    let sd = serial.to_dense();
    for t in [1usize, 2, 3, 4, 8, 16, 130] {
        let par = coo.assemble_parallel(t);
        let pd = par.to_dense();
        assert!(
            bits_eq(&sd, &pd),
            "assemble_parallel({t}) dense bits differ"
        );
    }
}
