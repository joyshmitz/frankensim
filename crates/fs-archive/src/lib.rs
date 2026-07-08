//! fs-archive — quality-diversity archives (MAP-Elites / CVT). Layer: L4.
//!
//! Optimization returns ONE point. ILLUMINATION returns a map: the best solution
//! found in every behavioral niche. That is the quality-diversity idea, and it
//! is exactly what a design study wants — not "the single stiffest bracket" but
//! "the best bracket at each mass, each footprint, each lip topology."
//!
//! [`MapElites`] discretizes a behavior-descriptor space into a grid and keeps
//! one ELITE per cell (the highest-fitness solution with that descriptor); a
//! worse candidate in a filled cell is rejected, a better one replaces it. So
//! [`MapElites::coverage`] (fraction of niches filled) and
//! [`MapElites::qd_score`] (total elite fitness) only ever rise. [`CvtArchive`]
//! is the same illumination over a Centroidal-Voronoi tessellation (assign to
//! the nearest centroid) for high-dimensional descriptors, and [`novelty`]
//! scores how far a descriptor is from the current archive (exploration
//! pressure). Deterministic; no dependencies.

use std::collections::BTreeMap;

/// An elite: the best solution recorded for its behavioral niche.
#[derive(Debug, Clone, PartialEq)]
pub struct Elite {
    /// The solution (genotype / design vector).
    pub solution: Vec<f64>,
    /// Its behavior descriptor.
    pub descriptor: Vec<f64>,
    /// Its fitness (higher is better).
    pub fitness: f64,
}

/// A MAP-Elites illumination archive over a gridded behavior space.
#[derive(Debug, Clone)]
pub struct MapElites {
    lo: Vec<f64>,
    hi: Vec<f64>,
    bins: Vec<usize>,
    cells: BTreeMap<Vec<usize>, Elite>,
}

impl MapElites {
    /// A new archive over `[lo, hi]` per descriptor dimension, discretized into
    /// `bins` cells per dimension.
    ///
    /// # Panics
    /// If the dimensions disagree or any bin count is zero.
    #[must_use]
    pub fn new(lo: Vec<f64>, hi: Vec<f64>, bins: Vec<usize>) -> MapElites {
        assert!(
            lo.len() == hi.len() && lo.len() == bins.len(),
            "descriptor dims disagree"
        );
        assert!(
            !lo.is_empty() && bins.iter().all(|&b| b > 0),
            "need >=1 dim and positive bins"
        );
        assert!(
            lo.iter().zip(&hi).all(|(l, h)| h > l),
            "each hi must exceed lo"
        );
        MapElites {
            lo,
            hi,
            bins,
            cells: BTreeMap::new(),
        }
    }

    /// The discrete cell index of a descriptor (clamped into the grid).
    #[must_use]
    pub fn cell_of(&self, descriptor: &[f64]) -> Vec<usize> {
        descriptor
            .iter()
            .zip(&self.lo)
            .zip(&self.hi)
            .zip(&self.bins)
            .map(|(((&d, &l), &h), &b)| {
                let frac = (d - l) / (h - l);
                let idx = (frac * b as f64).floor();
                // clamp into [0, b-1].
                if idx < 0.0 {
                    0
                } else if idx >= b as f64 {
                    b - 1
                } else {
                    idx as usize
                }
            })
            .collect()
    }

    /// Try to add a solution. Returns `true` if it became an elite (a new niche
    /// or a strict improvement over the incumbent).
    pub fn add(&mut self, solution: Vec<f64>, descriptor: Vec<f64>, fitness: f64) -> bool {
        let cell = self.cell_of(&descriptor);
        let improve = self.cells.get(&cell).is_none_or(|e| fitness > e.fitness);
        if improve {
            self.cells.insert(
                cell,
                Elite {
                    solution,
                    descriptor,
                    fitness,
                },
            );
        }
        improve
    }

    /// The total number of cells in the grid.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.bins.iter().product()
    }

    /// The number of filled niches.
    #[must_use]
    pub fn num_elites(&self) -> usize {
        self.cells.len()
    }

    /// The fraction of niches filled.
    #[must_use]
    pub fn coverage(&self) -> f64 {
        self.num_elites() as f64 / self.capacity() as f64
    }

    /// The quality-diversity score: the total fitness over all elites.
    #[must_use]
    pub fn qd_score(&self) -> f64 {
        self.cells.values().map(|e| e.fitness).sum()
    }

    /// The elite in the cell of a descriptor, if any.
    #[must_use]
    pub fn elite_at(&self, descriptor: &[f64]) -> Option<&Elite> {
        self.cells.get(&self.cell_of(descriptor))
    }

    /// The single highest-fitness elite.
    #[must_use]
    pub fn best(&self) -> Option<&Elite> {
        self.cells.values().max_by(|a, b| {
            a.fitness
                .partial_cmp(&b.fitness)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// All elites (deterministic cell order).
    pub fn elites(&self) -> impl Iterator<Item = &Elite> {
        self.cells.values()
    }
}

/// A CVT (Centroidal Voronoi Tessellation) archive: illumination over
/// arbitrary centroids, assigning each descriptor to its nearest centroid.
#[derive(Debug, Clone)]
pub struct CvtArchive {
    centroids: Vec<Vec<f64>>,
    cells: BTreeMap<usize, Elite>,
}

impl CvtArchive {
    /// A new archive over the given centroids.
    ///
    /// # Panics
    /// If `centroids` is empty.
    #[must_use]
    pub fn new(centroids: Vec<Vec<f64>>) -> CvtArchive {
        assert!(!centroids.is_empty(), "need at least one centroid");
        CvtArchive {
            centroids,
            cells: BTreeMap::new(),
        }
    }

    /// The index of the nearest centroid to a descriptor.
    #[must_use]
    pub fn nearest_centroid(&self, descriptor: &[f64]) -> usize {
        self.centroids
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                dist2(a, descriptor)
                    .partial_cmp(&dist2(b, descriptor))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map_or(0, |(i, _)| i)
    }

    /// Try to add a solution (nearest-centroid niche). Returns `true` if it
    /// became an elite.
    pub fn add(&mut self, solution: Vec<f64>, descriptor: Vec<f64>, fitness: f64) -> bool {
        let cell = self.nearest_centroid(&descriptor);
        let improve = self.cells.get(&cell).is_none_or(|e| fitness > e.fitness);
        if improve {
            self.cells.insert(
                cell,
                Elite {
                    solution,
                    descriptor,
                    fitness,
                },
            );
        }
        improve
    }

    /// The number of centroids (niche capacity).
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.centroids.len()
    }

    /// The number of filled niches.
    #[must_use]
    pub fn num_elites(&self) -> usize {
        self.cells.len()
    }

    /// The fraction of niches filled.
    #[must_use]
    pub fn coverage(&self) -> f64 {
        self.num_elites() as f64 / self.capacity() as f64
    }

    /// The quality-diversity score (total elite fitness).
    #[must_use]
    pub fn qd_score(&self) -> f64 {
        self.cells.values().map(|e| e.fitness).sum()
    }

    /// The single highest-fitness elite.
    #[must_use]
    pub fn best(&self) -> Option<&Elite> {
        self.cells.values().max_by(|a, b| {
            a.fitness
                .partial_cmp(&b.fitness)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }
}

/// The NOVELTY of a descriptor relative to a set of others: the mean Euclidean
/// distance to its `k` nearest neighbours (exploration pressure). An empty
/// neighbour set is maximally novel (`+∞`).
#[must_use]
pub fn novelty(descriptor: &[f64], others: &[Vec<f64>], k: usize) -> f64 {
    if others.is_empty() || k == 0 {
        return f64::INFINITY;
    }
    let mut dists: Vec<f64> = others.iter().map(|o| dist2(o, descriptor).sqrt()).collect();
    dists.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let kk = k.min(dists.len());
    dists[..kk].iter().sum::<f64>() / kk as f64
}

fn dist2(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}
