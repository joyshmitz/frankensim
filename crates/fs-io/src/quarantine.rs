//! THE QUARANTINE PRINCIPLE: imports land as [`Quarantined`] (raw value,
//! source receipt, detected defects) and promote to `Evidence` ONLY
//! after the repair suite runs and validity checks pass. Unrepaired
//! defects BLOCK promotion with actionable diagnostics. The receipt is
//! the ledger `imports` row payload (HELM writes it; L2 emits it).

use crate::IoError;
use fs_evidence::{
    Evidence, ModelEvidence, NumericalCertificate, ProvenanceHash, SensitivitySummary,
    StatisticalCertificate,
};
use fs_rep_mesh::{HalfEdgeMesh, Soup, repair};
use std::fmt::Write as _;

/// A defect found at import time (pre-repair census).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportDefect {
    /// Defect class ("degenerate-face", "duplicate-face",
    /// "unreferenced-vertex", "non-manifold-or-open").
    pub class: &'static str,
    /// How many instances.
    pub count: usize,
}

/// The certification receipt: full provenance for "where did this
/// geometry come from and what did we fix."
#[derive(Debug, Clone, PartialEq)]
pub struct ImportReceipt {
    /// Declared source format ("stl", "obj", "ply", …).
    pub format: &'static str,
    /// Content hash of the raw source bytes (FNV-1a; BLAKE3-class hash
    /// upgrades HELM-side with the same field).
    pub source_hash: u64,
    /// Parser version (this crate's version).
    pub parser_version: &'static str,
    /// Element counts as parsed (vertices, triangles).
    pub parsed: (usize, usize),
}

impl ImportReceipt {
    /// Canonical JSON (the ledger `imports` row payload).
    #[must_use]
    pub fn to_json(&self, defects: &[ImportDefect], trust: &str) -> String {
        let mut s = format!(
            "{{\"kind\":\"import-receipt\",\"format\":\"{}\",\"source_hash\":\"{:016x}\",\
             \"parser\":\"{}\",\"vertices\":{},\"triangles\":{},\"defects\":[",
            self.format, self.source_hash, self.parser_version, self.parsed.0, self.parsed.1
        );
        for (i, d) in defects.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let _ = write!(s, "{{\"class\":\"{}\",\"count\":{}}}", d.class, d.count);
        }
        let _ = write!(s, "],\"trust\":\"{trust}\"}}");
        s
    }
}

/// An imported value in quarantine: NOT trusted, carries its census.
#[derive(Debug)]
pub struct Quarantined<T> {
    /// The raw parsed value.
    pub raw: T,
    /// Source provenance.
    pub source_receipt: ImportReceipt,
    /// Defects detected at import.
    pub defects: Vec<ImportDefect>,
}

/// A structured promotion refusal: what blocked, and what to do.
#[derive(Debug, Clone, PartialEq)]
pub struct PromotionRefusal {
    /// The blocking defect classes after repair.
    pub blocking: Vec<String>,
    /// Actionable guidance.
    pub fixes: Vec<String>,
    /// The post-repair receipt JSON (for the ledger row, trust=refused).
    pub receipt_json: String,
}

/// Census a soup for import defects (pre-repair, read-only).
#[must_use]
pub fn census(soup: &Soup) -> Vec<ImportDefect> {
    let mut defects = Vec::new();
    let mut degenerate = 0usize;
    let mut seen = std::collections::BTreeSet::new();
    let mut duplicate = 0usize;
    for t in &soup.triangles {
        if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
            degenerate += 1;
        }
        let mut key = *t;
        key.sort_unstable();
        if !seen.insert(key) {
            duplicate += 1;
        }
    }
    if degenerate > 0 {
        defects.push(ImportDefect {
            class: "degenerate-face",
            count: degenerate,
        });
    }
    if duplicate > 0 {
        defects.push(ImportDefect {
            class: "duplicate-face",
            count: duplicate,
        });
    }
    let mut referenced = vec![false; soup.positions.len()];
    for t in &soup.triangles {
        for &i in t {
            if let Some(slot) = referenced.get_mut(i as usize) {
                *slot = true;
            }
        }
    }
    let unreferenced = referenced.iter().filter(|&&r| !r).count();
    if unreferenced > 0 {
        defects.push(ImportDefect {
            class: "unreferenced-vertex",
            count: unreferenced,
        });
    }
    // Closedness + edge-manifoldness by direct edge counting: every
    // undirected edge of a watertight 2-manifold appears exactly twice.
    // (The half-edge builder alone is insufficient: it legally accepts
    // open boundaries.)
    let mut edge_counts: std::collections::BTreeMap<(u32, u32), u32> =
        std::collections::BTreeMap::new();
    for t in &soup.triangles {
        if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
            continue; // degenerates counted above
        }
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let key = (a.min(b), a.max(b));
            *edge_counts.entry(key).or_insert(0) += 1;
        }
    }
    let bad_edges = edge_counts.values().filter(|&&c| c != 2).count();
    let vertex_nonmanifold =
        HalfEdgeMesh::from_triangles(soup.positions.clone(), &soup.triangles).is_err();
    if bad_edges > 0 || vertex_nonmanifold {
        defects.push(ImportDefect {
            class: "non-manifold-or-open",
            count: bad_edges.max(1),
        });
    }
    defects
}

/// Wrap a parsed soup in quarantine with its census and receipt.
#[must_use]
pub fn quarantine(soup: Soup, format: &'static str, source: &[u8]) -> Quarantined<Soup> {
    let defects = census(&soup);
    Quarantined {
        source_receipt: ImportReceipt {
            format,
            source_hash: fs_obs::fnv1a64(source),
            parser_version: crate::VERSION,
            parsed: (soup.positions.len(), soup.triangles.len()),
        },
        raw: soup,
        defects,
    }
}

/// Promote a quarantined mesh: run the fs-rep-mesh repair suite, re-run
/// the validity census, and REFUSE if blocking defects remain. On
/// success the value becomes `Evidence<Soup>` with the full receipt in
/// its provenance chain.
///
/// # Errors
/// [`PromotionRefusal`] with blocking defects + actionable fixes.
pub fn promote(
    q: Quarantined<Soup>,
    max_hole_edges: usize,
) -> Result<(Evidence<Soup>, String), Box<PromotionRefusal>> {
    let outcome = repair(q.raw, max_hole_edges);
    let post = census(&outcome.soup);
    let blocking: Vec<String> = post
        .iter()
        .filter(|d| d.class != "unreferenced-vertex") // cosmetic, repair keeps welded verts
        .map(|d| format!("{} x{}", d.class, d.count))
        .collect();
    if !blocking.is_empty() {
        let receipt_json = q.source_receipt.to_json(&post, "refused");
        return Err(Box::new(PromotionRefusal {
            fixes: post
                .iter()
                .map(|d| match d.class {
                    "non-manifold-or-open" => format!(
                        "{} unrepaired: increase max_hole_edges (currently {max_hole_edges}) \
                         or route through the SDF re-mesh pipeline",
                        d.class
                    ),
                    other => format!("{other} survived repair: report a repair-suite gap"),
                })
                .collect(),
            blocking,
            receipt_json,
        }));
    }
    // Trusted: exact numerics (the mesh IS the value), receipt-chained
    // provenance, no model claims (geometry, not physics).
    let receipt_json = q.source_receipt.to_json(&q.defects, "promoted");
    let mut canon = receipt_json.clone();
    let _ = write!(canon, ";repairs={}", outcome.receipts_json());
    let provenance = ProvenanceHash::of_bytes(canon.as_bytes());
    let n_tris = outcome.soup.triangles.len();
    #[allow(clippy::cast_precision_loss)]
    let qoi = n_tris as f64;
    Ok((
        Evidence {
            value: outcome.soup,
            qoi,
            numerical: NumericalCertificate::exact(qoi),
            statistical: StatisticalCertificate::None,
            model: ModelEvidence::none(),
            sensitivity: SensitivitySummary::default(),
            provenance,
            adjoint_ref: None,
        },
        receipt_json,
    ))
}

/// Convenience: parse-with-format + quarantine in one step.
///
/// # Errors
/// [`IoError`] from the parser (quarantine itself cannot fail).
pub fn import_mesh(bytes: &[u8], format: &'static str) -> Result<Quarantined<Soup>, IoError> {
    let soup = match format {
        "stl" => crate::stl::read_stl(bytes)?,
        "obj" => {
            let text = core::str::from_utf8(bytes).map_err(|e| IoError::Malformed {
                at: e.valid_up_to(),
                what: "OBJ must be UTF-8".to_string(),
            })?;
            crate::obj::read_obj(text)?
        }
        "ply" => crate::ply::read_ply(bytes)?,
        other => {
            return Err(IoError::Unsupported {
                what: format!("format {other:?} (stl/obj/ply)"),
            });
        }
    };
    Ok(quarantine(soup, format, bytes))
}
