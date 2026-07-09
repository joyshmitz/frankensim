//! Bulk-generation bitwise-equivalence gates (bead frankensim-1za9, item 2).
//! The batched fill path MUST produce the exact same stream as sequential
//! draws (the fs-simd twin doctrine) and advance the index identically.

use fs_rand::{Stream, StreamKey};

fn stream() -> Stream {
    StreamKey {
        seed: 0xDEAD_BEEF_1234,
        kernel: 9,
        tile: 5,
    }
    .stream()
}

#[test]
fn fill_f64_is_bitwise_sequential() {
    // A non-multiple-of-8 length exercises the batched body AND the scalar tail.
    for len in [0usize, 1, 7, 8, 9, 16, 1000, 1003] {
        let mut bulk = vec![0.0f64; len];
        let mut a = stream();
        a.fill_f64(&mut bulk);

        let mut b = stream();
        for (i, want) in (0..len).map(|_| b.next_f64()).enumerate() {
            assert_eq!(bulk[i].to_bits(), want.to_bits(), "len {len} idx {i}");
        }
        // The index advanced by exactly `len`, so the streams stay in lockstep.
        assert_eq!(a.index(), b.index(), "index mismatch at len {len}");
    }
}

#[test]
fn fill_u64_is_bitwise_sequential() {
    for len in [0usize, 3, 8, 15, 500] {
        let mut bulk = vec![0u64; len];
        let mut a = stream();
        a.fill_u64(&mut bulk);

        let mut b = stream();
        for (i, item) in bulk.iter().enumerate() {
            assert_eq!(*item, b.next_u64(), "len {len} idx {i}");
        }
        assert_eq!(a.index(), b.index(), "index mismatch at len {len}");
    }
}

#[test]
fn bulk_is_deterministic() {
    let mut a = vec![0.0f64; 4096];
    let mut b = vec![0.0f64; 4096];
    stream().fill_f64(&mut a);
    stream().fill_f64(&mut b);
    assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
}
