//! Oriented volume complexes (plan §7.2): tet element storage with the
//! full vertex/edge/face incidence chain — and the δδ = 0 identity
//! verified EXACTLY (integer arithmetic), because fs-feec's discrete
//! exterior calculus is only as good as the orientation bookkeeping here
//! (plan §8.1: "the discrete de Rham sequence is exact... purely
//! combinatorial"). Hex elements get SoA storage now; their incidence
//! joins when fs-feec's tensor-product families land (CONTRACT no-claim).

use std::collections::BTreeMap;

/// An oriented tetrahedral complex with derived, canonically-ordered edge
/// and face tables and signed incidence operators d0, d1, d2.
#[derive(Debug, Clone)]
pub struct TetComplex {
    /// Vertex count.
    pub vertex_count: usize,
    /// Tets as vertex quadruples (orientation = even permutations of the
    /// stored order).
    pub tets: Vec<[u32; 4]>,
    /// Canonical edges (sorted vertex pairs), sorted.
    pub edges: Vec<[u32; 2]>,
    /// Canonical faces (sorted vertex triples), sorted.
    pub faces: Vec<[u32; 3]>,
    edge_index: BTreeMap<[u32; 2], usize>,
    face_index: BTreeMap<[u32; 3], usize>,
}

/// A signed sparse incidence operator: rows of `(column, ±1)` pairs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incidence {
    /// `rows[r]` lists `(column, sign)` entries of row r.
    pub rows: Vec<Vec<(usize, i8)>>,
    /// Column count.
    pub cols: usize,
}

impl Incidence {
    /// Apply to an integer cochain (exact arithmetic — this is the δδ = 0
    /// verification path).
    #[must_use]
    pub fn apply(&self, x: &[i64]) -> Vec<i64> {
        self.rows
            .iter()
            .map(|row| row.iter().map(|&(c, s)| i64::from(s) * x[c]).sum())
            .collect()
    }
}

impl TetComplex {
    /// Build from tets (vertex indices; orientation as stored).
    #[must_use]
    pub fn from_tets(vertex_count: usize, tets: Vec<[u32; 4]>) -> Self {
        let mut edge_set: BTreeMap<[u32; 2], usize> = BTreeMap::new();
        let mut face_set: BTreeMap<[u32; 3], usize> = BTreeMap::new();
        for t in &tets {
            for (i, j) in [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)] {
                let mut e = [t[i], t[j]];
                e.sort_unstable();
                let next = edge_set.len();
                edge_set.entry(e).or_insert(next);
            }
            for skip in 0..4 {
                let mut f = [0u32; 3];
                let mut k = 0;
                for (v, &vert) in t.iter().enumerate() {
                    if v != skip {
                        f[k] = vert;
                        k += 1;
                    }
                }
                f.sort_unstable();
                let next = face_set.len();
                face_set.entry(f).or_insert(next);
            }
        }
        // Re-key to sorted order (BTreeMap iteration is sorted; indices
        // follow it for determinism).
        let edges: Vec<[u32; 2]> = edge_set.keys().copied().collect();
        let faces: Vec<[u32; 3]> = face_set.keys().copied().collect();
        let edge_index = edges.iter().enumerate().map(|(i, &e)| (e, i)).collect();
        let face_index = faces.iter().enumerate().map(|(i, &f)| (f, i)).collect();
        TetComplex {
            vertex_count,
            tets,
            edges,
            faces,
            edge_index,
            face_index,
        }
    }

    /// d0: vertices → edges (gradient shape). Row per edge `[a, b]` with
    /// a < b: `x(b) − x(a)`.
    #[must_use]
    pub fn d0(&self) -> Incidence {
        Incidence {
            rows: self
                .edges
                .iter()
                .map(|&[a, b]| vec![(a as usize, -1i8), (b as usize, 1i8)])
                .collect(),
            cols: self.vertex_count,
        }
    }

    /// d1: edges → faces (curl shape). Face `[a, b, c]` with a < b < c
    /// has boundary edges (a,b) +, (b,c) +, (a,c) −.
    #[must_use]
    pub fn d1(&self) -> Incidence {
        Incidence {
            rows: self
                .faces
                .iter()
                .map(|&[a, b, c]| {
                    vec![
                        (self.edge_index[&[a, b]], 1i8),
                        (self.edge_index[&[b, c]], 1i8),
                        (self.edge_index[&[a, c]], -1i8),
                    ]
                })
                .collect(),
            cols: self.edges.len(),
        }
    }

    /// d2: faces → tets (divergence shape). Tet `[v0, v1, v2, v3]`
    /// (canonically sorted per row) has face `omit i` with sign (−1)^i.
    #[must_use]
    pub fn d2(&self) -> Incidence {
        Incidence {
            rows: self
                .tets
                .iter()
                .map(|t| {
                    let mut sorted = *t;
                    sorted.sort_unstable();
                    (0..4)
                        .map(|skip| {
                            let mut f = [0u32; 3];
                            let mut k = 0;
                            for (v, &vert) in sorted.iter().enumerate() {
                                if v != skip {
                                    f[k] = vert;
                                    k += 1;
                                }
                            }
                            let sign = if skip % 2 == 0 { 1i8 } else { -1i8 };
                            (self.face_index[&f], sign)
                        })
                        .collect()
                })
                .collect(),
            cols: self.faces.len(),
        }
    }
}

/// Oriented hex element storage (SoA-friendly quadruple-pair layout).
/// Incidence operators join with fs-feec's tensor-product families —
/// storage lands now so meshing beads have a target (CONTRACT no-claim).
#[derive(Debug, Clone, Default)]
pub struct HexComplex {
    /// Vertex count.
    pub vertex_count: usize,
    /// Hexes as vertex octuples (VTK ordering).
    pub hexes: Vec<[u32; 8]>,
}
