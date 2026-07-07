//! SEMANTIC DIFF (addendum Proposal 10, bead lmp4.10; [F] — behind the
//! `semantic-diff` feature until its Gauntlet tier and kill metric are
//! green): not a text diff, a PHYSICS diff — "where do the fields differ
//! beyond tolerance, and WHICH upstream edits caused it." Diff objects
//! carry a RANKED LIST of contributing causal edits with per-edit
//! measured contributions (real differences frequently have multiple
//! upstream causes; single-cause attribution drops the secondary drivers
//! an agent needs). Entities without stable IDs degrade to a FLAGGED
//! geometric comparison, and that fallback fraction is the R3
//! early-warning metric.

use crate::ident::{EntityId, IdentityMap};
use crate::sheaf::{BAND_FRACTION, SAMPLES_PER_INTERFACE};
use crate::{Aabb, Chart, Point3};
use fs_exec::Cx;

/// One identified patch in a world snapshot.
pub struct IdentifiedPatch<'a> {
    /// The stable id (None = legacy/unidentified — fallback territory).
    pub id: Option<EntityId>,
    /// The chart presenting this entity.
    pub chart: &'a dyn Chart,
}

/// One semantic-diff finding.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffObject {
    /// The entity (None when matched by geometric fallback).
    pub entity: Option<EntityId>,
    /// Where (the shared-support box the difference lives in).
    pub region: Aabb,
    /// Which quantity differs.
    pub quantity: &'static str,
    /// Worst sampled |difference|.
    pub magnitude: f64,
    /// Contributing causal edits, RANKED by measured contribution
    /// (op id, contribution magnitude). Empty when unattributed.
    pub causes: Vec<(i64, f64)>,
    /// False when this finding came from the geometric fallback (the
    /// R3 flag).
    pub attributed: bool,
}

/// The diff report.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffReport {
    /// Findings beyond tolerance.
    pub objects: Vec<DiffObject>,
    /// Entities present only in A (deleted) / only in B (created).
    pub only_a: Vec<EntityId>,
    /// Entities present only in B.
    pub only_b: Vec<EntityId>,
    /// The R3 early-warning metric: fraction of compared pairs that fell
    /// back to unattributed geometric comparison.
    pub fallback_fraction: f64,
}

impl DiffReport {
    /// Filter findings by region overlap, quantity, and magnitude floor
    /// (large diffs need triage).
    #[must_use]
    pub fn filter(
        &self,
        region: Option<&Aabb>,
        quantity: Option<&str>,
        min_magnitude: f64,
    ) -> Vec<&DiffObject> {
        self.objects
            .iter()
            .filter(|o| o.magnitude >= min_magnitude)
            .filter(|o| quantity.is_none_or(|q| o.quantity == q))
            .filter(|o| {
                region.is_none_or(|r| {
                    o.region.min.x <= r.max.x
                        && r.min.x <= o.region.max.x
                        && o.region.min.y <= r.max.y
                        && r.min.y <= o.region.max.y
                        && o.region.min.z <= r.max.z
                        && r.min.z <= o.region.max.z
                })
            })
            .collect()
    }
}

fn overlap(a: &Aabb, b: &Aabb) -> Option<Aabb> {
    let min = Point3::new(
        a.min.x.max(b.min.x),
        a.min.y.max(b.min.y),
        a.min.z.max(b.min.z),
    );
    let max = Point3::new(
        a.max.x.min(b.max.x),
        a.max.y.min(b.max.y),
        a.max.z.min(b.max.z),
    );
    (min.x < max.x && min.y < max.y && min.z < max.z).then(|| Aabb::new(min, max))
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

fn box_seed(b: &Aabb) -> u64 {
    let mut bytes = Vec::with_capacity(48);
    for v in [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z] {
        bytes.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    fnv(&bytes)
}

fn lcg(state: &mut u64) -> f64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*state >> 11) as f64) / (1u64 << 53) as f64
}

/// Worst |sdf_a − sdf_b| over the shared zero band of two charts
/// (geometry-seeded sampling — the sheaf interface machinery's method,
/// shared here for support alignment). `None` when the supports do not
/// overlap or no shared band exists.
fn field_difference(a: &dyn Chart, b: &dyn Chart, cx: &Cx<'_>) -> Option<(Aabb, f64)> {
    let shared = overlap(&a.support(), &b.support())?;
    let diag = shared.max.delta_from(shared.min).norm();
    let band = BAND_FRACTION * diag;
    let mut state = box_seed(&shared);
    let mut worst = 0.0f64;
    let mut hits = 0usize;
    for _ in 0..SAMPLES_PER_INTERFACE * 64 {
        if hits >= SAMPLES_PER_INTERFACE {
            break;
        }
        let p = Point3::new(
            shared.min.x + lcg(&mut state) * (shared.max.x - shared.min.x),
            shared.min.y + lcg(&mut state) * (shared.max.y - shared.min.y),
            shared.min.z + lcg(&mut state) * (shared.max.z - shared.min.z),
        );
        let sa = a.eval(p, cx).signed_distance;
        let sb = b.eval(p, cx).signed_distance;
        // The shared band of EITHER side: a difference matters wherever
        // one of the versions places surface.
        if sa.abs() <= band || sb.abs() <= band {
            worst = worst.max((sa - sb).abs());
            hits += 1;
        }
    }
    (hits > 0).then_some((shared, worst))
}

/// The semantic diff between two identified worlds. `divergent_ops` are
/// the branch-difference ops (vcs `merge_views` only-A ∪ only-B);
/// `identity` is the ledgered identity map; `generations` optionally
/// provides the intermediate worlds after each divergent op IN ORDER —
/// when present, per-edit contributions are MEASURED (generation k vs
/// k−1 on the entity's chart), otherwise causes carry the entity's
/// touching ops with the total magnitude unpartitioned on the first.
#[must_use]
pub fn semantic_diff(
    world_a: &[IdentifiedPatch<'_>],
    world_b: &[IdentifiedPatch<'_>],
    identity: &IdentityMap,
    divergent_ops: &[i64],
    generations: &[Vec<IdentifiedPatch<'_>>],
    tol: f64,
    cx: &Cx<'_>,
) -> DiffReport {
    let mut objects = Vec::new();
    let mut compared = 0usize;
    let mut fallbacks = 0usize;
    let find =
        |world: &[IdentifiedPatch<'_>], id: EntityId| world.iter().position(|p| p.id == Some(id));
    // ID-aligned comparison.
    let mut only_a = Vec::new();
    for pa in world_a {
        let Some(id) = pa.id else { continue };
        let Some(bi) = find(world_b, id) else {
            only_a.push(id);
            continue;
        };
        compared += 1;
        let Some((region, magnitude)) = field_difference(pa.chart, world_b[bi].chart, cx) else {
            continue;
        };
        if magnitude <= tol {
            continue;
        }
        // Attribution: ops touching this entity, intersected with the
        // divergent set, contributions measured across generations.
        let touching: Vec<i64> = identity
            .ops_touching(id)
            .into_iter()
            .filter(|op| divergent_ops.contains(op))
            .collect();
        let mut causes: Vec<(i64, f64)> = Vec::new();
        if !touching.is_empty() && generations.len() == divergent_ops.len() {
            // Measure each divergent op's contribution on this entity:
            // generation k vs generation k−1 (generation −1 = world A).
            for (k, &op) in divergent_ops.iter().enumerate() {
                if !touching.contains(&op) {
                    continue;
                }
                let prev: &[IdentifiedPatch<'_>] =
                    if k == 0 { world_a } else { &generations[k - 1] };
                let cur = &generations[k];
                if let (Some(pi), Some(ci)) = (find(prev, id), find(cur, id))
                    && let Some((_, m)) = field_difference(prev[pi].chart, cur[ci].chart, cx)
                {
                    causes.push((op, m));
                }
            }
            causes.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
        } else if !touching.is_empty() {
            // No generations supplied: total magnitude unpartitioned on
            // the touching ops (first carries it, rest 0 — honest about
            // what was NOT measured).
            causes = touching
                .iter()
                .enumerate()
                .map(|(i, &op)| (op, if i == 0 { magnitude } else { 0.0 }))
                .collect();
        }
        let attributed = !causes.is_empty();
        objects.push(DiffObject {
            entity: Some(id),
            region,
            quantity: "signed-distance",
            magnitude,
            causes,
            attributed,
        });
    }
    let only_b: Vec<EntityId> = world_b
        .iter()
        .filter_map(|p| p.id)
        .filter(|id| find(world_a, *id).is_none())
        .collect();
    // Geometric FALLBACK for unidentified patches: support-overlap
    // matching, flagged unattributed (the R3 early-warning path).
    for pa in world_a.iter().filter(|p| p.id.is_none()) {
        for pb in world_b.iter().filter(|p| p.id.is_none()) {
            let Some((region, magnitude)) = field_difference(pa.chart, pb.chart, cx) else {
                continue;
            };
            compared += 1;
            fallbacks += 1;
            if magnitude > tol {
                objects.push(DiffObject {
                    entity: None,
                    region,
                    quantity: "signed-distance",
                    magnitude,
                    causes: Vec::new(),
                    attributed: false,
                });
            }
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let fallback_fraction = if compared == 0 {
        0.0
    } else {
        fallbacks as f64 / compared as f64
    };
    DiffReport {
        objects,
        only_a,
        only_b,
        fallback_fraction,
    }
}
