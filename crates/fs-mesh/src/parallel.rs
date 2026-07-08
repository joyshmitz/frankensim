//! Deterministic parallel domain coloring (bead uee3 item 4):
//! READ-PARALLEL, APPLY-CANONICAL prefix batches. Each window of BRIO
//! order gets every point's conflict region (cavity + growth repair +
//! one-ring, mirroring the kernel's insert transaction exactly)
//! computed READ-ONLY across scoped threads; the batch is then the
//! MAXIMAL PREFIX whose regions are pairwise disjoint — the scan
//! STOPS at the first clash rather than skipping past it, so the
//! applied insertion order is EXACTLY the kernel's BRIO order and the
//! finished mesh is bitwise identical to the sequential kernel's on
//! EVERY input, including massively cospherical degeneracies where
//! weak-Delaunay tie-breaking is order-dependent (a first-fit
//! multi-color scheduler was tried and measurably diverged on the
//! 6×6×6 grid — reordering is NOT free under ties). Thread count can
//! only change the wall clock, never a bit of the result; the
//! batches' mathematical content — same-batch insertions COMMUTE — is
//! gated adversarially by the battery (reversed application,
//! canonical equality).

use crate::delaunay::{GHOST, Mesh, MeshError, Tetrahedralization, bootstrap_mesh};
use fs_exec::Cx;
use std::collections::BTreeSet;

/// Batching ledger.
#[derive(Debug, Clone, Copy, Default)]
pub struct ColoredStats {
    /// Prefix batches applied.
    pub batches: u64,
    /// Largest batch (parallel width evidence).
    pub largest_batch: u64,
    /// Batches of size one (the serial tail of degenerate inputs).
    pub singleton_batches: u64,
    /// Points scheduled (excludes the bootstrap quad).
    pub points: u64,
    /// Thread count used for the read phase.
    pub threads: u64,
}

impl ColoredStats {
    /// Canonical JSON ledger row.
    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            "{{\"batches\":{},\"largest_batch\":{},\"singleton_batches\":{},\
             \"points\":{},\"threads\":{}}}",
            self.batches, self.largest_batch, self.singleton_batches, self.points, self.threads
        )
    }
}

/// Read-only visibility walk to a conflict seed (mirrors
/// `Mesh::locate` without stats/hint mutation).
fn locate_ro(mesh: &Mesh, p: [f64; 3], p_idx: u32) -> u32 {
    let mut t = (0..mesh.tets.len() as u32)
        .find(|&c| mesh.alive[c as usize] && !mesh.is_ghost(c))
        .expect("a live real tet always exists");
    let budget = 4 * mesh.tets.len() as u64 + 64;
    let mut steps = 0u64;
    'walk: loop {
        steps += 1;
        if steps > budget {
            return (0..mesh.tets.len() as u32)
                .find(|&c| mesh.alive[c as usize] && mesh.in_conflict(c, p, p_idx))
                .expect("a distinct point conflicts with some tet");
        }
        for i in 0..4 {
            let f = mesh.facet_verts(t, i);
            if mesh.facet_sees_sos(f, p, p_idx) == fs_ivl::Sign::Negative {
                let n = mesh.adj[t as usize][i];
                if mesh.is_ghost(n) {
                    return n;
                }
                t = n;
                continue 'walk;
            }
        }
        return t;
    }
}

/// The conflict REGION of a point against the current mesh: the
/// cavity (flood + growth repair, mirroring `Mesh::insert`) plus its
/// one-ring of outside neighbors (insertion rewires their adjacency
/// rows, so disjoint REGIONS — not just cavities — is what makes
/// same-color insertions commute). `None` marks a bitwise duplicate
/// of an existing vertex.
#[allow(clippy::float_cmp)] // duplicate detection is DELIBERATELY bitwise
fn conflict_region(mesh: &Mesh, p_idx: u32) -> Option<BTreeSet<u32>> {
    let p = mesh.points[p_idx as usize];
    let mut seed = locate_ro(mesh, p, p_idx);
    for &v in &mesh.tets[seed as usize] {
        if v != GHOST && mesh.points[v as usize] == p {
            return None;
        }
    }
    if !mesh.in_conflict(seed, p, p_idx) {
        seed = (0..mesh.tets.len() as u32)
            .find(|&c| mesh.alive[c as usize] && mesh.in_conflict(c, p, p_idx))
            .expect("a distinct point conflicts with some tet");
    }
    // Flood.
    let mut cavity: Vec<u32> = vec![seed];
    let mut in_cavity: BTreeSet<u32> = BTreeSet::from([seed]);
    let mut scan = 0;
    while scan < cavity.len() {
        let t = cavity[scan];
        scan += 1;
        for i in 0..4 {
            let n = mesh.adj[t as usize][i];
            if !in_cavity.contains(&n) && mesh.in_conflict(n, p, p_idx) {
                in_cavity.insert(n);
                cavity.push(n);
            }
        }
    }
    // Growth repair (identical rule to the kernel's).
    loop {
        let mut grew = false;
        let mut ci = 0;
        while ci < cavity.len() {
            let t = cavity[ci];
            ci += 1;
            for i in 0..4 {
                let n = mesh.adj[t as usize][i];
                if in_cavity.contains(&n) {
                    continue;
                }
                let f = mesh.facet_verts(t, i);
                if f.contains(&GHOST) {
                    continue;
                }
                let vis = fs_ivl::orient3d(
                    mesh.points[f[0] as usize],
                    mesh.points[f[1] as usize],
                    mesh.points[f[2] as usize],
                    p,
                );
                if vis != fs_ivl::Sign::Positive {
                    in_cavity.insert(n);
                    cavity.push(n);
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
    }
    // One-ring: outside neighbors (their adjacency rows get rewired).
    let mut region = in_cavity.clone();
    for &t in &cavity {
        for i in 0..4 {
            let n = mesh.adj[t as usize][i];
            if !in_cavity.contains(&n) {
                region.insert(n);
            }
        }
    }
    Some(region)
}

/// The maximal PREFIX of the window whose regions are pairwise
/// disjoint: the scan stops at the first clash (never skips past it),
/// so batch concatenation reproduces the exact BRIO insertion order.
/// Known duplicates (None regions) have an empty footprint and pass
/// through freely — the kernel's duplicate guard is the last line of
/// defense either way.
fn prefix_batch(regions: &[(u32, Option<BTreeSet<u32>>)]) -> usize {
    let mut occupied: BTreeSet<u32> = BTreeSet::new();
    let mut len = 0usize;
    for (_, reg) in regions {
        match reg {
            None => len += 1,
            Some(r) => {
                if occupied.is_disjoint(r) {
                    occupied.extend(r.iter().copied());
                    len += 1;
                } else {
                    break;
                }
            }
        }
    }
    len.max(1)
}

/// Build the Delaunay tetrahedralization by deterministic prefix
/// batches: conflict regions read-only across `threads` scoped
/// threads, application in EXACT BRIO order (batches never reorder).
/// The finished mesh is bitwise identical to the sequential kernel's
/// at any thread count — gated by the battery on general-position AND
/// degenerate inputs.
///
/// # Errors
/// Same surface as [`crate::delaunay::delaunay`].
///
/// # Panics
/// Only on kernel programmer contracts (a live real tet always
/// exists once bootstrapped).
pub fn delaunay_colored(
    points: &[fs_geom::Point3],
    threads: usize,
    window: usize,
    cx: &Cx<'_>,
) -> Result<(Tetrahedralization, ColoredStats), MeshError> {
    let (mut mesh, quad, order) = bootstrap_mesh(points)?;
    let work: Vec<u32> = order.iter().copied().filter(|i| !quad.contains(i)).collect();
    let threads = threads.max(1);
    let window = window.max(1);
    let mut stats = ColoredStats {
        threads: threads as u64,
        points: work.len() as u64,
        ..ColoredStats::default()
    };
    let mut i = 0usize;
    let mut since_checkpoint = 0usize;
    while i < work.len() {
        let win = &work[i..(i + window).min(work.len())];
        // Phase A: read-only conflict regions, deterministic thread
        // partition (contiguous chunks reassembled by position — the
        // schedule cannot change the result, only the wall clock).
        let chunk = win.len().div_ceil(threads);
        let mesh_ref = &mesh;
        let mut regions: Vec<(u32, Option<BTreeSet<u32>>)> = Vec::with_capacity(win.len());
        std::thread::scope(|scope| {
            let handles: Vec<_> = win
                .chunks(chunk)
                .map(|part| {
                    scope.spawn(move || {
                        part.iter()
                            .map(|&p| (p, conflict_region(mesh_ref, p)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            for h in handles {
                regions.extend(h.join().expect("region thread panicked"));
            }
        });
        // Phase B: the maximal disjoint prefix.
        let take = prefix_batch(&regions);
        stats.batches += 1;
        stats.largest_batch = stats.largest_batch.max(take as u64);
        if take == 1 {
            stats.singleton_batches += 1;
        }
        // Phase C: canonical application (exact BRIO order).
        for &p in &win[..take] {
            mesh.insert(p);
        }
        i += take;
        since_checkpoint += take;
        if since_checkpoint >= 256 {
            since_checkpoint = 0;
            cx.checkpoint()?;
        }
    }
    mesh.stats.tets_final = (0..mesh.tets.len() as u32)
        .filter(|&t| mesh.alive[t as usize] && !mesh.is_ghost(t))
        .count() as u64;
    let steiner_from = u32::try_from(points.len()).expect("point count fits u32");
    Ok((
        Tetrahedralization {
            mesh,
            steiner_from,
        },
        stats,
    ))
}

/// Apply each prefix batch REVERSED — the adversarial commutativity
/// probe: if batch regions were not truly pairwise disjoint, the
/// reversed order would produce a different mesh (compared
/// canonically, since allocation order legitimately differs).
///
/// # Errors
/// Same surface as [`delaunay_colored`].
pub fn delaunay_colored_reversed(
    points: &[fs_geom::Point3],
    window: usize,
    cx: &Cx<'_>,
) -> Result<Tetrahedralization, MeshError> {
    let (mut mesh, quad, order) = bootstrap_mesh(points)?;
    let work: Vec<u32> = order.iter().copied().filter(|i| !quad.contains(i)).collect();
    let window = window.max(1);
    let mut i = 0usize;
    let mut since_checkpoint = 0usize;
    while i < work.len() {
        let win = &work[i..(i + window).min(work.len())];
        let mesh_ref = &mesh;
        let regions: Vec<(u32, Option<BTreeSet<u32>>)> = win
            .iter()
            .map(|&p| (p, conflict_region(mesh_ref, p)))
            .collect();
        let take = prefix_batch(&regions);
        for &p in win[..take].iter().rev() {
            mesh.insert(p);
        }
        i += take;
        since_checkpoint += take;
        if since_checkpoint >= 256 {
            since_checkpoint = 0;
            cx.checkpoint()?;
        }
    }
    mesh.stats.tets_final = (0..mesh.tets.len() as u32)
        .filter(|&t| mesh.alive[t as usize] && !mesh.is_ghost(t))
        .count() as u64;
    let steiner_from = u32::try_from(points.len()).expect("point count fits u32");
    Ok(Tetrahedralization {
        mesh,
        steiner_from,
    })
}
